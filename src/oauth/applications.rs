//! Independent application grants. Requires the separately installed
//! Core lmm-cc-switch contract; no fallback to the read-only CLI or Pi client.
//! No credential or refresh operation implements Debug or enters a report.

use super::{AuthError, ModelCatalog, Transport, callback, now, protocol, storage};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Duration};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const CLIENT: &str = "lmm-cc-switch";
const SCOPES: [&str; 4] = [
    "balance:read",
    "catalog:read",
    "models:invoke",
    "usage:read",
];
const RECOVERY_SECONDS: u64 = 600;

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum ApplicationError {
    #[error("应用实例、issuer 或授权格式无效")]
    Invalid,
    #[error("服务端尚未启用具有可恢复续期能力的 CC Switch 独立授权")]
    Unavailable,
    #[error("应用授权未完成、已失效或不包含所需权限")]
    Unauthorized,
    #[error("该实例已有独立授权；必须先明确解除旧授权才能更换")]
    AlreadyAuthorized,
    #[error("系统凭据库不可用；不会把 OAuth 凭据存入普通文件")]
    CredentialStore,
    #[error("应用授权请求未获得可靠响应；尚未确认操作完成")]
    Network,
    #[error("原续期操作超过恢复窗口；请撤销旧授权后重新征得同意")]
    RecoveryExpired,
    #[error("另一项本机授权操作正在运行")]
    Busy,
    #[error("浏览器授权未完成；未配置目标软件")]
    Browser,
    #[error("新应用授权未能保存，且撤销结果未知；请在 LMM 授权页撤销此 CC Switch 实例")]
    UnstoredGrant,
    #[error("应用授权码交换结果未知；未配置软件，请在 LMM 授权页检查此 CC Switch 实例")]
    AuthorizationUnknown,
    #[error("无法向 Codex 凭据管道写入结果")]
    CredentialOutput,
}
impl From<AuthError> for ApplicationError {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::InvalidIssuer | AuthError::InvalidResponse => Self::Invalid,
            AuthError::CredentialStore => Self::CredentialStore,
            AuthError::Busy => Self::Busy,
            AuthError::Network => Self::Network,
            AuthError::Unauthorized | AuthError::Denied | AuthError::NotLoggedIn => {
                Self::Unauthorized
            }
            AuthError::AlreadyLoggedIn => Self::AlreadyAuthorized,
            AuthError::RefreshBlocked => Self::RecoveryExpired,
            AuthError::UnstoredGrant => Self::UnstoredGrant,
            AuthError::Timeout | AuthError::Callback | AuthError::Output => Self::Browser,
        }
    }
}
type Result<T> = std::result::Result<T, ApplicationError>;

