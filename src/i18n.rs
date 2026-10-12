//! Embedded UI messages. Protocol values, identifiers, and provider data are not translated.
//!
//! The CLI chooses one immutable language before building its command tree. Libraries
//! can use `text` with an explicit locale without changing process-wide state.

use serde::Serializer;
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    sync::{LazyLock, OnceLock},
};

/// Languages with complete, bundled message catalogs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Locale {
    #[default]
    En,
    ZhCn,
}

impl Locale {
    pub const ALL: [Self; 2] = [Self::En, Self::ZhCn];

    pub const fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhCn => "zh-CN",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::En => 0,
            Self::ZhCn => 1,
        }
    }

    /// Normalize common BCP 47 and POSIX locale names. Unsupported locales use English.
    /// Traditional Chinese is not silently labeled as a Simplified Chinese translation.
    pub fn from_environment(value: &str) -> Self {
        let name = value
            .trim()
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .replace('_', "-")
            .to_ascii_lowercase();
        match name.as_str() {
            "zh" | "zh-cn" | "zh-sg" | "zh-hans" => Self::ZhCn,
            name if name.starts_with("zh-hans-") => Self::ZhCn,
            _ => Self::En,
        }
    }
}

type Messages = BTreeMap<String, String>;

// Keep parse failures visible to tests. A damaged/missing translation must not
// crash a read-only command or change its authorization behavior.
static CATALOGS: LazyLock<[Result<Messages, serde_json::Error>; 2]> = LazyLock::new(|| {
    [
        serde_json::from_str(include_str!("../locales/en.json")),
        serde_json::from_str(include_str!("../locales/zh-CN.json")),
    ]
});
static ACTIVE_LOCALE: OnceLock<Locale> = OnceLock::new();

/// CLI argument > LMM_LANG > LC_ALL > LC_MESSAGES > LANG > English.
/// Empty values are ignored. An unsupported high-priority value still overrides
/// lower-priority values, so LC_ALL=C always selects English.
fn choose_locale(explicit: Option<&str>, environment: &[Option<&str>]) -> Locale {
    explicit
        .into_iter()
        .chain(environment.iter().copied().flatten())
        .find(|value| !value.trim().is_empty())
        .map(Locale::from_environment)
        .unwrap_or_default()
}

/// Read only the global language option before Clap generates help. Clap remains
/// responsible for validation. Never interpret positional text after `--` as flags.
fn language_argument(args: &[OsString]) -> Option<String> {
    let mut args = args.iter().skip(1);
    let mut language = None;
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--lang" {
            let value = args.next()?;
            if value == "--" {
                break;
            }
            language = value.to_str().map(str::to_owned);
        } else if let Some(value) = arg.to_str().and_then(|arg| arg.strip_prefix("--lang=")) {
            language = Some(value.to_owned());
        }
    }
    language
}

/// Initialize once, before building CLI help or starting an OAuth callback thread.
/// This reads no files, credentials, or network and does not modify the environment.
pub fn init_from_args(args: &[OsString]) {
    let explicit = language_argument(args);
    let values = ["LMM_LANG", "LC_ALL", "LC_MESSAGES", "LANG"].map(|key| env::var(key).ok());
    let environment = values.each_ref().map(|value| value.as_deref());
    let selected = choose_locale(explicit.as_deref(), &environment);
    let _ = ACTIVE_LOCALE.set(selected);
}

pub fn locale() -> Locale {
    ACTIVE_LOCALE.get().copied().unwrap_or_default()
}

/// Look up a message in one language, then English, then the stable message key.
/// Missing keys are also rejected by the catalog checks in CI.
pub fn text(locale: Locale, key: &str) -> &str {
    CATALOGS[locale.index()]
        .as_ref()
        .ok()
        .and_then(|messages| messages.get(key))
        .or_else(|| {
            CATALOGS[Locale::En.index()]
                .as_ref()
                .ok()
                .and_then(|messages| messages.get(key))
        })
        .map(String::as_str)
        .unwrap_or(key)
}

