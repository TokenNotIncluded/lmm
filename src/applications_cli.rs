//! Independent application commands using the managed issuer/client contract
//! and CC Switch's native SQLite transactions.
use clap::Args;
use lmm::{
    cc_switch,
    oauth::{
        self,
        applications::{self, ApplicationInstance},
    },
};
use rand::{RngCore, rngs::OsRng};
use serde::Serialize;
use std::{
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Args, Default)]
pub(super) struct Scope {
    /// 独立应用实例 ID；不与只读 CLI 登录或其他应用共用
    #[arg(long)]
    pub installation_id: Option<String>,
    /// 已确认的 HTTPS issuer；未指定时沿用该管理记录或官方 origin
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
        writeln!(
            io::stdout().lock(),
            "CC Switch · 独立应用授权与原生数据库；激活和实际调用单独验证"
        )
        .map_err(|_| "无法输出结果")?;
    }
    super::write_json(value)
}
fn instance(issuer: &str, id: &str, label: &str) -> Result<ApplicationInstance, String> {
    ApplicationInstance::new(issuer, id, label).map_err(|error| error.to_string())
}
fn random_id() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| "无法创建应用实例")?;
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
            "请在浏览器确认 {:?} 的独立 CC Switch 授权、分组和消费上限。\n实例：{}\n{}",
            label,
            instance.installation_id(),
            url
        )
        .map_err(|_| oauth::AuthError::Output)?;
        if !no_browser && open::that_detached(url).is_err() {
            writeln!(stderr, "浏览器未自动打开，请在本机浏览器打开上述地址。")
                .map_err(|_| oauth::AuthError::Output)?;
        }
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    Ok(())
}
pub(super) fn authorize(args: &AuthorizeArgs, interactive: bool, json: bool) -> Result<u8, String> {
    if !interactive {
        return Err("应用授权必须由本机浏览器中的用户同意；无人值守不会自动授权".into());
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
        return Err("credential 仅供 Codex 原生凭据管道调用；不能输出为 JSON 或显示在终端".into());
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
        return Err("配置接入需要 cc-switch 和独立 --instance，不能混用受管发行包目录".into());
    }
    let root = target
        .instance
        .as_ref()
        .ok_or("请用 --instance 明确选择 CC Switch 配置根目录")?;
    if !root.is_absolute() {
        return Err("--instance 必须是绝对路径".into());
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
            return Err(
                "这个实例有多个 LMM 管理记录；需要明确 --issuer 与 --installation-id".into(),
            );
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
            .map_err(|_| "无法显示应用实例")?;
        }
        drop(stderr);
        choose(matches.len(), "选择要维护的独立应用实例")?
    };
    let chosen = matches[selected];
    instance(&chosen.issuer, &chosen.installation_id, "CC Switch").map(Some)
}
fn choose(count: usize, label: &str) -> Result<usize, String> {
    write!(io::stderr().lock(), "{label}（回车取消）：").map_err(|_| "无法显示选择")?;
    io::stderr().lock().flush().map_err(|_| "无法显示选择")?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|_| "无法读取选择")?;
    line.trim()
        .parse::<usize>()
        .ok()
        .and_then(|value| value.checked_sub(1))
        .filter(|value| *value < count)
        .ok_or("没有选择；已停止操作".into())
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
            Err("所选模型不在这个应用的 Responses 目录中".into())
        };
    }
    if models.is_empty() {
        return Err("这个应用当前没有可选 Responses 模型".into());
    }
    if models.len() == 1 {
        return Ok(models[0].id.clone());
    }
    if !interactive {
        return Err("需要 --model-id 明确选择模型；交互 setup 可直接从当前目录选择".into());
    }
    let mut stderr = io::stderr().lock();
    for (index, model) in models.iter().enumerate() {
        writeln!(
            stderr,
            "{}. {:?} · {:?} · 输入 {:?} / 输出 {:?} / 每次 {:?}（{:?}，{:?}；未知值保留）",
            index + 1,
            model.name,
            model.group,
            model.pricing.input,
            model.pricing.output,
            model.pricing.request,
            model.pricing.currency,
            model.pricing.unit
        )
        .map_err(|_| "无法显示模型")?;
    }
    drop(stderr);
    Ok(models[choose(models.len(), "选择新 LMM 供应商的模型")?]
        .id
        .clone())
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
            Err(
                "状态或目录已改变，或未提供匹配的 --plan-id；请重新预览，未知提交请查询原回执"
                    .into(),
            )
        };
    }
    output(
        &serde_json::json!({"outcome":if args.dry_run {"planned"} else {"confirmation_required"},"binding_plan":planned,
        "next_step":"确认后使用相同参数与 --yes --plan-id；已有未知结果用 receipt 查询原 plan_id"}),
        json,
    )?;
    if args.dry_run || !interactive {
        return Ok(false);
    }
    write!(io::stderr().lock(), "应用上面的精确数据库计划？[y/N] ").map_err(|_| "无法请求确认")?;
    io::stderr().lock().flush().map_err(|_| "无法请求确认")?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|_| "无法读取确认")?;
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
        return Err("应用配置操作需要精确实例；SQLite 无可取消的半提交，--change 和 --configuration 只用于 restore".into());
    }
    if setup.is_some_and(|setup| !setup.add_only || setup.activate) {
        return Err(
            "此适配器支持 --add-only；完整激活必须通过 CC Switch 原生管理入口确认 live/Stack 状态"
                .into(),
        );
    }
    let path = database(&args.target)?;
    let existing = selection(&path, &args.application, interactive)?;
    let application = match existing {
        Some(application) => application,
        None => {
            let setup = setup.ok_or("此实例没有 LMM 管理记录；请先交互 setup --add-only")?;
            if !interactive || args.dry_run || args.yes {
                return Err(
                    "缺少独立应用授权；预览或无人值守不会创建授权，请先交互 setup 或 authorize"
                        .into(),
                );
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
                .ok_or("restore 需要 --change 原配置回执")?,
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
                return Err("重复 setup 保留当前模型；不能用 --model-id 隐式改动原选择".into());
            }
            cc_switch::bindings::preview_sync(&path, &current)
        } else {
            let setup = setup.ok_or("尚未添加 LMM；请先 setup --add-only")?;
            let model = choose_model(&current, setup.model_id.as_deref(), interactive)?;
            let helper = std::env::current_exe().map_err(|_| "无法确定当前 LMM 凭据命令的位置")?;
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
            "recovery":"使用 receipt 查询此 change_id；不要新建操作重复未知提交"}),
                json,
            )?;
            Ok(3)
        }
    }
}
pub(super) fn receipt(args: &ReceiptArgs, interactive: bool, json: bool) -> Result<u8, String> {
    let path = args.instance.join("cc-switch.db");
    let application = selection(&path, &args.scope, interactive)?.ok_or("未找到管理记录")?;
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
            return Err("独立授权撤销不使用受管程序目录".into());
        }
        instance(
            args.application
                .issuer
                .as_deref()
                .unwrap_or(oauth::DEFAULT_ISSUER),
            args.application
                .installation_id
                .as_deref()
                .ok_or("需要 --installation-id 或已配置的 --instance")?,
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
            .ok_or("未找到管理记录；独立授权需要 --installation-id 与 --issuer")?
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
