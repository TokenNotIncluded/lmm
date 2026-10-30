use super::{ManagedError, Release};
use fs2::FileExt;
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path},
};

const OWNER: &[u8] = b"lmm.cli.managed-software.v1:cc-switch\n";

pub(super) fn nonce() -> Result<String, ManagedError> {
    let mut bytes = [0_u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| ManagedError::Storage)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Reject symlinks at every existing path component, including a redirecting
/// parent. The installation namespace is private and is never adopted from PATH.
pub(super) fn check_path(path: &Path) -> Result<(), ManagedError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(ManagedError::UnsafePath);
    }
    let mut prefix = std::path::PathBuf::new();
    for part in path.components() {
        prefix.push(part);
        match fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ManagedError::UnsafePath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(_) => return Err(ManagedError::Storage),
        }
    }
    Ok(())
}

fn private_directory(path: &Path) -> Result<(), ManagedError> {
    check_path(path)?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|_| ManagedError::Storage)?;
    directory_is_private(path)
}

fn directory_is_private(path: &Path) -> Result<(), ManagedError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ManagedError::Storage)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ManagedError::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(ManagedError::UnsafePath);
        }
    }
    Ok(())
}

pub(super) fn private_file(path: &Path) -> Result<File, ManagedError> {
    check_path(path)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true).read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|_| ManagedError::Storage)
}

pub(super) fn read(path: &Path, maximum: u64) -> Result<Option<Vec<u8>>, ManagedError> {
    check_path(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ManagedError::Storage),
    };
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(ManagedError::UnsafePath);
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| ManagedError::Storage)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ManagedError::Storage)?;
    if bytes.len() as u64 > maximum {
        return Err(ManagedError::UnsafePath);
    }
    Ok(Some(bytes))
}

/// Reading never creates a directory or lock. A missing namespace is a clean
/// uninstalled state; an unrelated nonempty directory cannot become managed.
pub(super) fn read_lock(root: &Path) -> Result<Option<File>, ManagedError> {
    check_path(root)?;
    if !root.try_exists().map_err(|_| ManagedError::Storage)? {
        return Ok(None);
    }
    directory_is_private(root)?;
    require_owner(root)?;
    let path = root.join("manager.lock");
    check_path(&path)?;
    if !fs::symlink_metadata(&path)
        .map_err(|_| ManagedError::Storage)?
        .is_file()
    {
        return Err(ManagedError::UnsafePath);
    }
    let file = File::open(path).map_err(|_| ManagedError::Storage)?;
    FileExt::try_lock_shared(&file).map_err(|_| ManagedError::Busy)?;
    Ok(Some(file))
}

pub(super) fn write_lock(root: &Path) -> Result<File, ManagedError> {
    private_directory(root)?;
    let marker = root.join("owner");
    let owned = read(&marker, 128)?.is_some();
    if owned {
        require_owner(root)?;
    } else {
        let unrelated = fs::read_dir(root)
            .map_err(|_| ManagedError::Storage)?
            .any(|entry| !entry.is_ok_and(|entry| entry.file_name() == "manager.lock"));
        if unrelated {
            return Err(ManagedError::NotOwned);
        }
    }
    let lock_path = root.join("manager.lock");
    check_path(&lock_path)?;
    if fs::symlink_metadata(&lock_path).is_ok_and(|metadata| !metadata.is_file()) {
        return Err(ManagedError::UnsafePath);
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(lock_path).map_err(|_| ManagedError::Storage)?;
    lock.try_lock_exclusive().map_err(|_| ManagedError::Busy)?;
    if !owned {
        // Another first install may have initialized the marker while this
        // process was waiting for the same OS lock.
        if read(&marker, 128)?.is_none() {
            let mut owner = private_file(&marker)?;
            owner.write_all(OWNER).map_err(|_| ManagedError::Storage)?;
            owner.sync_all().map_err(|_| ManagedError::Storage)?;
        }
        require_owner(root)?;
    }
    private_directory(&root.join("artifacts"))?;
    Ok(lock)
}

fn require_owner(root: &Path) -> Result<(), ManagedError> {
    match read(&root.join("owner"), 128)? {
        Some(bytes) if bytes == OWNER => Ok(()),
        _ => Err(ManagedError::NotOwned),
    }
}

pub(super) fn publish(root: &Path, bytes: &[u8]) -> Result<(), ManagedError> {
    let temporary = root.join(format!(".state-{}.tmp", nonce()?));
    let result = (|| {
        let mut file = private_file(&temporary)?;
        file.write_all(bytes).map_err(|_| ManagedError::Storage)?;
        file.sync_all().map_err(|_| ManagedError::Storage)?;
        check_path(&root.join("state.json"))?;
        fs::rename(&temporary, root.join("state.json")).map_err(|_| ManagedError::Storage)?;
        sync_directory(root)
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn sync_directory(root: &Path) -> Result<(), ManagedError> {
    File::open(root)
        .and_then(|file| file.sync_all())
        .map_err(|_| ManagedError::Storage)
}

pub(super) fn verify(path: &Path, release: &Release) -> Result<bool, ManagedError> {
    check_path(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(ManagedError::Storage),
    };
    if !metadata.is_file() || metadata.len() != release.size {
        return Err(ManagedError::ModifiedArtifact);
    }
    let mut input = File::open(path)
        .map_err(|_| ManagedError::Storage)?
        .take(release.size + 1);
    let mut hash = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let size = input.read(&mut buffer).map_err(|_| ManagedError::Storage)?;
        if size == 0 {
            break;
        }
        count += size as u64;
        hash.update(&buffer[..size]);
    }
    if count != release.size || format!("{:x}", hash.finalize()) != release.sha256 {
        return Err(ManagedError::ModifiedArtifact);
    }
    Ok(true)
}

pub(super) fn store_download(
    root: &Path,
    release: &Release,
    input: &mut dyn Read,
) -> Result<(), ManagedError> {
    let destination = root
        .join("artifacts")
        .join(format!("{}.AppImage", release.sha256));
    if verify(&destination, release)? {
        return Ok(());
    }
    let temporary = root
        .join("artifacts")
        .join(format!(".download-{}.tmp", nonce()?));
    let result = (|| {
        let mut file = private_file(&temporary)?;
        let mut hash = Sha256::new();
        let mut count = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let size = input
                .read(&mut buffer)
                .map_err(|_| ManagedError::Download)?;
            if size == 0 {
                break;
            }
            count = count
                .checked_add(size as u64)
                .ok_or(ManagedError::Integrity)?;
            if count > release.size {
                return Err(ManagedError::Integrity);
            }
            file.write_all(&buffer[..size])
                .map_err(|_| ManagedError::Storage)?;
            hash.update(&buffer[..size]);
        }
        if count != release.size || format!("{:x}", hash.finalize()) != release.sha256 {
            return Err(ManagedError::Integrity);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o700))
                .map_err(|_| ManagedError::Storage)?;
        }
        file.sync_all().map_err(|_| ManagedError::Storage)?;
        // Hard-link publication has no replace operation: an unexpected file
        // is never overwritten, even between checking and publishing.
        match fs::hard_link(&temporary, &destination) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !verify(&destination, release)? {
                    return Err(ManagedError::Conflict);
                }
            }
            Err(_) => return Err(ManagedError::Storage),
        }
        sync_directory(&root.join("artifacts"))
    })();
    let _ = fs::remove_file(&temporary);
    result
}
