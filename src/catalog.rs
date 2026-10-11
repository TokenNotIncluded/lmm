//! Explicit project identities and independently stated integration capabilities.

use crate::i18n::{self, Locale};
use serde::Serialize;

/// A capability with no verified adapter must never be promoted by discovery.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// This release can inspect local evidence without modifying the target.
    InspectOnly,
    /// No implementation has been qualified for this action.
    Unavailable,
}

/// An individual software distribution; desktop and CLI identities stay separate.
#[derive(Debug, Serialize)]
pub struct Software {
    /// Stable CLI selector.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Canonical project identity, not a download mirror.
    pub source: &'static str,
    /// Message key for intended use; serialized as a localized description.
    #[serde(serialize_with = "i18n::serialize_message")]
    pub purpose: &'static str,
    /// Installation capability of this LMM release.
    pub installation: Capability,
    /// Integration capability of this LMM release.
    pub integration: Capability,
    /// Maintenance capability of this LMM release.
    pub maintenance: Capability,
    /// Verified version/OS/scenario tuples; an empty list means unverified.
    pub verified_combinations: &'static [&'static str],
    /// Binary names searched only in the current process's PATH.
    pub executables: &'static [&'static str],
}

const fn software(
    id: &'static str,
    name: &'static str,
    source: &'static str,
    purpose: &'static str,
    executables: &'static [&'static str],
) -> Software {
    Software {
        id,
        name,
        source,
        purpose,
        installation: Capability::Unavailable,
        integration: Capability::Unavailable,
        maintenance: Capability::InspectOnly,
        verified_combinations: &[],
        executables,
    }
}

/// Initial discovery catalog. No third-party setup is qualified yet.
pub static SOFTWARE: &[Software] = &[
    software(
        "astrbot",
        "AstrBot",
        "https://github.com/AstrBotDevs/AstrBot",
        "software.astrbot",
        &["astrbot"],
    ),
    software(
        "cc-switch",
        "CC Switch",
        "https://github.com/farion1231/cc-switch",
        "software.cc_switch",
        &["cc-switch"],
    ),
    software(
        "claude-code",
        "Claude Code CLI",
        "https://github.com/anthropics/claude-code",
        "software.coding_cli",
        &["claude"],
    ),
    software(
        "codex",
        "OpenAI Codex CLI",
        "https://github.com/openai/codex",
        "software.coding_cli",
        &["codex"],
    ),
    software(
        "gemini-cli",
        "Gemini CLI",
        "https://github.com/google-gemini/gemini-cli",
        "software.coding_cli",
        &["gemini"],
    ),
    software(
        "pi",
        "Pi coding agent",
        "https://github.com/badlogic/pi-mono",
        "software.pi",
        &["pi"],
    ),
];

/// Look up a single unambiguous project.
pub fn find(id: &str) -> Option<&'static Software> {
    SOFTWARE.iter().find(|software| software.id == id)
}

/// Search project identity and purpose without network access.
pub fn search(query: &str) -> Vec<&'static Software> {
    let query = query.to_lowercase();
    SOFTWARE
        .iter()
        .filter(|item| {
            [item.id, item.name]
                .iter()
                .any(|text| text.to_lowercase().contains(&query))
                || Locale::ALL.iter().any(|&locale| {
                    i18n::text(locale, item.purpose)
                        .to_lowercase()
                        .contains(&query)
                })
        })
        .collect()
}
