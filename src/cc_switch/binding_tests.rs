#![allow(clippy::unwrap_used)]
//! Real native SQLite/CAS evidence with synthetic provider data. Private
//! prepared plans isolate the database adapter; these tests do not assert Core
//! authorization, OS keyring, CC Switch UI activation or a model call passed.
use super::*;

fn fixture(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch("PRAGMA user_version=24;
      CREATE TABLE providers (
        id TEXT NOT NULL,app_type TEXT NOT NULL,name TEXT NOT NULL,settings_config TEXT NOT NULL,
        website_url TEXT,category TEXT,created_at INTEGER,sort_index INTEGER,notes TEXT,icon TEXT,icon_color TEXT,
        meta TEXT NOT NULL DEFAULT '{}',is_current BOOLEAN NOT NULL DEFAULT 0,in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
        PRIMARY KEY(id,app_type));
      INSERT INTO providers(id,app_type,name,settings_config,is_current)
        VALUES('unrelated','codex','User account','{\"auth\":{\"OPENAI_API_KEY\":\"UNRELATED_SECRET\"}}',1);").unwrap();
    connection
}
fn native(alpha: &str, gamma: bool) -> Value {
    let mut rows = vec![
        json!({"model":"alpha","displayName":alpha,"inputModalities":["text"]}),
        json!({"model":"beta","displayName":"Beta","inputModalities":["text"]}),
    ];
    if gamma {
        rows.push(json!({"model":"gamma","displayName":"Gamma","inputModalities":["text"]}));
    }
    json!({"models":rows})
}
fn data() -> BindingData {
    BindingData {
        contract: CONTRACT.into(),
        issuer: "https://synthetic.example".into(),
        installation_id: "a".repeat(64),
        provider_id: format!("lmm-{}", "a".repeat(64)),
        grant_id: format!("lmm_gr_{}", "b".repeat(64)),
        budget_credits: 200,
        group: "default".into(),
        display_name: "LMM workstation".into(),
        initial_model: "alpha".into(),
        helper_path: "/synthetic/bin/lmm".into(),
        helper_directory: "/synthetic/bin".into(),
        revision: 1,
        active: true,
        native_catalog: native("Alpha", false),
        prices: json!([]),
        original_row_digest: String::new(),
    }
}
fn added(path: &Path, connection: &Connection) -> (BindingPlan, BindingReceipt) {
    let planned = preview_add_in(connection, path, data()).unwrap();
    let receipt = apply_prepared(path, &planned).unwrap();
    (planned, receipt)
}
fn current(connection: &Connection) -> BindingData {
    binding(connection, &data().issuer, &data().installation_id)
        .unwrap()
        .unwrap()
}
fn settings(connection: &Connection) -> Value {
    provider(connection, &data().provider_id)
        .unwrap()
        .unwrap()
        .settings
}
fn write_settings(connection: &Connection, value: &Value) {
    connection
        .execute(
            "UPDATE providers SET settings_config=?1 WHERE id=?2 AND app_type='codex'",
            params![encode(value).unwrap(), data().provider_id],
        )
        .unwrap();
}
fn changes(connection: &Connection) -> i64 {
    connection
        .query_row("SELECT count(*) FROM lmm_cli_changes_v1", [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn add_is_readonly_until_confirmed_and_commits_an_exact_recoverable_receipt_without_activation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cc-switch.db");
    let db = fixture(&path);
    let planned = preview_add_in(&db, &path, data()).unwrap();
    assert!(!tables(&db).unwrap());
    assert!(provider(&db, &data().provider_id).unwrap().is_none());
    assert!(matches!(
        apply(&path, &planned, "wrong-plan", None),
        Err(BindingError::Confirmation)
    ));
    assert!(matches!(
        apply(&path, &planned, &planned.plan_id, None),
        Err(BindingError::Grant)
    ));
    assert!(!tables(&db).unwrap());
    let first = apply_prepared(&path, &planned).unwrap();
    let replay = apply_prepared(&path, &planned).unwrap();
    let recovered = receipt(
        &path,
        &data().issuer,
        &data().installation_id,
        &first.change_id,
    )
    .unwrap();
    assert_eq!(first.change_id, replay.change_id);
    assert_eq!(first.change_id, recovered.change_id);
    assert_eq!(changes(&db), 1);
    assert!(
        !provider(&db, &data().provider_id)
            .unwrap()
            .unwrap()
            .selected
    );
    assert!(provider(&db, "unrelated").unwrap().unwrap().selected);
    assert!(
        !first.selection_changed && !first.live_application_changed && !first.authorization_changed
    );
    let native = settings(&db);
    assert_eq!(native["auth"], json!({}));
    let config: toml::Value = toml::from_str(native["config"].as_str().unwrap()).unwrap();
    assert_eq!(
        config["model_providers"]["lmm"]["auth"]["command"].as_str(),
        Some("/synthetic/bin/lmm")
    );
    assert_eq!(
        config["model_providers"]["lmm"]["auth"]["refresh_interval_ms"].as_integer(),
        Some(60000)
    );
    assert_eq!(
        config["model_providers"]["lmm"]["http_headers"]["x-lmm-group"].as_str(),
        Some("ZGVmYXVsdA")
    );
    let journal: String = db
        .query_row("SELECT payload FROM lmm_cli_changes_v1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(!journal.contains("UNRELATED_SECRET"));
    assert!(!encode(&planned).unwrap().contains("UNRELATED_SECRET"));
    assert!(!journal.contains("lmm_at_") && !journal.contains("lmm_rt_"));
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        24
    );
}

#[test]
fn sync_keeps_user_names_extra_fields_native_model_choice_and_other_provider_selection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cc-switch.db");
    let db = fixture(&path);
    added(&path, &db);
    db.execute("UPDATE providers SET name='User title',notes='User notes',meta='{\"customFlag\":true}' WHERE id=?1", [data().provider_id]).unwrap();
    let mut native = settings(&db);
    let config = native["config"]
        .as_str()
        .unwrap()
        .replace("model = \"alpha\"", "model = \"beta\"")
        .replace(
            "[model_providers.lmm]",
            "model_reasoning_effort = \"low\"\n\n[model_providers.lmm]",
        );
    native["config"] = json!(config);
    native["userSetting"] = json!({"theme":"dark"});
    native["modelCatalog"]["models"][0]["displayName"] = json!("User's model name");
    native["modelCatalog"]["models"][0]["contextWindow"] = json!(12000);
    write_settings(&db, &native);
    let planned = preview_sync_in(
        &db,
        &path,
        current(&db),
        native_catalog("Updated server name", true),
        json!([{"model":"alpha","input":0.0001}]),
    )
    .unwrap();
    let applied = apply_prepared(&path, &planned).unwrap();
    let after = settings(&db);
    assert_eq!(after["config"], native["config"]);
    assert_eq!(after["userSetting"], native["userSetting"]);
    assert_eq!(
        after["modelCatalog"]["models"][0]["displayName"],
        "User's model name"
    );
    assert_eq!(after["modelCatalog"]["models"][0]["contextWindow"], 12000);
    assert_eq!(after["modelCatalog"]["models"].as_array().unwrap().len(), 3);
    assert!(provider(&db, "unrelated").unwrap().unwrap().selected);
    assert!(!applied.selection_changed);
    let row: (String, String, String) = db
        .query_row(
            "SELECT name,notes,meta FROM providers WHERE id=?1",
            [data().provider_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "User title".into(),
            "User notes".into(),
            "{\"customFlag\":true}".into()
        )
    );
}
fn native_catalog(alpha: &str, gamma: bool) -> Value {
    native(alpha, gamma)
}

#[test]
fn native_writer_lock_and_preview_changes_prevent_a_stale_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cc-switch.db");
    let db = fixture(&path);
    added(&path, &db);
    let planned = preview_sync_in(
        &db,
        &path,
        current(&db),
        native("Updated Alpha", true),
        json!([]),
    )
    .unwrap();
    db.execute_batch("BEGIN IMMEDIATE;").unwrap();
    assert!(matches!(
        apply_prepared(&path, &planned),
        Err(BindingError::Conflict)
    ));
    db.execute(
        "UPDATE providers SET notes='after preview' WHERE id=?1",
        [data().provider_id],
    )
    .unwrap();
    db.execute_batch("COMMIT;").unwrap();
    assert!(matches!(
        apply_prepared(&path, &planned),
        Err(BindingError::Conflict)
    ));
    assert_eq!(changes(&db), 1);
    assert_eq!(current(&db).revision, 1);
}

#[test]
fn restore_sync_merges_by_model_identity_preserving_following_user_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cc-switch.db");
    let db = fixture(&path);
    added(&path, &db);
    let sync = preview_sync_in(
        &db,
        &path,
        current(&db),
        native("Renamed Alpha", true),
        json!([{"model":"alpha","input":0.2}]),
    )
    .unwrap();
    let changed = apply_prepared(&path, &sync).unwrap();
    let mut newer = settings(&db);
    newer["modelCatalog"]["models"][0]["displayName"] = json!("User override after sync");
    newer["userSetting"] = json!("preserve me");
    write_settings(&db, &newer);
    db.execute(
        "UPDATE providers SET notes='new notes' WHERE id=?1",
        [data().provider_id],
    )
    .unwrap();
    let planned = preview_restore(
        &path,
        &data().issuer,
        &data().installation_id,
        &changed.change_id,
    )
    .unwrap();
    let restored = apply(&path, &planned, &planned.plan_id, None).unwrap();
    let after = settings(&db);
    assert_eq!(after["modelCatalog"]["models"].as_array().unwrap().len(), 2);
    assert_eq!(
        after["modelCatalog"]["models"][0]["displayName"],
        "User override after sync"
    );
    assert_eq!(after["userSetting"], "preserve me");
    assert_eq!(after["config"], newer["config"]);
    assert_eq!(restored.revision, 3);
    assert!(!restored.authorization_changed && !restored.selection_changed);
    assert_eq!(changes(&db), 3);
    assert_eq!(current(&db).prices, json!([]));
}

