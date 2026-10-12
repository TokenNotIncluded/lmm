use super::{AuthError, protocol::Credential};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
};
use zeroize::Zeroizing;

#[derive(Deserialize, Serialize)]
pub(super) struct StoredSession {
    pub credential: Credential,
    pub refresh_blocked: bool,
}

pub(super) trait SecretStore {
    fn load(&self) -> Result<Option<StoredSession>, AuthError>;
    fn save(&self, session: &StoredSession) -> Result<(), AuthError>;
    fn delete(&self) -> Result<(), AuthError>;
}

pub(super) struct SystemStore {
    entry: keyring::Entry,
    issuer: String,
}

impl SystemStore {
    pub fn new(issuer: &str) -> Result<Self, AuthError> {
        Ok(Self {
            entry: keyring::Entry::new("best.lmm.cli.oauth", issuer)
                .map_err(|_| AuthError::CredentialStore)?,
            issuer: issuer.to_owned(),
        })
    }
}

impl SecretStore for SystemStore {
    fn load(&self) -> Result<Option<StoredSession>, AuthError> {
        let raw = match self.entry.get_password() {
            Ok(raw) => Zeroizing::new(raw),
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(_) => return Err(AuthError::CredentialStore),
        };
        if raw.len() > 64 * 1024 {
            return Err(AuthError::CredentialStore);
        }
        let session: StoredSession =
            serde_json::from_str(&raw).map_err(|_| AuthError::CredentialStore)?;
        if !session.credential.valid_for(&self.issuer) {
            return Err(AuthError::CredentialStore);
        }
        Ok(Some(session))
    }

    fn save(&self, session: &StoredSession) -> Result<(), AuthError> {
        if !session.credential.valid_for(&self.issuer) {
            return Err(AuthError::CredentialStore);
        }
        let raw =
            Zeroizing::new(serde_json::to_string(session).map_err(|_| AuthError::CredentialStore)?);
        self.entry
            .set_password(&raw)
            .map_err(|_| AuthError::CredentialStore)
    }

    fn delete(&self) -> Result<(), AuthError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(AuthError::CredentialStore),
        }
    }
}

fn lock_directory() -> Result<PathBuf, AuthError> {
    crate::environment::user_directory(crate::environment::UserDirectory::State)
        .ok_or(AuthError::Busy)
}

/// The lock stores no secrets; OS process exit releases it even after cancellation.
pub(super) fn lock() -> Result<File, AuthError> {
    lock_at(&lock_directory()?)
}

fn lock_at(directory: &std::path::Path) -> Result<File, AuthError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory).map_err(|_| AuthError::Busy)?;
    let metadata = fs::symlink_metadata(directory).map_err(|_| AuthError::Busy)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AuthError::Busy);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(AuthError::Busy);
        }
    }
    let path = directory.join("oauth.lock");
    if fs::symlink_metadata(&path).is_ok_and(|meta| !meta.is_file()) {
        return Err(AuthError::Busy);
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| AuthError::Busy)?;
    // The creation mode does not fix an existing unsafe lock file.
    // Verify the opened file rather than changing its permissions.
    let metadata = file.metadata().map_err(|_| AuthError::Busy)?;
    if !metadata.is_file() {
        return Err(AuthError::Busy);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(AuthError::Busy);
        }
    }
    file.try_lock_exclusive().map_err(|_| AuthError::Busy)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn lock_serializes_processes_and_releases_after_drop() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state");
        let lock = lock_at(&path).unwrap();
        assert!(matches!(lock_at(&path), Err(AuthError::Busy)));
        drop(lock);
        assert!(lock_at(&path).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn lock_refuses_symlink_and_shared_directory() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(lock_at(&path), Err(AuthError::Busy)));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        symlink(root.path().join("elsewhere"), path.join("oauth.lock")).unwrap();
        assert!(matches!(lock_at(&path), Err(AuthError::Busy)));
    }

    #[cfg(unix)]
    #[test]
    fn lock_refuses_existing_shared_file_without_repairing_it() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        let initial = lock_at(&directory).unwrap();
        drop(initial);
        let path = directory.join("oauth.lock");
        fs::write(&path, b"synthetic lock marker").unwrap();
        for mode in [0o640, 0o644, 0o660, 0o666] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert!(matches!(lock_at(&directory), Err(AuthError::Busy)));
            assert_eq!(fs::read(&path).unwrap(), b"synthetic lock marker");
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, mode);
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(lock_at(&directory).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn lock_creates_private_directory_and_file() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        let file = lock_at(&directory).unwrap();
        let directory_mode = fs::metadata(&directory).unwrap().permissions().mode();
        let file_mode = file.metadata().unwrap().permissions().mode();
        assert_eq!(directory_mode & 0o077, 0);
        assert_eq!(file_mode & 0o077, 0);
    }
}
