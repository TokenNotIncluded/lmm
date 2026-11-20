#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lmm::pi::SOURCE;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

// @mark cli.pi-settings-inspection role=test contract=explicit-read-only-instance
struct Fixture {
    root: tempfile::TempDir,
    project: PathBuf,
    agent: PathBuf,
    sdk: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let agent = root.path().join("agent");
        let sdk = root.path().join("sdk");
        for path in [&project, &agent, &sdk, &root.path().join("home")] {
            fs::create_dir(path).unwrap();
        }
        // A declared SDK version is a format gate, not installed-code verification.
        fs::write(
            sdk.join("package.json"),
            br#"{"name":"@earendil-works/pi-coding-agent","version":"1.2.0"}"#,
        )
        .unwrap();
        fs::write(agent.join("auth.json"), "PRIVATE_AUTH_SENTINEL").unwrap();
        Self {
            root,
            project,
            agent,
            sdk,
        }
    }

    fn settings(&self, global: Value, project: Value) {
        fs::write(
            self.agent.join("settings.json"),
            serde_json::to_vec(&global).unwrap(),
        )
        .unwrap();
        fs::create_dir_all(self.project.join(".pi")).unwrap();
        fs::write(
            self.project.join(".pi/settings.json"),
            serde_json::to_vec(&project).unwrap(),
        )
        .unwrap();
    }

    fn command(&self, action: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lmm"));
        command
            .args([action, "pi", "--instance"])
            .arg(&self.project)
            .arg("--pi-package")
            .arg(&self.sdk)
            .arg("--agent-dir")
            .arg(&self.agent)
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("USERPROFILE", self.root.path().join("home"))
            .env("PATH", "")
            .current_dir(self.root.path())
            .stdin(Stdio::null());
        command
    }

    fn report(&self, action: &str, extra: &[&str]) -> (Output, Value) {
        let output = self
            .command(action)
            .args(extra)
            .arg("--json")
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!text.contains("PRIVATE_"), "private fixture values leaked");
        let report = serde_json::from_slice(&output.stdout).unwrap();
        (output, report)
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if metadata.file_type().is_symlink() {
                files.insert(
                    relative,
                    fs::read_link(&path)
                        .unwrap()
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec(),
                );
            } else if metadata.is_dir() {
                files.insert(relative, vec![]);
                visit(root, &path, files);
            } else {
                files.insert(relative, fs::read(path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn global_reference_is_distinct_from_selection_and_runtime_verification() {
    let fixture = Fixture::new();
    fixture.settings(
        json!({"packages":[SOURCE],"defaultProvider":"other","defaultModel":"PRIVATE_MODEL_SENTINEL","apiKey":"PRIVATE_KEY_SENTINEL"}),
        json!({}),
    );
    let before = snapshot(fixture.root.path());
    let (output, report) = fixture.report("status", &[]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["check_scope"], "explicit_pi_settings_snapshot");
    assert_eq!(report["pi_settings"]["provider_reference"], "pinned");
    assert_eq!(report["pi_settings"]["default_provider"], "other");
    assert_eq!(report["pi_settings"]["default_model"], "set");
    assert_eq!(report["pi_settings"]["provider_scope"], "global");
    let target = &report["targets"][0];
    assert_eq!(target["lmm_added"], "referenced_in_settings");
    assert_eq!(target["lmm_enabled"], "not_selected_in_settings");
    assert_eq!(target["verification"]["configuration"], "passed");
    assert_eq!(target["installation"], "unknown");
    assert_eq!(target["configuration_manager"], "unknown");
    assert_eq!(target["authorization"], "not_checked");
    assert_eq!(target["actual_route"], "not_checked");
    for key in [
        "connectivity",
        "target_call",
        "streaming",
        "tool_calling",
        "multi_turn",
    ] {
        assert_eq!(target["verification"][key], "not_checked", "{key}");
    }
    assert_eq!(snapshot(fixture.root.path()), before);
}

#[test]
fn project_selection_overrides_global_and_each_field_inherits_independently() {
    let fixture = Fixture::new();
    fixture.settings(
        json!({"packages":[SOURCE],"defaultProvider":"lmm","defaultModel":"PRIVATE_GLOBAL_MODEL"}),
        json!({"defaultProvider":"other"}),
    );
    let (_, report) = fixture.report("status", &[]);
    assert_eq!(
        report["targets"][0]["lmm_enabled"],
        "not_selected_in_settings"
    );
    assert_eq!(report["pi_settings"]["provider_scope"], "project");
    assert_eq!(report["pi_settings"]["model_scope"], "global");

    fixture.settings(
        json!({"defaultProvider":"other","defaultModel":"PRIVATE_GLOBAL_MODEL"}),
        json!({"packages":[{"source":SOURCE,"extensions":[]}],"defaultProvider":"lmm"}),
    );
    let (_, report) = fixture.report("status", &[]);
    assert_eq!(report["targets"][0]["lmm_enabled"], "selected_in_settings");
    assert_eq!(report["pi_settings"]["provider_reference"], "pinned");
    assert_eq!(report["pi_settings"]["model_scope"], "global");
    let output = fixture.command("status").output().unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("selected_in_settings"));
    assert!(!text.contains("PRIVATE_"));
}

#[test]
fn missing_settings_do_not_create_state_and_node_is_never_executed() {
    let fixture = Fixture::new();
    let node = fixture.root.path().join("must-not-execute");
    fs::write(&node, "#!/bin/sh\nexit 99\n").unwrap();
    let before = snapshot(fixture.root.path());
    let (output, report) = fixture.report("status", &["--node", node.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["pi_settings"]["default_provider"], "unset");
    assert_eq!(report["pi_settings"]["default_model"], "unset");
    assert_eq!(
        report["pi_settings"]["provider_reference"],
        "no_known_source"
    );
    assert_eq!(report["pi_settings"]["global_settings"], "not_found");
    assert_eq!(report["pi_settings"]["project_settings"], "not_found");
    assert_eq!(snapshot(fixture.root.path()), before);
    assert!(!fixture.project.join(".pi").exists());
}

#[test]
fn malformed_project_never_falls_back_to_a_valid_global_selection() {
    let fixture = Fixture::new();
    fixture.settings(
        json!({"packages":[SOURCE],"defaultProvider":"lmm"}),
        json!({}),
    );
    for raw in [
        r#"{"apiKey":"PRIVATE_BROKEN_SECRET", broken"#,
        r#"{"packages":{}}"#,
        r#"{"packages":[null]}"#,
        r#"{"defaultProvider":null}"#,
        r#"{"defaultModel":3}"#,
        "[]",
    ] {
        fs::write(fixture.project.join(".pi/settings.json"), raw).unwrap();
        let before = snapshot(fixture.root.path());
        let (output, report) = fixture.report("status", &[]);
        assert_eq!(output.status.code(), Some(3), "{raw}");
        assert_eq!(report["targets"][0]["lmm_enabled"], "unknown", "{raw}");
        assert_eq!(
            report["targets"][0]["verification"]["configuration"],
            "unknown"
        );
        assert_eq!(snapshot(fixture.root.path()), before);
    }
}

#[test]
fn native_bom_and_empty_file_rules_are_preserved() {
    let fixture = Fixture::new();
    fixture.settings(json!({"defaultProvider":"other"}), json!({}));
    let path = fixture.project.join(".pi/settings.json");
    fs::write(&path, b"\xef\xbb\xbf{\"defaultProvider\":\"lmm\"}").unwrap();
    let (output, report) = fixture.report("status", &[]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["pi_settings"]["provider_scope"], "project");
    assert_eq!(report["targets"][0]["lmm_enabled"], "selected_in_settings");
    fs::write(&path, b"").unwrap();
    let (output, report) = fixture.report("status", &[]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["pi_settings"]["provider_scope"], "global");
    fs::write(&path, b"\xef\xbb\xbf").unwrap();
    let (output, report) = fixture.report("status", &[]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(report["targets"][0]["lmm_enabled"], "unknown");
}

#[test]
fn unknown_sdk_and_oversized_settings_remain_unverified() {
    let fixture = Fixture::new();
    fixture.settings(json!({}), json!({"defaultProvider":"lmm"}));
    fs::write(
        fixture.sdk.join("package.json"),
        br#"{"name":"@earendil-works/pi-coding-agent","version":"9.0.0"}"#,
    )
    .unwrap();
    let (output, report) = fixture.report("status", &[]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        report["pi_settings"]["diagnostic"],
        "unsupported_package_manifest"
    );
    assert_eq!(report["targets"][0]["lmm_enabled"], "unknown");

    fs::write(
        fixture.sdk.join("package.json"),
        br#"{"name":"@earendil-works/pi-coding-agent","version":"1.2.0"}"#,
    )
    .unwrap();
    fs::File::create(fixture.project.join(".pi/settings.json"))
        .unwrap()
        .set_len(1024 * 1024 + 1)
        .unwrap();
    let (output, report) = fixture.report("status", &[]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        report["targets"][0]["verification"]["configuration"],
        "unknown"
    );
}

#[test]
fn doctor_report_redacts_paths_and_fix_never_writes() {
    let fixture = Fixture::new();
    fixture.settings(
        json!({"packages":[SOURCE],"defaultProvider":"lmm","defaultModel":"PRIVATE_MODEL"}),
        json!({"packages":["npm:@tokennotincluded/pi-lmm-provider@alpha"]}),
    );
    let before = snapshot(fixture.root.path());
    for flags in [vec!["--report"], vec!["--report", "--fix"]] {
        let (output, report) = fixture.report("doctor", &flags);
        assert_eq!(output.status.code(), Some(3));
        assert!(
            !String::from_utf8_lossy(&output.stdout)
                .contains(fixture.root.path().to_str().unwrap())
        );
        assert_eq!(
            report["pi_settings"]["provider_reference"],
            "multiple_known_sources"
        );
        assert_eq!(report["targets"][0]["actual_route"], "not_checked");
        assert_eq!(snapshot(fixture.root.path()), before);
    }
}

#[test]
fn native_pi_source_forms_are_recognized_without_claiming_ownership() {
    let fixture = Fixture::new();
    let revision = "f0c54764eb3d73e50790346f0dbab834587fe27b";
    let sources = [
        "npm: @tokennotincluded/pi-lmm-provider@0.0.7 ".to_owned(),
        format!("git:https://github.com/TokenNotIncluded/pi-lmm-provider@{revision}"),
        format!("git:github.com/TokenNotIncluded/pi-lmm-provider.git@{revision}"),
        format!("  {SOURCE}  "),
        format!("git:git@github.com:TokenNotIncluded/pi-lmm-provider@{revision}"),
        format!("https://github.com/TokenNotIncluded/pi-lmm-provider.git@{revision}"),
        "git:github.com/TokenNotIncluded/pi-lmm-provider%2Egit".to_owned(),
        "git:github.com/%54okenNotIncluded/pi-lmm-provider".to_owned(),
        "git:https://github.com/TokenNotIncluded/pi-lmm-provider?x=1".to_owned(),
    ];
    for source in sources {
        fixture.settings(json!({"packages":[source]}), json!({}));
        let before = snapshot(fixture.root.path());
        let (output, report) = fixture.report("status", &[]);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            report["pi_settings"]["provider_reference"], "other_known_source",
            "{source}"
        );
        assert_eq!(report["targets"][0]["lmm_added"], "referenced_in_settings");
        assert_eq!(report["targets"][0]["installation"], "unknown");
        assert_eq!(snapshot(fixture.root.path()), before);
    }
}

#[test]
fn unresolved_sources_are_unknown_and_do_not_hide_a_known_reference() {
    let fixture = Fixture::new();
    for source in [
        "./local-extension",
        " npm:@tokennotincluded/pi-lmm-provider",
        "npm:@tokennotincluded/pi-lmm-provider@",
        "npm:alias@npm:@tokennotincluded/pi-lmm-provider",
        "npm:other@file:../package",
        "npm:@tokennotincluded/pi-lmm-provider@npm:other",
        "git:github.com/TokenNotIncluded/pi-lmm-provider%ZZ",
        "git:github.com/TokenNotIncluded%2Fpi-lmm-provider",
    ] {
        fixture.settings(json!({"packages":[source]}), json!({}));
        let (_, report) = fixture.report("status", &[]);
        assert_eq!(
            report["pi_settings"]["provider_reference"], "unknown",
            "{source}"
        );
        assert_eq!(report["targets"][0]["lmm_added"], "unknown");
    }
    fixture.settings(json!({"packages":[SOURCE, "./local-extension"]}), json!({}));
    let (_, report) = fixture.report("status", &[]);
    assert_eq!(
        report["pi_settings"]["provider_reference"],
        "known_with_unresolved_sources"
    );
    assert_eq!(report["targets"][0]["lmm_added"], "referenced_in_settings");
    for source in [
        "npm:@tokennotincluded/pi-lmm-provider-else@1.0.0",
        "git:https://github.com.evil/TokenNotIncluded/pi-lmm-provider",
    ] {
        fixture.settings(json!({"packages":[source]}), json!({}));
        let (_, report) = fixture.report("status", &[]);
        assert_eq!(
            report["pi_settings"]["provider_reference"], "no_known_source",
            "{source}"
        );
        assert_eq!(report["targets"][0]["lmm_added"], "no_known_reference");
    }
}

#[cfg(unix)]
#[test]
fn symlinked_settings_or_parent_directories_are_not_followed() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    fixture.settings(json!({}), json!({}));
    let outside = fixture.root.path().join("outside.json");
    fs::write(
        &outside,
        r#"{"defaultProvider":"lmm","secret":"PRIVATE_OUTSIDE"}"#,
    )
    .unwrap();
    let settings = fixture.project.join(".pi/settings.json");
    fs::remove_file(&settings).unwrap();
    symlink(&outside, &settings).unwrap();
    let before = snapshot(fixture.root.path());
    let (output, report) = fixture.report("status", &[]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(report["targets"][0]["lmm_enabled"], "unknown");
    assert_eq!(snapshot(fixture.root.path()), before);

    fs::remove_file(settings).unwrap();
    fs::remove_dir(fixture.project.join(".pi")).unwrap();
    symlink(&fixture.agent, fixture.project.join(".pi")).unwrap();
    let (output, _) = fixture.report("status", &[]);
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn pi_readonly_paths_must_be_complete_and_cannot_be_applied_to_other_software() {
    let fixture = Fixture::new();
    for args in [
        vec![
            "status",
            "pi",
            "--agent-dir",
            fixture.agent.to_str().unwrap(),
        ],
        vec![
            "doctor",
            "astrbot",
            "--pi-package",
            fixture.sdk.to_str().unwrap(),
        ],
        vec![
            "status",
            "pi",
            "--instance",
            fixture.project.to_str().unwrap(),
            "--pi-package",
            "relative",
            "--agent-dir",
            fixture.agent.to_str().unwrap(),
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_lmm"))
            .args(&args)
            .arg("--json")
            .env_clear()
            .env("HOME", fixture.root.path().join("home"))
            .env("PATH", "")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
}
