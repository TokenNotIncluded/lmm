use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use lmm::{
    catalog,
    discovery::{self, ProbeContext, TargetStatus},
    environment::{self, Environment},
    i18n::{self, Locale, Message, msg, text},
    oauth,
    plan::{Intent, Preview},
    provider,
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
#[command(
    name = "lmm",
    version,
    about = text("cli.about")
)]
struct Cli {
    #[arg(long, global = true, value_name = "LANG", help = text("arg.lang"))]
    lang: Option<Locale>,
    #[arg(help = text("arg.json"))]
    #[arg(long, global = true)]
    json: bool,
    #[arg(help = text("arg.non_interactive"))]
    #[arg(long, global = true)]
    non_interactive: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = text("cmd.presets"))]
    Presets {
        #[arg(help = text("arg.query"))]
        query: Option<String>,
    },
    #[command(about = text("cmd.catalog"))]
    Catalog {
        #[arg(help = text("arg.query"))]
        query: Option<String>,
    },
    #[command(about = text("cmd.setup"))]
    Setup(SetupArgs),
    #[command(about = text("cmd.install"))]
    Install(OperationArgs),
    #[command(about = text("cmd.login"))]
    Login(LoginArgs),
    #[command(about = text("cmd.logout"))]
    Logout(AuthArgs),
    #[command(about = text("cmd.models"))]
    Models(AuthArgs),
    #[command(about = text("cmd.status"))]
    Status(TargetArgs),
    #[command(about = text("cmd.sync"))]
    Sync(OperationArgs),
    #[command(about = text("cmd.update"))]
    Update(OperationArgs),
    #[command(about = text("cmd.doctor"))]
    Doctor(DoctorArgs),
    #[command(about = text("cmd.restore"))]
    Restore(OperationArgs),
    #[command(about = text("cmd.disconnect"))]
    Disconnect(RequiredTarget),
    #[command(about = text("cmd.uninstall"))]
    Uninstall(RequiredTarget),
    #[command(about = text("cmd.run"))]
    Run(RequiredTarget),
}

#[derive(Args)]
struct AuthArgs {
    #[arg(help = text("arg.issuer"))]
    #[arg(long, default_value = oauth::DEFAULT_ISSUER)]
    issuer: String,
}

#[derive(Args)]
struct LoginArgs {
    #[command(flatten)]
    auth: AuthArgs,
    #[arg(help = text("arg.no_browser"))]
    #[arg(long)]
    no_browser: bool,
    #[arg(help = text("arg.timeout"))]
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(10..=600))]
    timeout: u64,
}

#[derive(Args)]
struct TargetArgs {
    #[arg(help = text("arg.software"))]
    software: Option<String>,
    #[arg(help = text("arg.instance"))]
    #[arg(long, requires = "software")]
    instance: Option<PathBuf>,
}

#[derive(Args)]
struct OperationArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(help = text("arg.all"))]
    #[arg(long, conflicts_with_all = ["software", "instance"])]
    all: bool,
    #[arg(help = text("arg.dry_run"))]
    #[arg(long)]
    dry_run: bool,
    #[arg(help = text("arg.yes"))]
    #[arg(long)]
    yes: bool,
}

#[derive(Args)]
struct SetupArgs {
    #[command(flatten)]
    operation: OperationArgs,
    #[arg(help = text("arg.add_only"))]
    #[arg(long, conflicts_with = "activate")]
    add_only: bool,
    #[arg(help = text("arg.activate"))]
    #[arg(long)]
    activate: bool,
}

#[derive(Args)]
struct DoctorArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(help = text("arg.fix"))]
    #[arg(long)]
    fix: bool,
    #[arg(long, help = text("arg.report"))]
    report: bool,
}

