#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use std::io::Cursor;

struct FixtureDownload {
    bytes: Vec<u8>,
    calls: usize,
    fail: bool,
}
impl Download for FixtureDownload {
    fn fetch(&mut self, root: &Path, release: &Release) -> Result<(), ManagedError> {
        self.calls += 1;
        if self.fail {
            return Err(ManagedError::Download);
        }
        files::store_download(root, release, &mut Cursor::new(&self.bytes))
    }
}

fn fixture(root: &Path, version: &str, bytes: &[u8]) -> (Manager, FixtureDownload) {
    let mut manager = Manager::new(root.join("managed"), "linux", "x86_64", None).unwrap();
    manager.candidate.version = version.into();
    manager.candidate.download_url = format!(
        "https://github.com/farion1231/cc-switch/releases/download/v{version}/CC-Switch-v{version}-Linux-x86_64.AppImage"
    );
    manager.candidate.sha256 = format!("{:x}", Sha256::digest(bytes));
    manager.candidate.size = bytes.len() as u64;
    (
        manager,
        FixtureDownload {
            bytes: bytes.to_vec(),
            calls: 0,
            fail: false,
        },
    )
}

fn apply(
    manager: &Manager,
    download: &mut FixtureDownload,
    action: Action,
    change: Option<&str>,
) -> ManagedResult {
    let plan = manager.preview(action, change).unwrap();
    manager
        .apply_with(action, change, &plan.plan_id, download)
        .unwrap()
}

#[test]
fn install_update_restore_and_uninstall_are_real_and_preserve_unrelated_files() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("application-data");
    fs::write(&data, b"user configuration and independent credentials").unwrap();
    let (first, mut bytes1) = fixture(root.path(), "4.0.21", b"synthetic old appimage");
    let preview = first.preview(Action::Install, None).unwrap();
    assert!(!first.root.exists());
    assert!(!preview.login_required);
    let installed = first
        .apply_with(Action::Install, None, &preview.plan_id, &mut bytes1)
        .unwrap();
    assert_eq!(installed.outcome, "committed");
    assert_eq!(
        fs::read(first.artifact(&first.candidate)).unwrap(),
        bytes1.bytes
    );
    fs::write(
        first.root.join("user-note"),
        b"preserve this subsequent user file",
    )
    .unwrap();
    let (second, mut bytes2) = fixture(root.path(), "4.0.22", b"synthetic newer appimage");
    let updated = apply(&second, &mut bytes2, Action::Update, None);
    assert_eq!(second.status().unwrap().current.unwrap().version, "4.0.22");
    assert!(first.artifact(&first.candidate).exists());
    let change = updated.receipt.unwrap().change_id;
    let restored = apply(&second, &mut bytes2, Action::Restore, Some(&change));
    assert_eq!(restored.receipt.unwrap().after.unwrap().version, "4.0.21");
    assert_eq!(
        bytes2.calls, 1,
        "restore reused the verified retained binary"
    );
    let removed = apply(&second, &mut bytes2, Action::Uninstall, None);
    assert_eq!(removed.outcome, "committed");
    assert!(!first.artifact(&first.candidate).exists());
    assert!(!second.artifact(&second.candidate).exists());
    assert_eq!(second.status().unwrap().installation, "not_installed");
    assert_eq!(
        fs::read(&data).unwrap(),
        b"user configuration and independent credentials"
    );
    assert!(first.root.join("user-note").exists());
    assert_eq!(second.status().unwrap().history.len(), 4);
}

