//! Describe the current process boundary without scanning remote systems.

use serde::Serialize;
use std::{env, fs, path::PathBuf};

/// Environmental signals are independent: WSL can also host containers.
#[derive(Debug, Serialize)]
pub struct Environment {
    /// Current process operating system.
    pub os: &'static str,
    /// Current process architecture.
    pub arch: &'static str,
    /// WSL marker observed, without implying access to the Windows host.
    pub wsl: bool,
    /// Container marker observed; false does not rule out a container.
    pub container_marker: bool,
    /// SSH marker observed; this still describes the machine running LMM.
    pub ssh_session: bool,
    /// No browser is launched during environment discovery.
    pub browser: &'static str,
    /// Scope of every probe in this release.
    pub scope: &'static str,
}

impl Environment {
    /// Inspect a few local process and OS markers only.
    pub fn detect() -> Self {
        let kernel = if cfg!(target_os = "linux") {
            fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default()
        } else {
            String::new()
        };
        Self {
            os: env::consts::OS,
            arch: env::consts::ARCH,
            wsl: env::var_os("WSL_DISTRO_NAME").is_some()
                || kernel.to_lowercase().contains("microsoft"),
            container_marker: PathBuf::from("/.dockerenv").exists()
                || PathBuf::from("/run/.containerenv").exists()
                || env::var_os("container").is_some(),
            ssh_session: env::var_os("SSH_CONNECTION").is_some(),
            browser: "not_checked",
            scope: "current_user_current_environment",
        }
    }
}

/// Obtain the current platform's user home; never fall back to the project root.
pub fn user_home() -> Option<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Personal path categories. Resolving a path does not create it or store secrets.
#[derive(Clone, Copy, Debug)]
pub enum UserDirectory {
    Config,
    State,
    Cache,
}

/// Resolve only the requested category. An unrelated invalid XDG variable must
/// not block an OAuth state operation. Values are paths, never shell expressions.
pub fn user_directory(kind: UserDirectory) -> Option<PathBuf> {
    if cfg!(windows) {
        let key = match kind {
            UserDirectory::Config => "APPDATA",
            UserDirectory::State | UserDirectory::Cache => "LOCALAPPDATA",
        };
        let directory = app_directory(env::var_os(key).map(PathBuf::from))?;
        return Some(match kind {
            UserDirectory::Cache => directory.join("cache"),
            UserDirectory::Config | UserDirectory::State => directory,
        });
    }
    if cfg!(target_os = "macos") {
        let suffix = match kind {
            UserDirectory::Config | UserDirectory::State => "Library/Application Support",
            UserDirectory::Cache => "Library/Caches",
        };
        return app_directory(user_home().map(|home| home.join(suffix)));
    }
    let (key, fallback) = match kind {
        UserDirectory::Config => ("XDG_CONFIG_HOME", ".config"),
        UserDirectory::State => ("XDG_STATE_HOME", ".local/state"),
        UserDirectory::Cache => ("XDG_CACHE_HOME", ".cache"),
    };
    xdg_directory(env::var_os(key), user_home(), fallback)
}

/// The canonical TOML path; this function does not read or write configuration.
pub fn user_config_file() -> Option<PathBuf> {
    user_directory(UserDirectory::Config).map(|directory| directory.join("config.toml"))
}

fn app_directory(base: Option<PathBuf>) -> Option<PathBuf> {
    base.filter(|path| path.is_absolute())
        .map(|path| path.join("lmm"))
}

fn xdg_directory(
    value: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
    fallback: &str,
) -> Option<PathBuf> {
    // Empty and unset XDG variables use the default. Nonempty relative paths
    // retain the previous refusal policy instead of selecting an unexpected root.
    let base = value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(fallback)));
    app_directory(base)
}

#[cfg(all(test, unix))]
mod directory_tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn unset_and_empty_xdg_use_each_personal_default() {
        for fallback in [".config", ".local/state", ".cache"] {
            let home = PathBuf::from("/synthetic-home");
            let expected = Some(home.join(fallback).join("lmm"));
            assert_eq!(xdg_directory(None, Some(home.clone()), fallback), expected);
            assert_eq!(xdg_directory(Some(OsString::new()), Some(home), fallback), expected);
        }
    }

    #[test]
    fn absolute_override_does_not_require_a_home_directory() {
        for fallback in [".config", ".local/state", ".cache"] {
            assert_eq!(
                xdg_directory(Some("/synthetic-xdg".into()), None, fallback),
                Some(PathBuf::from("/synthetic-xdg/lmm"))
            );
        }
    }

    #[test]
    fn relative_override_is_not_silently_replaced_by_home() {
        for value in ["state", "../state", "~/state", "$HOME/state"] {
            assert!(xdg_directory(
                Some(value.into()), Some(PathBuf::from("/synthetic-home")), ".local/state"
            ).is_none());
        }
    }

    #[test]
    fn missing_or_relative_home_never_uses_the_working_directory() {
        assert!(xdg_directory(None, None, ".config").is_none());
        assert!(xdg_directory(Some(OsString::new()), None, ".config").is_none());
        assert!(xdg_directory(None, Some(PathBuf::from("relative")), ".config").is_none());
    }

    #[test]
    fn absolute_paths_are_not_shell_expanded() {
        let path = "/synthetic/$HOME/~/$(do-not-execute)";
        assert_eq!(
            xdg_directory(Some(path.into()), None, ".cache"),
            Some(PathBuf::from(path).join("lmm"))
        );
    }

    #[test]
    fn unix_paths_do_not_require_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(b"/synthetic/\xff".to_vec());
        let expected = Some(PathBuf::from(&path).join("lmm"));
        assert_eq!(xdg_directory(Some(path), None, ".local/state"), expected);
    }
}