/// Descriptive installation input, not authority. A caller cannot construct a
/// VerifiedCatalog or obtain a model credential from this value alone.
pub struct ApplicationInstance {
    issuer: String,
    installation_id: String,
    display_label: String,
}
impl ApplicationInstance {
    pub fn new(issuer: &str, installation_id: &str, display_label: &str) -> Result<Self> {
        protocol::validate_issuer(issuer)?;
        if !hex_id(installation_id)
            || display_label.trim().is_empty()
            || display_label.len() > 160
            || display_label.chars().any(char::is_control)
        {
            return Err(ApplicationError::Invalid);
        }
        Ok(Self {
            issuer: issuer.into(),
            installation_id: installation_id.into(),
            display_label: display_label.into(),
        })
    }
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }
}

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct ApplicationCredential {
    issuer: String,
    installation_id: String,
    grant_id: String,
    budget_credits: u64,
    access: String,
    refresh: String,
    scope: String,
    expires_at: u64,
}
fn hex_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn secret(prefix: &str, value: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|raw| {
        URL_SAFE_NO_PAD
            .decode(raw)
            .is_ok_and(|bytes| bytes.len() == 32 && URL_SAFE_NO_PAD.encode(bytes) == raw)
    })
}
fn scopes(value: &str) -> Option<BTreeSet<&str>> {
    if value.len() > 8192 {
        return None;
    }
    let parts = value.split(' ').collect::<Vec<_>>();
    let distinct = parts.iter().copied().collect::<BTreeSet<_>>();
    if distinct.len() != parts.len()
        || !distinct.iter().any(|scope| SCOPES.contains(scope))
        || !distinct.iter().any(|scope| scope.starts_with("group:"))
        || !distinct.iter().all(|scope| {
            SCOPES.contains(scope)
                || scope.strip_prefix("group:").is_some_and(|encoded| {
                    URL_SAFE_NO_PAD.decode(encoded).is_ok_and(|bytes| {
                        !bytes.is_empty()
                            && bytes.len() <= 64
                            && URL_SAFE_NO_PAD.encode(&bytes) == encoded
                            && std::str::from_utf8(&bytes)
                                .is_ok_and(|group| !group.chars().any(char::is_control))
                    })
                })
        })
    {
        return None;
    }
    Some(distinct)
}
impl ApplicationCredential {
    fn valid_for(&self, instance: &ApplicationInstance) -> bool {
        self.issuer == instance.issuer
            && self.installation_id == instance.installation_id
            && self.grant_id.strip_prefix("lmm_gr_").is_some_and(hex_id)
            && (1..=9_007_199_254_740_991).contains(&self.budget_credits)
            && self.expires_at > 0
            && secret("lmm_at_", &self.access)
            && secret("lmm_rt_", &self.refresh)
            && scopes(&self.scope).is_some()
    }
    fn same_family(&self, previous: &Self) -> bool {
        self.issuer == previous.issuer
            && self.installation_id == previous.installation_id
            && self.grant_id == previous.grant_id
            && self.budget_credits == previous.budget_credits
            && self.refresh != previous.refresh
            && scopes(&self.scope)
                .zip(scopes(&previous.scope))
                .is_some_and(|(new, old)| new.is_subset(&old))
    }
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct ApplicationToken {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: u64,
    scope: String,
    client_id: String,
    installation_id: String,
    grant_id: String,
    budget_credits: u64,
}
impl ApplicationToken {
    fn credential(
        &self,
        instance: &ApplicationInstance,
        started: u64,
    ) -> Result<ApplicationCredential> {
        if self.client_id != CLIENT
            || self.token_type != "Bearer"
            || !(1..=86400).contains(&self.expires_in)
        {
            return Err(ApplicationError::Invalid);
        }
        let result = ApplicationCredential {
            issuer: instance.issuer.clone(),
            installation_id: self.installation_id.clone(),
            grant_id: self.grant_id.clone(),
            budget_credits: self.budget_credits,
            access: self.access_token.clone(),
            refresh: self.refresh_token.clone(),
            scope: self.scope.clone(),
            expires_at: started
                .checked_add(self.expires_in)
                .ok_or(ApplicationError::Invalid)?,
        };
        if !result.valid_for(instance) {
            return Err(ApplicationError::Invalid);
        }
        Ok(result)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum ApplicationSession {
    Ready {
        credential: ApplicationCredential,
    },
    Refreshing {
        credential: ApplicationCredential,
        rotation_id: String,
        started_at: u64,
    },
}
impl ApplicationSession {
    fn credential(&self) -> &ApplicationCredential {
        match self {
            Self::Ready { credential } | Self::Refreshing { credential, .. } => credential,
        }
    }
    fn valid_for(&self, instance: &ApplicationInstance) -> bool {
        self.credential().valid_for(instance)
            && match self {
                Self::Ready { .. } => true,
                Self::Refreshing {
                    rotation_id,
                    started_at,
                    ..
                } => hex_id(rotation_id) && *started_at > 0,
            }
    }
}
trait ApplicationStore {
    fn load(&self) -> Result<Option<ApplicationSession>>;
    fn save(&self, session: &ApplicationSession) -> Result<()>;
    fn delete(&self) -> Result<()>;
}
struct SystemApplicationStore<'a> {
    entry: keyring::Entry,
    instance: &'a ApplicationInstance,
}
impl<'a> SystemApplicationStore<'a> {
    fn new(instance: &'a ApplicationInstance) -> Result<Self> {
        let account = format!("{}#{}", instance.issuer, instance.installation_id);
        let entry = keyring::Entry::new("best.lmm.cli.applications", &account)
            .map_err(|_| ApplicationError::CredentialStore)?;
        Ok(Self { entry, instance })
    }
}
impl ApplicationStore for SystemApplicationStore<'_> {
    fn load(&self) -> Result<Option<ApplicationSession>> {
        let raw = match self.entry.get_password() {
            Ok(raw) => Zeroizing::new(raw),
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(_) => return Err(ApplicationError::CredentialStore),
        };
        if raw.len() > 64 * 1024 {
            return Err(ApplicationError::CredentialStore);
        }
        let result: ApplicationSession =
            serde_json::from_str(&raw).map_err(|_| ApplicationError::CredentialStore)?;
        if !result.valid_for(self.instance) {
            return Err(ApplicationError::CredentialStore);
        }
        Ok(Some(result))
    }
    fn save(&self, session: &ApplicationSession) -> Result<()> {
        if !session.valid_for(self.instance) {
            return Err(ApplicationError::CredentialStore);
        }
        let raw = Zeroizing::new(
            serde_json::to_string(session).map_err(|_| ApplicationError::CredentialStore)?,
        );
        self.entry
            .set_password(&raw)
            .map_err(|_| ApplicationError::CredentialStore)
    }
    fn delete(&self) -> Result<()> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(ApplicationError::CredentialStore),
        }
    }
}

