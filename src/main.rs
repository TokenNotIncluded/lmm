mod applications_cli;

use clap::{Args, Parser, Subcommand};
use lmm::{
    catalog, cc_switch,
    discovery::{self, ProbeContext, TargetStatus},
    environment::{self, Environment},
    managed::{self, Action as ManagedAction},
    oauth, pi,
    plan::{Intent, Preview},
    provider,
};
use serde::Serialize;
use std::{
    env,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Parser)]
#[command(
    name = "lmm",
    version,
    about = "LMM 安装与接入工具（CC Switch 受管安装与独立授权、Pi 原生适配器）"
)]
struct Cli {
    /// 输出结构化 JSON；不交互、不输出配置内容
    #[arg(long, global = true)]
    json: bool,
    /// 无人值守：缺少目标或授权时直接返回
    #[arg(long, global = true)]
    non_interactive: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 搜索内置服务商预设快照；不读取配置、凭据或网络
    Presets { query: Option<String> },
    /// 搜索软件目录；目录条目不代表已验证兼容
    Catalog { query: Option<String> },
    /// 规划完整接入；当前版本在未具备适配器时明确阻止写入
    Setup(SetupArgs),
    /// 在浏览器中授予独立应用权限；不会复用只读 CLI 登录
    Authorize(applications_cli::AuthorizeArgs),
    /// 查询原生配置操作的不可变回执；不重发写入
    Receipt(applications_cli::ReceiptArgs),
    /// 仅供 Codex 原生 auth.command 凭据管道
    #[command(hide = true)]
    Credential(applications_cli::CredentialArgs),
    /// 安装软件；不要求 LMM 登录
    Install(OperationArgs),
    /// 通过浏览器登录，凭据保存在系统凭据库
    Login(LoginArgs),
    /// 退出 CLI；不撤销独立应用授权
    Logout(AuthArgs),
    /// 读取当前授权模型与计费信息，不发起模型调用或更改选择
    Models(AuthArgs),
    /// 读取当前账号余额；保留服务端整数积分和美元单位
    Balance(AuthArgs),
    /// 只读检查环境、安装线索与各层验证状态
    Status(TargetArgs),
    /// 同步 LMM 信息，保留当前服务商与模型
    Sync(OperationArgs),
    /// 更新明确纳入管理的软件
    Update(OperationArgs),
    /// 只读诊断；--fix 仍受适配器能力与确认范围限制
    Doctor(DoctorArgs),
    /// 按字段恢复 LMM 变更，保留后续修改
    Restore(OperationArgs),
    /// 解除指定软件接入并撤销对应授权
    Disconnect(RequiredTarget),
    /// 卸载已确认纳入管理的软件，默认保留数据
    Uninstall(OperationArgs),
    /// 按当前配置启动指定软件
    Run(RequiredTarget),
}

#[derive(Args)]
struct AuthArgs {
    /// 信任的 LMM HTTPS origin（自托管时显式指定）
    #[arg(long, default_value = oauth::DEFAULT_ISSUER)]
    issuer: String,
}

#[derive(Args)]
struct LoginArgs {
    #[command(flatten)]
    auth: AuthArgs,
    /// 只显示登录链接；仍需要浏览器能访问当前机器的回环端口
    #[arg(long)]
    no_browser: bool,
    /// 浏览器回调等待上限（秒）
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(10..=600))]
    timeout: u64,
}