#[derive(Args)]
struct RequiredTarget {
    #[arg(help = text("arg.software"))]
    software: String,
    #[arg(long, help = text("arg.instance"))]
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
    notes: Vec<Message>,
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

fn inspect(target: &TargetArgs, context: &ProbeContext) -> Result<Vec<TargetStatus>, Message> {
    if target
        .instance
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(msg("error.invalid_instance"));
    }
    match target.software.as_deref() {
        Some(id) => {
            let software = catalog::find(id).ok_or(msg("error.unknown_software"))?;
            if target.instance.is_some() && !matches!(id, "astrbot" | "cc-switch") {
                return Err(msg("error.instance_unsupported"));
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

fn select_software() -> Result<String, Message> {
    let mut output = io::stderr().lock();
    writeln!(output, "{}", msg("select.title")).map_err(|_| msg("error.menu_output"))?;
    for (i, software) in catalog::SOFTWARE.iter().enumerate() {
        writeln!(output, "  {}. {} ({})", i + 1, software.name, software.id)
            .map_err(|_| msg("error.menu_output"))?;
    }
    write!(output, "{}", msg("select.prompt")).map_err(|_| msg("error.menu_output"))?;
    output.flush().map_err(|_| msg("error.menu_output"))?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|_| msg("error.selection_input"))?;
    let index = input
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1));
    index
        .and_then(|index| catalog::SOFTWARE.get(index))
        .map(|software| software.id.to_owned())
        .ok_or(msg("error.no_selection"))
}

fn operation_report(
    name: &'static str,
    args: &OperationArgs,
    intent: Intent,
    interactive: bool,
    context: &ProbeContext,
) -> Result<Report, Message> {
    let selected;
    let target = if args.target.software.is_none() && !args.all {
        if !interactive {
            return Err(msg("error.target_required"));
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
            preview.blockers = vec![msg("plan.install_adapter")];
        } else if name == "restore" {
            preview.account = "not_required";
            preview.blockers = vec![msg("plan.restore_adapter")];
        }
        report.plans.push(preview);
    }
    if args.all {
        report.notes.push(msg("note.all_blocked"));
    }
    if args.dry_run {
        report.notes.push(msg("note.dry_run"));
    }
    if args.yes {
        report.notes.push(msg("note.yes"));
    }
    if name == "install" {
        report.notes.push(msg("note.install"));
    }
    Ok(report)
}

fn write_json(value: &impl Serialize) -> Result<(), Message> {
    let mut output = io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, value).map_err(|_| msg("error.json_output"))?;
    writeln!(output).map_err(|_| msg("error.json_output"))
}

fn emit(report: &Report, json: bool) -> Result<(), Message> {
    if json {
        return write_json(report);
    }
    let mut output = io::stdout().lock();
    writeln!(
        output,
        "LMM - {} - {} / {}",
        report.command, report.environment.os, report.environment.arch
    )
    .map_err(|_| msg("error.output"))?;
    writeln!(
        output,
        "{}",
        msg("output.scope").render(&[("time", &report.checked_at.to_string())])
    )
    .map_err(|_| msg("error.output"))?;
    for target in &report.targets {
        writeln!(
            output,
            "{}",
            msg("output.target").render(&[("software", target.software)])
        )
        .map_err(|_| msg("error.output"))?;
        for evidence in &target.evidence {
            // Debug formatting still escapes control characters in user-controlled paths.
            let path = format!("{:?}", evidence.path);
            let observation = evidence.observation.label();
            writeln!(
                output,
                "{}",
                msg("output.evidence")
                    .render(&[("path", &path), ("observation", observation.text())])
            )
            .map_err(|_| msg("error.output"))?;
        }
        writeln!(output, "{}", msg("output.verification")).map_err(|_| msg("error.output"))?;
    }
    for plan in &report.plans {
        writeln!(
            output,
            "{}",
            msg("output.blocked_plan").render(&[("software", plan.software)])
        )
        .map_err(|_| msg("error.output"))?;
        for blocker in &plan.blockers {
            writeln!(output, "  - {blocker}").map_err(|_| msg("error.output"))?;
        }
    }
    for note in &report.notes {
        writeln!(output, "\n{note}").map_err(|_| msg("error.output"))?;
    }
    Ok(())
}

