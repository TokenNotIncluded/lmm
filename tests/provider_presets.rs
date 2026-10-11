#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use std::{collections::BTreeSet, process::Command};

fn run(args: &[&str]) -> Value {
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lmm"))
        .args(args)
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn json_exposes_builtin_definitions_and_their_source_without_claiming_configuration() {
    let report = run(&["presets", "--json"]);
    assert_eq!(report["scope"], "builtin_preset_snapshot");
    assert_eq!(report["configuration"], "not_inspected");
    assert_eq!(report["connectivity"], "not_checked");
    assert_eq!(
        report["upstream"]["repository"],
        "https://github.com/yetone/magpie"
    );
    assert_eq!(
        report["upstream"]["revision"],
        "d5cae267a727e23f4b6e2b28aa107ecdeac5aa9c"
    );
    let presets = report["presets"].as_array().unwrap();
    assert_eq!(presets.len(), 56);
    let ids: BTreeSet<_> = presets
        .iter()
        .map(|preset| preset["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), presets.len());
    let openai = presets
        .iter()
        .find(|preset| preset["id"] == "openai")
        .unwrap();
    assert_eq!(openai["chat"], "https://api.openai.com/v1");
    assert_eq!(openai["responses"], "https://api.openai.com/v1");
    assert!(openai.get("anthropic").is_none());
}

#[test]
fn search_preserves_vendor_plan_endpoints_and_does_not_treat_plans_as_configured() {
    let report = run(&["presets", "  ZHIPU  ", "--json"]);
    let presets = report["presets"].as_array().unwrap();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0]["regionLabel"], "Plan");
    assert_eq!(presets[0]["regions"][0]["id"], "api");
    assert_eq!(presets[0]["regions"][1]["id"], "coding");
    assert_eq!(
        presets[0]["regions"][1]["chat"],
        "https://open.bigmodel.cn/api/coding/paas/v4"
    );
    assert_eq!(
        presets[0]["regions"][1]["responses"],
        "https://open.bigmodel.cn/api/v1"
    );
    assert_eq!(
        run(&["presets", "unknown-provider", "--json"])["presets"],
        serde_json::json!([])
    );
}

#[test]
fn local_and_remote_categories_remain_definitions_without_probe_results() {
    let locals = run(&["--json", "presets", "local"]);
    let locals = locals["presets"].as_array().unwrap();
    assert_eq!(locals.len(), 4);
    assert!(
        locals
            .iter()
            .all(|preset| preset["kind"] == "local" && preset["noKey"] == true)
    );
    assert!(
        locals
            .iter()
            .any(|preset| preset["chat"] == "http://localhost:11434/v1")
    );
    let remote = run(&["presets", "remote", "--json"]);
    assert_eq!(remote["presets"].as_array().unwrap().len(), 1);
    assert_eq!(remote["presets"][0]["id"], "remote-magpie");
    assert_eq!(remote["configuration"], "not_inspected");
}

#[test]
fn presets_work_with_private_config_and_unavailable_credentials_without_writing_state() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config/lmm");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config = config_dir.join("config.json");
    let original = b"{\"secret\":\"PRESET_TEST_PRIVATE_SENTINEL\",\"providers\":[]}";
    std::fs::write(&config, original).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lmm"))
        .args(["presets", "--json", "--non-interactive"])
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/lmm-presets-test",
        )
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRESET_TEST_PRIVATE_SENTINEL"));
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read(&config).unwrap(), original);
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read_dir(home.path().join(".config"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(std::fs::read_dir(config_dir).unwrap().count(), 1);
}
