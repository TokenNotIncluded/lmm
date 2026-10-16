//! Embedded CLI translations. Machine-readable messages always use English.

use serde::{Serialize, Serializer};
use std::{collections::BTreeMap, ffi::OsString, fmt, str::FromStr, sync::OnceLock};

type Catalog = BTreeMap<String, String>;
static ENGLISH: OnceLock<Catalog> = OnceLock::new();
static CHINESE: OnceLock<Catalog> = OnceLock::new();
static LANGUAGE: OnceLock<Locale> = OnceLock::new();

/// Languages shipped with the binary; no files or network are needed at runtime.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Locale {
    #[default]
    En,
    ZhCn,
}

impl Locale {
    pub const ALL: [Self; 2] = [Self::En, Self::ZhCn];

    /// Recognize supported language tags and common POSIX locale names.
    pub fn from_name(value: &str) -> Option<Self> {
        let name = value
            .trim()
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .replace('_', "-")
            .to_ascii_lowercase();
        match name.as_str() {
            "c" | "posix" | "en" => Some(Self::En),
            "zh" | "zh-cn" | "zh-sg" | "zh-hans" => Some(Self::ZhCn),
            name if name.starts_with("en-") => Some(Self::En),
            name if name.starts_with("zh-hans-") => Some(Self::ZhCn),
            _ => None,
        }
    }

    /// Resolve language before clap renders help. Clap still validates all arguments.
    /// Stop at `--` so positional data cannot change the language.
    pub fn detect(args: &[OsString], get_env: impl Fn(&str) -> Option<OsString>) -> Self {
        let mut selected = None;
        let mut args = args.iter().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--" {
                break;
            }
            let value = if arg == "--lang" {
                args.next().and_then(|value| value.to_str())
            } else {
                arg.to_str().and_then(|arg| arg.strip_prefix("--lang="))
            };
            if let Some(value) = value {
                selected = Self::from_name(value);
            }
        }
        if let Some(locale) = selected {
            return locale;
        }
        for key in ["LMM_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Some(value) = get_env(key).filter(|value| !value.is_empty()) {
                return value.to_str().and_then(Self::from_name).unwrap_or_default();
            }
        }
        Self::default()
    }

    fn catalog(self) -> &'static Catalog {
        let (cache, source) = match self {
            Self::En => (&ENGLISH, include_str!("../locales/en.json")),
            Self::ZhCn => (&CHINESE, include_str!("../locales/zh-CN.json")),
        };
        cache.get_or_init(|| {
            serde_json::from_str(source)
                .unwrap_or_else(|error| panic!("invalid embedded translation catalog: {error}"))
        })
    }

    /// Missing translations fall back to English, then to the visible message key.
    pub fn text(self, key: &'static str) -> &'static str {
        self.catalog()
            .get(key)
            .or_else(|| Self::En.catalog().get(key))
            .map_or(key, String::as_str)
    }
}

impl FromStr for Locale {
    type Err = Message;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_name(value).ok_or(msg("error.unsupported_language"))
    }
}

/// Select once at process startup. Tests should use explicit locales instead.
pub fn initialize(locale: Locale) -> Result<(), Locale> {
    LANGUAGE.set(locale)
}

pub fn locale() -> Locale {
    LANGUAGE.get().copied().unwrap_or_default()
}

/// A stable message key, separated from both presentation and serialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Message(&'static str);

pub const fn msg(key: &'static str) -> Message {
    Message(key)
}

pub fn text(key: &'static str) -> &'static str {
    msg(key).text()
}

impl Message {
    pub const fn key(self) -> &'static str {
        self.0
    }

    pub fn text(self) -> &'static str {
        locale().text(self.0)
    }

    /// Substitute named values once; inserted text is never parsed as a template.
    pub fn render(self, values: &[(&str, &str)]) -> String {
        self.render_in(locale(), values)
    }

    pub fn render_in(self, locale: Locale, values: &[(&str, &str)]) -> String {
        let mut rest = locale.text(self.0);
        let mut output = String::with_capacity(rest.len());
        while let Some(start) = rest.find('{') {
            output.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}').map(|end| start + end) else {
                output.push_str(&rest[start..]);
                return output;
            };
            let name = &rest[start + 1..end];
            match values.iter().find(|(key, _)| *key == name) {
                Some((_, value)) => output.push_str(value),
                None => output.push_str(&rest[start..=end]),
            }
            rest = &rest[end + 1..];
        }
        output.push_str(rest);
        output
    }
}

impl fmt::Display for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.text())
    }
}

impl std::error::Error for Message {}

impl Serialize for Message {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(Locale::En.text(self.0))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn shipped_catalogs_have_the_same_nonempty_keys() {
        let reference = Locale::En.catalog();
        assert!(!reference.is_empty());
        for locale in Locale::ALL {
            let catalog = locale.catalog();
            assert_eq!(
                reference.keys().collect::<Vec<_>>(),
                catalog.keys().collect::<Vec<_>>()
            );
            assert!(catalog.values().all(|value| !value.trim().is_empty()));
        }
    }

    #[test]
    fn unknown_keys_remain_visible() {
        assert_eq!(Locale::ZhCn.text("missing.test.key"), "missing.test.key");
    }

    #[test]
    fn replacement_values_cannot_inject_other_placeholders() {
        let output = msg("output.catalog_entry").render_in(
            Locale::En,
            &[
                ("id", "{source}"),
                ("name", "name"),
                ("purpose", "purpose"),
                ("source", "URL"),
            ],
        );
        assert!(output.starts_with("{source} - name"));
        assert!(output.contains("Source: URL"));
    }

    #[test]
    fn serialization_does_not_depend_on_display_language() {
        let message = msg("error.unknown_software");
        assert_ne!(
            Locale::En.text(message.key()),
            Locale::ZhCn.text(message.key())
        );
        assert_eq!(
            serde_json::to_value(message).unwrap(),
            Locale::En.text(message.key())
        );
    }
}