#[derive(Args)]
struct PiArgs {
    /// 显式 Node 可执行文件绝对路径（仅 Pi 适配器）
    #[arg(long)]
    node: Option<PathBuf>,
    /// 已安装官方 Pi 1.2.0 npm 包根目录（仅 Pi 适配器）
    #[arg(long)]
    pi_package: Option<PathBuf>,
    /// 显式 Pi agent 目录；不会猜测或读取其他实例
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
    /// 精确的软件目录 ID，例如 astrbot 或 cc-switch
    software: Option<String>,
    /// 明确的本地实例根目录；不自动扫描远程或 Docker 实例
    #[arg(long, requires = "software")]
    instance: Option<PathBuf>,
    /// LMM 拥有的程序目录；与 --instance 指定的应用数据目录分开
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
    /// restore 还原原生供应商目录；省略时还原受管程序版本
    #[arg(long)]
    configuration: bool,
    /// 仅包含当前环境中已安装且受支持的目标；更新还要求管理授权
    #[arg(long, conflicts_with_all = ["software", "instance", "managed_root"])]
    all: bool,
    /// 只生成计划，不执行修改
    #[arg(long)]
    dry_run: bool,
    /// 确认已展示范围；不代表允许切换账号、服务商或付费测试
    #[arg(long)]
    yes: bool,
    /// 确认先前预览的精确计划；状态改变时拒绝执行
    #[arg(long, requires = "yes")]
    plan_id: Option<String>,
    /// restore 要撤销的精确 change_id，可从 status 的 history 获取
    #[arg(long)]
    change: Option<String>,
    /// 取消仍未提交且未删除原文件的 pending 操作；必须确认原 plan_id
    #[arg(long, requires_all = ["yes", "plan_id"], conflicts_with = "dry_run")]
    cancel_pending: bool,
}

#[derive(Args)]
struct SetupArgs {
    #[command(flatten)]
    operation: OperationArgs,
    /// 无人值守时的精确目录模型；交互模式可从目录直接选择
    #[arg(long)]
    model_id: Option<String>,
    #[arg(long, default_value = "LMM")]
    display_name: String,
    #[arg(long)]
    no_browser: bool,
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(10..=600))]
    timeout: u64,
    /// 仅添加 LMM，保留当前选择
    #[arg(long, conflicts_with = "activate")]
    add_only: bool,
    /// 明确请求添加并切换到 LMM（不表示已执行）
    #[arg(long)]
    activate: bool,
}

#[derive(Args)]
struct DoctorArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// 请求修复已获允许的 LMM 配置；不会绕过能力检查
    #[arg(long)]
    fix: bool,
    /// 生成可主动分享的摘要，去除路径和配置内容
    #[arg(long)]
    report: bool,
}

