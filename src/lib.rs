//! Environment discovery and conservative planning for LMM integrations.
//!
//! Discovery is read-only. A catalog entry is not a compatibility claim.

pub mod catalog;
pub mod cc_switch;
pub mod discovery;
pub mod environment;
pub mod managed;
pub mod oauth;
pub mod plan;
pub mod provider;
pub mod restore;

pub mod pi;
