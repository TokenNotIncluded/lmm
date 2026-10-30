//! Verified distributions in an explicitly owned, per-user namespace.
//!
//! This module does not modify CC Switch's providers, application credentials,
//! current selection, PATH or desktop integration. A managed binary is not an
//! assertion that the application has successfully called LMM.

mod files;
mod releases;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_STATE: u64 = 1024 * 1024;
const MAX_EVENTS: usize = 128;

/// Typed refusal or incomplete operation. No response includes secret content.
#[derive(Debug, thiserror::Error)]
pub enum ManagedError {
    #[error("此受管安装适配器仅支持 Linux x86_64 和 aarch64")]
    UnsupportedPlatform,
    #[error("受管目录必须是绝对、私有且不经过符号链接的路径")]
    UnsafePath,
    #[error("该目录不是 LMM 创建的受管安装，不能接管或删除")]
    NotOwned,
    #[error("受管安装日志损坏或版本未知，未执行变更")]
    InvalidState,
    #[error("另一个 LMM 操作正在处理此安装，请稍后重试原计划")]
    Busy,
    #[error("安装状态已在预览之后改变，请重新查看计划")]
    Conflict,
    #[error("受管程序被后续修改，已保留文件；不能覆盖、执行或删除")]
    ModifiedArtifact,
    #[error("尚无受管安装；不能把 PATH 中的程序当作 LMM 安装")]
    NotInstalled,
    #[error("找不到该变更回执，或后续变更与恢复目标冲突")]
    UnknownChange,
    #[error("存在未完成操作；请使用原计划 ID 恢复同一操作")]
    Pending,
    #[error("此更新会降低版本，请使用明确变更回执的恢复流程")]
    Downgrade,
    #[error("本机数据库版本不适合目标程序；没有覆盖或回退用户数据")]
    DatabaseCompatibility,
    #[error("下载失败；变更尚未提交，使用原计划 ID 可继续或取消")]
    Download,
    #[error("下载长度或 SHA-256 不符合固定官方发布记录")]
    Integrity,
    #[error("无法持久保存或读取安装日志；请查看状态并用原计划 ID 查回执")]
    Storage,
    #[error("无法启动已校验程序；请确认图形会话、FUSE 和运行依赖")]
    Launch,
    #[error("操作已移除部分受管文件，必须用原计划完成，不能宣称已取消")]
    CannotCancel,
}

/// A pinned distribution, including the supported native database version.
/// Fields are private so callers cannot turn arbitrary URLs into install plans.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Release {
    version: String,
    source_commit: String,
    download_url: String,
    sha256: String,
    size: u64,
    max_database_version: u32,
}

/// The exact operation to preview and then confirm.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    Update,
    Restore,
    Uninstall,
    Cancel,
}

/// A plan is deterministic for one namespace and one committed generation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManagedPlan {
    pub contract: String,
    pub plan_id: String,
    pub action: Action,
    pub software: String,
    pub managed_root: PathBuf,
    pub expected_generation: u64,
    pub before: Option<Release>,
    pub after: Option<Release>,
    pub restore_change: Option<String>,
    pub remove_artifacts: Vec<String>,
    pub database: Option<PathBuf>,
    pub database_version: Option<u32>,
    pub application_data: String,
    pub login_required: bool,
    pub paid_test: bool,
    pub no_change: bool,
}

/// Immutable, non-secret record of one committed installation change.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedReceipt {
    pub change_id: String,
    pub plan_id: String,
    pub action: Action,
    pub before: Option<Release>,
    pub after: Option<Release>,
    pub committed_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    change_id: String,
    plan: ManagedPlan,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    contract: String,
    software: String,
    arch: String,
    generation: u64,
    current: Option<Release>,
    events: Vec<ManagedReceipt>,
    pending: Option<Pending>,
}

