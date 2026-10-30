//! Native schema-24 SQLite transactions add
//! and maintain only this adapter's Codex provider. A database row is not live
//! activation; CC Switch remains responsible for its own live write engine.
//! No OAuth credential, user-owned provider backup or auth.json is persisted.
#[path = "binding_catalog.rs"]
mod catalog;
#[cfg(test)]
#[path = "binding_tests.rs"]
mod tests;

use crate::oauth::applications::VerifiedCatalog;
use rusqlite::{Connection, OptionalExtension, params, types::ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

const CONTRACT: &str = "lmm.cc-switch.binding.v1";
const BINDINGS_TABLE: &str = "CREATE TABLE lmm_cli_bindings_v1 (
 issuer TEXT NOT NULL, installation_id TEXT NOT NULL, provider_id TEXT NOT NULL UNIQUE,
 revision INTEGER NOT NULL CHECK(revision>0), payload TEXT NOT NULL,
 PRIMARY KEY(issuer,installation_id))";
const CHANGES_TABLE: &str = "CREATE TABLE lmm_cli_changes_v1 (
 change_id TEXT PRIMARY KEY NOT NULL, issuer TEXT NOT NULL, installation_id TEXT NOT NULL,
 payload TEXT NOT NULL, created_at INTEGER NOT NULL)";
const PROVIDER_COLUMNS: [&str; 14] = [
    "id",
    "app_type",
    "name",
    "settings_config",
    "website_url",
    "category",
    "created_at",
    "sort_index",
    "notes",
    "icon",
    "icon_color",
    "meta",
    "is_current",
    "in_failover_queue",
];
type Result<T> = std::result::Result<T, BindingError>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BindingError {
    #[error("CC Switch 目标、模型选择或凭据命令路径无效")]
    Invalid,
    #[error("需要仍有效且属于该实例的独立 CC Switch 模型授权")]
    Grant,
    #[error("此数据库不是已核验的 CC Switch schema 24，或本适配器的历史结构不完整")]
    Schema,
    #[error("目标已被修改、已有同名项目或原生数据库写入正在进行；请重新预览")]
    Conflict,
    #[error("原模型已不在当前授权目录中；已保留选择，请先明确选择另一个模型")]
    SelectedModelUnavailable,
    #[error("变更回执或本实例的管理记录不存在")]
    NotFound,
    #[error(
        "删除供应商需要 CC Switch 原生管理入口确认 live、代理和 Stack 引用；仅凭数据库选择状态不能删除"
    )]
    NativeRemoval,
    #[error("必须确认当前预览的精确 plan_id")]
    Confirmation,
    #[error("CC Switch 数据库不可用")]
    Storage,
    #[error("数据库提交结果未知；请先使用原 change_id 查询回执，不能新建操作重试")]
    CommitUnknown,
}
impl From<rusqlite::Error> for BindingError {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::SqliteFailure(code, _)
                if matches!(
                    code.code,
                    rusqlite::ErrorCode::DatabaseBusy
                        | rusqlite::ErrorCode::DatabaseLocked
                        | rusqlite::ErrorCode::ConstraintViolation
                ) =>
            {
                Self::Conflict
            }
            _ => Self::Storage,
        }
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}
fn encode(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|_| BindingError::Invalid)
}
fn digest(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(encode(value)?.as_bytes())))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    if value.len() > 8 * 1024 * 1024 {
        return Err(BindingError::Storage);
    }
    serde_json::from_str(value).map_err(|_| BindingError::Storage)
}

