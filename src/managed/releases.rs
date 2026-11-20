use super::{ManagedError, Release};

pub(super) const SOURCE_COMMIT: &str = "b76426741ef93069e2f374ac01ca7ef5760e1d0e";
pub(super) const RELEASE_VERSION: &str = "4.0.22";
pub(super) const MAX_BYTES: u64 = 200 * 1024 * 1024;

/// Release digests are pinned from the official GitHub release, never supplied
/// by a server reached through an OAuth credential or a user-controlled URL.
pub(super) fn current(os: &str, arch: &str) -> Result<Release, ManagedError> {
    let (asset_arch, sha256, size) = match (os, arch) {
        ("linux", "x86_64") => (
            "x86_64",
            "9090a1bc5f58ccbc72444302c1a3d8001dd1b00fbff7b0d6b64bd1be58077313",
            97_339_896,
        ),
        ("linux", "aarch64") => (
            "arm64",
            "b00ffbe004fe7885ddc7fa33badb965a2cf4d626a259287ea9a4f89fa78bc66e",
            94_763_528,
        ),
        _ => return Err(ManagedError::UnsupportedPlatform),
    };
    Ok(Release {
        version: RELEASE_VERSION.into(),
        source_commit: SOURCE_COMMIT.into(),
        download_url: asset_url(RELEASE_VERSION, asset_arch),
        sha256: sha256.into(),
        size,
        max_database_version: 24,
    })
}

fn asset_url(version: &str, arch: &str) -> String {
    format!(
        "https://github.com/farion1231/cc-switch/releases/download/v{version}/CC-Switch-v{version}-Linux-{arch}.AppImage"
    )
}

pub(super) fn valid(release: &Release, arch: &str) -> bool {
    let asset_arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        _ => return false,
    };
    version(&release.version).is_some()
        && super::hex(&release.sha256, 64)
        && super::hex(&release.source_commit, 40)
        && (1..=MAX_BYTES).contains(&release.size)
        && (1..=24).contains(&release.max_database_version)
        && release.download_url == asset_url(&release.version, asset_arch)
}

pub(super) fn version(value: &str) -> Option<(u32, u32, u32)> {
    if value.len() > 40 {
        return None;
    }
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}