#[derive(Args)]
struct RequiredTarget {
    software: String,
    #[command(flatten)]
    application: applications_cli::Scope,
    #[arg(long)]
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
    notes: Vec<&'static str>,
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

fn inspect(target: &TargetArgs, context: &ProbeContext) -> Result<Vec<TargetStatus>, String> {
    if target.managed_root.is_some() && target.software.as_deref() != Some("cc-switch") {
        return Err("--managed-root 当前只支持 CC Switch 的受管发行包".into());
    }
    if target
        .instance
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err("--instance 必须是当前环境的绝对本地路径；远程与容器适配器尚未提供".into());
    }
    match target.software.as_deref() {
        Some(id) => {
            let software = catalog::find(id)
                .ok_or_else(|| "未知软件 ID；运行 lmm catalog 查看精确名称".to_owned())?;
            if target.instance.is_some() && !matches!(id, "astrbot" | "cc-switch" | "pi") {
                return Err("此软件尚未实现实例选择，不能忽略 --instance 后继续操作".into());
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
) -> Result<Report, String> {
    let mut report = Report::new(name, inspect(target, context)?);
    let requested =
        target.pi.node.is_some() || target.pi.pi_package.is_some() || target.pi.agent_dir.is_some();
    if !requested {
        return Ok(report);
    }
    if target.software.as_deref() != Some("pi") {
        return Err("Pi 路径参数只适用于明确选择的 pi 实例".into());
    }
    let (Some(project), Some(sdk), Some(agent)) = (
        &target.instance,
        &target.pi.pi_package,
        &target.pi.agent_dir,
    ) else {
        return Err(
            "Pi 只读检查需要 --instance、--pi-package、--agent-dir 的绝对路径；不需要 --node"
                .into(),
        );
    };
    if ![project, sdk, agent].iter().all(|path| path.is_absolute())
        || target
            .pi
            .node
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
    {
        return Err("Pi 路径必须是当前环境的绝对本地路径".into());
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
    report.notes.push("仅解析所选 Pi 1.2.0 设置中的已知包引用与默认选择，不输出配置值；不执行 Node 或 Pi，不读取凭据。");
    report.notes.push("两个设置文件分别读取；代码安装、项目信任、会话覆盖、配置管理归属及实际路由未验证。项目默认选择仅描述信任该项目设置时的覆盖规则。");
    report.pi_settings = Some(inspection);
    Ok(report)
}

fn select_software() -> Result<String, String> {
    let mut output = io::stderr().lock();
    writeln!(output, "选择要规划接入的软件（当前均未完成集成验证）：")
        .map_err(|_| "无法显示选择菜单")?;
    for (i, software) in catalog::SOFTWARE.iter().enumerate() {
        writeln!(output, "  {}. {} ({})", i + 1, software.name, software.id)
            .map_err(|_| "无法显示选择菜单")?;
    }
    write!(output, "输入序号，或直接回车取消：").map_err(|_| "无法显示选择菜单")?;
    output.flush().map_err(|_| "无法显示选择菜单")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|_| "无法读取选择")?;
    let index = input
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1));
    index
        .and_then(|index| catalog::SOFTWARE.get(index))
        .map(|software| software.id.to_owned())
        .ok_or_else(|| "未选择目标；没有执行任何修改".to_owned())
}

fn operation_report(
    name: &'static str,
    args: &OperationArgs,
    intent: Intent,
    interactive: bool,
    context: &ProbeContext,
) -> Result<Report, String> {
    if args.application.supplied() || args.configuration {
        return Err("此目标尚未提供独立应用配置适配器，不能忽略授权参数继续执行".into());
    }
    let selected;
    let target = if args.target.software.is_none() && !args.all {
        if !interactive {
            return Err("无人值守模式需要明确软件或 --all；没有执行任何修改".into());
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
            preview.blockers = vec!["缺少经过验证的软件版本、下载来源校验及安装适配器"];
        } else if name == "restore" {
            preview.account = "not_required";
            preview.blockers = vec!["尚未接入目标的变更日志与原生事务适配器，不能直接覆盖备份"];
        }
        report.plans.push(preview);
    }
    if args.all {
        report.notes.push(
            "当前版本没有经过验证的自动接入适配器，--all 没有可执行目标；未安装或修改任何软件",
        );
    }
    if args.dry_run {
        report
            .notes
            .push("仅规划；阻塞项仍需解决，不能作为接入完成的证据");
    }
    if args.yes {
        report.notes.push("--yes 不会跳过缺失能力、授权或费用限制");
    }
    if name == "install" {
        report
            .notes
            .push("安装本身不需要 LMM 登录；当前缺少经过验证的安装适配器");
    }
    Ok(report)
}

fn write_json(value: &impl Serialize) -> Result<(), String> {
    let mut output = io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, value).map_err(|_| "无法输出 JSON".to_owned())?;
    writeln!(output).map_err(|_| "无法输出 JSON".to_owned())
}

fn managed_manager(target: &TargetArgs) -> Result<managed::Manager, String> {
    if target
        .instance
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err("--instance 必须是绝对本地路径".into());
    }
    let database = target
        .instance
        .as_ref()
        .map(|root| root.join("cc-switch.db"))
        .map_or_else(managed::default_database, Ok)
        .map_err(|error| error.to_string())?;
    managed::Manager::for_current_environment(target.managed_root.clone(), Some(database))
        .map_err(|error| error.to_string())
}

fn managed_output(value: &impl Serialize, json: bool) -> Result<(), String> {
    // Plans contain source, digest, exact scope and receipt IDs. The same
    // structured preview is available in terminals and unattended workflows.
    if !json {
        writeln!(
            io::stdout().lock(),
            "CC Switch · LMM 受管发行包；应用配置与调用验证独立报告"
        )
        .map_err(|_| "无法输出计划")?;
    }
    write_json(value)
}

