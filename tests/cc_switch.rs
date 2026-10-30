#![allow(clippy::unwrap_used, clippy::expect_used)]

use lmm::cc_switch::{self, DatabaseError};
use rusqlite::Connection;

fn fixture(path: &std::path::Path, version: u32) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection
        .pragma_update(None, "user_version", version)
        .unwrap();
    connection.execute_batch(
        "CREATE TABLE providers (
            id TEXT NOT NULL, app_type TEXT NOT NULL, name TEXT NOT NULL,
            settings_config TEXT NOT NULL, meta TEXT NOT NULL DEFAULT '{}',
            is_current BOOLEAN NOT NULL DEFAULT 0, PRIMARY KEY(id,app_type));
         INSERT INTO providers(id,app_type,name,settings_config,is_current)
         VALUES('lmm-codex','codex','LMM','{\"auth\":{\"OPENAI_API_KEY\":\"DO_NOT_LEAK_THIS_KEY\"}}',1),
               ('other','codex','User choice','{\"unrelated\":true}',0);"
    ).unwrap();
    connection
}

#[test]
fn schema_24_inspection_distinguishes_database_selection_from_live_application_and_never_emits_keys()
 {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cc-switch.db");
    let connection = fixture(&path, 24);
    let before: String = connection
        .query_row(
            "SELECT settings_config FROM providers WHERE id='lmm-codex'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let status = cc_switch::inspect(&path).unwrap();
    assert_eq!(status.providers.len(), 2);
    assert_eq!(status.schema_version, 24);
    assert!(status.providers[0].selected_in_database);
    assert_eq!(
        status.providers[0].live_application_selection,
        "not_checked"
    );
    assert_eq!(status.providers[0].authorization, "not_checked");
    let json = serde_json::to_string(&status).unwrap();
    assert!(!json.contains("DO_NOT_LEAK_THIS_KEY"));
    assert!(!json.contains("settings_config"));
    assert!(!status.credential_contents_read);
    assert_eq!(
        connection
            .query_row(
                "SELECT settings_config FROM providers WHERE id='lmm-codex'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        before
    );
}

#[test]
fn unknown_schema_views_in_place_of_native_tables_and_malformed_databases_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cc-switch.db");
    let connection = fixture(&path, 23);
    assert!(matches!(
        cc_switch::inspect(&path),
        Err(DatabaseError::UnsupportedSchema)
    ));
    connection.execute_batch("PRAGMA user_version=24; ALTER TABLE providers RENAME TO other; CREATE VIEW providers AS SELECT * FROM other;").unwrap();
    assert!(matches!(
        cc_switch::inspect(&path),
        Err(DatabaseError::UnsupportedSchema)
    ));
    let invalid = root.path().join("invalid.db");
    std::fs::write(&invalid, b"not a SQLite database").unwrap();
    assert!(matches!(
        cc_switch::inspect(&invalid),
        Err(DatabaseError::Storage)
    ));
}

#[test]
fn uncommitted_native_writer_changes_do_not_leak_into_a_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cc-switch.db");
    let connection = fixture(&path, 24);
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL; BEGIN IMMEDIATE; UPDATE providers SET is_current=0;",
        )
        .unwrap();
    let status = cc_switch::inspect(&path).unwrap();
    assert!(status.providers[0].selected_in_database);
    connection.execute_batch("ROLLBACK;").unwrap();
}
