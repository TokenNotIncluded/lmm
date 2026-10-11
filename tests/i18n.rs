#![allow(clippy::unwrap_used)]

use lmm::{
    catalog,
    i18n::{self, Locale},
};
use serde_json::Value;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], environment: &[(&str, &str)]) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_lmm"));
    command
        .args(args)
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("PATH", "")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null());
    for key in ["LMM_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        command.env_remove(key);
    }
    command.envs(environment.iter().copied());
    command.output().unwrap()
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}

#[test]
fn help_uses_explicit_language_in_any_global_flag_position() {
    for args in [
        vec!["--lang", "zh-CN", "--help"],
        vec!["--help", "--lang=zh-CN"],
        vec!["status", "--help", "--lang", "zh-CN"],
        vec!["--lang=zh-CN", "help", "status"],
    ] {
        let output = run(&args, &[("LMM_LANG", "en")]);
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(
            stdout(&output).contains("用法"),
            "{args:?}: {}",
            stdout(&output)
        );
        assert!(stdout(&output).contains("选项"));
        assert!(!stdout(&output).contains("cli.about"));
    }
    let output = run(&["--lang", "en", "--help"], &[("LANG", "zh_CN.UTF-8")]);
    assert!(stdout(&output).contains("Usage:"));
    assert!(!stdout(&output).contains("用法"));
}

#[test]
fn environment_precedence_normalization_and_fallback_work_in_real_processes() {
    for (environment, chinese) in [
        (vec![], false),
        (vec![("LANG", "zh_CN.UTF-8")], true),
        (vec![("LC_ALL", "C"), ("LANG", "zh_CN.UTF-8")], false),
        (vec![("LC_MESSAGES", "zh-Hans"), ("LANG", "en_US.UTF-8")], true),
        (vec![("LMM_LANG", "en"), ("LC_ALL", "zh_CN.UTF-8")], false),
        (vec![("LMM_LANG", ""), ("LANG", "zh_CN.UTF-8")], true),
        (vec![("LMM_LANG", "unknown"), ("LANG", "zh_CN.UTF-8")], false),
        (vec![("LANG", "zh-TW")], false),
    ] {
        let output = run(&["--help"], &environment);
        assert!(output.status.success());
        assert_eq!(stdout(&output).contains("用法"), chinese, "{environment:?}");
    }
}

#[test]
fn translated_prose_does_not_change_json_keys_statuses_or_exit_codes() {
    let mut reports = Vec::new();
    for language in ["en", "zh-CN"] {
        let output = run(
            &["setup", "astrbot", "--dry-run", "--json", "--lang", language],
            &[],
        );
        assert_eq!(output.status.code(), Some(3));
        let mut report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["outcome"], "blocked");
        assert_eq!(report["plans"][0]["activation"], "preserve");
        assert_eq!(report["targets"][0]["installation"], "unknown");
        let blockers = report["plans"][0]["blockers"].as_array().unwrap();
        assert!(
            blockers
                .iter()
                .all(|message| !message.as_str().unwrap().starts_with("blocker."))
        );
        report.as_object_mut().unwrap().remove("checked_at");
        report.as_object_mut().unwrap().remove("notes");
        report["plans"][0].as_object_mut().unwrap().remove("blockers");
        reports.push(report);
    }
    assert_eq!(reports[0], reports[1]);
}

#[test]
fn oauth_errors_are_localized_without_changing_the_machine_code() {
    for language in ["en", "zh-CN"] {
        let output = run(
            &[
                "models",
                "--issuer",
                "http://127.0.0.1",
                "--json",
                "--lang",
                language,
            ],
            &[],
        );
        assert_eq!(output.status.code(), Some(3));
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error_code"], "oauth.invalid_issuer");
        assert_eq!(error["outcome"], "blocked");
        assert_eq!(
            error["error"],
            i18n::text(Locale::from_environment(language), "oauth.invalid_issuer")
        );
    }
}

#[test]
fn catalog_search_accepts_both_languages_and_serializes_descriptions() {
    let english = catalog::search("Terminal coding assistant");
    let chinese = catalog::search("终端编程助手");
    assert_eq!(
        english.iter().map(|item| item.id).collect::<Vec<_>>(),
        chinese.iter().map(|item| item.id).collect::<Vec<_>>()
    );
    assert_eq!(english.len(), 3);
    for language in ["en", "zh-CN"] {
        let output = run(&["catalog", "astrbot", "--json", "--lang", language], &[]);
        assert!(output.status.success());
        let entries: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            entries[0]["purpose"],
            i18n::text(Locale::from_environment(language), "software.astrbot")
        );
        assert_eq!(entries[0]["id"], "astrbot");
        assert_eq!(entries[0]["installation"], "unavailable");
    }
}

#[test]
fn provider_snapshot_is_language_independent() {
    let english = run(&["presets", "--json", "--lang", "en"], &[]);
    let chinese = run(&["presets", "--json", "--lang", "zh-CN"], &[]);
    assert!(english.status.success());
    assert!(chinese.status.success());
    assert_eq!(english.stdout, chinese.stdout);
}

#[test]
fn invalid_language_and_separator_do_not_execute_operations() {
    assert_eq!(
        run(&["--lang", "unknown", "setup", "astrbot"], &[])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(run(&["--lang"], &[]).status.code(), Some(2));
    let output = run(&["catalog", "--json", "--", "--lang=zh-CN"], &[]);
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
}

#[cfg(unix)]
#[test]
fn translated_output_keeps_untrusted_paths_escaped() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("line\n\u{1b}[31m{software}");
    std::fs::create_dir(&path).unwrap();
    for language in ["en", "zh-CN"] {
        let output = run(
            &[
                "status",
                "astrbot",
                "--instance",
                path.to_str().unwrap(),
                "--lang",
                language,
            ],
            &[],
        );
        assert!(output.status.success());
        let text = stdout(&output);
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("\\n"));
        assert!(text.contains("{software}"));
    }
}
