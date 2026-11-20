#![allow(clippy::unwrap_used)]
//! Synthetic unit evidence for durable refresh transitions only. These cases
//! do not claim Core issuance, operating-system vault or application acceptance.
use super::*;
use std::cell::{Cell, RefCell};

struct Store {
    value: RefCell<Option<String>>,
    saves: Cell<usize>,
    fail_on: Cell<usize>,
}
impl Store {
    fn with(credential: ApplicationCredential) -> Self {
        Self {
            value: RefCell::new(Some(
                serde_json::to_string(&ApplicationSession::Ready { credential }).unwrap(),
            )),
            saves: Cell::new(0),
            fail_on: Cell::new(0),
        }
    }
}
impl ApplicationStore for Store {
    fn load(&self) -> Result<Option<ApplicationSession>> {
        self.value
            .borrow()
            .as_ref()
            .map(|raw| serde_json::from_str(raw).map_err(|_| ApplicationError::CredentialStore))
            .transpose()
    }
    fn save(&self, session: &ApplicationSession) -> Result<()> {
        let count = self.saves.get() + 1;
        self.saves.set(count);
        if self.fail_on.get() == count {
            return Err(ApplicationError::CredentialStore);
        }
        *self.value.borrow_mut() =
            Some(serde_json::to_string(session).map_err(|_| ApplicationError::CredentialStore)?);
        Ok(())
    }
    fn delete(&self) -> Result<()> {
        *self.value.borrow_mut() = None;
        Ok(())
    }
}
struct Exchange {
    sent: RefCell<Vec<(String, String)>>,
    lose_first: bool,
    next: ApplicationCredential,
}
impl RotationExchange for Exchange {
    fn rotate(
        &self,
        old: &ApplicationCredential,
        operation: &str,
    ) -> Result<ApplicationCredential> {
        let mut sent = self.sent.borrow_mut();
        sent.push((old.refresh.clone(), operation.to_owned()));
        if self.lose_first && sent.len() == 1 {
            return Err(ApplicationError::Network);
        }
        Ok(self.next.clone())
    }
}
fn instance() -> ApplicationInstance {
    ApplicationInstance::new(
        "https://synthetic.example",
        &"a".repeat(64),
        "Synthetic workstation",
    )
    .unwrap()
}
fn credential(byte: u8, expires_at: u64) -> ApplicationCredential {
    ApplicationCredential {
        issuer: "https://synthetic.example".into(),
        installation_id: "a".repeat(64),
        grant_id: format!("lmm_gr_{}", "b".repeat(64)),
        budget_credits: 300,
        access: format!("lmm_at_{}", URL_SAFE_NO_PAD.encode([byte; 32])),
        refresh: format!("lmm_rt_{}", URL_SAFE_NO_PAD.encode([byte + 1; 32])),
        scope: format!("{} group:ZGVmYXVsdA", SCOPES.join(" ")),
        expires_at,
    }
}
fn exchange(lose_first: bool) -> Exchange {
    Exchange {
        sent: RefCell::new(Vec::new()),
        lose_first,
        next: credential(3, 3000),
    }
}

#[test]
fn missing_persistent_rotation_prevents_any_http_write() {
    let instance = instance();
    let store = Store::with(credential(1, 1001));
    store.fail_on.set(1);
    let transport = exchange(false);
    assert!(matches!(
        refresh(
            &instance,
            &transport,
            &store,
            store.load().unwrap().unwrap(),
            1000
        ),
        Err(ApplicationError::CredentialStore)
    ));
    assert!(transport.sent.borrow().is_empty());
    assert!(matches!(
        store.load().unwrap(),
        Some(ApplicationSession::Ready { .. })
    ));
}

#[test]
fn response_loss_preserves_the_old_token_and_same_operation_through_reload() {
    let instance = instance();
    let store = Store::with(credential(1, 1001));
    let transport = exchange(true);
    assert!(matches!(
        refresh(
            &instance,
            &transport,
            &store,
            store.load().unwrap().unwrap(),
            1000
        ),
        Err(ApplicationError::Network)
    ));
    assert!(matches!(
        store.load().unwrap(),
        Some(ApplicationSession::Refreshing { .. })
    ));
    assert_eq!(transport.sent.borrow().len(), 1);
    let current = refresh(
        &instance,
        &transport,
        &store,
        store.load().unwrap().unwrap(),
        1001,
    )
    .unwrap();
    assert_eq!(current.refresh, transport.next.refresh);
    let sent = transport.sent.borrow();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], sent[1]);
    assert!(hex_id(&sent[0].1));
    assert!(matches!(
        store.load().unwrap(),
        Some(ApplicationSession::Ready { .. })
    ));
}

#[test]
fn successor_save_failure_keeps_recoverable_original_request_without_revocation() {
    let instance = instance();
    let store = Store::with(credential(1, 1001));
    store.fail_on.set(2);
    let transport = exchange(false);
    assert!(matches!(
        refresh(
            &instance,
            &transport,
            &store,
            store.load().unwrap().unwrap(),
            1000
        ),
        Err(ApplicationError::CredentialStore)
    ));
    assert!(matches!(
        store.load().unwrap(),
        Some(ApplicationSession::Refreshing { .. })
    ));
    refresh(
        &instance,
        &transport,
        &store,
        store.load().unwrap().unwrap(),
        1002,
    )
    .unwrap();
    let sent = transport.sent.borrow();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], sent[1]);
}