/// Status has separate binary integrity and application verification facts.
#[derive(Serialize)]
pub struct ManagedStatus {
    pub contract: &'static str,
    pub software: &'static str,
    pub managed_root: PathBuf,
    pub generation: u64,
    pub installation: &'static str,
    pub installation_manager: &'static str,
    pub current: Option<Release>,
    pub executable: Option<PathBuf>,
    pub binary_integrity: &'static str,
    pub pending: Option<ManagedPlan>,
    pub history: Vec<ManagedReceipt>,
    pub application_configuration: &'static str,
    pub application_health: &'static str,
    pub target_call: &'static str,
}

#[derive(Serialize)]
pub struct ManagedResult {
    pub outcome: &'static str,
    pub plan: Option<ManagedPlan>,
    pub receipt: Option<ManagedReceipt>,
    pub replayed: bool,
    pub application_configuration: &'static str,
    pub application_health: &'static str,
    pub target_call: &'static str,
}

/// Default state scope follows the existing CLI OAuth state convention but
/// uses a separate subdirectory. Merely computing this path does not create it.
pub fn default_root() -> Result<PathBuf, ManagedError> {
    let base = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::environment::user_home().map(|home| home.join(".local/state")))
        .filter(|path| path.is_absolute())
        .ok_or(ManagedError::UnsafePath)?;
    Ok(base.join("lmm/software/cc-switch"))
}

pub fn default_database() -> Result<PathBuf, ManagedError> {
    crate::environment::user_home()
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".cc-switch/cc-switch.db"))
        .ok_or(ManagedError::UnsafePath)
}

/// The installer owns only this namespace. Native configuration has a separate
/// adapter and is never inferred from this installation manifest.
pub struct Manager {
    root: PathBuf,
    arch: String,
    candidate: Release,
    database: Option<PathBuf>,
}

impl Manager {
    pub fn new(
        root: PathBuf,
        os: &str,
        arch: &str,
        database: Option<PathBuf>,
    ) -> Result<Self, ManagedError> {
        let candidate = releases::current(os, arch)?;
        files::check_path(&root)?;
        if let Some(path) = &database {
            files::check_path(path)?;
        }
        Ok(Self {
            root,
            arch: arch.into(),
            candidate,
            database,
        })
    }

    pub fn for_current_environment(
        root: Option<PathBuf>,
        database: Option<PathBuf>,
    ) -> Result<Self, ManagedError> {
        Self::new(
            root.map_or_else(default_root, Ok)?,
            env::consts::OS,
            env::consts::ARCH,
            database,
        )
    }

    pub fn status(&self) -> Result<ManagedStatus, ManagedError> {
        let _lock = files::read_lock(&self.root)?;
        let state = self.load()?;
        let integrity = if let Some(release) = &state.current {
            match files::verify(&self.artifact(release), release) {
                Ok(true) => "sha256_and_size_verified",
                Ok(false) => "missing",
                Err(ManagedError::ModifiedArtifact) => "modified",
                Err(error) => return Err(error),
            }
        } else {
            "not_installed"
        };
        Ok(ManagedStatus {
            contract: "lmm.managed-status.v1",
            software: "cc-switch",
            managed_root: self.root.clone(),
            generation: state.generation,
            installation: if state.pending.is_some() {
                "operation_incomplete"
            } else if state.current.is_some() {
                "managed_distribution"
            } else {
                "not_installed"
            },
            installation_manager: if self.root.exists() { "lmm" } else { "none" },
            executable: state.current.as_ref().map(|release| self.artifact(release)),
            current: state.current,
            binary_integrity: integrity,
            pending: state.pending.map(|pending| pending.plan),
            history: state.events,
            application_configuration: "not_changed",
            application_health: "not_checked",
            target_call: "not_checked",
        })
    }

    pub fn preview(
        &self,
        action: Action,
        change_id: Option<&str>,
    ) -> Result<ManagedPlan, ManagedError> {
        let _lock = files::read_lock(&self.root)?;
        let state = self.load()?;
        if let Some(pending) = &state.pending {
            if pending.plan.action == action && pending.plan.restore_change.as_deref() == change_id
            {
                return Ok(pending.plan.clone());
            }
            return Err(ManagedError::Pending);
        }
        self.plan(&state, action, change_id)
    }

