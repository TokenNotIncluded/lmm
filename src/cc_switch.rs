//! Native SQLite inspection for the reviewed CC Switch schema version 24.
//! Credentials and settings blobs never leave SQLite or enter public reports.
//! A selected database row alone does not establish the application's live
//! configuration, local proxy route or a successful model call.

pub mod bindings;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::{
    fs,
    io::Read,
    path::{Component, Path},
    time::Duration,
};

pub const UPSTREAM_COMMIT: &str = "b76426741ef93069e2f374ac01ca7ef5760e1d0e";
pub const SCHEMA_VERSION: u32 = 24;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("需要明确、存在且不经过符号链接的 CC Switch 数据库")]
    Path,
    #[error("CC Switch 数据库忙、不可读或内容损坏")]
    Storage,
    #[error("CC Switch 数据库版本或表结构不属于已核验的 schema 24")]
    UnsupportedSchema,
    #[error("CC Switch 数据超出语义检查的有界范围")]
    Limit,
}

#[derive(Debug, Serialize)]
pub struct ProviderSummary {
    pub id: String,
    pub app_type: String,
    pub name: String,
    pub selected_in_database: bool,
    pub live_application_selection: &'static str,
    pub authorization: &'static str,
}

#[derive(Debug, Serialize)]
pub struct DatabaseStatus {
    pub contract: &'static str,
    pub schema_version: u32,
    pub upstream_commit: &'static str,
    pub providers: Vec<ProviderSummary>,
    pub configuration_manager: &'static str,
    pub credential_contents_read: bool,
    pub actual_route: &'static str,
    pub target_call: &'static str,
}

fn validate_path(path: &Path) -> Result<(), DatabaseError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(DatabaseError::Path);
    }
    let mut prefix = std::path::PathBuf::new();
    for part in path.components() {
        prefix.push(part);
        if fs::symlink_metadata(&prefix)
            .map_err(|_| DatabaseError::Path)?
            .file_type()
            .is_symlink()
        {
            return Err(DatabaseError::Path);
        }
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| DatabaseError::Path)?;
    if !metadata.is_file() {
        return Err(DatabaseError::Path);
    }
    let mut header = [0_u8; 16];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| DatabaseError::Storage)?;
    if &header != b"SQLite format 3\0" {
        return Err(DatabaseError::Storage);
    }
    Ok(())
}

fn open(path: &Path, writable: bool) -> Result<Connection, DatabaseError> {
    validate_path(path)?;
    let mode = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(path, mode | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|_| DatabaseError::Storage)?;
    connection
        .busy_timeout(Duration::from_millis(250))
        .map_err(|_| DatabaseError::Storage)?;
    connection
        .execute_batch("PRAGMA trusted_schema=OFF;")
        .map_err(|_| DatabaseError::Storage)?;
    Ok(connection)
}

pub fn database_version(path: &Path) -> Result<u32, DatabaseError> {
    let connection = open(path, false)?;
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| DatabaseError::Storage)
}

/// An existing database is opened without CREATE. BEGIN IMMEDIATE prevents a
/// concurrent schema writer from racing a local binary-version commit. Closing
/// the connection rolls back this transaction; no database value was changed.
pub(crate) fn lock_database_version(path: &Path) -> Result<Connection, DatabaseError> {
    let connection = open(path, true)?;
    connection
        .execute_batch("BEGIN IMMEDIATE;")
        .map_err(|_| DatabaseError::Storage)?;
    Ok(connection)
}

pub fn inspect(path: &Path) -> Result<DatabaseStatus, DatabaseError> {
    let connection = open(path, false)?;
    connection
        .execute_batch("BEGIN;")
        .map_err(|_| DatabaseError::Storage)?;
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| DatabaseError::Storage)?;
    if version != SCHEMA_VERSION {
        return Err(DatabaseError::UnsupportedSchema);
    }
    let table: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='providers' AND type='table'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| DatabaseError::Storage)?;
    if table != 1 {
        return Err(DatabaseError::UnsupportedSchema);
    }
    let columns = connection
        .prepare("PRAGMA table_info(providers)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|_| DatabaseError::Storage)?;
    for (name, kind, primary_key) in [
        ("id", "TEXT", 1),
        ("app_type", "TEXT", 2),
        ("name", "TEXT", 0),
        ("settings_config", "TEXT", 0),
        ("meta", "TEXT", 0),
        ("is_current", "BOOLEAN", 0),
    ] {
        if !columns.iter().any(|(actual, actual_kind, actual_pk)| {
            actual == name && actual_kind.eq_ignore_ascii_case(kind) && *actual_pk == primary_key
        }) {
            return Err(DatabaseError::UnsupportedSchema);
        }
    }
    let mut statement = connection
        .prepare(
            "SELECT id,app_type,name,is_current FROM providers ORDER BY app_type,id LIMIT 1001",
        )
        .map_err(|_| DatabaseError::Storage)?;
    let rows = statement
        .query_map([], |row| {
            Ok(ProviderSummary {
                id: row.get(0)?,
                app_type: row.get(1)?,
                name: row.get(2)?,
                selected_in_database: row.get(3)?,
                live_application_selection: "not_checked",
                authorization: "not_checked",
            })
        })
        .map_err(|_| DatabaseError::Storage)?;
    let providers = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| DatabaseError::Storage)?;
    if providers.len() > 1000
        || providers.iter().any(|provider| {
            [&provider.id, &provider.app_type, &provider.name]
                .iter()
                .any(|text| text.len() > 1024 || text.chars().any(char::is_control))
        })
    {
        return Err(DatabaseError::Limit);
    }
    Ok(DatabaseStatus {
        contract: "lmm.cc-switch-inspection.v1",
        schema_version: version,
        upstream_commit: UPSTREAM_COMMIT,
        providers,
        configuration_manager: "cc-switch",
        credential_contents_read: false,
        actual_route: "not_checked",
        target_call: "not_checked",
    })
}