#[test]
fn wrong_instance_family_budget_or_expanded_scope_cannot_replace_the_grant() {
    let instance = instance();
    for mode in 0..4 {
        let store = Store::with(credential(1, 1001));
        let mut transport = exchange(false);
        match mode {
            0 => transport.next.installation_id = "c".repeat(64),
            1 => transport.next.grant_id = format!("lmm_gr_{}", "d".repeat(64)),
            2 => transport.next.budget_credits += 1,
            _ => transport.next.scope.push_str(" remote:control"),
        }
        assert!(matches!(
            refresh(
                &instance,
                &transport,
                &store,
                store.load().unwrap().unwrap(),
                1000
            ),
            Err(ApplicationError::Invalid)
        ));
        assert!(matches!(
            store.load().unwrap(),
            Some(ApplicationSession::Refreshing { .. })
        ));
    }
    for client in ["lmm", "lmm-pi", "cc-switch"] {
        let old = credential(1, 1001);
        let wire = ApplicationToken {
            access_token: old.access.clone(),
            refresh_token: old.refresh.clone(),
            token_type: "Bearer".into(),
            expires_in: 300,
            scope: old.scope.clone(),
            client_id: client.into(),
            installation_id: old.installation_id.clone(),
            grant_id: old.grant_id.clone(),
            budget_credits: old.budget_credits,
        };
        assert!(matches!(
            wire.credential(&instance, 1000),
            Err(ApplicationError::Invalid)
        ));
    }
}

#[test]
fn recovery_window_expiry_never_sends_a_new_old_token_operation() {
    let instance = instance();
    let store = Store::with(credential(1, 1001));
    let transport = exchange(true);
    assert!(
        refresh(
            &instance,
            &transport,
            &store,
            store.load().unwrap().unwrap(),
            1000
        )
        .is_err()
    );
    assert!(matches!(
        refresh(
            &instance,
            &transport,
            &store,
            store.load().unwrap().unwrap(),
            1600
        ),
        Err(ApplicationError::RecoveryExpired)
    ));
    assert_eq!(transport.sent.borrow().len(), 1);
    assert!(matches!(
        store.load().unwrap(),
        Some(ApplicationSession::Refreshing { .. })
    ));
}

#[test]
fn remote_bearer_status_must_confirm_the_client_instance_family_and_model_scope() {
    let credential = credential(1, 2000);
    let mut live = LiveApplicationStatus {
        contract: "lmm.application-status.v1".into(),
        client_id: CLIENT.into(),
        application: "cc-switch".into(),
        installation_id: credential.installation_id.clone(),
        grant_id: credential.grant_id.clone(),
        budget_credits: credential.budget_credits,
        scopes: credential.scope.split(' ').map(str::to_owned).collect(),
        access_expires_at: 2000,
    };
    assert!(live.validate(&credential, 1000).is_ok());
    for client in ["lmm", "lmm-pi"] {
        live.client_id = client.into();
        assert!(matches!(
            live.validate(&credential, 1000),
            Err(ApplicationError::Unauthorized)
        ));
    }
    live.client_id = CLIENT.into();
    live.installation_id = "c".repeat(64);
    assert!(live.validate(&credential, 1000).is_err());
    live.installation_id = credential.installation_id.clone();
    live.scopes.retain(|scope| scope != "models:invoke");
    assert!(live.validate(&credential, 1000).is_err());
}

#[test]
fn codex_pipe_emits_only_the_confirmed_live_token_and_never_a_replaced_grant() {
    let credential = credential(1, 2000);
    let mut live = LiveApplicationStatus {
        contract: "lmm.application-status.v1".into(),
        client_id: CLIENT.into(),
        application: "cc-switch".into(),
        installation_id: credential.installation_id.clone(),
        grant_id: credential.grant_id.clone(),
        budget_credits: credential.budget_credits,
        scopes: credential.scope.split(' ').map(str::to_owned).collect(),
        access_expires_at: 2000,
    };
    let mut output = Vec::new();
    emit_codex_credential(&credential, &live, &credential.grant_id, 1000, &mut output).unwrap();
    assert_eq!(output, format!("{}\n", credential.access).as_bytes());
    output.clear();
    let replacement = format!("lmm_gr_{}", "d".repeat(64));
    assert!(emit_codex_credential(&credential, &live, &replacement, 1000, &mut output).is_err());
    assert!(output.is_empty());
    live.access_expires_at = 1090;
    assert!(
        emit_codex_credential(&credential, &live, &credential.grant_id, 1000, &mut output).is_err()
    );
    assert!(output.is_empty());
    live.access_expires_at = 2000;
    live.client_id = "lmm".into();
    assert!(
        emit_codex_credential(&credential, &live, &credential.grant_id, 1000, &mut output).is_err()
    );
    assert!(output.is_empty());
}

#[test]
fn codex_refreshes_before_its_native_cache_could_outlive_the_access_token() {
    let instance = instance();
    let store = Store::with(credential(1, 1070));
    let transport = exchange(false);
    let next = refresh_with_margin(
        &instance,
        &transport,
        &store,
        store.load().unwrap().unwrap(),
        1000,
        90,
    )
    .unwrap();
    assert_eq!(transport.sent.borrow().len(), 1);
    assert_eq!(next.access, transport.next.access);
}
