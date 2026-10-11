use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use lmm::{
    catalog,
    discovery::{self, ProbeContext, TargetStatus},
    environment::{self, Environment},
    i18n::{self, tr},
    oauth,
    plan::{Intent, Preview},
    provider, trf,
};
use serde::Serialize;
use std::{
    env,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Parser)]
#[command(name = "lmm", version, about = tr("cli.about"))]
struct Cli {
    #[arg(
        long = "lang",
        global = true,
        value_name = "LANG",
        value_parser = ["en", "zh-CN"],
        help = tr("arg.lang")
    )]
    _lang: Option<String>,
    #[arg(help = tr("arg.json"))]
    #[arg(long, global = true)]
    json: bool,
    #[arg(help = tr("arg.non_interactive"))]
    #[arg(long, global = true)]
    non_interactive: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = tr("command.presets"))]
    Presets { query: Option<String> },
    #[command(about = tr("command.catalog"))]
    Catalog { query: Option<String> },
    #[command(about = tr("command.setup"))]
    Setup(SetupArgs),
    #[command(about = tr("command.install"))]
    Install(OperationArgs),
    #[command(about = tr("command.login"))]
    Login(LoginArgs),
    #[command(about = tr("command.logout"))]
    Logout(AuthArgs),
    #[command(about = tr("command.models"))]
    Models(AuthArgs),
    #[command(about = tr("command.status"))]
    Status(TargetArgs),
    #[command(about = tr("command.sync"))]
    Sync(OperationArgs),
    #[command(about = tr("command.update"))]
    Update(OperationArgs),
    #[command(about = tr("command.doctor"))]
    Doctor(DoctorArgs),
    #[command(about = tr("command.restore"))]
    Restore(OperationArgs),
    #[command(about = tr("command.disconnect"))]
    Disconnect(RequiredTarget),
    #[command(about = tr("command.uninstall"))]
    Uninstall(RequiredTarget),
    #[command(about = tr("command.run"))]
    Run(RequiredTarget),
}

#[derive(Args)]
struct AuthArgs {
    #[arg(help = tr("arg.issuer"))]
    #[arg(long, default_value = oauth::DEFAULT_ISSUER)]
    issuer: String,
}

#[derive(Args)]
struct LoginArgs {
    #[command(flatten)]
    auth: AuthArgs,
    #[arg(help = tr("arg.no_browser"))]
    #[arg(long)]
    no_browser: bool,
    #[arg(help = tr("arg.timeout"))]
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(10..=600))]
    timeout: u64,
}

#[derive(Args)]
struct TargetArgs {
    #[arg(help = tr("arg.software"))]
    software: Option<String>,
    #[arg(help = tr("arg.instance"))]
    #[arg(long, requires = "software")]
    instance: Option<PathBuf>,
}

#[derive(Args)]
struct OperationArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(help = tr("arg.all"))]
    #[arg(long, conflicts_with_all = ["software", "instance"])]
    all: bool,
    #[arg(help = tr("arg.dry_run"))]
    #[arg(long)]
    dry_run: bool,
    #[arg(help = tr("arg.yes"))]
    #[arg(long)]
    yes: bool,
}

#[derive(Args)]
struct SetupArgs {
    #[command(flatten)]
    operation: OperationArgs,
    #[arg(help = tr("arg.add_only"))]
    #[arg(long, conflicts_with = "activate")]
    add_only: bool,
    #[arg(help = tr("arg.activate"))]
    #[arg(long)]
    activate: bool,
}

#[derive(Args)]
struct DoctorArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(help = tr("arg.fix"))]
    #[arg(long)]
    fix: bool,
    #[arg(help = tr("arg.report"))]
    #[arg(long)]
    report: bool,
}

#[derive(Args)]
struct RequiredTarget {
    #[arg(help = tr("arg.software"))]
    software: String,
    #[arg(long, help = tr("arg.instance"))]
    instance: Option<PathBuf>,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    command: &'static str,
    outcome: &'static str,
    checked_at: u64,
    check_scope: &'static str,
    environment: Environment,
    targets: Vec<TargetStatus>,
    plans: Vec<Preview>,
    notes: Vec<&'static str>,
}