#[test]
fn stale_plan_cannot_overwrite_a_later_change_and_receipt_replay_does_not_reapply() {
    let root = tempfile::tempdir().unwrap();
    let (first, mut bytes1) = fixture(root.path(), "4.0.21", b"v1");
    let install = apply(&first, &mut bytes1, Action::Install, None);
    let stale = first.preview(Action::Uninstall, None).unwrap();
    let (second, mut bytes2) = fixture(root.path(), "4.0.22", b"v2");
    apply(&second, &mut bytes2, Action::Update, None);
    assert!(matches!(
        second.apply_with(Action::Uninstall, None, &stale.plan_id, &mut bytes2),
        Err(ManagedError::Conflict)
    ));
    let replay = first
        .apply_with(
            Action::Install,
            None,
            &install.plan.unwrap().plan_id,
            &mut bytes1,
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(second.status().unwrap().current.unwrap().version, "4.0.22");
    assert_eq!(second.status().unwrap().history.len(), 2);
    assert_eq!(bytes1.calls, 1);
}

#[test]
fn failed_download_is_resumable_with_original_plan_without_destroying_current_install() {
    let root = tempfile::tempdir().unwrap();
    let (first, mut bytes1) = fixture(root.path(), "4.0.21", b"v1");
    apply(&first, &mut bytes1, Action::Install, None);
    let (second, mut bytes2) = fixture(root.path(), "4.0.22", b"v2");
    let plan = second.preview(Action::Update, None).unwrap();
    bytes2.fail = true;
    assert!(matches!(
        second.apply_with(Action::Update, None, &plan.plan_id, &mut bytes2),
        Err(ManagedError::Download)
    ));
    let pending = second.status().unwrap();
    assert_eq!(pending.installation, "operation_incomplete");
    assert_eq!(pending.binary_integrity, "sha256_and_size_verified");
    assert_eq!(pending.current.unwrap().version, "4.0.21");
    assert_eq!(
        second.preview(Action::Update, None).unwrap().plan_id,
        plan.plan_id
    );
    assert!(matches!(
        second.preview(Action::Uninstall, None),
        Err(ManagedError::Pending)
    ));
    bytes2.fail = false;
    let completed = second
        .apply_with(Action::Update, None, &plan.plan_id, &mut bytes2)
        .unwrap();
    assert_eq!(completed.outcome, "committed");
    assert!(second.status().unwrap().pending.is_none());
}

#[test]
fn cancel_releases_a_pending_operation_and_permanently_invalidates_its_old_plan() {
    let root = tempfile::tempdir().unwrap();
    let (manager, mut bytes) = fixture(root.path(), "4.0.22", b"v1");
    let plan = manager.preview(Action::Install, None).unwrap();
    bytes.fail = true;
    assert!(
        manager
            .apply_with(Action::Install, None, &plan.plan_id, &mut bytes)
            .is_err()
    );
    let canceled = manager
        .cancel(Action::Install, None, &plan.plan_id)
        .unwrap();
    assert_eq!(canceled.action, Action::Cancel);
    assert!(manager.status().unwrap().pending.is_none());
    bytes.fail = false;
    assert!(matches!(
        manager.apply_with(Action::Install, None, &plan.plan_id, &mut bytes),
        Err(ManagedError::Conflict)
    ));
    assert_eq!(
        manager
            .cancel(Action::Install, None, &plan.plan_id)
            .unwrap()
            .change_id,
        canceled.change_id
    );
    let new = manager.preview(Action::Install, None).unwrap();
    assert_ne!(new.plan_id, plan.plan_id);
    manager
        .apply_with(Action::Install, None, &new.plan_id, &mut bytes)
        .unwrap();
}

#[test]
fn checksum_failure_does_not_publish_binary_and_modified_files_are_never_removed() {
    let root = tempfile::tempdir().unwrap();
    let (manager, mut download) = fixture(root.path(), "4.0.22", b"trusted");
    let plan = manager.preview(Action::Install, None).unwrap();
    download.bytes = b"tampered".to_vec();
    assert!(matches!(
        manager.apply_with(Action::Install, None, &plan.plan_id, &mut download),
        Err(ManagedError::Integrity)
    ));
    assert!(!manager.artifact(&manager.candidate).exists());
    download.bytes = b"trusted".to_vec();
    manager
        .apply_with(Action::Install, None, &plan.plan_id, &mut download)
        .unwrap();
    let removal = manager.preview(Action::Uninstall, None).unwrap();
    fs::write(
        manager.artifact(&manager.candidate),
        b"user-modified-binary",
    )
    .unwrap();
    assert!(matches!(
        manager.apply_with(Action::Uninstall, None, &removal.plan_id, &mut download),
        Err(ManagedError::ModifiedArtifact)
    ));
    assert_eq!(
        fs::read(manager.artifact(&manager.candidate)).unwrap(),
        b"user-modified-binary"
    );
    assert_eq!(manager.status().unwrap().binary_integrity, "modified");
}

#[test]
fn own_writer_lock_blocks_concurrent_operations_and_unknown_directories_are_not_adopted() {
    let root = tempfile::tempdir().unwrap();
    let (manager, mut download) = fixture(root.path(), "4.0.22", b"v1");
    let plan = manager.preview(Action::Install, None).unwrap();
    let lock = files::write_lock(&manager.root).unwrap();
    assert!(matches!(
        manager.apply_with(Action::Install, None, &plan.plan_id, &mut download),
        Err(ManagedError::Busy)
    ));
    drop(lock);
    manager
        .apply_with(Action::Install, None, &plan.plan_id, &mut download)
        .unwrap();
    let other = tempfile::tempdir().unwrap();
    let (unowned, mut bytes) = fixture(other.path(), "4.0.22", b"v1");
    fs::create_dir(&unowned.root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unowned.root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(unowned.root.join("owned-by-user"), b"keep").unwrap();
    assert!(matches!(
        unowned.apply_with(Action::Install, None, &plan.plan_id, &mut bytes),
        Err(ManagedError::NotOwned)
    ));
    assert_eq!(
        fs::read(unowned.root.join("owned-by-user")).unwrap(),
        b"keep"
    );
}

#[test]
fn version_restore_refuses_newer_native_database_without_reverting_application_data() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("cc-switch.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("PRAGMA user_version=23; CREATE TABLE user_data(value TEXT); INSERT INTO user_data VALUES('keep');").unwrap();
    let (mut first, mut bytes1) = fixture(root.path(), "4.0.21", b"v1");
    first.candidate.max_database_version = 23;
    first.database = Some(database.clone());
    apply(&first, &mut bytes1, Action::Install, None);
    let (mut second, mut bytes2) = fixture(root.path(), "4.0.22", b"v2");
    second.database = Some(database.clone());
    let update = apply(&second, &mut bytes2, Action::Update, None);
    connection.execute_batch("PRAGMA user_version=24;").unwrap();
    let id = update.receipt.unwrap().change_id;
    assert!(matches!(
        second.preview(Action::Restore, Some(&id)),
        Err(ManagedError::DatabaseCompatibility)
    ));
    assert_eq!(second.status().unwrap().current.unwrap().version, "4.0.22");
    assert_eq!(
        connection
            .query_row("SELECT value FROM user_data", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_parent_and_artifact_are_refused() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join("redirect")).unwrap();
    assert!(matches!(
        Manager::new(
            root.path().join("redirect/managed"),
            "linux",
            "x86_64",
            None
        ),
        Err(ManagedError::UnsafePath)
    ));
    let (manager, mut bytes) = fixture(root.path(), "4.0.22", b"v1");
    apply(&manager, &mut bytes, Action::Install, None);
    let path = manager.artifact(&manager.candidate);
    fs::remove_file(&path).unwrap();
    symlink(outside.path().join("user-binary"), &path).unwrap();
    assert!(matches!(
        manager.preview(Action::Uninstall, None),
        Err(ManagedError::UnsafePath)
    ));
}