    /// Applying only accepts the digest from a previously displayed plan.
    /// Pending operations resume with the same digest; committed ones return
    /// their original receipt without repeating filesystem changes.
    pub fn apply(
        &self,
        action: Action,
        change_id: Option<&str>,
        plan_id: &str,
    ) -> Result<ManagedResult, ManagedError> {
        self.apply_with(action, change_id, plan_id, &mut HttpDownload)
    }

    fn apply_with(
        &self,
        action: Action,
        change_id: Option<&str>,
        plan_id: &str,
        download: &mut dyn Download,
    ) -> Result<ManagedResult, ManagedError> {
        if !hex(plan_id, 64) {
            return Err(ManagedError::Conflict);
        }
        // A mistyped plan cannot create a state directory. Validate read-only
        // first, then revalidate under the writer lock before any mutation.
        {
            let _read_lock = files::read_lock(&self.root)?;
            let before = self.load()?;
            if !before
                .events
                .iter()
                .any(|event| event.plan_id == plan_id && event.action == action)
            {
                match &before.pending {
                    Some(pending)
                        if pending.plan.plan_id == plan_id
                            && pending.plan.action == action
                            && pending.plan.restore_change.as_deref() == change_id => {}
                    Some(_) => return Err(ManagedError::Pending),
                    None if self.plan(&before, action, change_id)?.plan_id == plan_id => {}
                    None => return Err(ManagedError::Conflict),
                }
            }
        }
        let _lock = files::write_lock(&self.root)?;
        let mut state = self.load()?;
        if let Some(receipt) = state.events.iter().find(|event| event.plan_id == plan_id) {
            if receipt.action != action {
                return Err(ManagedError::Conflict);
            }
            return Ok(ManagedResult {
                outcome: "committed_receipt",
                plan: None,
                receipt: Some(receipt.clone()),
                replayed: true,
                application_configuration: "not_changed",
                application_health: "not_checked",
                target_call: "not_checked",
            });
        }
        let pending = match state.pending.clone() {
            Some(pending) => {
                if pending.plan.plan_id != plan_id
                    || pending.plan.action != action
                    || pending.plan.restore_change.as_deref() != change_id
                {
                    return Err(ManagedError::Pending);
                }
                pending
            }
            None => {
                let plan = self.plan(&state, action, change_id)?;
                if plan.plan_id != plan_id {
                    return Err(ManagedError::Conflict);
                }
                if plan.no_change {
                    return Ok(ManagedResult {
                        outcome: "already_at_requested_state",
                        plan: Some(plan),
                        receipt: None,
                        replayed: false,
                        application_configuration: "not_changed",
                        application_health: "not_checked",
                        target_call: "not_checked",
                    });
                }
                if state.events.len() >= MAX_EVENTS {
                    return Err(ManagedError::InvalidState);
                }
                let pending = Pending {
                    change_id: files::nonce()?,
                    plan,
                };
                state.pending = Some(pending.clone());
                self.save(&state)?;
                pending
            }
        };

        if pending.plan.after.is_some()
            && let Some(before) = &pending.plan.before
            && !files::verify(&self.artifact(before), before)?
        {
            return Err(ManagedError::ModifiedArtifact);
        }

        if let Some(release) = &pending.plan.after
            && !files::verify(&self.artifact(release), release)?
        {
            download.fetch(&self.root, release)?;
        }
        // Hold the target's native writer lock only for the local commit, not
        // during a network download. No target database value is changed.
        let _database = if let Some(release) = &pending.plan.after {
            self.database_guard(release, pending.plan.database_version)?
        } else {
            None
        };
        let known = known_releases(&state);
        for sha in &pending.plan.remove_artifacts {
            let release = known.get(sha).ok_or(ManagedError::InvalidState)?;
            let path = self.artifact(release);
            if files::verify(&path, release)? {
                fs::remove_file(&path).map_err(|_| ManagedError::Storage)?;
            }
        }
        files::sync_directory(&self.root.join("artifacts"))?;
        let receipt = ManagedReceipt {
            change_id: pending.change_id,
            plan_id: pending.plan.plan_id.clone(),
            action,
            before: pending.plan.before.clone(),
            after: pending.plan.after.clone(),
            committed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| ManagedError::Storage)?
                .as_secs(),
        };
        state.current = pending.plan.after.clone();
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or(ManagedError::InvalidState)?;
        state.events.push(receipt.clone());
        state.pending = None;
        self.save(&state)?;
        Ok(ManagedResult {
            outcome: "committed",
            plan: Some(pending.plan),
            receipt: Some(receipt),
            replayed: false,
            application_configuration: "not_changed",
            application_health: "not_checked",
            target_call: "not_checked",
        })
    }

    /// Cancel only while the committed installation is still intact. A canceled
    /// operation gets a permanent receipt and advances the generation, so a
    /// delayed retry of its formerly approved plan cannot apply it afterward.
    pub fn cancel(
        &self,
        action: Action,
        change_id: Option<&str>,
        plan_id: &str,
    ) -> Result<ManagedReceipt, ManagedError> {
        if !hex(plan_id, 64) {
            return Err(ManagedError::Conflict);
        }
        {
            let _lock = files::read_lock(&self.root)?;
            let state = self.load()?;
            if let Some(event) = state
                .events
                .iter()
                .find(|event| event.plan_id == plan_id && event.action == Action::Cancel)
            {
                return Ok(event.clone());
            }
            if state
                .pending
                .as_ref()
                .is_none_or(|pending| pending.plan.plan_id != plan_id)
            {
                return Err(ManagedError::Conflict);
            }
        }
        let _lock = files::write_lock(&self.root)?;
        let mut state = self.load()?;
        let pending = state.pending.clone().ok_or(ManagedError::Conflict)?;
        if pending.plan.plan_id != plan_id
            || pending.plan.action != action
            || pending.plan.restore_change.as_deref() != change_id
        {
            return Err(ManagedError::Conflict);
        }
        let known = known_releases(&state);
        if let Some(current) = &state.current
            && !files::verify(&self.artifact(current), current)?
        {
            return Err(ManagedError::CannotCancel);
        }
        for sha in &pending.plan.remove_artifacts {
            let release = known.get(sha).ok_or(ManagedError::InvalidState)?;
            if !files::verify(&self.artifact(release), release)? {
                return Err(ManagedError::CannotCancel);
            }
        }
        if let Some(after) = &pending.plan.after
            && !known.contains_key(&after.sha256)
            && files::verify(&self.artifact(after), after)?
        {
            fs::remove_file(self.artifact(after)).map_err(|_| ManagedError::Storage)?;
            files::sync_directory(&self.root.join("artifacts"))?;
        }
        if state.events.len() >= MAX_EVENTS {
            return Err(ManagedError::InvalidState);
        }
        let receipt = ManagedReceipt {
            change_id: pending.change_id,
            plan_id: plan_id.into(),
            action: Action::Cancel,
            before: state.current.clone(),
            after: state.current.clone(),
            committed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| ManagedError::Storage)?
                .as_secs(),
        };
        state.events.push(receipt.clone());
        state.generation += 1;
        state.pending = None;
        self.save(&state)?;
        Ok(receipt)
    }

    /// Explicit launch only. Verification does not execute an upstream binary.
    /// A process ID means spawn succeeded, not that the UI or LMM works.
    pub fn launch(&self) -> Result<u32, ManagedError> {
        let _lock = files::read_lock(&self.root)?;
        let state = self.load()?;
        if state.pending.is_some() {
            return Err(ManagedError::Pending);
        }
        let release = state.current.as_ref().ok_or(ManagedError::NotInstalled)?;
        if !files::verify(&self.artifact(release), release)? {
            return Err(ManagedError::NotInstalled);
        }
        let version = self.database_version()?;
        if version.is_some_and(|version| version > release.max_database_version) {
            return Err(ManagedError::DatabaseCompatibility);
        }
        let child = Command::new(self.artifact(release))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ManagedError::Launch)?;
        Ok(child.id())
    }

    fn plan(
        &self,
        state: &State,
        action: Action,
        change_id: Option<&str>,
    ) -> Result<ManagedPlan, ManagedError> {
        if state.pending.is_some() {
            return Err(ManagedError::Pending);
        }
        if action != Action::Restore && change_id.is_some() {
            return Err(ManagedError::UnknownChange);
        }
        if let Some(current) = &state.current
            && !files::verify(&self.artifact(current), current)?
        {
            return Err(ManagedError::ModifiedArtifact);
        }
        let after = match action {
            Action::Cancel => return Err(ManagedError::Conflict),
            Action::Install => {
                if state
                    .current
                    .as_ref()
                    .is_some_and(|current| current != &self.candidate)
                {
                    return Err(ManagedError::Conflict);
                }
                Some(self.candidate.clone())
            }
            Action::Update => {
                let current = state.current.as_ref().ok_or(ManagedError::NotInstalled)?;
                if releases::version(&current.version) > releases::version(&self.candidate.version)
                {
                    return Err(ManagedError::Downgrade);
                }
                Some(self.candidate.clone())
            }
            Action::Uninstall => None,
            Action::Restore => {
                let change = state
                    .events
                    .iter()
                    .find(|event| Some(event.change_id.as_str()) == change_id)
                    .ok_or(ManagedError::UnknownChange)?;
                if state.current != change.after && state.current != change.before {
                    return Err(ManagedError::Conflict);
                }
                change.before.clone()
            }
        };
        let mut remove_artifacts = Vec::new();
        if after.is_none() && (action == Action::Uninstall || state.current != after) {
            let known = known_releases(state);
            for (sha, release) in &known {
                if (action == Action::Uninstall || state.current.as_ref() == Some(release))
                    && files::verify(&self.artifact(release), release)?
                {
                    remove_artifacts.push(sha.clone());
                }
            }
        }
        let no_change = state.current == after && remove_artifacts.is_empty();
        let database_version = self.database_version()?;
        if after.as_ref().is_some_and(|release| {
            database_version.is_some_and(|version| version > release.max_database_version)
        }) {
            return Err(ManagedError::DatabaseCompatibility);
        }
        let mut plan = ManagedPlan {
            contract: "lmm.managed-plan.v1".into(),
            plan_id: String::new(),
            action,
            software: "cc-switch".into(),
            managed_root: self.root.clone(),
            expected_generation: state.generation,
            before: state.current.clone(),
            after,
            restore_change: change_id.map(str::to_owned),
            remove_artifacts,
            database: self.database.clone(),
            database_version,
            application_data: "preserved; no provider, model, credentials, PATH or desktop changes"
                .into(),
            login_required: false,
            paid_test: false,
            no_change,
        };
        plan.plan_id = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&plan).map_err(|_| ManagedError::Storage)?)
        );
        Ok(plan)
    }

    fn artifact(&self, release: &Release) -> PathBuf {
        self.root
            .join("artifacts")
            .join(format!("{}.AppImage", release.sha256))
    }

    fn load(&self) -> Result<State, ManagedError> {
        let Some(bytes) = files::read(&self.root.join("state.json"), MAX_STATE)? else {
            return Ok(State {
                contract: "lmm.managed-state.v1".into(),
                software: "cc-switch".into(),
                arch: self.arch.clone(),
                generation: 0,
                current: None,
                events: Vec::new(),
                pending: None,
            });
        };
        let state: State =
            serde_json::from_slice(&bytes).map_err(|_| ManagedError::InvalidState)?;
        if state.contract != "lmm.managed-state.v1"
            || state.software != "cc-switch"
            || state.arch != self.arch
            || state.generation != state.events.len() as u64
            || state.events.len() > MAX_EVENTS
        {
            return Err(ManagedError::InvalidState);
        }
        let mut previous = None;
        for event in &state.events {
            if !hex(&event.change_id, 64)
                || !hex(&event.plan_id, 64)
                || event.before != previous
                || event
                    .after
                    .as_ref()
                    .is_some_and(|release| !releases::valid(release, &self.arch))
            {
                return Err(ManagedError::InvalidState);
            }
            previous.clone_from(&event.after);
        }
        if state.current != previous {
            return Err(ManagedError::InvalidState);
        }
        if let Some(pending) = &state.pending {
            if !hex(&pending.change_id, 64)
                || pending.plan.expected_generation != state.generation
                || pending.plan.before != state.current
                || pending.plan.managed_root != self.root
                || pending.plan.software != "cc-switch"
                || pending.plan.database != self.database
                || pending
                    .plan
                    .after
                    .as_ref()
                    .is_some_and(|release| !releases::valid(release, &self.arch))
                || pending
                    .plan
                    .remove_artifacts
                    .iter()
                    .any(|sha| !hex(sha, 64))
            {
                return Err(ManagedError::InvalidState);
            }
            let mut check = pending.plan.clone();
            check.plan_id.clear();
            let digest = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&check).map_err(|_| ManagedError::InvalidState)?)
            );
            if digest != pending.plan.plan_id {
                return Err(ManagedError::InvalidState);
            }
        }
        Ok(state)
    }

    fn save(&self, state: &State) -> Result<(), ManagedError> {
        let bytes = serde_json::to_vec_pretty(state).map_err(|_| ManagedError::Storage)?;
        if bytes.len() as u64 > MAX_STATE {
            return Err(ManagedError::Storage);
        }
        files::publish(&self.root, &bytes)
    }

    fn database_version(&self) -> Result<Option<u32>, ManagedError> {
        let Some(path) = &self.database else {
            return Ok(None);
        };
        files::check_path(path)?;
        if !path.try_exists().map_err(|_| ManagedError::Storage)? {
            return Ok(None);
        }
        crate::cc_switch::database_version(path)
            .map(Some)
            .map_err(|_| ManagedError::DatabaseCompatibility)
    }

    fn database_guard(
        &self,
        release: &Release,
        expected: Option<u32>,
    ) -> Result<Option<rusqlite::Connection>, ManagedError> {
        let Some(path) = &self.database else {
            return Ok(None);
        };
        files::check_path(path)?;
        if !path.try_exists().map_err(|_| ManagedError::Storage)? {
            if expected.is_some() {
                return Err(ManagedError::Conflict);
            }
            return Ok(None);
        }
        let connection = crate::cc_switch::lock_database_version(path)
            .map_err(|_| ManagedError::DatabaseCompatibility)?;
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| ManagedError::DatabaseCompatibility)?;
        if version > release.max_database_version || Some(version) != expected {
            return Err(ManagedError::DatabaseCompatibility);
        }
        Ok(Some(connection))
    }
}

