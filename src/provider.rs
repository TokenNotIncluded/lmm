//! Read-only built-in provider presets ported from Magpie's MIT data.
//! No provider configuration, credentials, or network requests are accessed.

use crate::i18n::{Locale, Message, msg};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const UPSTREAM_REVISION: &str = "d5cae267a727e23f4b6e2b28aa107ecdeac5aa9c";
const PRESETS: &str = include_str!("../third_party/magpie/presets.json");

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Vendor,
    Relay,
    Local,
    Remote,
}

impl Kind {
    /// Localized category label; as_str and serialized values stay stable.
    pub const fn label(self) -> Message {
        msg(match self {
            Self::Vendor => "kind.vendor",
            Self::Relay => "kind.relay",
            Self::Local => "kind.local",
            Self::Remote => "kind.remote",
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vendor => "vendor",
            Self::Relay => "relay",
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

/// An available preset definition, never a configured or tested provider.
#[derive(Debug, Deserialize, Serialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    /// Retain all upstream endpoint, region, and plan fields without loss.
    #[serde(flatten)]
    pub definition: Map<String, Value>,
}

#[derive(Serialize)]
pub struct Upstream {
    repository: &'static str,
    revision: &'static str,
    source: &'static str,
    license: &'static str,
}

#[derive(Serialize)]
pub struct Snapshot {
    schema_version: u32,
    scope: &'static str,
    configuration: &'static str,
    connectivity: &'static str,
    pub upstream: Upstream,
    pub presets: Vec<Preset>,
}

/// Search only the embedded snapshot by id, display name, or provider kind.
pub fn search(query: &str) -> Result<Snapshot, serde_json::Error> {
    let query = query.trim().to_lowercase();
    let presets: Vec<Preset> = serde_json::from_str(PRESETS)?;
    let presets = presets
        .into_iter()
        .filter(|preset| {
            query.is_empty()
                || preset.id.to_lowercase().contains(&query)
                || preset.name.to_lowercase().contains(&query)
                || preset.kind.as_str().contains(&query)
                || Locale::ALL.iter().any(|locale| {
                    locale
                        .text(preset.kind.label().key())
                        .to_lowercase()
                        .contains(&query)
                })
        })
        .collect();
    Ok(Snapshot {
        schema_version: 1,
        scope: "builtin_preset_snapshot",
        configuration: "not_inspected",
        connectivity: "not_checked",
        upstream: Upstream {
            repository: "https://github.com/yetone/magpie",
            revision: UPSTREAM_REVISION,
            source: "internal/provider/presets.go",
            license: "MIT",
        },
        presets,
    })
}
