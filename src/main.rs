mod applications_cli;

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use lmm::{
    catalog, cc_switch,
    discovery::{self, ProbeContext, TargetStatus},
    environment::{self, Environment},
    i18n::{self, Locale, Message, english, localize, msg, text},
    managed::{self, Action as ManagedAction},
    oauth, pi,
    plan::{Intent, Preview},
    provider,
};
use serde::Serialize;
use std::{
    env, fmt,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
enum CliError {
    Message(Message),
    Adapter(String),
}

impl CliError {
    fn key(&self) -> Option<&'static str> {
        match self {
            Self::Message(message) => Some(message.key()),
            Self::Adapter(_) => None,
        }
    }
}

impl From<Message> for CliError {
    fn from(message: Message) -> Self {
        Self::Message(message)
    }
}

impl From<String> for CliError {
    fn from(value: String) -> Self {
        i18n::message_from_english(&value).map_or_else(|| Self::Adapter(value), Self::Message)
    }
}

impl From<&str> for CliError {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => message.fmt(formatter),
            Self::Adapter(value) => formatter.write_str(value),
        }
    }
}

impl Serialize for CliError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Message(message) => message.serialize(serializer),
            Self::Adapter(value) => value.serialize(serializer),
        }
    }
}

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
    #[command(about = text("cmd.authorize"))]
    Authorize(applications_cli::AuthorizeArgs),
    #[command(about = text("cmd.receipt"))]
    Receipt(applications_cli::ReceiptArgs),
    #[command(about = text("cmd.credential"))]
    #[command(hide = true)]
    Credential(applications_cli::CredentialArgs),
    #[command(about = text("cmd.install"))]
    Install(OperationArgs),
    #[command(about = text("cmd.login"))]
    Login(LoginArgs),
    #[command(about = text("cmd.logout"))]
    Logout(AuthArgs),
    #[command(about = text("cmd.models"))]
    Models(AuthArgs),
    #[command(about = text("cmd.balance"))]
    Balance(AuthArgs),
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
    Uninstall(OperationArgs),
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
struct PiArgs {
    #[arg(help = text("arg.node"))]
    #[arg(long)]
    node: Option<PathBuf>,
    #[arg(help = text("arg.pi_package"))]
    #[arg(long)]
    pi_package: Option<PathBuf>,
    #[arg(help = text("arg.agent_dir"))]
    #[arg(long)]
    agent_dir: Option<PathBuf>,
}

impl PiArgs {
    fn supplied(&self) -> bool {
        self.node.is_some() || self.pi_package.is_some() || self.agent_dir.is_some()
    }
}

#[derive(Args)]
struct TargetArgs {
    #[arg(help = text("arg.software"))]
    software: Option<String>,
    #[arg(help = text("arg.instance"))]
    #[arg(long, requires = "software")]
    instance: Option<PathBuf>,
    #[arg(help = text("arg.managed_root"))]
    #[arg(long, requires = "software")]
    managed_root: Option<PathBuf>,
    #[command(flatten)]
    pi: PiArgs,
}

#[derive(Args)]
struct OperationArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[command(flatten)]
    application: applications_cli::Scope,
    #[arg(help = text("arg.configuration"))]
    #[arg(long)]
    configuration: bool,
    #[arg(help = text("arg.all"))]
    #[arg(long, conflicts_with_all = ["software", "instance", "managed_root"])]
    all: bool,
    #[arg(help = text("arg.dry_run"))]
    #[arg(long)]
    dry_run: bool,
    #[arg(help = text("arg.yes"))]
    #[arg(long)]
    yes: bool,
    #[arg(help = text("arg.plan_id"))]
    #[arg(long, requires = "yes")]
    plan_id: Option<String>,
    #[arg(help = text("arg.change"))]
    #[arg(long)]
    change: Option<String>,
    #[arg(help = text("arg.cancel_pending"))]
    #[arg(long, requires_all = ["yes", "plan_id"], conflicts_with = "dry_run")]
    cancel_pending: bool,
}