trait RotationExchange {
    fn rotate(
        &self,
        credential: &ApplicationCredential,
        rotation_id: &str,
    ) -> Result<ApplicationCredential>;
}
struct ApplicationTransport<'a> {
    transport: Transport,
    instance: &'a ApplicationInstance,
}
impl<'a> ApplicationTransport<'a> {
    fn new(instance: &'a ApplicationInstance) -> Result<Self> {
        Ok(Self {
            transport: Transport::new(&instance.issuer)?,
            instance,
        })
    }
    fn discover(&self) -> Result<()> {
        let base = &self.transport;
        let authorization = base.request(
            base.client.get(format!(
                "{}/.well-known/oauth-authorization-server",
                base.issuer
            )),
            64 * 1024,
        )?;
        let resource = base.request(
            base.client.get(format!(
                "{}/.well-known/oauth-protected-resource/api/oauth2",
                base.issuer
            )),
            64 * 1024,
        )?;
        protocol::validate_metadata(&authorization, &resource, &base.issuer)?;
        let value: serde_json::Value =
            serde_json::from_slice(&authorization).map_err(|_| ApplicationError::Invalid)?;
        let clients = value["lmm_managed_clients"]
            .as_array()
            .ok_or(ApplicationError::Unavailable)?;
        let mut profiles = clients
            .iter()
            .filter(|profile| profile["client_id"] == CLIENT);
        let profile = profiles.next().ok_or(ApplicationError::Unavailable)?;
        if profiles.next().is_some()
            || profile["application"] != "cc-switch"
            || profile["status_endpoint"] != format!("{}/application", base.resource)
            || profile["refresh_recovery"]["contract"] != "lmm.refresh-recovery.v1"
            || profile["refresh_recovery"]["window_seconds"].as_u64() != Some(RECOVERY_SECONDS)
        {
            return Err(ApplicationError::Unavailable);
        }
        Ok(())
    }
    fn token(&self, fields: &[(&str, &str)]) -> Result<ApplicationCredential> {
        let started = now();
        let body = self.transport.request(
            self.transport
                .client
                .post(format!("{}/token", self.transport.resource))
                .form(fields),
            64 * 1024,
        )?;
        let wire: ApplicationToken =
            serde_json::from_slice(&body).map_err(|_| ApplicationError::Invalid)?;
        wire.credential(self.instance, started)
    }
    fn revoke(&self, credential: &ApplicationCredential) -> Result<()> {
        self.transport.request(
            self.transport
                .client
                .post(format!("{}/revoke", self.transport.resource))
                .form(&[
                    ("client_id", CLIENT),
                    ("token", credential.refresh.as_str()),
                    ("token_type_hint", "refresh_token"),
                ]),
            64 * 1024,
        )?;
        Ok(())
    }
}
impl RotationExchange for ApplicationTransport<'_> {
    fn rotate(
        &self,
        credential: &ApplicationCredential,
        rotation_id: &str,
    ) -> Result<ApplicationCredential> {
        self.token(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT),
            ("refresh_token", &credential.refresh),
            ("resource", &self.transport.resource),
            ("installation_id", &self.instance.installation_id),
            ("rotation_id", rotation_id),
        ])
    }
}
fn new_operation() -> Result<String> {
    let mut bytes = Zeroizing::new([0_u8; 32]);
    OsRng
        .try_fill_bytes(&mut *bytes)
        .map_err(|_| ApplicationError::Invalid)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn refresh(
    instance: &ApplicationInstance,
    exchange: &dyn RotationExchange,
    store: &dyn ApplicationStore,
    session: ApplicationSession,
    at: u64,
) -> Result<ApplicationCredential> {
    refresh_with_margin(instance, exchange, store, session, at, 30)
}
fn refresh_with_margin(
    instance: &ApplicationInstance,
    exchange: &dyn RotationExchange,
    store: &dyn ApplicationStore,
    session: ApplicationSession,
    at: u64,
    minimum_validity: u64,
) -> Result<ApplicationCredential> {
    if !session.valid_for(instance) {
        return Err(ApplicationError::CredentialStore);
    }
    let pending = match session {
        ApplicationSession::Ready { credential }
            if credential.expires_at > at.saturating_add(minimum_validity) =>
        {
            return Ok(credential);
        }
        ApplicationSession::Ready { credential } => {
            let pending = ApplicationSession::Refreshing {
                credential,
                rotation_id: new_operation()?,
                started_at: at,
            };
            // A failed save means zero HTTP writes. Keep the old grant in the
            // vault and retry loading it on the next explicitly requested call.
            store.save(&pending)?;
            pending
        }
        pending @ ApplicationSession::Refreshing { .. } => pending,
    };
    let ApplicationSession::Refreshing {
        credential,
        rotation_id,
        started_at,
    } = pending
    else {
        return Err(ApplicationError::Invalid);
    };
    if at < started_at || at.saturating_sub(started_at) >= RECOVERY_SECONDS {
        return Err(ApplicationError::RecoveryExpired);
    }
    let next = exchange.rotate(&credential, &rotation_id)?;
    if !next.valid_for(instance) || !next.same_family(&credential) || next.expires_at <= at {
        return Err(ApplicationError::Invalid);
    }
    // If this save is ambiguous, the vault contains either Ready(next) or the
    // same Refreshing(old,operation). Neither state sends a new old-token op.
    store.save(&ApplicationSession::Ready {
        credential: next.clone(),
    })?;
    Ok(next)
}

#[derive(Serialize)]
pub struct ApplicationStatus {
    pub contract: &'static str,
    pub client_id: &'static str,
    pub issuer: String,
    pub installation_id: String,
    pub grant_id: String,
    pub budget_credits: u64,
    pub access_expires_at: u64,
    pub software_configuration_changed: bool,
}
fn status(credential: &ApplicationCredential) -> ApplicationStatus {
    ApplicationStatus {
        contract: "lmm.application-grant.v1",
        client_id: CLIENT,
        issuer: credential.issuer.clone(),
        installation_id: credential.installation_id.clone(),
        grant_id: credential.grant_id.clone(),
        budget_credits: credential.budget_credits,
        access_expires_at: credential.expires_at,
        software_configuration_changed: false,
    }
}

pub fn authorize(
    instance: &ApplicationInstance,
    timeout: Duration,
    show_url: impl FnOnce(&str) -> std::result::Result<(), AuthError>,
) -> Result<ApplicationStatus> {
    if !(Duration::from_secs(10)..=Duration::from_secs(600)).contains(&timeout) {
        return Err(ApplicationError::Invalid);
    }
    let _lock = storage::lock()?;
    let store = SystemApplicationStore::new(instance)?;
    if store.load()?.is_some() {
        return Err(ApplicationError::AlreadyAuthorized);
    }
    let transport = ApplicationTransport::new(instance)?;
    transport.discover()?;
    let callback = callback::Callback::bind()?;
    let handshake = protocol::Handshake::new()?;
    let redirect = callback.redirect_uri()?;
    let mut authorization = handshake.authorization_url(&instance.issuer, &redirect)?;
    let fields = authorization
        .query_pairs()
        .map(|(key, value)| {
            let value = match key.as_ref() {
                "client_id" => CLIENT.to_owned(),
                "scope" => SCOPES.join(" "),
                _ => value.into_owned(),
            };
            (key.into_owned(), value)
        })
        .collect::<Vec<_>>();
    authorization.set_query(None);
    authorization
        .query_pairs_mut()
        .extend_pairs(fields)
        .extend_pairs([
            ("installation_id", instance.installation_id.as_str()),
            ("application", "cc-switch"),
            ("display_label", instance.display_label.as_str()),
        ]);
    show_url(authorization.as_str())?;
    let code = Zeroizing::new(callback.wait(&instance.issuer, &handshake.state, timeout)?);
    let credential = transport
        .token(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT),
            ("resource", &transport.transport.resource),
            ("code", &code),
            ("redirect_uri", &redirect),
            ("code_verifier", &handshake.verifier),
            ("installation_id", &instance.installation_id),
        ])
        .map_err(|error| match error {
            ApplicationError::Network | ApplicationError::Invalid => {
                ApplicationError::AuthorizationUnknown
            }
            other => other,
        })?;
    if !scopes(&credential.scope)
        .is_some_and(|values| SCOPES.iter().all(|scope| values.contains(scope)))
    {
        return Err(if transport.revoke(&credential).is_ok() {
            ApplicationError::Unauthorized
        } else {
            ApplicationError::UnstoredGrant
        });
    }
    let result = status(&credential);
    if store
        .save(&ApplicationSession::Ready {
            credential: credential.clone(),
        })
        .is_err()
    {
        return Err(if transport.revoke(&credential).is_ok() {
            ApplicationError::CredentialStore
        } else {
            ApplicationError::UnstoredGrant
        });
    }
    Ok(result)
}

