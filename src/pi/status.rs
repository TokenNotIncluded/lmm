//! Read only explicitly selected Pi 1.2.0 settings without running its SDK or acquiring file locks.

use crate::discovery::Observation;
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::Path;

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

// Each directory descriptor anchors the next lookup, so replacing an ancestor with a
// symlink cannot redirect the final read. The leaf is opened with O_NOFOLLOW too.
#[cfg(target_os = "linux")]
mod safe_read {
    use super::{MAX_BYTES, Map, Value};
    use rustix::{
        fd::OwnedFd,
        fs::{Mode, OFlags, open, openat},
        io::Errno,
    };
    use std::{
        fs::File,
        io::Read,
        path::{Component, Path},
    };

    fn directory_fd(path: &Path) -> Result<Option<OwnedFd>, ()> {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(());
        }
        let flags = OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut current = open("/", flags, Mode::empty()).map_err(|_| ())?;
        for part in path.components() {
            match part {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => match openat(&current, name, flags, Mode::empty()) {
                    Ok(next) => current = next,
                    Err(Errno::NOENT) => return Ok(None),
                    Err(_) => return Err(()),
                },
                _ => return Err(()),
            }
        }
        Ok(Some(current))
    }

    pub(super) fn directory(path: &Path) -> Result<bool, ()> {
        directory_fd(path).map(|fd| fd.is_some())
    }

    pub(super) fn read_json(path: &Path) -> Result<Option<Map<String, Value>>, ()> {
        let Some(parent) = directory_fd(path.parent().ok_or(())?)? else {
            return Ok(None);
        };
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        let fd = match openat(&parent, path.file_name().ok_or(())?, flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::NOENT) => return Ok(None),
            Err(_) => return Err(()),
        };
        let file = File::from(fd);
        let metadata = file.metadata().map_err(|_| ())?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return Err(());
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(());
        }
        if bytes.is_empty() {
            return Ok(Some(Map::new()));
        }
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
        serde_json::from_slice(bytes).map(Some).map_err(|_| ())
    }
}

#[cfg(target_os = "linux")]
use safe_read::{directory, read_json};

#[cfg(not(target_os = "linux"))]
fn directory(_path: &Path) -> Result<bool, ()> {
    Err(())
}

#[cfg(not(target_os = "linux"))]
fn read_json(_path: &Path) -> Result<Option<Map<String, Value>>, ()> {
    Err(())
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Lmm,
    Other,
    Unknown,
}

// Follow Pi 1.2.0's npm spec trimming and Git host/path parsing for known forms.
// Local and unrecognized forms stay unknown; they are never resolved on disk.
fn source_kind(source: &str) -> SourceKind {
    const NPM_NAME: &str = "@tokennotincluded/pi-lmm-provider";
    if let Some(spec) = source.strip_prefix("npm:") {
        let spec = spec.trim();
        let version_at = if let Some(scoped) = spec.strip_prefix('@') {
            scoped.find('@').map(|index| index + 1)
        } else {
            spec.find('@')
        };
        let (name, valid_version) = match version_at {
            Some(index) => (
                &spec[..index],
                index + 1 < spec.len()
                    && spec[index + 1..].chars().all(|c| {
                        c.is_ascii_alphanumeric()
                            || matches!(
                                c,
                                '.' | '_'
                                    | '-'
                                    | '+'
                                    | '*'
                                    | '^'
                                    | '~'
                                    | '<'
                                    | '>'
                                    | '='
                                    | '|'
                                    | ' '
                            )
                    }),
            ),
            None => (spec, true),
        };
        let valid_name = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '.' | '_' | '-'))
            && if let Some(scoped) = name.strip_prefix('@') {
                scoped.split_once('/').is_some_and(|(scope, package)| {
                    !scope.is_empty() && !package.is_empty() && !package.contains('/')
                })
            } else {
                !name.contains(['@', '/'])
            };
        return match (valid_name && valid_version, name == NPM_NAME) {
            (true, true) => SourceKind::Lmm,
            (true, false) => SourceKind::Other,
            _ => SourceKind::Unknown,
        };
    }
    let trimmed = source.trim();
    let git_prefixed = trimmed.starts_with("git:");
    let git = trimmed.strip_prefix("git:").map_or(trimmed, str::trim);
    let (host, path) = if let Some(path) = git.strip_prefix("git@github.com:") {
        if !git_prefixed {
            return SourceKind::Unknown;
        }
        ("github.com".to_owned(), path.to_owned())
    } else if let Some(path) = git.strip_prefix("github:") {
        if !git_prefixed {
            return SourceKind::Unknown;
        }
        ("github.com".to_owned(), path.to_owned())
    } else if git.contains("://") {
        let Ok(url) = url::Url::parse(git) else {
            return SourceKind::Unknown;
        };
        if !matches!(url.scheme(), "http" | "https" | "ssh" | "git") {
            return SourceKind::Unknown;
        }
        let Some(host) = url.host_str() else {
            return SourceKind::Unknown;
        };
        (
            host.to_owned(),
            url.path().trim_start_matches('/').to_owned(),
        )
    } else if git_prefixed {
        let Some((host, path)) = git.split_once('/') else {
            return SourceKind::Unknown;
        };
        if !host.contains('.') {
            return SourceKind::Unknown;
        }
        (host.to_owned(), path.to_owned())
    } else {
        return SourceKind::Unknown;
    };
    let path = path.split('#').next().unwrap_or_default();
    let path = match path.split_once('@') {
        Some((repo, reference)) if !repo.is_empty() && !reference.is_empty() => repo,
        Some(_) => return SourceKind::Unknown,
        None => path,
    };
    let Some((owner, repo)) = path.split_once('/') else {
        return SourceKind::Unknown;
    };
    // Decode each component separately: an encoded slash must not change the
    // Git host/path boundary recognized by Pi's hosted-git parser.
    let Ok(owner) = percent_encoding::percent_decode_str(owner).decode_utf8() else {
        return SourceKind::Unknown;
    };
    let Ok(repo) = percent_encoding::percent_decode_str(repo).decode_utf8() else {
        return SourceKind::Unknown;
    };
    let repo = repo.strip_suffix(".git").unwrap_or(&repo);
    let valid = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if !valid(&owner) || !valid(repo) {
        return SourceKind::Unknown;
    }
    if host.eq_ignore_ascii_case("github.com")
        && owner.eq_ignore_ascii_case("TokenNotIncluded")
        && repo.eq_ignore_ascii_case("pi-lmm-provider")
    {
        SourceKind::Lmm
    } else {
        SourceKind::Other
    }
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
    let sources: Vec<_> = global.packages.iter().chain(&local.packages).collect();
    let references: Vec<_> = sources
        .iter()
        .filter(|source| source_kind(source) == SourceKind::Lmm)
        .collect();
    let unresolved = sources
        .iter()
        .any(|source| source_kind(source) == SourceKind::Unknown);
    result.provider_reference = match (references.as_slice(), unresolved) {
        ([], true) => "unknown",
        (_, true) => "known_with_unresolved_sources",
        ([], false) => "no_known_source",
        ([source], false) if source.as_str() == super::SOURCE => "pinned",
        ([_], false) => "other_known_source",
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