fn execute(cli: &Cli) -> Result<u8, Message> {
    if cli.non_interactive
        && matches!(
            &cli.command,
            Command::Login(_) | Command::Logout(_) | Command::Models(_)
        )
    {
        return Err(msg("error.auth_unattended"));
    }
    let context = context();
    let interactive = !cli.json
        && !cli.non_interactive
        && io::stdin().is_terminal()
        && io::stderr().is_terminal();
    let report = match &cli.command {
        Command::Presets { query } => {
            let snapshot = provider::search(query.as_deref().unwrap_or_default())
                .map_err(|_| msg("error.presets_read"))?;
            if cli.json {
                write_json(&snapshot)?;
            } else {
                let mut output = io::stdout().lock();
                writeln!(
                    output,
                    "{}",
                    msg("output.presets").render(&[("revision", provider::UPSTREAM_REVISION)])
                )
                .map_err(|_| msg("error.presets_output"))?;
                for preset in &snapshot.presets {
                    writeln!(
                        output,
                        "{} — {} ({})",
                        preset.id,
                        preset.name,
                        preset.kind.label()
                    )
                    .map_err(|_| msg("error.presets_output"))?;
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
                        msg("output.catalog_entry").render(&[
                            ("id", software.id),
                            ("name", software.name),
                            ("purpose", software.purpose.text()),
                            ("source", software.source),
                        ])
                    )
                    .map_err(|_| msg("error.catalog_output"))?;
                }
            }
            return Ok(0);
        }
        Command::Status(target) => Report::new("status", inspect(target, &context)?),
        Command::Doctor(args) => {
            let mut report = Report::new("doctor", inspect(&args.target, &context)?);
            report.outcome = "incomplete";
            report.notes.push(msg("note.doctor_incomplete"));
            if args.fix {
                report.outcome = "blocked";
                report.notes.push(msg("note.fix_blocked"));
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
                return Err(msg("error.login_interactive"));
            }
            let result = oauth::login(
                &args.auth.issuer,
                std::time::Duration::from_secs(args.timeout),
                |url| {
                    let mut output = io::stderr().lock();
                    writeln!(
                        output,
                        "{}",
                        msg("output.authorization_url").render(&[("url", url)])
                    )
                    .map_err(|_| oauth::AuthError::Output)?;
                    if !args.no_browser && open::that_detached(url).is_err() {
                        writeln!(output, "{}", msg("output.browser_failed"))
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
                        writeln!(io::stdout().lock(), "{}", msg("output.login"))
                            .map_err(|_| msg("error.login_output"))?;
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
                        writeln!(io::stdout().lock(), "{}", msg("output.logout"))
                            .map_err(|_| msg("error.logout_output"))?;
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
                        writeln!(output, "{}", msg("output.models"))
                            .map_err(|_| msg("error.models_output"))?;
                        for model in &catalog.models {
                            writeln!(
                                output,
                                "{}",
                                msg("output.model_entry").render(&[
                                    ("id", &format!("{:?}", model.id)),
                                    ("upstream", &format!("{:?}", model.upstream_model)),
                                    ("group", &format!("{:?}", model.group)),
                                    ("currency", &format!("{:?}", model.pricing.currency)),
                                    ("unit", &format!("{:?}", model.pricing.unit)),
                                    ("input", &format!("{:?}", model.pricing.input)),
                                    ("output", &format!("{:?}", model.pricing.output)),
                                    ("request", &format!("{:?}", model.pricing.request)),
                                    ("basis", &format!("{:?}", model.pricing.price_basis)),
                                ])
                            )
                            .map_err(|_| msg("error.models_output"))?;
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
            report.notes.push(msg("note.execution_blocked"));
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

fn auth_failure(error: oauth::AuthError, json: bool) -> Result<u8, Message> {
    if json {
        write_json(
            &serde_json::json!({"schema_version":1,"outcome":"blocked","error":error.message(),"error_code":error.message().key()}),
        )?;
    } else {
        eprintln!("lmm: {error}");
    }
    Ok(3)
}

fn localized_help(mut command: clap::Command) -> clap::Command {
    command.build();
    if command.get_name() == "help" {
        command = command.about(text("cmd.help"));
    }
    command
        .help_template(text("help.template"))
        .subcommand_help_heading(text("help.commands"))
        .mut_args(|arg| {
            let heading = if arg.is_positional() {
                text("help.arguments")
            } else {
                text("help.options")
            };
            let arg = match arg.get_id().as_str() {
                "help" => arg.help(text("help.help")),
                "version" => arg.help(text("help.version")),
                _ => arg,
            };
            arg.help_heading(heading)
        })
        .mut_subcommands(localized_help)
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().collect();
    let selected = Locale::detect(&args, |key| env::var_os(key));
    let _ = i18n::initialize(selected);
    let matches = localized_help(Cli::command()).get_matches_from(args);
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    debug_assert!(cli.lang.is_none_or(|locale| locale == selected));
    match execute(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if cli.json {
                let _ = write_json(
                    &serde_json::json!({"schema_version": 1, "outcome": "invalid_request", "error": error, "error_code": error.key()}),
                );
            } else {
                eprintln!("lmm: {error}");
            }
            ExitCode::from(2)
        }
    }
}
