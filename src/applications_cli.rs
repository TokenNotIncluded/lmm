//! Independent application commands using the managed issuer/client contract
//! and CC Switch's native SQLite transactions.
use clap::Args;
use lmm::{
    cc_switch,
    i18n::{english, msg, text},
    oauth::{
        self,
        applications::{self, ApplicationInstance},
    },
};
use rand::{TryRng, rngs::SysRng};
use serde::Serialize;
use std::{
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Args, Default)]
pub(super) struct Scope {
    #[arg(help = text("arg.installation_id"))]
    #[arg(long)]
    pub installation_id: Option<String>,
    #[arg(help = text("arg.application_issuer"))]
    #[arg(long)]
    pub issuer: Option<String>,
}
impl Scope {
    pub fn supplied(&self) -> bool {
        self.installation_id.is_some() || self.issuer.is_some()
    }
}
#[derive(Args)]
pub(super) struct AuthorizeArgs {
    #[arg(value_parser = ["cc-switch"])]
    pub software: String,
    #[command(flatten)]
    pub scope: Scope,
    #[arg(long, default_value = "CC Switch on this device")]
    pub display_name: String,
    #[arg(long)]
    pub no_browser: bool,
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(10..=600))]
    pub timeout: u64,
}
#[derive(Args)]
pub(super) struct CredentialArgs {
    #[arg(value_parser = ["cc-switch"])]
    pub software: String,
    #[arg(long)]
    pub issuer: String,
    #[arg(long)]
    pub installation_id: String,
    #[arg(long)]
    pub grant_id: String,
}
#[derive(Args)]
pub(super) struct ReceiptArgs {
    #[arg(value_parser = ["cc-switch"])]
    pub software: String,
    #[arg(long)]
    pub instance: PathBuf,
    #[command(flatten)]
    pub scope: Scope,
    #[arg(long)]
    pub change: String,
}