#[test]
fn disappearing_selected_model_or_removal_of_a_user_extended_item_cannot_be_silently_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cc-switch.db");
    let db = fixture(&path);
    added(&path, &db);
    let without_alpha =
        json!({"models":[{"model":"beta","displayName":"Beta","inputModalities":["text"]}]});
    assert!(matches!(
        preview_sync_in(&db, &path, current(&db), without_alpha, json!([])),
        Err(BindingError::SelectedModelUnavailable)
    ));
    let sync = preview_sync_in(&db, &path, current(&db), native("Alpha", true), json!([])).unwrap();
    let changed = apply_prepared(&path, &sync).unwrap();
    let mut newer = settings(&db);
    newer["modelCatalog"]["models"][2]["contextWindow"] = json!(64000);
    write_settings(&db, &newer);
    assert!(matches!(
        preview_restore(
            &path,
            &data().issuer,
            &data().installation_id,
            &changed.change_id
        ),
        Err(BindingError::Conflict)
    ));
    assert_eq!(settings(&db), newer);
    assert_eq!(changes(&db), 2);
}

#[test]
fn restore_add_never_treats_an_unselected_sqlite_row_as_proof_that_native_stack_references_are_absent()
 {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cc-switch.db");
    let db = fixture(&path);
    let (_, added) = added(&path, &db);
    assert!(
        !provider(&db, &data().provider_id)
            .unwrap()
            .unwrap()
            .selected
    );
    assert!(matches!(
        preview_restore(
            &path,
            &data().issuer,
            &data().installation_id,
            &added.change_id
        ),
        Err(BindingError::NativeRemoval)
    ));
    assert!(provider(&db, &data().provider_id).unwrap().is_some());
    assert_eq!(changes(&db), 1);
}

#[test]
fn unknown_schema_existing_foreign_provider_and_broken_management_namespace_are_not_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cc-switch.db");
    let db = fixture(&path);
    db.execute_batch("PRAGMA user_version=25;").unwrap();
    assert!(matches!(
        connection(&path, false),
        Err(BindingError::Schema)
    ));
    db.execute_batch("PRAGMA user_version=24;").unwrap();
    db.execute("INSERT INTO providers(id,app_type,name,settings_config) VALUES(?1,'codex','Other owner','{}')", [data().provider_id]).unwrap();
    assert!(matches!(
        preview_add_in(&db, &path, data()),
        Err(BindingError::Conflict)
    ));
    db.execute("DELETE FROM providers WHERE id=?1", [data().provider_id])
        .unwrap();
    db.execute_batch("CREATE TABLE lmm_cli_bindings_v1(unrelated TEXT);")
        .unwrap();
    assert!(matches!(
        preview_add_in(&db, &path, data()),
        Err(BindingError::Schema)
    ));
}