/// Revocation remains possible when new-client discovery is disabled. Local
/// records are deleted only after the issuer acknowledges family revocation.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevocationOutcome {
    Revoked,
    NoLocalCredential,
}
pub fn revoke(instance: &ApplicationInstance) -> Result<RevocationOutcome> {
    let _lock = storage::lock()?;
    let store = SystemApplicationStore::new(instance)?;
    if let Some(session) = store.load()? {
        ApplicationTransport::new(instance)?.revoke(session.credential())?;
        store.delete()?;
        return Ok(RevocationOutcome::Revoked);
    }
    Ok(RevocationOutcome::NoLocalCredential)
}

/// This proof is constructed only after the application's own grant obtains a
/// current catalog from the confirmed issuer. It contains no bearer credential.
pub struct VerifiedCatalog {
    identity: ApplicationStatus,
    catalog: ModelCatalog,
}
impl VerifiedCatalog {
    pub fn status(&self) -> &ApplicationStatus {
        &self.identity
    }
    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }
    pub fn allows_model_calls(&self, at: u64) -> bool {
        self.identity.access_expires_at > at
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveApplicationStatus {
    contract: String,
    client_id: String,
    application: String,
    installation_id: String,
    grant_id: String,
    budget_credits: u64,
    scopes: Vec<String>,
    access_expires_at: u64,
}
impl LiveApplicationStatus {
    fn validate(&self, credential: &ApplicationCredential, at: u64) -> Result<()> {
        let joined = self.scopes.join(" ");
        let current = scopes(&joined).ok_or(ApplicationError::Invalid)?;
        let original = scopes(&credential.scope).ok_or(ApplicationError::Invalid)?;
        if self.contract != "lmm.application-status.v1"
            || self.client_id != CLIENT
            || self.application != "cc-switch"
            || self.installation_id != credential.installation_id
            || self.grant_id != credential.grant_id
            || self.budget_credits != credential.budget_credits
            || self.access_expires_at <= at
            || self.access_expires_at > at.saturating_add(86400)
            || !current.contains("models:invoke")
            || !current.contains("catalog:read")
            || !current.is_subset(&original)
        {
            return Err(ApplicationError::Unauthorized);
        }
        Ok(())
    }
}
fn live_status(
    transport: &ApplicationTransport<'_>,
    credential: &ApplicationCredential,
) -> Result<LiveApplicationStatus> {
    let proof = transport.transport.request(
        transport
            .transport
            .client
            .get(format!("{}/application", transport.transport.resource))
            .bearer_auth(&credential.access),
        64 * 1024,
    )?;
    let live: LiveApplicationStatus =
        serde_json::from_slice(&proof).map_err(|_| ApplicationError::Invalid)?;
    live.validate(credential, now())?;
    Ok(live)
}

/// Codex rust-v0.166.0 invokes its provider auth command directly (no shell),
/// trims stdout, and sends it as Authorization: Bearer. This is an explicit
/// credential pipe, never a status/report field. The command entry must reject
/// terminal stdout and --json before calling this function.
///
/// The generated native config caches this result for at most 60 seconds. Do
/// not hand it a token with less than 90 seconds of confirmed life remaining.
/// No browser authorization, model request, or fallback grant happens here.
pub fn write_codex_credential(
    instance: &ApplicationInstance,
    expected_grant_id: &str,
    output: &mut dyn std::io::Write,
) -> Result<()> {
    if !expected_grant_id
        .strip_prefix("lmm_gr_")
        .is_some_and(hex_id)
    {
        return Err(ApplicationError::Invalid);
    }
    let _lock = storage::lock()?;
    let store = SystemApplicationStore::new(instance)?;
    let transport = ApplicationTransport::new(instance)?;
    let previous = store.load()?.ok_or(ApplicationError::Unauthorized)?;
    if previous.credential().grant_id != expected_grant_id {
        return Err(ApplicationError::Unauthorized);
    }
    let credential = refresh_with_margin(instance, &transport, &store, previous, now(), 90)?;
    let live = live_status(&transport, &credential)?;
    emit_codex_credential(&credential, &live, expected_grant_id, now(), output)
}
fn emit_codex_credential(
    credential: &ApplicationCredential,
    live: &LiveApplicationStatus,
    expected_grant_id: &str,
    at: u64,
    output: &mut dyn std::io::Write,
) -> Result<()> {
    live.validate(credential, at)?;
    if credential.grant_id != expected_grant_id
        || credential.expires_at.min(live.access_expires_at) <= at.saturating_add(90)
    {
        return Err(ApplicationError::Unauthorized);
    }
    output
        .write_all(credential.access.as_bytes())
        .and_then(|()| output.write_all(b"\n"))
        .and_then(|()| output.flush())
        .map_err(|_| ApplicationError::CredentialOutput)
}

pub fn catalog(instance: &ApplicationInstance) -> Result<VerifiedCatalog> {
    let _lock = storage::lock()?;
    let store = SystemApplicationStore::new(instance)?;
    let transport = ApplicationTransport::new(instance)?;
    let previous = store.load()?.ok_or(ApplicationError::Unauthorized)?;
    let credential = refresh(instance, &transport, &store, previous, now())?;
    let allowed = scopes(&credential.scope).ok_or(ApplicationError::Invalid)?;
    if !allowed.contains("catalog:read") || !allowed.contains("models:invoke") {
        return Err(ApplicationError::Unauthorized);
    }
    // Bind source claims to the remote bearer again after loading the vault.
    // A copied CLI/Pi token and edited local metadata cannot create this proof.
    let live = live_status(&transport, &credential)?;
    let body = transport.transport.request(
        transport
            .transport
            .client
            .get(format!("{}/catalog", transport.transport.resource))
            .bearer_auth(&credential.access),
        2 * 1024 * 1024,
    )?;
    let catalog: ModelCatalog =
        serde_json::from_slice(&body).map_err(|_| ApplicationError::Invalid)?;
    if catalog.schema_version != 1
        || catalog.resource != transport.transport.resource
        || credential.expires_at <= now()
    {
        return Err(ApplicationError::Invalid);
    }
    let mut identity = status(&credential);
    identity.access_expires_at = identity.access_expires_at.min(live.access_expires_at);
    Ok(VerifiedCatalog { identity, catalog })
}

#[cfg(test)]
#[path = "application_tests.rs"]
mod tests;
