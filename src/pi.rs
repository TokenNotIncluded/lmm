//! Pi adapter: explicit SDK/instance paths, native settings transactions and an owned journal.
//! OAuth stays in Pi; this adapter never reads, copies or grants credentials.
pub mod status;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const SOURCE: &str =
    "git:github.com/TokenNotIncluded/pi-lmm-provider@f0c54764eb3d73e50790346f0dbab834587fe27b";
const BRIDGE: &str = include_str!("pi-bridge.mjs");
const ERROR: &str =
    "Pi 接入未完成；请检查显式路径、Pi 1.2.0、配置冲突或待恢复日志。未读取或复制应用凭据";

#[derive(Debug)]
pub struct Target<'a> {
    pub node: &'a Path,
    pub sdk: &'a Path,
    pub project: &'a Path,
    pub agent_dir: &'a Path,
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    version: u8,
    source: String,
    phase: String,
}

#[derive(Serialize)]
pub struct Outcome {
    pub schema_version: u8,
    pub action: String,
    pub source: &'static str,
    pub project: PathBuf,
    pub state: &'static str,
    pub authorization: &'static str,
    pub activation: &'static str,
    pub paid_call: bool,
    pub notes: &'static str,
}

impl Target<'_> {
    fn validate(&self) -> Result<(), String> {
        if !cfg!(target_os = "linux") {
            return Err("Pi 原生写入适配器目前仅验证 Linux；当前平台尚未开放".into());
        }
        for path in [self.node, self.sdk, self.project, self.agent_dir] {
            if !path.is_absolute()
                || fs::symlink_metadata(path)
                    .map_err(|_| ERROR)?
                    .file_type()
                    .is_symlink()
            {
                return Err(ERROR.into());
            }
        }
        if !self.node.is_file()
            || ![self.sdk, self.project, self.agent_dir]
                .iter()
                .all(|p| p.is_dir())
        {
            return Err(ERROR.into());
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(self.sdk.join("package.json")).map_err(|_| ERROR)?)
                .map_err(|_| ERROR)?;
        if manifest["name"] != "@earendil-works/pi-coding-agent" || manifest["version"] != "1.2.0" {
            return Err(ERROR.into());
        }
        let version = Command::new(self.node)
            .arg("--version")
            .output()
            .map_err(|_| ERROR)?;
        let text = std::str::from_utf8(&version.stdout).map_err(|_| ERROR)?;
        let parts: Vec<_> = text
            .trim()
            .strip_prefix('v')
            .ok_or(ERROR)?
            .split('.')
            .collect();
        let major = parts
            .first()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or(ERROR)?;
        let minor = parts
            .get(1)
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or(ERROR)?;
        if !version.status.success() || major < 22 || (major == 22 && minor < 19) {
            return Err(ERROR.into());
        }
        Ok(())
    }
    fn native(&self, action: &str) -> Result<serde_json::Value, String> {
        let mut child = Command::new(self.node)
            .args(["--input-type=module", "-e", BRIDGE])
            .current_dir(self.project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ERROR)?;
        let request = serde_json::json!({"sdk":self.sdk,"cwd":self.project,"agentDir":self.agent_dir,"source":SOURCE,"action":action});
        let input = serde_json::to_vec(&request).map_err(|_| ERROR)?;
        child
            .stdin
            .take()
            .ok_or(ERROR)?
            .write_all(&input)
            .map_err(|_| ERROR)?;
        let output = child.wait_with_output().map_err(|_| ERROR)?;
        if !output.status.success() || output.stdout.len() > 64 * 1024 {
            return Err(ERROR.into());
        }
        serde_json::from_slice(&output.stdout).map_err(|_| ERROR.into())
    }
    fn journal(&self) -> PathBuf {
        self.project.join(".pi/lmm-owned.json")
    }
    fn receipt(&self) -> Result<Option<Receipt>, String> {
        let path = self.journal();
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 4096 => {}
            _ => return Err(ERROR.into()),
        }
        let receipt: Receipt =
            serde_json::from_slice(&fs::read(path).map_err(|_| ERROR)?).map_err(|_| ERROR)?;
        if receipt.version != 1 || receipt.source != SOURCE {
            return Err(ERROR.into());
        }
        Ok(Some(receipt))
    }
    fn save(&self, phase: &str) -> Result<(), String> {
        let path = self.journal();
        let temp = path.with_extension("pending");
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&temp).map_err(|_| ERROR)?;
        let result = (|| {
            serde_json::to_writer(
                &mut file,
                &Receipt {
                    version: 1,
                    source: SOURCE.into(),
                    phase: phase.into(),
                },
            )
            .map_err(|_| ERROR)?;
            file.sync_all().map_err(|_| ERROR)?;
            fs::rename(&temp, &path).map_err(|_| ERROR)?;
            File::open(path.parent().ok_or(ERROR)?)
                .and_then(|dir| dir.sync_all())
                .map_err(|_| ERROR)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    pub fn run(&self) -> Result<u8, String> {
        self.validate()?;
        if self.native("inspect")?["installed"] != true {
            return Err(ERROR.into());
        }
        let status = Command::new(self.node)
            .arg(self.sdk.join("dist/bundle/cli.js"))
            .current_dir(self.project)
            .env("PI_CODING_AGENT_DIR", self.agent_dir)
            .status()
            .map_err(|_| ERROR)?;
        Ok(status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(3))
    }
    pub fn execute(&self, action: &str, dry_run: bool, yes: bool) -> Result<Outcome, String> {
        self.validate()?;
        let mut result = Outcome {
            schema_version: 1,
            action: action.into(),
            source: SOURCE,
            project: self.project.into(),
            state: "preview",
            authorization: "not_checked_use_pi_login",
            activation: "preserved",
            paid_call: false,
            notes: "仅管理所选项目中的固定 LMM Provider；Pi 本体、其他插件、凭据和默认模型保持用户管理。安装后在 Pi /login 选择 LMM，再用 /model 明确选择模型。",
        };
        if dry_run {
            return Ok(result);
        }
        if action == "sync" {
            if self.native("inspect")?["installed"] != true {
                return Err(ERROR.into());
            }
            result.state = "provider_present_catalog_refresh_pending";
            return Ok(result);
        }
        if !yes {
            return Err("请先用 --dry-run 查看固定来源和实例，再用 --yes 确认该范围".into());
        }
        let directory = self.project.join(".pi");
        if directory.exists()
            && fs::symlink_metadata(&directory)
                .map_err(|_| ERROR)?
                .file_type()
                .is_symlink()
        {
            return Err(ERROR.into());
        }
        if matches!(action, "restore" | "disconnect") && self.receipt()?.is_none() {
            return Err(ERROR.into());
        }
        fs::create_dir_all(&directory).map_err(|_| ERROR)?;
        // Pi 1.2.0 reads an absent settings file before acquiring its first lock.
        // Create exclusively first, so all native mutations read under an existing-file lock.
        let settings_path = directory.join("settings.json");
        let mut settings_options = OpenOptions::new();
        settings_options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            settings_options.mode(0o600);
        }
        match settings_options.open(&settings_path) {
            Ok(mut file) => {
                file.write_all(b"{}\n").map_err(|_| ERROR)?;
                file.sync_all().map_err(|_| ERROR)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&settings_path).map_err(|_| ERROR)?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(ERROR.into());
                }
            }
            Err(_) => return Err(ERROR.into()),
        }
        let lock_path = directory.join("lmm-adapter.lock");
        if fs::symlink_metadata(&lock_path)
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err(ERROR.into());
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(lock_path).map_err(|_| ERROR)?;
        lock.try_lock_exclusive().map_err(|_| ERROR)?;
        let receipt = self.receipt()?;
        match action {
            "setup" | "install" | "update" => {
                if receipt
                    .as_ref()
                    .is_some_and(|r| r.phase != "installed" && r.phase != "restored")
                {
                    return Err(ERROR.into());
                }
                let current = self.native("inspect")?;
                if current["conflicting"] != false
                    || (current["configured"] == true && current["installed"] != true)
                {
                    return Err(ERROR.into());
                }
                if action == "update"
                    && (receipt.as_ref().is_none_or(|r| r.phase != "installed")
                        || current["installed"] != true)
                {
                    return Err(ERROR.into());
                }
                if current["installed"] == true {
                    result.state = "already_present";
                    return Ok(result);
                }
                self.save("pending_add")?;
                let added = self.native("add")?;
                if added["added"] != true {
                    return Err(ERROR.into());
                }
                if self.native("inspect")?["installed"] != true {
                    return Err(ERROR.into());
                }
                self.save("installed")?;
                result.state = "provider_added_authorization_pending";
            }
            "restore" | "disconnect" => {
                match receipt {
                    Some(r) if r.phase == "installed" => {}
                    Some(r) if r.phase == "restored" => {
                        result.state = "already_restored";
                        return Ok(result);
                    }
                    _ => return Err(ERROR.into()),
                }
                self.save("pending_remove")?;
                self.native("remove")?;
                if self.native("inspect")?["configured"] != false {
                    return Err(ERROR.into());
                }
                self.save("restored")?;
                result.state = "provider_detached_credentials_unchanged";
            }
            "sync" => {
                if self.native("inspect")?["installed"] != true {
                    return Err(ERROR.into());
                }
                result.state = "provider_present_catalog_refresh_pending";
            }
            _ => return Err(ERROR.into()),
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn rejects_unknown_sdk_before_creating_project_state() {
        let root = tempfile::tempdir().unwrap();
        let sdk = root.path().join("sdk");
        let project = root.path().join("project");
        let agent = root.path().join("agent");
        for path in [&sdk, &project, &agent] {
            fs::create_dir(path).unwrap();
        }
        fs::write(
            sdk.join("package.json"),
            br#"{"name":"unrelated","version":"1.2.0"}"#,
        )
        .unwrap();
        let target = Target {
            node: &std::env::current_exe().unwrap(),
            sdk: &sdk,
            project: &project,
            agent_dir: &agent,
        };
        assert!(target.execute("setup", false, true).is_err());
        assert!(!project.join(".pi").exists());
    }
    #[test]
    fn incomplete_journal_cannot_be_claimed_as_owned_installation() {
        let root = tempfile::tempdir().unwrap();
        let target = Target {
            node: Path::new("/unused"),
            sdk: root.path(),
            project: root.path(),
            agent_dir: root.path(),
        };
        fs::create_dir(root.path().join(".pi")).unwrap();
        target.save("pending_add").unwrap();
        assert_eq!(target.receipt().unwrap().unwrap().phase, "pending_add");
        target.save("installed").unwrap();
        assert_eq!(target.receipt().unwrap().unwrap().phase, "installed");
        fs::write(
            target.journal(),
            br#"{"version":1,"source":"npm:foreign","phase":"installed"}"#,
        )
        .unwrap();
        assert!(target.receipt().is_err());
    }
    #[cfg(unix)]
    #[test]
    fn receipt_is_private_and_symlink_receipts_are_refused() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let target = Target {
            node: Path::new("/unused"),
            sdk: root.path(),
            project: root.path(),
            agent_dir: root.path(),
        };
        fs::create_dir(root.path().join(".pi")).unwrap();
        target.save("installed").unwrap();
        assert_eq!(
            fs::metadata(target.journal()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(target.journal()).unwrap();
        let outside = root.path().join("unrelated");
        fs::write(&outside, b"preserve").unwrap();
        symlink(&outside, target.journal()).unwrap();
        assert!(target.receipt().is_err());
        assert_eq!(fs::read(outside).unwrap(), b"preserve");
    }
}