fn managed_operation(
    name: &'static str,
    action: ManagedAction,
    args: &OperationArgs,
    interactive: bool,
    json: bool,
) -> Result<u8, String> {
    if args.application.supplied() || args.configuration {
        return Err("程序生命周期不接收应用授权参数；恢复配置请用 restore --configuration".into());
    }
    if action != ManagedAction::Restore && args.change.is_some() {
        return Err("--change 只用于 restore".into());
    }
    if action == ManagedAction::Restore && args.change.is_none() {
        return Err(
            "restore 需要 --change 指定 status history 中的变更回执，不能覆盖整个备份".into(),
        );
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
            "next_step":"确认计划后用相同参数加 --yes --plan-id <plan_id>；--dry-run 不执行下载或修改"
        }),
        json,
    )?;
    if args.dry_run {
        return Ok(0);
    }
    if !interactive || args.yes {
        return Ok(3);
    }
    write!(io::stderr().lock(), "应用上面的精确计划？[y/N] ").map_err(|_| "无法请求确认")?;
    io::stderr().lock().flush().map_err(|_| "无法请求确认")?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|_| "无法读取确认")?;
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
    match managed_manager(target)
        .and_then(|manager| manager.status().map_err(|error| error.to_string()))
    {
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
            report.adapter_error = Some(error);
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
    .map_err(|_| "无法输出结果")?;
    writeln!(
        output,
        "范围：当前用户、当前运行环境；检查时间：{}（Unix 秒）",
        report.checked_at
    )
    .map_err(|_| "无法输出结果")?;
    for target in &report.targets {
        writeln!(
            output,
            "\n{}：安装 {}；安装管理者 {}；配置管理者 {}",
            target.software,
            target.installation,
            target.installation_manager,
            target.configuration_manager
        )
        .map_err(|_| "无法输出结果")?;
        for evidence in &target.evidence {
            // Debug formatting escapes control characters in user-controlled paths.
            writeln!(
                output,
                "  {:?}: {:?}（仅路径线索）",
                evidence.path, evidence.observation
            )
            .map_err(|_| "无法输出结果")?;
        }
        if let Some(settings) = &report.pi_settings {
            writeln!(
                output,
                "  设置中的 LMM 引用/选择：{} / {}；默认模型：{}（{}）\n  配置检查：{}；诊断：{}\n  授权、连通性、目标调用、流式及工具调用：未检查",
                target.lmm_added, target.lmm_enabled, settings.default_model,
                settings.model_scope, settings.configuration, settings.diagnostic,
            )
            .map_err(|_| "无法输出结果")?;
        } else {
            writeln!(
                output,
                "  LMM 添加/启用：未知；授权、连通性、目标调用、流式及工具调用：未检查"
            )
            .map_err(|_| "无法输出结果")?;
        }
    }
    for plan in &report.plans {
        writeln!(
            output,
            "\n{} 计划已阻止：无配置修改、无付费调用、无重启",
            plan.software
        )
        .map_err(|_| "无法输出结果")?;
        for blocker in &plan.blockers {
            writeln!(output, "  - {blocker}").map_err(|_| "无法输出结果")?;
        }
    }
    for note in &report.notes {
        writeln!(output, "\n{note}").map_err(|_| "无法输出结果")?;
    }
    if let Some(status) = &report.managed_installation {
        writeln!(
            output,
            "\n受管发行包：{}，文件校验 {}，程序健康 {}",
            status.installation, status.binary_integrity, status.application_health
        )
        .map_err(|_| "无法输出受管状态")?;
        for receipt in &status.history {
            writeln!(
                output,
                "  {:?} · change_id={} · plan_id={}",
                receipt.action, receipt.change_id, receipt.plan_id
            )
            .map_err(|_| "无法输出变更回执")?;
        }
    }
    if let Some(status) = &report.cc_switch_database {
        writeln!(
            output,
            "\nCC Switch schema {}：{} 个服务商记录；当前应用选择与实际路由尚未验证",
            status.schema_version,
            status.providers.len()
        )
        .map_err(|_| "无法输出数据库检查")?;
    }
    if let Some(error) = &report.adapter_error {
        writeln!(output, "\n适配器：{error}").map_err(|_| "无法输出适配器结果")?;
    }
    Ok(())
}

fn pi_failure(action: &str, error: &str, json: bool) -> Result<Option<u8>, String> {
    if json {
        write_json(&serde_json::json!({"action":action,"outcome":"incomplete","error":error}))?;
    } else {
        writeln!(io::stderr().lock(), "{error}").map_err(|_| "无法输出 Pi 执行结果")?;
    }
    Ok(Some(3))
}