/// Look up a fixed message for the active CLI language.
pub fn tr(key: &'static str) -> &'static str {
    text(locale(), key)
}

/// Serialize a catalog description, not its internal message key.
pub fn serialize_message<S: Serializer>(key: &str, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(text(locale(), key))
}

/// Substitute named placeholders in one pass. Values are never parsed as templates.
/// Callers must still escape untrusted terminal values (for example with Debug).
fn interpolate(template: &str, arguments: &[(&str, String)]) -> String {
    let mut output = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some((prefix, after_open)) = remaining.split_once('{') {
        output.push_str(prefix);
        let Some((name, after_close)) = after_open.split_once('}') else {
            output.push('{');
            output.push_str(after_open);
            return output;
        };
        if let Some((_, value)) = arguments.iter().find(|(key, _)| *key == name) {
            output.push_str(value);
        } else {
            output.push('{');
            output.push_str(name);
            output.push('}');
        }
        remaining = after_close;
    }
    output.push_str(remaining);
    output
}

pub fn format(key: &str, arguments: &[(&str, String)]) -> String {
    interpolate(text(locale(), key), arguments)
}

/// Format a catalog message with named, reorderable placeholders.
#[macro_export]
macro_rules! trf {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::format($key, &[$((stringify!($name), ($value).to_string())),*])
    };
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn locale_names_and_unsupported_languages_are_normalized() {
        for name in [
            "zh",
            "zh_CN.UTF-8",
            "ZH-cn",
            "zh-Hans",
            "zh_Hans_CN",
            "zh_SG@variant",
        ] {
            assert_eq!(Locale::from_environment(name), Locale::ZhCn, "{name}");
        }
        for name in [
            "en",
            "en_US.UTF-8",
            "C",
            "C.UTF-8",
            "POSIX",
            "fr_FR",
            "zh-TW",
            "zh-Hant",
            "",
        ] {
            assert_eq!(Locale::from_environment(name), Locale::En, "{name}");
        }
    }

    #[test]
    fn locale_precedence_and_empty_values_are_explicit() {
        assert_eq!(choose_locale(Some("en"), &[Some("zh-CN")]), Locale::En);
        assert_eq!(
            choose_locale(None, &[Some("zh-CN"), Some("C")]),
            Locale::ZhCn
        );
        assert_eq!(
            choose_locale(None, &[None, Some("C"), Some("zh-CN")]),
            Locale::En
        );
        assert_eq!(
            choose_locale(None, &[Some(""), Some(" "), Some("zh-CN")]),
            Locale::ZhCn
        );
        assert_eq!(
            choose_locale(None, &[Some("fr"), Some("zh-CN")]),
            Locale::En
        );
        assert_eq!(choose_locale(None, &[]), Locale::En);
    }

    #[test]
    fn global_language_flag_is_found_before_help_but_not_after_separator() {
        for words in [
            vec!["lmm", "--lang", "zh-CN", "--help"],
            vec!["lmm", "status", "--help", "--lang=zh-CN"],
        ] {
            let args: Vec<_> = words.into_iter().map(OsString::from).collect();
            assert_eq!(language_argument(&args).as_deref(), Some("zh-CN"));
        }
        let args: Vec<_> = ["lmm", "presets", "--", "--lang=zh-CN"]
            .into_iter()
            .map(OsString::from)
            .collect();
        assert_eq!(language_argument(&args), None);
    }

    #[test]
    fn bundled_catalogs_parse_and_have_matching_nonempty_keys() {
        let english = CATALOGS[0].as_ref().unwrap();
        assert!(!english.is_empty());
        for catalog in CATALOGS.iter() {
            let catalog = catalog.as_ref().unwrap();
            assert_eq!(
                english.keys().collect::<Vec<_>>(),
                catalog.keys().collect::<Vec<_>>()
            );
            assert!(catalog.values().all(|value| !value.trim().is_empty()));
        }
    }

    #[test]
    fn unknown_keys_are_safe_and_requested_locale_does_not_change_global_state() {
        assert_eq!(text(Locale::ZhCn, "missing.key"), "missing.key");
        assert_ne!(
            text(Locale::En, "cli.about"),
            text(Locale::ZhCn, "cli.about")
        );
        assert_eq!(locale(), Locale::En);
    }

    #[test]
    fn interpolation_allows_reordering_without_reinterpreting_values() {
        let values = [("first", "{second}".to_owned()), ("second", "B".to_owned())];
        assert_eq!(interpolate("{second}: {first}", &values), "B: {second}");
        assert_eq!(interpolate("{missing}", &values), "{missing}");
        assert_eq!(interpolate("literal {", &values), "literal {");
        assert_eq!(interpolate("plain", &[]), "plain");
    }
}
