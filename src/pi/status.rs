//! Read only explicitly selected Pi 1.2.0 settings without running its SDK or acquiring file locks.

use crate::discovery::Observation;
use serde::Serialize;
use serde_json::{Map, Value};
use std::{
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

/// Bound each manifest or settings read to the existing discovery configuration limit.
const MAX_BYTES: u64 = 1024 * 1024;

/// A settings snapshot says nothing about loaded code, credentials or live session overrides.
#[derive(Debug, Serialize)]
pub struct Inspection {
    /// Only the declared 1.2.0 package format is supported.
    pub format: &'static str,
    /// File observations expose no setting values.
    pub global_settings: Observation,
    /// Project settings may be absent on a fresh instance.
    pub project_settings: Observation,
    /// Recognized declared sources, without resolving local paths or inspecting installed code.
    pub provider_reference: &'static str,
    /// LMM, another provider, unset or unknown; arbitrary provider names are never copied.
    pub default_provider: &'static str,
    /// Whether a model is set; model identifiers are never copied.
    pub default_model: &'static str,
    /// Scope supplying the provider field, before runtime overrides.
    pub provider_scope: &'static str,
    /// Scope supplying the model field, independently of the provider field.
    pub model_scope: &'static str,
    /// Validation covers only the two selected settings files and known fields.
    pub configuration: &'static str,
    /// Static diagnostic code; never a parser error or file content.
    pub diagnostic: &'static str,
}

impl Inspection {
    fn unknown(diagnostic: &'static str) -> Self {
        Self {
            format: "unknown",
            global_settings: Observation::Unknown,
            project_settings: Observation::Unknown,
            provider_reference: "unknown",
            default_provider: "unknown",
            default_model: "unknown",
            provider_scope: "unknown",
            model_scope: "unknown",
            configuration: "unknown",
            diagnostic,
        }
    }
}

#[derive(Default)]
struct Settings {
    packages: Vec<String>,
    provider: Option<String>,
    model: Option<String>,
}

impl Settings {
    fn parse(map: &Map<String, Value>) -> Option<Self> {
        fn selection(map: &Map<String, Value>, key: &str) -> Option<Option<String>> {
            match map.get(key) {
                None => Some(None),
                Some(Value::String(text)) if !text.is_empty() => Some(Some(text.clone())),
                _ => None,
            }
        }
        let packages = match map.get("packages") {
            None => vec![],
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .or_else(|| item.as_object()?.get("source")?.as_str())
                        .filter(|source| !source.is_empty())
                        .map(str::to_owned)
                })
                .collect::<Option<Vec<_>>>()?,
            _ => return None,
        };
        Some(Self {
            packages,
            provider: selection(map, "defaultProvider")?,
            model: selection(map, "defaultModel")?,
        })
    }
}

// Reject symlinked ancestors as well as the selected file; never resolve a package source.
fn directory(path: &Path) -> Result<bool, ()> {
    if !path.is_absolute() {
        return Err(());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        if matches!(part, Component::ParentDir) {
            return Err(());
        }
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(()),
        }
    }
    Ok(true)
}

fn read_json(path: &Path) -> Result<Option<Map<String, Value>>, ()> {
    if !directory(path.parent().ok_or(())?)? {
        return Ok(None);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_BYTES => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        _ => return Err(()),
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| ())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(());
    }
    if bytes.is_empty() {
        return Ok(Some(Map::new()));
    }
    // Pi strips a UTF-8 BOM after distinguishing an empty file from nonempty input.
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    serde_json::from_slice(bytes).map(Some).map_err(|_| ())
}

fn settings(path: &Path) -> (Observation, Option<Settings>) {
    match read_json(path) {
        Ok(None) => (Observation::NotFound, Some(Settings::default())),
        Ok(Some(map)) => match Settings::parse(&map) {
            Some(settings) => (Observation::Present, Some(settings)),
            None => (Observation::Unknown, None),
        },
        Err(()) => (Observation::Unknown, None),
    }
}

fn known_source(source: &str) -> bool {
    if let Some(suffix) = source.strip_prefix("npm:@tokennotincluded/pi-lmm-provider") {
        return suffix.is_empty() || suffix.starts_with('@');
    }
    let prefix = "git:github.com/TokenNotIncluded/pi-lmm-provider";
    source
        .get(..prefix.len())
        .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
        && source
            .get(prefix.len()..)
            .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with('@'))
}

fn selected<'a>(
    global: &'a Option<String>,
    project: &'a Option<String>,
) -> (Option<&'a str>, &'static str) {
    if let Some(value) = project {
        (Some(value), "project")
    } else if let Some(value) = global {
        (Some(value), "global")
    } else {
        (None, "unset")
    }
}

/// Inspect explicitly selected directories; no processes, state directories or locks are created.
///
/// Unknown versions, inaccessible paths and invalid known fields remain unverified. Only Linux
/// is qualified. Project selection assumes its settings are trusted; runtime trust is not probed.
// @mark cli.pi-settings-inspection role=reader contract=explicit-read-only-instance
pub fn inspect(project: &Path, sdk: &Path, agent: &Path) -> Inspection {
    let mut result = Inspection::unknown("explicit_directories_unavailable");
    if !cfg!(target_os = "linux") {
        result.diagnostic = "platform_not_verified";
        return result;
    }
    if ![project, sdk, agent]
        .iter()
        .all(|path| directory(path) == Ok(true))
    {
        return result;
    }
    match read_json(&sdk.join("package.json")) {
        Ok(Some(manifest))
            if manifest.get("name").and_then(Value::as_str)
                == Some("@earendil-works/pi-coding-agent")
                && manifest.get("version").and_then(Value::as_str) == Some("1.2.0") => {}
        _ => {
            result.diagnostic = "unsupported_package_manifest";
            return result;
        }
    }
    result.format = "pi_1_2_0";
    let (global_observation, global) = settings(&agent.join("settings.json"));
    let (project_observation, local) = settings(&project.join(".pi/settings.json"));
    result.global_settings = global_observation;
    result.project_settings = project_observation;
    let (Some(global), Some(local)) = (global, local) else {
        result.diagnostic = "settings_unreadable_or_invalid";
        return result;
    };
    let references: Vec<_> = global
        .packages
        .iter()
        .chain(&local.packages)
        .filter(|source| known_source(source))
        .collect();
    result.provider_reference = match references.as_slice() {
        [] => "no_known_source",
        [source] if source.as_str() == super::SOURCE => "pinned",
        [_] => "other_known_source",
        _ => "multiple_known_sources",
    };
    let (provider, provider_scope) = selected(&global.provider, &local.provider);
    let (model, model_scope) = selected(&global.model, &local.model);
    result.default_provider = match provider {
        Some("lmm") => "lmm",
        Some(_) => "other",
        None => "unset",
    };
    result.default_model = if model.is_some() { "set" } else { "unset" };
    result.provider_scope = provider_scope;
    result.model_scope = model_scope;
    result.configuration = "passed";
    result.diagnostic = "settings_inspected";
    result
}