#[derive(Args)]
struct SetupArgs {
    #[command(flatten)]
    operation: OperationArgs,
    #[arg(help = text("arg.model_id"))]
    #[arg(long)]
    model_id: Option<String>,
    #[arg(long, default_value = "LMM")]
    display_name: String,
    #[arg(long)]
    no_browser: bool,
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(10..=600))]
    timeout: u64,
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
    #[command(flatten)]
    application: applications_cli::Scope,
    #[arg(long, help = text("arg.instance"))]
    instance: Option<PathBuf>,
    #[arg(long)]
    managed_root: Option<PathBuf>,
    #[command(flatten)]
    pi: PiArgs,

    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    yes: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    managed_installation: Option<managed::ManagedStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cc_switch_database: Option<cc_switch::DatabaseStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adapter_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pi_settings: Option<pi::status::Inspection>,
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
            managed_installation: None,
            cc_switch_database: None,
            adapter_error: None,
            pi_settings: None,
        }
    }

    fn redact(&mut self) {
        if let Some(managed) = &mut self.managed_installation {
            managed.managed_root = PathBuf::from("[redacted]");
            managed.executable = managed
                .executable
                .as_ref()
                .map(|_| PathBuf::from("[redacted]"));
            if let Some(plan) = &mut managed.pending {
                plan.managed_root = PathBuf::from("[redacted]");
                plan.database = plan.database.as_ref().map(|_| PathBuf::from("[redacted]"));
            }
        }
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

fn inspect(target: &TargetArgs, context: &ProbeContext) -> Result<Vec<TargetStatus>, CliError> {
    if target.managed_root.is_some() && target.software.as_deref() != Some("cc-switch") {
        return Err(english("error.managed_target").into());
    }
    if target
        .instance
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(msg("error.invalid_instance").into());
    }
    match target.software.as_deref() {
        Some(id) => {
            let software = catalog::find(id).ok_or(msg("error.unknown_software"))?;
            if target.instance.is_some() && !matches!(id, "astrbot" | "cc-switch" | "pi") {
                return Err(msg("error.instance_unsupported").into());
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

// @mark cli.pi-settings-inspection role=command contract=explicit-read-only-instance
fn readonly_report(
    name: &'static str,
    target: &TargetArgs,
    context: &ProbeContext,
) -> Result<Report, CliError> {
    let mut report = Report::new(name, inspect(target, context)?);
    let requested =
        target.pi.node.is_some() || target.pi.pi_package.is_some() || target.pi.agent_dir.is_some();
    if !requested {
        return Ok(report);
    }
    if target.software.as_deref() != Some("pi") {
        return Err(english("error.pi_read_target").into());
    }
    let (Some(project), Some(sdk), Some(agent)) = (
        &target.instance,
        &target.pi.pi_package,
        &target.pi.agent_dir,
    ) else {
        return Err(english("error.pi_read_paths").into());
    };
    if ![project, sdk, agent].iter().all(|path| path.is_absolute())
        || target
            .pi
            .node
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
    {
        return Err(english("error.pi_paths_absolute").into());
    }
    let inspection = pi::status::inspect(project, sdk, agent);
    report.check_scope = "explicit_pi_settings_snapshot";
    if inspection.configuration != "passed" {
        report.outcome = "incomplete";
    }
    for state in &mut report.targets {
        state.lmm_added = match inspection.provider_reference {
            "pinned"
            | "other_known_source"
            | "multiple_known_sources"
            | "known_with_unresolved_sources" => "referenced_in_settings",
            "no_known_source" => "no_known_reference",
            _ => "unknown",
        };
        state.lmm_enabled = match inspection.default_provider {
            "lmm" => "selected_in_settings",
            "other" => "not_selected_in_settings",
            "unset" => "unset_in_settings",
            _ => "unknown",
        };
        state.verification.configuration = inspection.configuration;
        if inspection.configuration == "passed" {
            state.verification.verified_at = Some(report.checked_at);
        }
        for (path, observation) in [
            (agent.join("settings.json"), inspection.global_settings),
            (
                project.join(".pi/settings.json"),
                inspection.project_settings,
            ),
        ] {
            state.evidence.push(discovery::Evidence {
                path,
                observation,
                kind: "explicit_pi_settings_snapshot",
            });
        }
    }
    report.notes.push(msg("note.pi_read_scope"));
    report.notes.push(msg("note.pi_read_limits"));
    report.pi_settings = Some(inspection);
    Ok(report)
}

fn select_software() -> Result<String, CliError> {
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
        .ok_or_else(|| msg("error.no_selection").into())
}

fn operation_report(
    name: &'static str,
    args: &OperationArgs,
    intent: Intent,
    interactive: bool,
    context: &ProbeContext,
) -> Result<Report, CliError> {
    if args.application.supplied() || args.configuration {
        return Err(english("error.application_adapter").into());
    }
    let selected;
    let target = if args.target.software.is_none() && !args.all {
        if !interactive {
            return Err(msg("error.target_required").into());
        }
        selected = TargetArgs {
            software: Some(select_software()?),
            instance: None,
            managed_root: None,
            pi: PiArgs {
                node: None,
                pi_package: None,
                agent_dir: None,
            },
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

fn write_json(value: &impl Serialize) -> Result<(), CliError> {
    let mut output = io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, value).map_err(|_| msg("error.json_output"))?;
    writeln!(output).map_err(|_| msg("error.json_output").into())
}

fn managed_manager(target: &TargetArgs) -> Result<managed::Manager, CliError> {
    if target
        .instance
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(english("error.absolute_instance").into());
    }
    let database = target
        .instance
        .as_ref()
        .map(|root| root.join("cc-switch.db"))
        .map_or_else(managed::default_database, Ok)
        .map_err(|error| error.to_string())?;
    managed::Manager::for_current_environment(target.managed_root.clone(), Some(database))
        .map_err(|error| CliError::from(error.to_string()))
}

fn managed_output(value: &impl Serialize, json: bool) -> Result<(), CliError> {
    // Plans contain source, digest, exact scope and receipt IDs. The same
    // structured preview is available in terminals and unattended workflows.
    if !json {
        writeln!(io::stdout().lock(), "{}", msg("output.managed_heading"))
            .map_err(|_| english("error.plan_output"))?;
    }
    write_json(value)
}

fn managed_operation(
    name: &'static str,
    action: ManagedAction,
    args: &OperationArgs,
    interactive: bool,
    json: bool,
) -> Result<u8, CliError> {
    if args.application.supplied() || args.configuration {
        return Err(english("error.lifecycle_authorization").into());
    }
    if action != ManagedAction::Restore && args.change.is_some() {
        return Err(english("error.change_restore_only").into());
    }
    if action == ManagedAction::Restore && args.change.is_none() {
        return Err(english("error.restore_change").into());
    }
    let manager = managed_manager(&args.target)?;
    if !args.dry_run
        && args.yes
        && let Some(plan_id) = &args.plan_id
    {
        if args.cancel_pending {
            return match manager.cancel(action, args.change.as_deref(), plan_id) {
                Ok(receipt) => {
                    managed_output(
                        &serde_json::json!({"schema_version":1,"command":name,"outcome":"canceled","receipt":receipt}),
                        json,
                    )?;
                    Ok(0)
                }
                Err(error) => {
                    managed_output(
                        &serde_json::json!({"schema_version":1,"command":name,"outcome":"blocked","error":error.to_string()}),
                        json,
                    )?;
                    Ok(3)
                }
            };
        }
        return match manager.apply(action, args.change.as_deref(), plan_id) {
            Ok(result) => {
                managed_output(
                    &serde_json::json!({"schema_version":1,"command":name,"outcome":result.outcome,"managed_operation":result}),
                    json,
                )?;
                Ok(0)
            }
            Err(error) => {
                let status = manager.status().ok();
                let outcome = if status
                    .as_ref()
                    .is_some_and(|status| status.pending.is_some())
                {
                    "incomplete"
                } else {
                    "blocked"
                };
                managed_output(
                    &serde_json::json!({"schema_version":1,"command":name,"outcome":outcome,"error":error.to_string(),"managed_installation":status}),
                    json,
                )?;
                Ok(3)
            }
        };
    }
    let plan = match manager.preview(action, args.change.as_deref()) {
        Ok(plan) => plan,
        Err(error) => {
            managed_output(
                &serde_json::json!({"schema_version":1,"command":name,"outcome":"blocked","error":error.to_string()}),
                json,
            )?;
            return Ok(3);
        }
    };
    let outcome = if args.dry_run {
        "planned"
    } else {
        "confirmation_required"
    };
    managed_output(
        &serde_json::json!({
            "schema_version":1,"command":name,"outcome":outcome,"managed_plan":plan,
            "next_step":english("note.managed_next_step")
        }),
        json,
    )?;
    if args.dry_run {
        return Ok(0);
    }
    if !interactive || args.yes {
        return Ok(3);
    }
    write!(io::stderr().lock(), "{}", msg("select.managed_confirm"))
        .map_err(|_| english("error.confirm_output"))?;
    io::stderr()
        .lock()
        .flush()
        .map_err(|_| english("error.confirm_output"))?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|_| english("error.confirm_input"))?;
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        return Ok(3);
    }
    match manager.apply(action, args.change.as_deref(), &plan.plan_id) {
        Ok(result) => {
            managed_output(
                &serde_json::json!({"schema_version":1,"command":name,"outcome":result.outcome,"managed_operation":result}),
                json,
            )?;
            Ok(0)
        }
        Err(error) => {
            managed_output(
                &serde_json::json!({"schema_version":1,"command":name,"outcome":"incomplete","error":error.to_string(),"plan_id":plan.plan_id}),
                json,
            )?;
            Ok(3)
        }
    }
}

fn enrich_cc_switch(report: &mut Report, target: &TargetArgs) {
    if target.software.as_deref() != Some("cc-switch") {
        return;
    }
    report.check_scope = "managed_distribution_and_explicit_native_database";
    match managed_manager(target).and_then(|manager| {
        manager
            .status()
            .map_err(|error| CliError::from(error.to_string()))
    }) {
        Ok(status) => {
            if let Some(row) = report
                .targets
                .iter_mut()
                .find(|row| row.software == "cc-switch")
            {
                row.installation = status.installation;
                row.installation_manager = status.installation_manager;
            }
            if status.pending.is_some() || matches!(status.binary_integrity, "modified" | "missing")
            {
                report.outcome = "incomplete";
            }
            report.managed_installation = Some(status);
        }
        Err(error) => {
            report.adapter_error = Some(match error {
                CliError::Message(message) => english(message.key()).to_owned(),
                CliError::Adapter(value) => value,
            });
            report.outcome = "incomplete";
        }
    }
    if let Some(instance) = &target.instance {
        match cc_switch::inspect(&instance.join("cc-switch.db")) {
            Ok(status) => {
                if let Some(row) = report
                    .targets
                    .iter_mut()
                    .find(|row| row.software == "cc-switch")
                {
                    row.configuration_manager = "cc-switch";
                }
                report.cc_switch_database = Some(status);
            }
            Err(error) => {
                report.adapter_error = Some(error.to_string());
                report.outcome = "incomplete";
            }
        }
    }
}

fn emit(report: &Report, json: bool) -> Result<(), CliError> {
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
            msg("output.target").render(&[
                ("software", target.software),
                ("installation", target.installation),
                ("installation_manager", target.installation_manager),
                ("configuration_manager", target.configuration_manager),
            ])
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
        if let Some(settings) = &report.pi_settings {
            writeln!(
                output,
                "{}",
                msg("output.pi_settings").render(&[
                    ("added", target.lmm_added),
                    ("enabled", target.lmm_enabled),
                    ("model", settings.default_model),
                    ("scope", settings.model_scope),
                    ("configuration", settings.configuration),
                    ("diagnostic", settings.diagnostic),
                ])
            )
            .map_err(|_| english("error.output"))?;
        } else {
            writeln!(output, "{}", msg("output.verification")).map_err(|_| msg("error.output"))?;
        }
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
    if let Some(status) = &report.managed_installation {
        writeln!(
            output,
            "{}",
            msg("output.managed_status").render(&[
                ("installation", status.installation),
                ("integrity", status.binary_integrity),
                ("health", status.application_health),
            ])
        )
        .map_err(|_| english("error.managed_output"))?;
        for receipt in &status.history {
            writeln!(
                output,
                "  {:?} · change_id={} · plan_id={}",
                receipt.action, receipt.change_id, receipt.plan_id
            )
            .map_err(|_| english("error.receipt_output"))?;
        }
    }
    if let Some(status) = &report.cc_switch_database {
        writeln!(
            output,
            "{}",
            msg("output.database_status").render(&[
                ("schema", &status.schema_version.to_string()),
                ("providers", &status.providers.len().to_string()),
            ])
        )
        .map_err(|_| english("error.database_output"))?;
    }
    if let Some(error) = &report.adapter_error {
        writeln!(
            output,
            "{}",
            msg("output.adapter").render(&[("error", localize(error))])
        )
        .map_err(|_| english("error.adapter_output"))?;
    }
    Ok(())
}

fn pi_failure(action: &str, error: &str, json: bool) -> Result<Option<u8>, CliError> {
    if json {
        write_json(&serde_json::json!({"action":action,"outcome":"incomplete","error":error}))?;
    } else {
        writeln!(io::stderr().lock(), "{}", localize(error))
            .map_err(|_| english("error.pi_output"))?;
    }
    Ok(Some(3))
}

fn validate_target_arguments(
    software: Option<&str>,
    managed_root: Option<&Path>,
    pi: &PiArgs,
) -> Result<(), CliError> {
    if pi.supplied() && software != Some("pi") {
        return Err(english("error.pi_target").into());
    }
    if managed_root.is_some() && software != Some("cc-switch") {
        return Err(english("error.managed_target").into());
    }
    Ok(())
}

fn validate_operation_arguments(name: &str, args: &OperationArgs) -> Result<(), CliError> {
    validate_target_arguments(
        args.target.software.as_deref(),
        args.target.managed_root.as_deref(),
        &args.target.pi,
    )?;
    let managed_target =
        args.target.software.as_deref() == Some("cc-switch") || (name == "update" && args.all);
    if !managed_target
        && (args.application.supplied()
            || args.configuration
            || args.plan_id.is_some()
            || args.change.is_some()
            || args.cancel_pending)
    {
        return Err(english("error.managed_arguments").into());
    }
    Ok(())
}

// Adapter flags share command structs, but never an execution meaning. Validate
// them before either adapter can touch a credential store, process or file.
fn validate_adapter_arguments(cli: &Cli) -> Result<(), CliError> {
    match &cli.command {
        Command::Status(target) => validate_target_arguments(
            target.software.as_deref(),
            target.managed_root.as_deref(),
            &target.pi,
        ),
        Command::Doctor(args) => validate_target_arguments(
            args.target.software.as_deref(),
            args.target.managed_root.as_deref(),
            &args.target.pi,
        ),
        Command::Setup(args) => {
            validate_operation_arguments("setup", &args.operation)?;
            if args.operation.target.software.as_deref() != Some("cc-switch")
                && (args.model_id.is_some()
                    || args.display_name != "LMM"
                    || args.no_browser
                    || args.timeout != 180)
            {
                return Err(english("error.application_arguments").into());
            }
            Ok(())
        }
        Command::Install(args) => validate_operation_arguments("install", args),
        Command::Sync(args) => validate_operation_arguments("sync", args),
        Command::Update(args) => validate_operation_arguments("update", args),
        Command::Restore(args) => validate_operation_arguments("restore", args),
        Command::Uninstall(args) => validate_operation_arguments("uninstall", args),
        Command::Disconnect(args) | Command::Run(args) => {
            validate_target_arguments(
                Some(args.software.as_str()),
                args.managed_root.as_deref(),
                &args.pi,
            )?;
            if args.software == "pi" && args.application.supplied() {
                return Err(english("error.pi_application_grant").into());
            }
            if args.software != "pi" && (args.dry_run || args.yes) {
                return Err(english("error.pi_run_flags").into());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn execute_pi(cli: &Cli) -> Result<Option<u8>, CliError> {
    let target_owned;
    let (action, target, dry_run, yes, activate) = match &cli.command {
        Command::Setup(args) => (
            "setup",
            &args.operation.target,
            args.operation.dry_run,
            args.operation.yes,
            args.activate,
        ),
        Command::Install(args) => ("install", &args.target, args.dry_run, args.yes, false),
        Command::Update(args) => ("update", &args.target, args.dry_run, args.yes, false),
        Command::Restore(args) => ("restore", &args.target, args.dry_run, args.yes, false),
        Command::Sync(args) => ("sync", &args.target, args.dry_run, args.yes, false),
        Command::Disconnect(args) | Command::Run(args) => {
            target_owned = TargetArgs {
                software: Some(args.software.clone()),
                instance: args.instance.clone(),
                managed_root: args.managed_root.clone(),
                pi: PiArgs {
                    node: args.pi.node.clone(),
                    pi_package: args.pi.pi_package.clone(),
                    agent_dir: args.pi.agent_dir.clone(),
                },
            };
            (
                if matches!(&cli.command, Command::Run(_)) {
                    "run"
                } else {
                    "disconnect"
                },
                &target_owned,
                args.dry_run,
                args.yes,
                false,
            )
        }
        _ => return Ok(None),
    };
    if target.software.as_deref() != Some("pi") {
        return Ok(None);
    }
    if activate {
        return Err(english("error.pi_activation").into());
    }
    let (Some(node), Some(sdk), Some(project), Some(agent_dir)) = (
        &target.pi.node,
        &target.pi.pi_package,
        &target.instance,
        &target.pi.agent_dir,
    ) else {
        if target.pi.node.is_some()
            || target.pi.pi_package.is_some()
            || target.pi.agent_dir.is_some()
        {
            return Err(english("error.pi_adapter_paths").into());
        }
        return Ok(None);
    };
    let target = pi::Target {
        node,
        sdk,
        project,
        agent_dir,
    };
    if action == "run" && !dry_run {
        if cli.json
            || cli.non_interactive
            || !io::stdin().is_terminal()
            || !io::stderr().is_terminal()
        {
            return Err(english("error.pi_run_interactive").into());
        }
        return match target.run() {
            Ok(code) => Ok(Some(code)),
            Err(error) => pi_failure(action, &error, cli.json),
        };
    }
    let outcome = match target.execute(action, dry_run, yes) {
        Ok(outcome) => outcome,
        Err(error) => return pi_failure(action, &error, cli.json),
    };
    write_json(&outcome)?;
    Ok(Some(
        if !dry_run && matches!(action, "setup" | "sync" | "disconnect") {
            3
        } else {
            0
        },
    ))
}

fn execute(cli: &Cli) -> Result<u8, CliError> {
    validate_adapter_arguments(cli)?;
    if let Command::Credential(args) = &cli.command {
        if cli.non_interactive {
            return Err(english("error.credential_unattended").into());
        }
        return applications_cli::credential(args, cli.json).map_err(Into::into);
    }
    let application_vault = match &cli.command {
        Command::Authorize(_) => true,
        Command::Setup(args) => args.operation.target.software.as_deref() == Some("cc-switch"),
        Command::Sync(args) => args.target.software.as_deref() == Some("cc-switch"),
        Command::Disconnect(args) => args.software == "cc-switch",
        _ => false,
    };
    if cli.non_interactive && application_vault {
        return Err(english("error.application_unattended").into());
    }
    if cli.non_interactive
        && matches!(
            &cli.command,
            Command::Login(_) | Command::Logout(_) | Command::Models(_) | Command::Balance(_)
        )
    {
        return Err(msg("error.auth_unattended").into());
    }
    if let Some(code) = execute_pi(cli)? {
        return Ok(code);
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
                            ("installation", &format!("{:?}", software.installation)),
                            ("integration", &format!("{:?}", software.integration)),
                            ("maintenance", &format!("{:?}", software.maintenance)),
                        ])
                    )
                    .map_err(|_| msg("error.catalog_output"))?;
                }
            }
            return Ok(0);
        }
        Command::Status(target) => {
            let mut report = readonly_report("status", target, &context)?;
            enrich_cc_switch(&mut report, target);
            report
        }
        Command::Doctor(args) => {
            let mut report = readonly_report("doctor", &args.target, &context)?;
            report.outcome = "incomplete";
            report.notes.push(if report.pi_settings.is_some() {
                msg("note.pi_doctor_incomplete")
            } else {
                msg("note.doctor_incomplete")
            });
            enrich_cc_switch(&mut report, &args.target);
            if args.fix {
                report.outcome = "blocked";
                report.notes.push(msg("note.fix_blocked"));
            }
            if args.report {
                report.redact();
            }
            report
        }
        Command::Authorize(args) => {
            return applications_cli::authorize(args, interactive, cli.json).map_err(Into::into);
        }
        Command::Receipt(args) => {
            return applications_cli::receipt(args, interactive, cli.json).map_err(Into::into);
        }
        Command::Credential(args) => {
            return applications_cli::credential(args, cli.json).map_err(Into::into);
        }
        Command::Setup(args) => {
            if args.operation.target.software.as_deref() == Some("cc-switch") {
                return applications_cli::operation(
                    "setup",
                    &args.operation,
                    Some(args),
                    interactive,
                    cli.json,
                )
                .map_err(Into::into);
            }
            operation_report(
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
            )?
        }
        Command::Install(args) => {
            if args.target.software.as_deref() == Some("cc-switch") {
                return managed_operation(
                    "install",
                    ManagedAction::Install,
                    args,
                    interactive,
                    cli.json,
                );
            }
            operation_report("install", args, Intent::AddOnly, interactive, &context)?
        }
        Command::Sync(args) => {
            if args.target.software.as_deref() == Some("cc-switch") {
                return applications_cli::operation("sync", args, None, interactive, cli.json)
                    .map_err(Into::into);
            }
            operation_report("sync", args, Intent::Maintain, interactive, &context)?
        }
        Command::Update(args) => {
            if args.target.software.as_deref() == Some("cc-switch") || args.all {
                return managed_operation(
                    "update",
                    ManagedAction::Update,
                    args,
                    interactive,
                    cli.json,
                );
            }
            operation_report("update", args, Intent::Maintain, interactive, &context)?
        }
        Command::Restore(args) => {
            if args.target.software.as_deref() == Some("cc-switch") && args.configuration {
                return applications_cli::operation("restore", args, None, interactive, cli.json)
                    .map_err(Into::into);
            }
            if args.target.software.as_deref() == Some("cc-switch") {
                return managed_operation(
                    "restore",
                    ManagedAction::Restore,
                    args,
                    interactive,
                    cli.json,
                );
            }
            operation_report("restore", args, Intent::Maintain, interactive, &context)?
        }
        Command::Uninstall(args) => {
            if args.target.software.as_deref() == Some("cc-switch") {
                return managed_operation(
                    "uninstall",
                    ManagedAction::Uninstall,
                    args,
                    interactive,
                    cli.json,
                );
            }
            operation_report("uninstall", args, Intent::Maintain, interactive, &context)?
        }
        Command::Login(args) => {
            if !interactive {
                return Err(msg("error.login_interactive").into());
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
        Command::Balance(args) => {
            return match oauth::balance(&args.issuer) {
                Ok(balance) => {
                    if cli.json {
                        write_json(&balance)?;
                    } else {
                        writeln!(
                            io::stdout().lock(),
                            "{}",
                            msg("output.balance").render(&[
                                ("dollars", &balance.balance.to_string()),
                                ("credits", &balance.public_credit_balance.to_string()),
                                ("rate", &balance.credits_per_usd.to_string()),
                            ])
                        )
                        .map_err(|_| english("error.balance_output"))?;
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
                            writeln!(
                                output,
                                "{}",
                                msg("output.model_metadata").render(&[
                                    ("metadata", &format!("{:?}", model.metadata)),
                                    ("cache_read", &format!("{:?}", model.pricing.cache_read)),
                                    ("cache_write", &format!("{:?}", model.pricing.cache_write)),
                                    ("group", &format!("{:?}", model.pricing.group_multiplier)),
                                    ("trust", &format!("{:?}", model.pricing.trust_multiplier)),
                                ])
                            )
                            .map_err(|_| english("error.models_output"))?;
                        }
                    }
                    Ok(0)
                }
                Err(error) => auth_failure(error, cli.json),
            };
        }
        Command::Disconnect(args) | Command::Run(args) => {
            if matches!(&cli.command, Command::Disconnect(_)) && args.software == "cc-switch" {
                return applications_cli::disconnect(args, interactive, cli.json)
                    .map_err(Into::into);
            }
            if args.application.supplied() {
                return Err(english("error.application_flags_unsupported").into());
            }
            let name = match &cli.command {
                Command::Disconnect(_) => "disconnect",
                _ => "run",
            };
            let target = TargetArgs {
                software: Some(args.software.clone()),
                instance: args.instance.clone(),
                managed_root: args.managed_root.clone(),
                pi: PiArgs {
                    node: args.pi.node.clone(),
                    pi_package: args.pi.pi_package.clone(),
                    agent_dir: args.pi.agent_dir.clone(),
                },
            };
            if name == "run" && args.software == "cc-switch" {
                if args.instance.is_some() {
                    return Err(english("error.cc_switch_launch_instance").into());
                }
                let manager = managed_manager(&target)?;
                return match manager.launch() {
                    Ok(pid) => {
                        managed_output(
                            &serde_json::json!({"schema_version":1,"command":"run","outcome":"process_started","pid":pid,"application_health":"not_checked","target_call":"not_checked"}),
                            cli.json,
                        )?;
                        Ok(0)
                    }
                    Err(error) => {
                        managed_output(
                            &serde_json::json!({"schema_version":1,"command":"run","outcome":"blocked","error":error.to_string()}),
                            cli.json,
                        )?;
                        Ok(3)
                    }
                };
            }
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

fn auth_failure(error: oauth::AuthError, json: bool) -> Result<u8, CliError> {
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
            if cli.json && !matches!(&cli.command, Command::Credential(_)) {
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