/// This record contains only adapter-created fields and a redacted price
/// snapshot. User edits and secrets are never copied into the journal.
#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct BindingData {
    contract: String,
    issuer: String,
    installation_id: String,
    provider_id: String,
    grant_id: String,
    budget_credits: u64,
    group: String,
    display_name: String,
    initial_model: String,
    helper_path: String,
    helper_directory: String,
    revision: i64,
    active: bool,
    native_catalog: Value,
    prices: Value,
    original_row_digest: String,
}
impl BindingData {
    fn same_grant(&self, proof: &VerifiedCatalog) -> bool {
        let status = proof.status();
        proof.allows_model_calls(now())
            && status.client_id == "lmm-cc-switch"
            && status.issuer == self.issuer
            && status.installation_id == self.installation_id
            && status.grant_id == self.grant_id
            && status.budget_credits == self.budget_credits
    }
}
struct ProviderRow {
    digest: String,
    settings: Value,
    selected: bool,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BindingAction {
    Add,
    Sync,
    Restore,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    action: BindingAction,
    target: String,
    restores: Option<String>,
    before: Option<BindingData>,
    after: BindingData,
    expected_row_digest: Option<String>,
}

/// Only previews can construct executable plans. JSON output has no credentials
/// and cannot be deserialized into an authority-bearing apply request.
#[derive(Serialize)]
pub struct BindingPlan {
    pub contract: &'static str,
    pub plan_id: String,
    pub action: BindingAction,
    pub database: String,
    pub provider_id: String,
    pub issuer: String,
    pub installation_id: String,
    pub group: String,
    pub expected_revision: i64,
    pub next_revision: i64,
    pub was_selected_in_database: bool,
    pub selection: &'static str,
    pub live_application_state: &'static str,
    pub requires_native_activation: bool,
    pub prices: Value,
    pub paid_test: bool,
    #[serde(skip)]
    payload: Payload,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BindingReceipt {
    pub contract: String,
    pub change_id: String,
    pub action: BindingAction,
    pub provider_id: String,
    pub revision: i64,
    pub provider_present: bool,
    pub selection_changed: bool,
    pub live_application_changed: bool,
    pub authorization_changed: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredChange {
    request: Payload,
    result: BindingReceipt,
}

#[derive(Serialize)]
pub struct InstallationSummary {
    pub issuer: String,
    pub installation_id: String,
    pub provider_id: String,
    pub group: String,
    pub revision: i64,
    pub authorization: &'static str,
}

/// Lists this adapter's own redacted index. It is configuration history, not a
/// proof that a grant remains active; callers still load the OS vault and ask
/// the issuer before model-related writes.
pub fn installations(path: &Path) -> Result<Vec<InstallationSummary>> {
    let connection = connection(path, false)?;
    if !tables(&connection)? {
        return Ok(Vec::new());
    }
    let keys = connection.prepare("SELECT issuer,installation_id FROM lmm_cli_bindings_v1 ORDER BY issuer,installation_id LIMIT 1001")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if keys.len() > 1000 {
        return Err(BindingError::Storage);
    }
    let mut result = Vec::new();
    for (issuer, id) in keys {
        let stored = binding(&connection, &issuer, &id)?.ok_or(BindingError::Storage)?;
        result.push(InstallationSummary {
            issuer,
            installation_id: id,
            provider_id: stored.provider_id,
            group: stored.group,
            revision: stored.revision,
            authorization: "not_checked",
        });
    }
    Ok(result)
}

fn connection(path: &Path, write: bool) -> Result<Connection> {
    let connection = super::open(path, write).map_err(|_| BindingError::Storage)?;
    connection.execute_batch(if write { "BEGIN IMMEDIATE;" } else { "BEGIN;" })?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != super::SCHEMA_VERSION {
        return Err(BindingError::Schema);
    }
    let native: bool = connection.query_row(
        "SELECT count(*)=1 FROM sqlite_schema WHERE type='table' AND name='providers'",
        [],
        |row| row.get(0),
    )?;
    if !native {
        return Err(BindingError::Schema);
    }
    let columns = connection
        .prepare("PRAGMA table_info(providers)")?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if columns.len() != PROVIDER_COLUMNS.len()
        || PROVIDER_COLUMNS.iter().any(|name| {
            let expected = match *name {
                "created_at" | "sort_index" => "INTEGER",
                "is_current" | "in_failover_queue" => "BOOLEAN",
                _ => "TEXT",
            };
            !columns
                .iter()
                .any(|(actual, kind, _)| actual == name && kind.eq_ignore_ascii_case(expected))
        })
        || !columns
            .iter()
            .any(|(name, _, key)| name == "id" && *key == 1)
        || !columns
            .iter()
            .any(|(name, _, key)| name == "app_type" && *key == 2)
    {
        return Err(BindingError::Schema);
    }
    Ok(connection)
}
fn tables(connection: &Connection) -> Result<bool> {
    let definitions = connection.prepare("SELECT name,type,sql FROM sqlite_schema WHERE name IN ('lmm_cli_bindings_v1','lmm_cli_changes_v1')")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if definitions.is_empty() {
        return Ok(false);
    }
    let normalize = |sql: &str| sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if definitions.len() != 2
        || definitions.iter().any(|(name, kind, sql)| {
            let expected = if name == "lmm_cli_bindings_v1" {
                BINDINGS_TABLE
            } else {
                CHANGES_TABLE
            };
            kind != "table" || normalize(sql) != normalize(expected)
        })
    {
        return Err(BindingError::Schema);
    }
    Ok(true)
}
fn binding(
    connection: &Connection,
    issuer: &str,
    installation: &str,
) -> Result<Option<BindingData>> {
    if !tables(connection)? {
        return Ok(None);
    }
    let row: Option<(i64, String, String)> = connection.query_row(
        "SELECT revision,provider_id,payload FROM lmm_cli_bindings_v1 WHERE issuer=?1 AND installation_id=?2",
        params![issuer, installation], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
    row.map(|(revision, provider, payload)| {
        let value: BindingData = decode(&payload)?;
        if value.contract != CONTRACT
            || value.issuer != issuer
            || value.installation_id != installation
            || value.revision != revision
            || revision <= 0
            || value.provider_id != provider
        {
            return Err(BindingError::Storage);
        }
        Ok(value)
    })
    .transpose()
}
fn provider(connection: &Connection, id: &str) -> Result<Option<ProviderRow>> {
    let query = format!(
        "SELECT {} FROM providers WHERE id=?1 AND app_type='codex'",
        PROVIDER_COLUMNS.join(",")
    );
    let row = connection
        .query_row(&query, [id], |row| {
            let mut digest = Sha256::new();
            for index in 0..PROVIDER_COLUMNS.len() {
                match row.get_ref(index)? {
                    ValueRef::Null => digest.update([0]),
                    ValueRef::Integer(value) => {
                        digest.update([1]);
                        digest.update(value.to_be_bytes());
                    }
                    ValueRef::Real(value) => {
                        digest.update([2]);
                        digest.update(value.to_bits().to_be_bytes());
                    }
                    ValueRef::Text(value) => {
                        digest.update([3]);
                        digest.update((value.len() as u64).to_be_bytes());
                        digest.update(value);
                    }
                    ValueRef::Blob(value) => {
                        digest.update([4]);
                        digest.update((value.len() as u64).to_be_bytes());
                        digest.update(value);
                    }
                }
            }
            Ok((
                format!("{:x}", digest.finalize()),
                row.get::<_, String>(3)?,
                row.get::<_, bool>(12)?,
            ))
        })
        .optional()?;
    row.map(|(digest, settings, selected)| {
        Ok(ProviderRow {
            digest,
            settings: decode(&settings)?,
            selected,
        })
    })
    .transpose()
}
fn plan(payload: Payload, selected: bool) -> Result<BindingPlan> {
    let plan_id = digest(&payload)?;
    Ok(BindingPlan {
        contract: CONTRACT,
        plan_id,
        action: payload.action,
        database: payload.target.clone(),
        provider_id: payload.after.provider_id.clone(),
        issuer: payload.after.issuer.clone(),
        installation_id: payload.after.installation_id.clone(),
        group: payload.after.group.clone(),
        expected_revision: payload.before.as_ref().map_or(0, |before| before.revision),
        next_revision: payload.after.revision,
        was_selected_in_database: selected,
        selection: "preserve",
        live_application_state: "not_checked",
        requires_native_activation: payload.after.active,
        prices: payload.after.prices.clone(),
        paid_test: false,
        payload,
    })
}
fn file_path(path: &Path) -> Result<String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(BindingError::Invalid);
    }
    let mut prefix = std::path::PathBuf::new();
    for part in path.components() {
        prefix.push(part);
        if fs::symlink_metadata(&prefix)
            .map_err(|_| BindingError::Invalid)?
            .file_type()
            .is_symlink()
        {
            return Err(BindingError::Invalid);
        }
    }
    path.to_str()
        .filter(|path| !path.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(BindingError::Invalid)
}

/// Add-only setup. model_id is an exact entry from the application's freshly
/// verified catalog; no API key, token, implicit group or default model is used.
pub fn preview_add(
    path: &Path,
    proof: &VerifiedCatalog,
    model_id: &str,
    display_name: &str,
    helper: &Path,
) -> Result<BindingPlan> {
    if !proof.allows_model_calls(now()) || proof.status().client_id != "lmm-cc-switch" {
        return Err(BindingError::Grant);
    }
    let selected = proof
        .catalog()
        .models
        .iter()
        .filter(|model| model.id == model_id)
        .collect::<Vec<_>>();
    if selected.len() != 1
        || !selected[0].apis.iter().any(|api| api == "openai-responses")
        || display_name.trim().is_empty()
        || display_name.len() > 160
        || display_name.chars().any(char::is_control)
    {
        return Err(BindingError::Invalid);
    }
    let helper_path = file_path(helper)?;
    let metadata = fs::metadata(helper).map_err(|_| BindingError::Invalid)?;
    if !metadata.is_file() {
        return Err(BindingError::Invalid);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(BindingError::Invalid);
        }
    }
    let helper_directory = file_path(helper.parent().ok_or(BindingError::Invalid)?)?;
    let status = proof.status();
    let (native_catalog, prices) = catalog::source(proof, &selected[0].group)?;
    let after = BindingData {
        contract: CONTRACT.into(),
        issuer: status.issuer.clone(),
        installation_id: status.installation_id.clone(),
        provider_id: format!("lmm-{}", status.installation_id),
        grant_id: status.grant_id.clone(),
        budget_credits: status.budget_credits,
        group: selected[0].group.clone(),
        display_name: display_name.into(),
        initial_model: selected[0].upstream_model.clone(),
        helper_path,
        helper_directory,
        revision: 1,
        active: true,
        native_catalog,
        prices,
        original_row_digest: String::new(),
    };
    let connection = connection(path, false)?;
    preview_add_in(&connection, path, after)
}
fn preview_add_in(connection: &Connection, path: &Path, after: BindingData) -> Result<BindingPlan> {
    if binding(connection, &after.issuer, &after.installation_id)?.is_some()
        || provider(connection, &after.provider_id)?.is_some()
    {
        return Err(BindingError::Conflict);
    }
    plan(
        Payload {
            action: BindingAction::Add,
            target: file_path(path)?,
            restores: None,
            before: None,
            after,
            expected_row_digest: None,
        },
        false,
    )
}

pub fn preview_sync(path: &Path, proof: &VerifiedCatalog) -> Result<BindingPlan> {
    let connection = connection(path, false)?;
    let before = binding(
        &connection,
        &proof.status().issuer,
        &proof.status().installation_id,
    )?
    .ok_or(BindingError::NotFound)?;
    if !before.active || !before.same_grant(proof) {
        return Err(BindingError::Grant);
    }
    let (native, prices) = catalog::source(proof, &before.group)?;
    preview_sync_in(&connection, path, before, native, prices)
}
fn preview_sync_in(
    connection: &Connection,
    path: &Path,
    before: BindingData,
    native_catalog: Value,
    prices: Value,
) -> Result<BindingPlan> {
    let row = provider(connection, &before.provider_id)?.ok_or(BindingError::Conflict)?;
    let selected_model = catalog::selected_model(&row.settings, &before)?;
    if !catalog::contains(&native_catalog, &selected_model)? {
        return Err(BindingError::SelectedModelUnavailable);
    }
    catalog::merge(
        &row.settings["modelCatalog"],
        &before.native_catalog,
        &native_catalog,
    )?;
    let mut after = before.clone();
    after.revision = before
        .revision
        .checked_add(1)
        .ok_or(BindingError::Storage)?;
    after.native_catalog = native_catalog;
    after.prices = prices;
    plan(
        Payload {
            action: BindingAction::Sync,
            target: file_path(path)?,
            restores: None,
            before: Some(before),
            after,
            expected_row_digest: Some(row.digest),
        },
        row.selected,
    )
}

fn stored_change(
    connection: &Connection,
    issuer: &str,
    installation: &str,
    change: &str,
) -> Result<StoredChange> {
    if !tables(connection)? {
        return Err(BindingError::NotFound);
    }
    let raw: Option<String> = connection.query_row("SELECT payload FROM lmm_cli_changes_v1 WHERE issuer=?1 AND installation_id=?2 AND change_id=?3",
        params![issuer, installation, change], |row| row.get(0)).optional()?;
    let result: StoredChange = decode(&raw.ok_or(BindingError::NotFound)?)?;
    if result.result.change_id != change
        || result.request.after.issuer != issuer
        || result.request.after.installation_id != installation
        || digest(&result.request)? != change
    {
        return Err(BindingError::Storage);
    }
    Ok(result)
}
pub fn receipt(
    path: &Path,
    issuer: &str,
    installation: &str,
    change: &str,
) -> Result<BindingReceipt> {
    let connection = connection(path, false)?;
    Ok(stored_change(&connection, issuer, installation, change)?.result)
}

/// Restore local metadata only. No credential is read, refreshed, reauthorized
/// or recreated. New user edits remain present or produce a clear conflict.
pub fn preview_restore(
    path: &Path,
    issuer: &str,
    installation: &str,
    change: &str,
) -> Result<BindingPlan> {
    let connection = connection(path, false)?;
    let original = stored_change(&connection, issuer, installation, change)?;
    if original.request.action == BindingAction::Restore {
        return Err(BindingError::Invalid);
    }
    // v4.0.22 keeps Stack/proxy references and pending live writes in device
    // files protected by an in-process mutex, not this SQLite transaction.
    // is_current=false cannot prove the provider is safe to remove.
    if original.request.action == BindingAction::Add {
        return Err(BindingError::NativeRemoval);
    }
    let before = binding(&connection, issuer, installation)?.ok_or(BindingError::NotFound)?;
    if !before.active {
        return Err(BindingError::Conflict);
    }
    let row = provider(&connection, &before.provider_id)?.ok_or(BindingError::Conflict)?;
    let mut after = before.clone();
    after.revision = before
        .revision
        .checked_add(1)
        .ok_or(BindingError::Storage)?;
    match original.request.action {
        BindingAction::Add => return Err(BindingError::NativeRemoval),
        BindingAction::Sync => {
            let old = original
                .request
                .before
                .as_ref()
                .ok_or(BindingError::Storage)?;
            if before.native_catalog != original.request.after.native_catalog
                || before.prices != original.request.after.prices
            {
                return Err(BindingError::Conflict);
            }
            let selected = catalog::selected_model(&row.settings, &before)?;
            if !catalog::contains(&old.native_catalog, &selected)? {
                return Err(BindingError::SelectedModelUnavailable);
            }
            catalog::merge(
                &row.settings["modelCatalog"],
                &before.native_catalog,
                &old.native_catalog,
            )?;
            after.native_catalog = old.native_catalog.clone();
            after.prices = old.prices.clone();
        }
        BindingAction::Restore => return Err(BindingError::Invalid),
    }
    plan(
        Payload {
            action: BindingAction::Restore,
            target: file_path(path)?,
            restores: Some(change.into()),
            before: Some(before),
            after,
            expected_row_digest: Some(row.digest),
        },
        row.selected,
    )
}

/// The caller must fetch current application proof again before add/sync apply.
/// A readonly CLI catalog response cannot construct that proof. Recovery reads
/// the original receipt and never retries an unknown commit with a new plan.
pub fn apply(
    path: &Path,
    planned: &BindingPlan,
    confirmation: &str,
    proof: Option<&VerifiedCatalog>,
) -> Result<BindingReceipt> {
    if confirmation != planned.plan_id
        || planned.plan_id != digest(&planned.payload)?
        || planned.action != planned.payload.action
        || planned.database != planned.payload.target
        || planned.payload.target != file_path(path)?
    {
        return Err(BindingError::Confirmation);
    }
    if planned.payload.action != BindingAction::Restore {
        let proof = proof.ok_or(BindingError::Grant)?;
        if !planned.payload.after.same_grant(proof) {
            return Err(BindingError::Grant);
        }
        let (native, prices) = catalog::source(proof, &planned.payload.after.group)?;
        if native != planned.payload.after.native_catalog || prices != planned.payload.after.prices
        {
            return Err(BindingError::Conflict);
        }
    }
    apply_prepared(path, planned)
}
fn apply_prepared(path: &Path, planned: &BindingPlan) -> Result<BindingReceipt> {
    let connection = connection(path, true)?;
    let payload = &planned.payload;
    let after = &payload.after;
    if tables(&connection)? {
        match stored_change(
            &connection,
            &after.issuer,
            &after.installation_id,
            &planned.plan_id,
        ) {
            Ok(previous) => return Ok(previous.result),
            Err(BindingError::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    let current_binding = binding(&connection, &after.issuer, &after.installation_id)?;
    let row = provider(&connection, &after.provider_id)?;
    if current_binding != payload.before
        || row.as_ref().map(|row| &row.digest) != payload.expected_row_digest.as_ref()
    {
        return Err(BindingError::Conflict);
    }
    if !tables(&connection)? {
        connection.execute_batch(&format!("{BINDINGS_TABLE};{CHANGES_TABLE};"))?;
    }
    let mut saved = after.clone();
    match &payload.before {
        None => {
            let settings = json!({"auth":{},"config":catalog::config(after)?,"modelCatalog":after.native_catalog});
            connection.execute("INSERT INTO providers(id,app_type,name,settings_config,website_url,created_at,meta,is_current,in_failover_queue) VALUES(?1,'codex',?2,?3,?4,?5,'{}',0,0)",
                params![after.provider_id, after.display_name, encode(&settings)?, after.issuer, i64::try_from(now()).map_err(|_| BindingError::Invalid)?])?;
            let row = provider(&connection, &after.provider_id)?.ok_or(BindingError::Storage)?;
            if row.selected || row.settings != settings {
                return Err(BindingError::Conflict);
            }
            saved.original_row_digest = row.digest;
            connection.execute("INSERT INTO lmm_cli_bindings_v1(issuer,installation_id,provider_id,revision,payload) VALUES(?1,?2,?3,?4,?5)",
                params![saved.issuer, saved.installation_id, saved.provider_id, saved.revision, encode(&saved)?])?;
        }
        Some(before) => {
            let row = row.ok_or(BindingError::Conflict)?;
            if !after.active {
                return Err(BindingError::NativeRemoval);
            } else {
                let selected = catalog::selected_model(&row.settings, before)?;
                if !catalog::contains(&after.native_catalog, &selected)? {
                    return Err(BindingError::SelectedModelUnavailable);
                }
                let native = catalog::merge(
                    &row.settings["modelCatalog"],
                    &before.native_catalog,
                    &after.native_catalog,
                )?;
                let mut settings = row
                    .settings
                    .as_object()
                    .cloned()
                    .ok_or(BindingError::Conflict)?;
                settings.insert("modelCatalog".into(), native);
                connection.execute(
                    "UPDATE providers SET settings_config=?1 WHERE id=?2 AND app_type='codex'",
                    params![encode(&settings)?, after.provider_id],
                )?;
            }
            if connection.execute("UPDATE lmm_cli_bindings_v1 SET revision=?1,payload=?2 WHERE issuer=?3 AND installation_id=?4 AND revision=?5",
                params![saved.revision, encode(&saved)?, saved.issuer, saved.installation_id, before.revision])? != 1 { return Err(BindingError::Conflict); }
        }
    }
    let receipt = BindingReceipt {
        contract: CONTRACT.into(),
        change_id: planned.plan_id.clone(),
        action: planned.action,
        provider_id: after.provider_id.clone(),
        revision: after.revision,
        provider_present: after.active,
        selection_changed: false,
        live_application_changed: false,
        authorization_changed: false,
    };
    let stored = StoredChange {
        request: payload.clone(),
        result: receipt.clone(),
    };
    connection.execute("INSERT INTO lmm_cli_changes_v1(change_id,issuer,installation_id,payload,created_at) VALUES(?1,?2,?3,?4,?5)",
        params![planned.plan_id, after.issuer, after.installation_id, encode(&stored)?, i64::try_from(now()).map_err(|_| BindingError::Invalid)?])?;
    connection
        .execute_batch("COMMIT;")
        .map_err(|_| BindingError::CommitUnknown)?;
    Ok(receipt)
}
