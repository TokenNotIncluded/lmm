#![allow(clippy::unwrap_used, clippy::expect_used)]

use lmm::i18n::Locale;
use serde_json::Value;
use std::{
    ffi::OsString,
    process::{Command, Output, Stdio},
};

fn run(args: &[&str], variables: &[(&str, &str)]) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_lmm"));
    command
        .args(args)
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("LOCALAPPDATA", home.path())
        .env("XDG_STATE_HOME", home.path())
        .env("PATH", "")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .stdin(Stdio::null());
    for key in ["LMM_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        command.env_remove(key);
    }
    command.envs(variables.iter().copied());
    command.output().unwrap()
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}

fn detect(args: &[&str], variables: &[(&str, &str)]) -> Locale {
    let args = args
        .iter()
        .map(|value| OsString::from(*value))
        .collect::<Vec<_>>();
    Locale::detect(&args, |key| {
        variables
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| OsString::from(*value))
    })
}

#[test]
fn recognizes_language_tags_without_confusing_traditional_chinese_with_simplified() {
    for name in ["en", "EN-us", "en_GB.UTF-8", "C", "C.UTF-8", "POSIX"] {
        assert_eq!(Locale::from_name(name), Some(Locale::En), "{name}");
    }
    for name in [
        "zh",
        "zh-CN",
        "zh_CN.UTF-8",
        "zh_SG",
        "zh-Hans",
        "zh-Hans-CN",
        "zh_CN@variant",
    ] {
        assert_eq!(Locale::from_name(name), Some(Locale::ZhCn), "{name}");
    }
    for name in ["", "fr_FR.UTF-8", "zh-TW", "zh-Hant", "pirate"] {
        assert_eq!(Locale::from_name(name), None, "{name}");
    }
}

#[test]
fn language_precedence_and_fallback_are_deterministic() {
    assert_eq!(
        detect(
            &["lmm", "--lang", "en", "catalog"],
            &[("LMM_LANG", "zh-CN")]
        ),
        Locale::En
    );
    assert_eq!(
        detect(&["lmm", "catalog", "--lang=zh-CN"], &[("LC_ALL", "C")]),
        Locale::ZhCn
    );
    assert_eq!(
        detect(&["lmm"], &[("LMM_LANG", "zh-CN"), ("LC_ALL", "C")]),
        Locale::ZhCn
    );
    assert_eq!(
        detect(&["lmm"], &[("LC_ALL", "C"), ("LANG", "zh-CN")]),
        Locale::En
    );
    assert_eq!(
        detect(
            &["lmm"],
            &[("LC_MESSAGES", "zh_CN.UTF-8"), ("LANG", "en_US")]
        ),
        Locale::ZhCn
    );
    assert_eq!(
        detect(&["lmm"], &[("LMM_LANG", ""), ("LANG", "zh_CN.UTF-8")]),
        Locale::ZhCn
    );
    assert_eq!(
        detect(&["lmm"], &[("LC_ALL", "fr_FR.UTF-8"), ("LANG", "zh-CN")]),
        Locale::En
    );
    assert_eq!(detect(&["lmm"], &[]), Locale::En);
}

#[test]
fn positional_data_after_double_dash_does_not_select_a_language() {
    assert_eq!(
        detect(&["lmm", "presets", "--", "--lang=zh-CN"], &[]),
        Locale::En
    );
    let output = run(&["presets", "--", "--lang=zh-CN"], &[]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("Built-in provider presets"));
}

#[cfg(unix)]
#[test]
fn non_unicode_environment_does_not_panic_or_override_a_cli_choice() {
    use std::os::unix::ffi::OsStringExt;
    let args = vec![OsString::from("lmm")];
    assert_eq!(
        Locale::detect(&args, |_| Some(OsString::from_vec(vec![0xff]))),
        Locale::En
    );
    let args = vec![OsString::from("lmm"), OsString::from("--lang=zh-CN")];
    assert_eq!(
        Locale::detect(&args, |_| Some(OsString::from_vec(vec![0xff]))),
        Locale::ZhCn
    );
}