fn known_releases(state: &State) -> BTreeMap<String, Release> {
    state
        .events
        .iter()
        .flat_map(|event| event.before.iter().chain(event.after.iter()))
        .map(|release| (release.sha256.clone(), release.clone()))
        .collect()
}

fn hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

trait Download {
    fn fetch(&mut self, root: &Path, release: &Release) -> Result<(), ManagedError>;
}

struct HttpDownload;
impl Download for HttpDownload {
    fn fetch(&mut self, root: &Path, release: &Release) -> Result<(), ManagedError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(20))
            .user_agent(concat!("lmm/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let url = attempt.url();
                if attempt.previous().len() >= 5
                    || url.scheme() != "https"
                    || url.port_or_known_default() != Some(443)
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || !matches!(
                        url.host_str(),
                        Some(
                            "github.com"
                                | "release-assets.githubusercontent.com"
                                | "objects.githubusercontent.com"
                        )
                    )
                {
                    attempt.error("unapproved release redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| ManagedError::Download)?;
        let response = client
            .get(&release.download_url)
            .send()
            .map_err(|_| ManagedError::Download)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|size| size != release.size)
        {
            return Err(ManagedError::Download);
        }
        let mut limited = response.take(release.size + 1);
        files::store_download(root, release, &mut limited)
    }
}

#[cfg(test)]
mod tests;