impl Report {
    fn new(command: &'static str, targets: Vec<TargetStatus>) -> Self {
        Self {
            schema_version: 1,
            command,
            outcome: "inspected",
            checked_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs()),
            check_scope: "local_path_evidence_only",
            environment: Environment::detect(),
            targets,
            plans: vec![],
            notes: vec![],
        }
    }

    fn redact(&mut self) {
        for target in &mut self.targets {
            target.instance = target
                .instance
                .as_ref()
                .map(|_| PathBuf::from("[redacted]"));
            for evidence in &mut target.evidence {
                evidence.path = PathBuf::from("[redacted]");
            }
        }
    }
}

fn context() -> ProbeContext {
    ProbeContext {
        home: environment::user_home(),
        path: env::var_os("PATH")
            .map(|value| env::split_paths(&value).collect())
            .unwrap_or_default(),
    }
}

fn inspect(target: &TargetArgs, context: &ProbeContext) -> Result<Vec<TargetStatus>, String> {
    if target
        .instance
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(tr("error.absolute_instance").into());
    }
    match target.software.as_deref() {
        Some(id) => {
            let software =
                catalog::find(id).ok_or_else(|| tr("error.unknown_software").to_owned())?;
            if target.instance.is_some() && !matches!(id, "astrbot" | "cc-switch") {
                return Err(tr("error.unsupported_instance").into());
            }
            Ok(vec![discovery::discover(
                software,
                context,
                target.instance.as_deref(),
            )])
        }
        None => Ok(catalog::SOFTWARE
            .iter()
            .map(|software| discovery::discover(software, context, None))
            .collect()),
    }
}

fn select_software() -> Result<String, String> {
    let mut output = io::stderr().lock();
    writeln!(output, "{}", tr("menu.title")).map_err(|_| tr("error.menu_output"))?;
    for (i, software) in catalog::SOFTWARE.iter().enumerate() {
        writeln!(output, "  {}. {} ({})", i + 1, software.name, software.id)
            .map_err(|_| tr("error.menu_output"))?;
    }
    write!(output, "{}", tr("menu.prompt")).map_err(|_| tr("error.menu_output"))?;
    output.flush().map_err(|_| tr("error.menu_output"))?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|_| tr("error.menu_input"))?;
    let index = input
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1));
    index
        .and_then(|index| catalog::SOFTWARE.get(index))
        .map(|software| software.id.to_owned())
        .ok_or_else(|| tr("error.no_selection").to_owned())
}

fn operation_report(
    name: &'static str,
    args: &OperationArgs,
    intent: Intent,
    interactive: bool,
    context: &ProbeContext,
) -> Result<Report, String> {
    let selected;
    let target = if args.target.software.is_none() && !args.all {
        if !interactive {
            return Err(tr("error.target_required").into());
        }
        selected = TargetArgs {
            software: Some(select_software()?),
            instance: None,
        };
        &selected
    } else {
        &args.target
    };
    let mut targets = inspect(target, context)?;
    // Qualified installation and adapter evidence are prerequisites for --all.
    // No adapter is qualified in this milestone; PATH hits cannot bypass this gate.
    if args.all {
        targets.clear();
    }
    let mut report = Report::new(name, targets);
    report.outcome = "blocked";
    for target in &report.targets {
        let mut preview = Preview::blocked(target.software, intent);
        if name == "install" {
            preview.account = "not_required";
            preview.blockers = vec![tr("blocker.install")];
        } else if name == "restore" {
            preview.account = "not_required";
            preview.blockers = vec![tr("blocker.restore")];
        }
        report.plans.push(preview);
    }
    if args.all {
        report.notes.push(tr("note.all"));
    }
    if args.dry_run {
        report.notes.push(tr("note.dry_run"));
    }
    if args.yes {
        report.notes.push(tr("note.yes"));
    }
    if name == "install" {
        report.notes.push(tr("note.install"));
    }
    Ok(report)
}

fn write_json(value: &impl Serialize) -> Result<(), String> {
    let mut output = io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, value).map_err(|_| tr("error.json_output").to_owned())?;
    writeln!(output).map_err(|_| tr("error.json_output").to_owned())
}

fn observation_label(observation: discovery::Observation) -> &'static str {
    match observation {
        discovery::Observation::Present => tr("observation.present"),
        discovery::Observation::NotFound => tr("observation.not_found"),
        discovery::Observation::Unknown => tr("observation.unknown"),
    }
}