#[test]
fn root_and_subcommand_help_use_the_selected_language() {
    for (language, heading) in [("en", "Usage:"), ("zh-CN", "用法：")] {
        let root = run(&["--lang", language, "--help"], &[]);
        assert!(root.status.success(), "{:?}", root.stderr);
        assert!(stdout(&root).contains(heading));
        assert!(stdout(&root).contains("--lang"));
        for name in [
            "presets",
            "catalog",
            "setup",
            "install",
            "login",
            "logout",
            "models",
            "status",
            "sync",
            "update",
            "doctor",
            "restore",
            "disconnect",
            "uninstall",
            "run",
        ] {
            let output = run(&[name, "--lang", language, "--help"], &[]);
            assert!(output.status.success(), "{name}: {:?}", output.stderr);
            assert!(
                stdout(&output).contains(heading),
                "{name}: {}",
                stdout(&output)
            );
            assert!(stdout(&output).contains("--lang"));
            assert!(!stdout(&output).contains("arg."));
            assert!(!stdout(&output).contains("cmd."));
        }
        let help = run(&["--lang", language, "help", "login"], &[]);
        assert!(help.status.success(), "{:?}", help.stderr);
        assert!(stdout(&help).contains(heading));
        assert!(stdout(&help).contains("--issuer"));
    }
}

#[test]
fn environment_selection_and_global_flag_work_for_real_commands() {
    let chinese = run(&["catalog", "astrbot"], &[("LANG", "zh_CN.UTF-8")]);
    assert!(stdout(&chinese).contains("消息平台"));
    let english = run(
        &["catalog", "astrbot", "--lang=en-US"],
        &[("LMM_LANG", "zh-CN")],
    );
    assert!(stdout(&english).contains("messaging platforms"));
    assert!(!stdout(&english).contains("消息平台"));
    let chinese = run(
        &["--lang=zh_CN.UTF-8", "catalog", "astrbot"],
        &[("LC_ALL", "C")],
    );
    assert!(stdout(&chinese).contains("消息平台"));
}

#[test]
fn invalid_explicit_language_fails_without_running_a_command() {
    for args in [
        vec!["--lang", "pirate", "catalog"],
        vec!["catalog", "--lang"],
        vec!["--lang=", "catalog"],
    ] {
        let output = run(&args, &[]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn runtime_errors_and_blocked_plans_are_translated() {
    let english = run(&["--lang=en", "status", "unknown-software"], &[]);
    let chinese = run(&["--lang=zh-CN", "status", "unknown-software"], &[]);
    assert_eq!(english.status.code(), Some(2));
    assert_eq!(chinese.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&english.stderr).contains("Unknown software ID"));
    assert!(String::from_utf8_lossy(&chinese.stderr).contains("未知软件 ID"));
    let chinese = run(&["setup", "astrbot", "--dry-run", "--lang=zh-CN"], &[]);
    assert_eq!(chinese.status.code(), Some(3));
    assert!(stdout(&chinese).contains("计划已阻止"));
    assert!(stdout(&chinese).contains("写入适配器"));
    let chinese = run(
        &["models", "--issuer", "http://127.0.0.1", "--lang=zh-CN"],
        &[],
    );
    assert_eq!(chinese.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&chinese.stderr).contains("HTTPS 域名"));
}

#[test]
fn json_messages_and_codes_do_not_change_with_the_display_language() {
    for arguments in [
        vec!["catalog", "astrbot"],
        vec!["presets", "local"],
        vec!["status", "astrbot"],
        vec!["doctor", "astrbot", "--report"],
        vec!["setup", "astrbot", "--dry-run"],
        vec!["status", "unknown-software"],
        vec!["models", "--issuer", "http://127.0.0.1"],
        vec!["login", "--non-interactive"],
    ] {
        let mut results = Vec::new();
        for language in ["en", "zh-CN"] {
            let mut args = vec!["--json", "--lang", language];
            args.extend_from_slice(&arguments);
            let output = run(&args, &[]);
            assert!(output.stderr.is_empty(), "{args:?}: {:?}", output.stderr);
            let mut value: Value = serde_json::from_slice(&output.stdout).unwrap();
            if let Some(object) = value.as_object_mut() {
                object.remove("checked_at");
            }
            results.push((output.status.code(), value));
        }
        assert_eq!(results[0], results[1], "{arguments:?}");
    }
    let output = run(
        &["status", "unknown-software", "--lang=zh-CN", "--json"],
        &[],
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error_code"], "error.unknown_software");
    assert!(
        value["error"]
            .as_str()
            .unwrap()
            .starts_with("Unknown software ID")
    );
}

#[test]
fn search_matches_translated_descriptions_without_changing_identifiers() {
    for query in ["messaging platforms", "  消息平台  "] {
        let output = run(&["catalog", query, "--json", "--lang=en"], &[]);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["id"], "astrbot");
    }
    let local = run(&["presets", "local", "--json"], &[]);
    let translated = run(&["presets", "本地", "--json"], &[]);
    assert_eq!(local.stdout, translated.stdout);
}
