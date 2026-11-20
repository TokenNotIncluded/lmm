//! Policy decisions shared by future adapters; discovery never authorizes a write.

use crate::i18n::{Message, msg};
use serde::Serialize;
use thiserror::Error;

/// Authoritative selection read from the chosen software instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// Qualified adapter confirms no provider has been configured.
    Unconfigured,
    /// LMM is currently selected.
    Lmm,
    /// User selected another provider, including after earlier LMM setup.
    Other,
    /// Selection could not be established.
    Unknown,
}

/// Explicit intent, separate from broad unattended confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Add only; retain selection, even in a new installation.
    AddOnly,
    /// User explicitly requested a switch to LMM.
    Activate,
    /// Choose a conservative default from authoritative current state.
    Default,
    /// Sync, update and repair must always preserve user selection.
    Maintain,
}

/// The only selection change an adapter may propose.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Activation {
    /// Leave current provider and model selection unchanged.
    Preserve,
    /// Proposed change; still requires approval of the plan.
    ProposeLmm,
}

/// A condition that prevents a safe plan.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlanError {
    /// Unknown state must not be silently replaced with defaults.
    #[error("{}", msg("plan.unknown_selection"))]
    UnknownSelection,
    /// A revoked application must not be silently granted permission again.
    #[error("{}", msg("plan.revoked_grant"))]
    RevokedGrant,
    /// Cost checks must use a server-enforced reservation.
    #[error("{}", msg("plan.paid_verification"))]
    PaidVerificationNotAuthorized,
}

/// Plan activation without turning setup repetition into a provider switch.
pub fn activation(selection: Selection, intent: Intent) -> Result<Activation, PlanError> {
    if matches!(intent, Intent::AddOnly | Intent::Maintain) {
        return Ok(Activation::Preserve);
    }
    if selection == Selection::Unknown {
        return Err(PlanError::UnknownSelection);
    }
    if intent == Intent::Activate || selection == Selection::Unconfigured {
        Ok(Activation::ProposeLmm)
    } else {
        Ok(Activation::Preserve)
    }
}

/// Require explicit reauthorization after revocation even under `--all` or `--yes`.
pub fn check_revocation(revoked: bool, explicitly_reauthorize: bool) -> Result<(), PlanError> {
    if revoked && !explicitly_reauthorize {
        Err(PlanError::RevokedGrant)
    } else {
        Ok(())
    }
}

/// Paid verification is possible only with an enforceable, positive budget.
/// Amounts use fixed integer micro-USD rather than floating-point balances.
pub fn check_paid_verification(
    consented: bool,
    quoted_micro_usd: Option<u64>,
    reserved_micro_usd: Option<u64>,
) -> Result<(), PlanError> {
    match (consented, quoted_micro_usd, reserved_micro_usd) {
        (true, Some(quote), Some(limit)) if limit > 0 && quote <= limit => Ok(()),
        _ => Err(PlanError::PaidVerificationNotAuthorized),
    }
}

/// A read-only setup preview; no unavailable adapter is represented as executable.
#[derive(Debug, Serialize)]
pub struct Preview {
    /// The selected software identity.
    pub software: &'static str,
    /// Current setup phase.
    pub outcome: &'static str,
    /// Nothing is applied when required capabilities are missing.
    pub changes: Vec<&'static str>,
    /// Account identity remains unknown without verified CLI login.
    pub account: &'static str,
    /// Requested selection intent; it is not an achieved state.
    pub activation: &'static str,
    /// Whether a model is changed by this plan.
    pub model_change: bool,
    /// This milestone never invokes a paid endpoint.
    pub paid_test: bool,
    /// No discovered service is restarted.
    pub restart: bool,
    /// Specific missing prerequisites.
    pub blockers: Vec<Message>,
}

impl Preview {
    /// Produce an honest blocked plan for integrations still under development.
    pub fn blocked(software: &'static str, intent: Intent) -> Self {
        Self {
            software,
            outcome: "blocked",
            changes: vec![],
            account: "not_checked",
            activation: if intent == Intent::Activate {
                "requested_not_applied"
            } else {
                "preserve"
            },
            model_change: false,
            paid_test: false,
            restart: false,
            blockers: vec![
                msg("plan.write_adapter"),
                msg("plan.application_grant"),
                msg("plan.unverified_target"),
            ],
        }
    }
}