fn emit(report: &Report, json: bool) -> Result<(), String> {
    if json {
        return write_json(report);
    }
    let mut output = io::stdout().lock();
    writeln!(
        output,
        "LMM · {} · {} / {}",
        report.command, report.environment.os, report.environment.arch
    )
    .map_err(|_| tr("error.output"))?;
    writeln!(
        output,
        "{}",
        trf!("output.scope", timestamp = report.checked_at)
    )
    .map_err(|_| tr("error.output"))?;
    for target in &report.targets {
        writeln!(
            output,
            "{}",
            trf!("output.target", software = target.software)
        )
        .map_err(|_| tr("error.output"))?;
        for evidence in &target.evidence {
            // Debug formatting escapes control characters in user-controlled paths.
            writeln!(
                output,
                "{}",
                trf!(
                    "output.evidence",
                    path = format!("{:?}", evidence.path),
                    observation = observation_label(evidence.observation)
                )
            )
            .map_err(|_| tr("error.output"))?;
        }
        writeln!(output, "{}", tr("output.verification")).map_err(|_| tr("error.output"))?;
    }
    for plan in &report.plans {
        writeln!(
            output,
            "{}",
            trf!("output.blocked", software = plan.software)
        )
        .map_err(|_| tr("error.output"))?;
        for blocker in &plan.blockers {
            writeln!(output, "  - {blocker}").map_err(|_| tr("error.output"))?;
        }
    }
    for note in &report.notes {
        writeln!(output, "\n{note}").map_err(|_| tr("error.output"))?;
    }
    Ok(())
}

fn execute(cli: &Cli) -> Result<u8, String> {
    if cli.non_interactive
        && matches!(
            &cli.command,
            Command::Login(_) | Command::Logout(_) | Command::Models(_)
        )
    {
        return Err(tr("error.auth_non_interactive").into());
    }
    let context = context();
    let interactive = !cli.json
        && !cli.non_interactive
        && io::stdin().is_terminal()
        && io::stderr().is_terminal();
    let report = match &cli.command {
        Command::Presets { query } => {
            let snapshot = provider::search(query.as_deref().unwrap_or_default())
                .map_err(|_| tr("error.presets_read").to_owned())?;
            if cli.json {
                write_json(&snapshot)?;
            } else {
                let mut output = io::stdout().lock();
                writeln!(
                    output,
                    "{}",
                    trf!("presets.title", revision = provider::UPSTREAM_REVISION)
                )
                .map_err(|_| tr("error.presets_output"))?;
                for preset in &snapshot.presets {
                    writeln!(
                        output,
                        "{} — {} ({})",
                        preset.id,
                        preset.name,
                        preset.kind.as_str()
                    )
                    .map_err(|_| tr("error.presets_output"))?;
                }
            }
            return Ok(0);
        }
        Command::Catalog { query } => {
            let entries = catalog::search(query.as_deref().unwrap_or_default());
            if cli.json {
                write_json(&entries)?;
            } else {
                let mut output = io::stdout().lock();
                for software in entries {
                    writeln!(
                        output,
                        "{}",
                        trf!(
                            "catalog.entry",
                            id = software.id,
                            name = software.name,
                            purpose = tr(software.purpose),
                            source = software.source
                        )
                    )
                    .map_err(|_| tr("error.catalog_output"))?;
                }
            }
            return Ok(0);
        }
        Command::Status(target) => Report::new("status", inspect(target, &context)?),
        Command::Doctor(args) => {
            let mut report = Report::new("doctor", inspect(&args.target, &context)?);
            report.outcome = "incomplete";
            report.notes.push(tr("note.doctor"));
            if args.fix {
                report.outcome = "blocked";
                report.notes.push(tr("note.fix"));
            }
            if args.report {
                report.redact();
            }
            report
        }
        Command::Setup(args) => operation_report(
            "setup",
            &args.operation,
            if args.activate {
                Intent::Activate
            } else if args.add_only {
                Intent::AddOnly
            } else {
                Intent::Default
            },
            interactive,
            &context,
        )?,
        Command::Install(args) => {
            operation_report("install", args, Intent::AddOnly, interactive, &context)?
        }
        Command::Sync(args) => {
            operation_report("sync", args, Intent::Maintain, interactive, &context)?
        }
        Command::Update(args) => {
            operation_report("update", args, Intent::Maintain, interactive, &context)?
        }
        Command::Restore(args) => {
            operation_report("restore", args, Intent::Maintain, interactive, &context)?
        }
        Command::Login(args) => {
            if !interactive {
                return Err(tr("error.login_interactive").into());
            }
            let result = oauth::login(
                &args.auth.issuer,
                std::time::Duration::from_secs(args.timeout),
                |url| {
                    let mut output = io::stderr().lock();
                    writeln!(output, "{}", trf!("login.authorize", url = url))
                    .map_err(|_| oauth::AuthError::Output)?;
                    if !args.no_browser && open::that_detached(url).is_err() {
                        writeln!(output, "{}", tr("login.browser_failed"))
                            .map_err(|_| oauth::AuthError::Output)?;
                    }
                    Ok(())
                },
            );
            return match result {
                Ok(status) => {
                    if cli.json {
                        write_json(&status)?;
                    } else {
                        writeln!(io::stdout().lock(), "{}", tr("login.success"))
                            .map_err(|_| tr("error.login_output"))?;
                    }
                    Ok(0)
                }
                Err(error) => auth_failure(error, cli.json),
            };
        }
        Command::Logout(args) => {
            return match oauth::logout(&args.issuer) {
                Ok(outcome) => {
                    if cli.json {
                        write_json(
                            &serde_json::json!({"outcome": outcome, "applications_changed": false, "other_application_grants": "not_checked"}),
                        )?;
                    } else {
                        writeln!(io::stdout().lock(), "{}", tr("logout.success"))
                            .map_err(|_| tr("error.logout_output"))?;
                    }
                    Ok(0)
                }
                Err(error) => auth_failure(error, cli.json),
            };
        }
        Command::Models(args) => {
            return match oauth::models(&args.issuer) {
                Ok(catalog) => {
                    if cli.json {
                        write_json(&catalog)?;
                    } else {
                        let mut output = io::stdout().lock();
                        writeln!(output, "{}", tr("models.title"))
                            .map_err(|_| tr("error.models_output"))?;
                        for model in &catalog.models {
                            writeln!(
                                output,
                                "{}",
                                trf!(
                                    "models.entry",
                                    id = format!("{:?}", model.id),
                                    upstream = format!("{:?}", model.upstream_model),
                                    group = format!("{:?}", model.group),
                                    currency = format!("{:?}", model.pricing.currency),
                                    unit = format!("{:?}", model.pricing.unit),
                                    input = format!("{:?}", model.pricing.input),
                                    output = format!("{:?}", model.pricing.output),
                                    request = format!("{:?}", model.pricing.request),
                                    basis = format!("{:?}", model.pricing.price_basis)
                                )
                            )
                            .map_err(|_| tr("error.models_output"))?;
                        }
                    }
                    Ok(0)
                }
                Err(error) => auth_failure(error, cli.json),
            };
        }
        Command::Disconnect(args) | Command::Uninstall(args) | Command::Run(args) => {
            let name = match &cli.command {
                Command::Disconnect(_) => "disconnect",
                Command::Uninstall(_) => "uninstall",
                _ => "run",
            };
            let target = TargetArgs {
                software: Some(args.software.clone()),
                instance: args.instance.clone(),
            };
            let mut report = Report::new(name, inspect(&target, &context)?);
            report.outcome = "blocked";
            report.notes.push(tr("note.execution"));
            report
        }
    };
    emit(&report, cli.json)?;
    Ok(if matches!(report.outcome, "blocked" | "incomplete") {
        3
    } else {
        0
    })
}