fn validate_target_arguments(
    software: Option<&str>,
    managed_root: Option<&Path>,
    pi: &PiArgs,
) -> Result<(), String> {
    if pi.supplied() && software != Some("pi") {
        return Err("Pi 路径参数只适用于明确选择的 pi 实例，不能被其他适配器忽略".into());
    }
    if managed_root.is_some() && software != Some("cc-switch") {
        return Err("--managed-root 当前只支持 CC Switch 的受管发行包".into());
    }
    Ok(())
}

fn validate_operation_arguments(name: &str, args: &OperationArgs) -> Result<(), String> {
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
        return Err(
            "独立应用授权、配置恢复与受管回执参数只适用于 CC Switch，不能在其他目标上忽略".into(),
        );
    }
    Ok(())
}

// Adapter flags share command structs, but never an execution meaning. Validate
// them before either adapter can touch a credential store, process or file.
fn validate_adapter_arguments(cli: &Cli) -> Result<(), String> {
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
                return Err("应用模型选择和浏览器授权参数只适用于明确选择的 CC Switch 实例".into());
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
                return Err("Pi 在目标应用内管理授权，不接受 CC Switch 的授权实例参数".into());
            }
            if args.software != "pi" && (args.dry_run || args.yes) {
                return Err("run/disconnect 的 --dry-run 和 --yes 当前只适用于 Pi，未执行其他目标的启动或撤销".into());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn execute_pi(cli: &Cli) -> Result<Option<u8>, String> {
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
        return Err("Pi 激活需要在目标 Pi 的 /login 和 /model 完成明确授权和选择；可先 --add-only 安装 Provider".into());
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
            return Err(
                "Pi 适配器需要同时指定 --instance、--node、--pi-package、--agent-dir 的绝对路径"
                    .into(),
            );
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
            return Err("Pi run 需要交互终端；不会在 JSON 或无人值守模式暗中启动应用".into());
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

fn execute(cli: &Cli) -> Result<u8, String> {
    validate_adapter_arguments(cli)?;
    if let Command::Credential(args) = &cli.command {
        if cli.non_interactive {
            return Err("系统凭据库可能要求解锁；credential 不接受 --non-interactive".into());
        }
        return applications_cli::credential(args, cli.json);
    }
    let application_vault = match &cli.command {
        Command::Authorize(_) => true,
        Command::Setup(args) => args.operation.target.software.as_deref() == Some("cc-switch"),
        Command::Sync(args) => args.target.software.as_deref() == Some("cc-switch"),
        Command::Disconnect(args) => args.software == "cc-switch",
        _ => false,
    };
    if cli.non_interactive && application_vault {
        return Err(
            "系统凭据库可能需要解锁；这些应用命令暂不接受 --non-interactive，未访问凭据或网络"
                .into(),
        );
    }
    if cli.non_interactive
        && matches!(
            &cli.command,
            Command::Login(_) | Command::Logout(_) | Command::Models(_) | Command::Balance(_)
        )
    {
        return Err("当前系统凭据库可能需要解锁交互；OAuth 命令尚不支持 --non-interactive，未访问凭据或网络".into());
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
                .map_err(|_| "内置服务商预设无法读取".to_owned())?;
            if cli.json {
                write_json(&snapshot)?;
            } else {
                let mut output = io::stdout().lock();
                writeln!(
                    output,
                    "内置服务商预设（Magpie 快照 {}）；配置与连通性未检查，未添加服务商。",
                    provider::UPSTREAM_REVISION
                )
                .map_err(|_| "无法输出服务商预设")?;
                for preset in &snapshot.presets {
                    writeln!(
                        output,
                        "{} — {} ({})",
                        preset.id,
                        preset.name,
                        preset.kind.as_str()
                    )
                    .map_err(|_| "无法输出服务商预设")?;
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
                    writeln!(output, "{} — {}\n  {}\n  来源：{}\n  安装：{:?}；接入：{:?}；维护：{:?}；真实应用兼容场景：未验证\n", software.id, software.name, software.purpose, software.source, software.installation, software.integration, software.maintenance).map_err(|_| "无法输出目录")?;
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
                "诊断未完成：只读设置检查不验证目标应用加载、授权、连通性或真实调用；没有执行网络或付费测试"
            } else {
                "诊断未完成：仅检查本地路径线索；请核对目标实例、版本及配置管理关系。没有执行网络或付费测试"
            });
            enrich_cc_switch(&mut report, &args.target);
            if args.fix {
                report.outcome = "blocked";
                report
                    .notes
                    .push("当前没有经过验证的修复适配器；未修改配置");
            }
            if args.report {
                report.redact();
            }
            report
        }
        Command::Authorize(args) => {
            return applications_cli::authorize(args, interactive, cli.json);
        }
        Command::Receipt(args) => return applications_cli::receipt(args, interactive, cli.json),
        Command::Credential(args) => return applications_cli::credential(args, cli.json),
        Command::Setup(args) => {
            if args.operation.target.software.as_deref() == Some("cc-switch") {
                return applications_cli::operation(
                    "setup",
                    &args.operation,
                    Some(args),
                    interactive,
                    cli.json,
                );
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
                return applications_cli::operation("sync", args, None, interactive, cli.json);
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
                return applications_cli::operation("restore", args, None, interactive, cli.json);
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
                return Err("登录需要浏览器授权；请在交互终端运行 lmm login。无人值守模式不会等待或代替用户同意".into());
            }
            let result = oauth::login(
                &args.auth.issuer,
                std::time::Duration::from_secs(args.timeout),
                |url| {
                    let mut output = io::stderr().lock();
                    writeln!(
                        output,
                        "请在浏览器确认 LMM CLI 授权（仅目录和余额读取，不迁移任何应用）：\n{url}"
                    )
                    .map_err(|_| oauth::AuthError::Output)?;
                    if !args.no_browser && open::that_detached(url).is_err() {
                        writeln!(output, "浏览器未能自动打开，请在本机浏览器打开上述链接。")
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
                        writeln!(io::stdout().lock(), "LMM CLI 已登录，凭据已保存到系统凭据库。尚未授予应用调用权限；可运行 lmm models。")
                            .map_err(|_| "无法输出登录结果")?;
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
                        writeln!(
                            io::stdout().lock(),
                            "CLI 已退出；其他应用授权未撤销，当前版本尚不能列出这些授权。"
                        )
                        .map_err(|_| "无法输出退出结果")?;
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
                            "余额：{} USD（{} 积分；每美元 {} 积分）",
                            balance.balance,
                            balance.public_credit_balance,
                            balance.credits_per_usd
                        )
                        .map_err(|_| "无法输出余额")?;
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
                        writeln!(
                            output,
                            "当前授权模型（不代表目标软件兼容；价格可能随使用变化）："
                        )
                        .map_err(|_| "无法输出模型")?;
                        for model in &catalog.models {
                            writeln!(output, "{:?} · {:?} · {:?}\n  价格单位：{:?} / {:?}；输入：{:?}，输出：{:?}，每次请求：{:?}；依据：{:?}\n  null/None 表示未知；未调用或切换模型。", model.id, model.upstream_model, model.group, model.pricing.currency, model.pricing.unit, model.pricing.input, model.pricing.output, model.pricing.request, model.pricing.price_basis).map_err(|_| "无法输出模型")?;
                        }
                    }
                    Ok(0)
                }
                Err(error) => auth_failure(error, cli.json),
            };
        }
        Command::Disconnect(args) | Command::Run(args) => {
            if matches!(&cli.command, Command::Disconnect(_)) && args.software == "cc-switch" {
                return applications_cli::disconnect(args, interactive, cli.json);
            }
            if args.application.supplied() {
                return Err("此操作不支持应用授权参数".into());
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
                    return Err(
                        "CC Switch 使用其原生实例设置启动；此版本不能用 --instance 改写其启动配置"
                            .into(),
                    );
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
            report
                .notes
                .push("未实现该目标的安全执行适配器；未启动或卸载软件，未修改配置或撤销授权");
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
            &serde_json::json!({"schema_version":1,"outcome":"blocked","error":error.to_string()}),
        )?;
    } else {
        eprintln!("lmm: {error}");
    }
    Ok(3)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if cli.json && !matches!(&cli.command, Command::Credential(_)) {
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