fn output(value: &impl Serialize, json: bool) -> Result<(), String> {
    if !json {
        writeln!(io::stdout().lock(), "{}", msg("output.application_heading"))
            .map_err(|_| english("error.output"))?;
    }
    super::write_json(value).map_err(|error| match error {
        super::CliError::Message(message) => lmm::i18n::english(message.key()).to_owned(),
        super::CliError::Adapter(value) => value,
    })
}
fn instance(issuer: &str, id: &str, label: &str) -> Result<ApplicationInstance, String> {
    ApplicationInstance::new(issuer, id, label).map_err(|error| error.to_string())
}
fn random_id() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| english("error.application_id"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn authorize_instance(
    instance: &ApplicationInstance,
    label: &str,
    timeout: u64,
    no_browser: bool,
) -> Result<(), String> {
    applications::authorize(instance, Duration::from_secs(timeout), |url| {
        let mut stderr = io::stderr().lock();
        writeln!(
            stderr,
            "{}",
            msg("output.application_authorization").render(&[
                ("label", &format!("{label:?}")),
                ("instance", instance.installation_id()),
                ("url", url),
            ])
        )
        .map_err(|_| oauth::AuthError::Output)?;
        if !no_browser && open::that_detached(url).is_err() {
            writeln!(stderr, "{}", msg("output.application_browser_failed"))
                .map_err(|_| oauth::AuthError::Output)?;
        }
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    Ok(())
}
pub(super) fn authorize(args: &AuthorizeArgs, interactive: bool, json: bool) -> Result<u8, String> {
    if !interactive {
        return Err(english("error.application_interactive").into());
    }
    let generated;
    let id = match args.scope.installation_id.as_deref() {
        Some(id) => id,
        None => {
            generated = random_id()?;
            &generated
        }
    };
    let issuer = args
        .scope
        .issuer
        .as_deref()
        .unwrap_or(oauth::DEFAULT_ISSUER);
    let application = instance(issuer, id, &args.display_name)?;
    authorize_instance(
        &application,
        &args.display_name,
        args.timeout,
        args.no_browser,
    )?;
    output(
        &serde_json::json!({"outcome":"application_authorized","issuer":issuer,"installation_id":id,"client_id":"lmm-cc-switch","software_configuration_changed":false}),
        json,
    )?;
    Ok(0)
}
pub(super) fn credential(args: &CredentialArgs, json: bool) -> Result<u8, String> {
    if json || io::stdout().is_terminal() {
        return Err(english("error.credential_pipe").into());
    }
    let application = instance(
        &args.issuer,
        &args.installation_id,
        "CC Switch credential pipe",
    )?;
    applications::write_codex_credential(&application, &args.grant_id, &mut io::stdout().lock())
        .map_err(|error| error.to_string())?;
    Ok(0)
}
fn database(target: &super::TargetArgs) -> Result<PathBuf, String> {
    if target.software.as_deref() != Some("cc-switch") || target.managed_root.is_some() {
        return Err(english("error.binding_target").into());
    }
    let root = target
        .instance
        .as_ref()
        .ok_or(english("error.binding_instance"))?;
    if !root.is_absolute() {
        return Err(english("error.instance_absolute").into());
    }
    Ok(root.join("cc-switch.db"))
}
fn selection(
    path: &Path,
    scope: &Scope,
    interactive: bool,
) -> Result<Option<ApplicationInstance>, String> {
    let records = cc_switch::bindings::installations(path).map_err(|error| error.to_string())?;
    let matches = records
        .iter()
        .filter(|entry| {
            scope
                .issuer
                .as_ref()
                .is_none_or(|issuer| entry.issuer == *issuer)
                && scope
                    .installation_id
                    .as_ref()
                    .is_none_or(|id| entry.installation_id == *id)
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return scope
            .installation_id
            .as_ref()
            .map(|id| {
                instance(
                    scope.issuer.as_deref().unwrap_or(oauth::DEFAULT_ISSUER),
                    id,
                    "CC Switch",
                )
            })
            .transpose();
    }
    let selected = if matches.len() == 1 {
        0
    } else {
        if !interactive {
            return Err(english("error.application_multiple").into());
        }
        let mut stderr = io::stderr().lock();
        for (index, entry) in matches.iter().enumerate() {
            writeln!(
                stderr,
                "{}. {:?} · {:?} · {}",
                index + 1,
                entry.issuer,
                entry.group,
                entry.installation_id
            )
            .map_err(|_| english("error.application_output"))?;
        }
        drop(stderr);
        choose(matches.len(), text("select.application"))?
    };
    let chosen = matches[selected];
    instance(&chosen.issuer, &chosen.installation_id, "CC Switch").map(Some)
}
fn choose(count: usize, label: &str) -> Result<usize, String> {
    write!(
        io::stderr().lock(),
        "{}",
        msg("select.application_prompt").render(&[("label", label)])
    )
    .map_err(|_| english("error.selection_output"))?;
    io::stderr()
        .lock()
        .flush()
        .map_err(|_| english("error.selection_output"))?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|_| english("error.selection_input"))?;
    line.trim()
        .parse::<usize>()
        .ok()
        .and_then(|value| value.checked_sub(1))
        .filter(|value| *value < count)
        .ok_or(english("error.selection_empty").into())
}
fn choose_model(
    proof: &applications::VerifiedCatalog,
    explicit: Option<&str>,
    interactive: bool,
) -> Result<String, String> {
    let models = proof
        .catalog()
        .models
        .iter()
        .filter(|model| model.apis.iter().any(|api| api == "openai-responses"))
        .collect::<Vec<_>>();
    if let Some(id) = explicit {
        return if models.iter().any(|model| model.id == id) {
            Ok(id.into())
        } else {
            Err(english("error.application_model").into())
        };
    }
    if models.is_empty() {
        return Err(english("error.application_no_models").into());
    }
    if models.len() == 1 {
        return Ok(models[0].id.clone());
    }
    if !interactive {
        return Err(english("error.model_required").into());
    }
    let mut stderr = io::stderr().lock();
    for (index, model) in models.iter().enumerate() {
        writeln!(
            stderr,
            "{}",
            msg("output.application_model").render(&[
                ("index", &(index + 1).to_string()),
                ("name", &format!("{:?}", model.name)),
                ("group", &format!("{:?}", model.group)),
                ("input", &format!("{:?}", model.pricing.input)),
                ("output", &format!("{:?}", model.pricing.output)),
                ("request", &format!("{:?}", model.pricing.request)),
                ("currency", &format!("{:?}", model.pricing.currency)),
                ("unit", &format!("{:?}", model.pricing.unit)),
            ])
        )
        .map_err(|_| english("error.model_selection_output"))?;
    }
    drop(stderr);
    Ok(
        models[choose(models.len(), text("select.application_model"))?]
            .id
            .clone(),
    )
}
fn confirmed(
    args: &super::OperationArgs,
    planned: &cc_switch::bindings::BindingPlan,
    interactive: bool,
    json: bool,
) -> Result<bool, String> {
    if args.yes && !args.dry_run {
        return if args.plan_id.as_deref() == Some(&planned.plan_id) {
            Ok(true)
        } else {
            Err(english("error.binding_plan_changed").into())
        };
    }
    output(
        &serde_json::json!({"outcome":if args.dry_run {"planned"} else {"confirmation_required"},"binding_plan":planned,
        "next_step":english("note.binding_next_step")}),
        json,
    )?;
    if args.dry_run || !interactive {
        return Ok(false);
    }
    write!(io::stderr().lock(), "{}", msg("select.binding_confirm"))
        .map_err(|_| english("error.confirm_output"))?;
    io::stderr()
        .lock()
        .flush()
        .map_err(|_| english("error.confirm_output"))?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|_| english("error.confirm_input"))?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}
pub(super) fn operation(
    name: &str,
    args: &super::OperationArgs,
    setup: Option<&super::SetupArgs>,
    interactive: bool,
    json: bool,
) -> Result<u8, String> {
    if args.all
        || args.cancel_pending
        || (name != "restore" && (args.change.is_some() || args.configuration))
    {
        return Err(english("error.binding_scope").into());
    }
    if setup.is_some_and(|setup| !setup.add_only || setup.activate) {
        return Err(english("error.binding_activation").into());
    }
    let path = database(&args.target)?;
    let existing = selection(&path, &args.application, interactive)?;
    let application = match existing {
        Some(application) => application,
        None => {
            let setup = setup.ok_or(english("error.binding_setup_first"))?;
            if !interactive || args.dry_run || args.yes {
                return Err(english("error.application_authorize_first").into());
            }
            let application = instance(
                args.application
                    .issuer
                    .as_deref()
                    .unwrap_or(oauth::DEFAULT_ISSUER),
                &random_id()?,
                &setup.display_name,
            )?;
            authorize_instance(
                &application,
                &setup.display_name,
                setup.timeout,
                setup.no_browser,
            )?;
            output(
                &serde_json::json!({"outcome":"application_authorized","issuer":application.issuer(),"installation_id":application.installation_id(),"software_configuration_changed":false}),
                json,
            )?;
            application
        }
    };
    let mut proof = None;
    let planned = if name == "restore" {
        cc_switch::bindings::preview_restore(
            &path,
            application.issuer(),
            application.installation_id(),
            args.change
                .as_deref()
                .ok_or(english("error.binding_restore_change"))?,
        )
    } else {
        let current = applications::catalog(&application).map_err(|error| error.to_string())?;
        let already = cc_switch::bindings::installations(&path)
            .map_err(|error| error.to_string())?
            .iter()
            .any(|entry| {
                entry.issuer == application.issuer()
                    && entry.installation_id == application.installation_id()
            });
        let result = if already {
            if setup.is_some_and(|setup| setup.model_id.is_some()) {
                return Err(english("error.binding_repeat_model").into());
            }
            cc_switch::bindings::preview_sync(&path, &current)
        } else {
            let setup = setup.ok_or(english("error.binding_not_added"))?;
            let model = choose_model(&current, setup.model_id.as_deref(), interactive)?;
            let helper =
                std::env::current_exe().map_err(|_| english("error.credential_executable"))?;
            cc_switch::bindings::preview_add(&path, &current, &model, &setup.display_name, &helper)
        };
        proof = Some(current);
        result
    }
    .map_err(|error| error.to_string())?;
    if !confirmed(args, &planned, interactive, json)? {
        return Ok(if args.dry_run { 0 } else { 3 });
    }
    // Refresh the server proof after a human may have spent time reviewing.
    if proof.is_some() {
        proof = Some(applications::catalog(&application).map_err(|error| error.to_string())?);
    }
    match cc_switch::bindings::apply(&path, &planned, &planned.plan_id, proof.as_ref()) {
        Ok(receipt) => {
            output(
                &serde_json::json!({"outcome":"database_updated","receipt":receipt,"live_application":"requires_native_activation","model_call":"not_run"}),
                json,
            )?;
            Ok(0)
        }
        Err(error) => {
            output(
                &serde_json::json!({"outcome":"incomplete","error":error.to_string(),"issuer":application.issuer(),"installation_id":application.installation_id(),"change_id":planned.plan_id,
            "recovery":english("note.binding_recovery")}),
                json,
            )?;
            Ok(3)
        }
    }
}
pub(super) fn receipt(args: &ReceiptArgs, interactive: bool, json: bool) -> Result<u8, String> {
    let path = args.instance.join("cc-switch.db");
    let application = selection(&path, &args.scope, interactive)?
        .ok_or(english("error.binding_missing_record"))?;
    let receipt = cc_switch::bindings::receipt(
        &path,
        application.issuer(),
        application.installation_id(),
        &args.change,
    )
    .map_err(|error| error.to_string())?;
    output(
        &serde_json::json!({"outcome":"original_operation_recorded","receipt":receipt,"new_operation_sent":false}),
        json,
    )?;
    Ok(0)
}
pub(super) fn disconnect(
    args: &super::RequiredTarget,
    interactive: bool,
    json: bool,
) -> Result<u8, String> {
    let application = if args.instance.is_none() {
        if args.managed_root.is_some() {
            return Err(english("error.application_revoke_root").into());
        }
        instance(
            args.application
                .issuer
                .as_deref()
                .unwrap_or(oauth::DEFAULT_ISSUER),
            args.application
                .installation_id
                .as_deref()
                .ok_or(english("error.application_instance_required"))?,
            "CC Switch",
        )?
    } else {
        let target = super::TargetArgs {
            software: Some(args.software.clone()),
            instance: args.instance.clone(),
            managed_root: args.managed_root.clone(),
            pi: super::PiArgs {
                node: args.pi.node.clone(),
                pi_package: args.pi.pi_package.clone(),
                agent_dir: args.pi.agent_dir.clone(),
            },
        };
        let path = database(&target)?;
        selection(&path, &args.application, interactive)?
            .ok_or(english("error.application_record_required"))?
    };
    let outcome = applications::revoke(&application).map_err(|error| error.to_string())?;
    let confirmed = matches!(outcome, applications::RevocationOutcome::Revoked);
    output(
        &serde_json::json!({"outcome":outcome,"issuer":application.issuer(),"installation_id":application.installation_id(),
        "server_revocation_confirmed":confirmed,"configuration_retained":true,"native_removal":"use_cc_switch_manager","other_application_grants":"unchanged"}),
        json,
    )?;
    Ok(if confirmed { 0 } else { 3 })
}