fn auth_failure(error: oauth::AuthError, json: bool) -> Result<u8, String> {
    if json {
        write_json(
            &serde_json::json!({
                "schema_version": 1,
                "outcome": "blocked",
                "error": error.to_string(),
                "error_code": error.code(),
            }),
        )?;
    } else {
        eprintln!("lmm: {error}");
    }
    Ok(3)
}

fn localize_help(command: clap::Command) -> clap::Command {
    command
        .help_template(tr("help.template"))
        .subcommand_help_heading(tr("help.commands"))
        .mut_args(|arg| {
            let arg = match arg.get_id().as_str() {
                "help" => arg.help(tr("help.flag")),
                "version" => arg.help(tr("help.version")),
                _ => arg,
            };
            let heading = if arg.is_positional() {
                tr("help.arguments")
            } else {
                tr("help.options")
            };
            arg.help_heading(heading)
        })
        .mut_subcommands(|command| {
            if command.get_name() == "help" {
                command.about(tr("help.subcommand"))
            } else {
                localize_help(command)
            }
        })
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().collect();
    i18n::init_from_args(&args);
    let mut command = Cli::command();
    command.build();
    let matches = localize_help(command).get_matches_from(args);
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    match execute(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if cli.json {
                let _ = write_json(
                    &serde_json::json!({"schema_version": 1, "outcome": "invalid_request", "error": error}),
                );
            } else {
                eprintln!("lmm: {error}");
            }
            ExitCode::from(2)
        }
    }
}
