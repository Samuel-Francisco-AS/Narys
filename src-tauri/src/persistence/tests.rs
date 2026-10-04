use super::*;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
fn fixture() -> (Database, PathBuf) {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lr4-synthetic-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    (
        Database::for_test(dir.join("test.sqlite3")),
        dir.join("bootstrap.json"),
    )
}
fn data(version: &str) -> Value {
    json!({
    "identity":{"version":version,"canonicalName":"Synthetic","presentation":"neutral","primaryLanguage":"pt-BR","concept":"test",
      "traits":{"curiosity":"high"},"behavioralInvariants":["be_clear"],"modes":{"test":{"priority":"test","tone":"calm"}},
      "relationship":{"primaryPersonName":"Tester","relationModes":["testing"],
        "affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,"playfulTerritoriality":false,"coercion":false,"isolation":false,"emotionalBlackmail":false},
        "interactionPreferences":{"wantsRealDisagreement":true,"wantsLunaToProposeDirectionsDuringStructuring":false,"prefersLinearFlowDuringImplementation":true}},
      "memoryPolicy":{"retrieval":"selective","history":"versioned","continuity":"revisable","storePrivateChainOfThought":false},
      "provenance":"synthetic test","effectiveFrom":"2026-01-01"},
    "memories":[
      {"importKey":"synthetic-a","type":"preference","domains":["tests"],"state":"active","title":"A","summary":"Synthetic A","importance":9,"confidence":"high","eventDate":"2026-01-02"},
      {"importKey":"synthetic-b","type":"decision","domains":["other"],"state":"active","title":"B","summary":"Synthetic B","importance":4,"confidence":"medium","eventDate":"2026-01-01"}
    ]})
}
fn write(path: &std::path::Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn assert_sqlite_integrity(conn: &rusqlite::Connection) {
    let integrity: String = conn
        .pragma_query_value(None, "integrity_check", |row| row.get(0))
        .unwrap();
    let foreign_keys: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(integrity, "ok");
    assert_eq!(foreign_keys, 0);
}
#[test]
fn migration_empty_and_twice() {
    let (db, _) = fixture();
    let conn = db.open().unwrap();
    let v: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(v, 12);
    assert_sqlite_integrity(&conn);
    drop(conn);
    assert_sqlite_integrity(&db.open().unwrap());
}

#[test]
fn migration_003_upgrades_existing_version_2_without_changing_conversations() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "{} {} PRAGMA user_version=2;",
        include_str!("../../migrations/001_initial_persistence.sql"),
        include_str!("../../migrations/002_conversation_history.sql")
    ))
    .unwrap();
    conn.execute("INSERT INTO conversation_sessions(kind,status,title) VALUES ('product','closed','Antes da policy')", []).unwrap();
    migrations::apply(&conn).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 12);
    let title: String = conn
        .query_row(
            "SELECT title FROM conversation_sessions WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(title, "Antes da policy");
    let rows: Vec<(String, String, Option<String>, Option<i64>, i64)> = conn.prepare(
    "SELECT p.role,t.model,t.thinking_level,p.max_output_tokens,p.max_provider_calls FROM cognitive_role_policies p JOIN cognitive_role_targets t ON t.role=p.role AND t.position=0 ORDER BY p.role").unwrap()
    .query_map([], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(
        rows,
        vec![
            (
                "conversation".into(),
                "gemini-3.8-flash".into(),
                Some("low".into()),
                Some(4096),
                2
            ),
            (
                "orchestrator".into(),
                "gemini-3.8-flash".into(),
                Some("low".into()),
                Some(4096),
                2
            ),
            (
                "summary".into(),
                "gemini-3.8-flash".into(),
                Some("low".into()),
                Some(1024),
                1
            ),
            (
                "worker".into(),
                "openai/gpt-oss-20b".into(),
                Some("low".into()),
                Some(4096),
                4
            )
        ]
    );
    migrations::apply(&conn).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM cognitive_role_policies", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_sqlite_integrity(&conn);
}

#[test]
fn migration_backfills_only_untitled_product_sessions() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "{} PRAGMA user_version=1;",
        include_str!("../../migrations/001_initial_persistence.sql")
    ))
    .unwrap();
    conn.execute_batch("INSERT INTO conversation_sessions(title,status) VALUES
    (NULL,'active'),(NULL,'closed'),('Diagnóstico LR-4','diagnostic'),('Luna · Gemini LR-6','active'),('Named legacy','closed');").unwrap();
    migrations::apply(&conn).unwrap();
    let kinds: Vec<String> = conn
        .prepare("SELECT kind FROM conversation_sessions ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(kinds, ["product", "product", "legacy", "legacy", "legacy"]);
    migrations::apply(&conn).unwrap();
    let status: String = conn
        .query_row(
            "SELECT summary_status FROM conversation_sessions WHERE id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "none");
}

#[test]
fn history_list_detail_and_orphan_normalization_are_isolated() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let legacy = conversation::create_diagnostic(&mut conn).unwrap();
    let legacy_active: i64 = {
        conn.execute("INSERT INTO conversation_sessions(title,status,kind) VALUES ('Legacy active','active','legacy')",[]).unwrap();
        conn.last_insert_rowid()
    };
    let a = conversation::create_session(&conn).unwrap();
    let b = conversation::create_session(&conn).unwrap();
    let empty = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(
        &mut conn,
        a,
        "HISTORICO-SECRETO-55 😀 ",
        "Resposta A",
    )
    .unwrap();
    conversation::append_exchange_to_session(&mut conn, b, &"B".repeat(400), "Resposta B").unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET updated_at='2026-01-01T00:00:00Z' WHERE id=?1",
        [a],
    )
    .unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET updated_at='2026-01-02T00:00:00Z' WHERE id=?1",
        [b],
    )
    .unwrap();
    let items = conversation::list_history(&conn, 50).unwrap();
    assert_eq!(
        items.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![b, a]
    );
    assert_eq!(items[0].message_count, 2);
    assert!(items[0].preview.chars().count() <= 101);
    assert!(items[0].title.chars().count() <= 61);
    assert!(!items
        .iter()
        .any(|item| item.id == legacy || item.id == empty));
    assert_eq!(conversation::list_history(&conn, 1).unwrap().len(), 1);
    conn.execute(
        "UPDATE conversation_sessions SET updated_at='2026-01-01T00:00:00Z' WHERE id=?1",
        [b],
    )
    .unwrap();
    assert_eq!(
        conversation::list_history(&conn, 50)
            .unwrap()
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![b, a]
    );
    let a_detail = conversation::history_session(&conn, a).unwrap().unwrap();
    assert_eq!(a_detail.messages.len(), 2);
    assert_eq!(a_detail.messages[0].content, "HISTORICO-SECRETO-55 😀 ");
    assert!(!a_detail
        .messages
        .iter()
        .any(|message| message.content.contains("Resposta B")));
    assert!(conversation::history_session(&conn, legacy)
        .unwrap()
        .is_none());
    assert!(conversation::history_session(&conn, i64::MAX)
        .unwrap()
        .is_none());
    assert!(conversation::history_session(&conn, 0).unwrap().is_none());
    let before:(String,String,i64)=conn.query_row("SELECT status,summary_status,(SELECT COUNT(*) FROM conversation_messages WHERE session_id=?1) FROM conversation_sessions WHERE id=?1",[a],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    conversation::list_history(&conn, 50).unwrap();
    conversation::history_session(&conn, a).unwrap();
    let after:(String,String,i64)=conn.query_row("SELECT status,summary_status,(SELECT COUNT(*) FROM conversation_messages WHERE session_id=?1) FROM conversation_sessions WHERE id=?1",[a],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(before, after);
    assert_eq!(
        conversation::close_orphaned_product_sessions(&conn).unwrap(),
        3
    );
    assert_eq!(
        conversation::close_orphaned_product_sessions(&conn).unwrap(),
        0
    );
    for id in [a, b] {
        let (status, summary): (String, String) = conn
            .query_row(
                "SELECT status,summary_status FROM conversation_sessions WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((status.as_str(), summary.as_str()), ("closed", "pending"));
    }
    let empty_summary: String = conn
        .query_row(
            "SELECT summary_status FROM conversation_sessions WHERE id=?1",
            [empty],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(empty_summary, "none");
    let legacy_status: String = conn
        .query_row(
            "SELECT status FROM conversation_sessions WHERE id=?1",
            [legacy],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(legacy_status, "diagnostic");
    let active_status: String = conn
        .query_row(
            "SELECT status FROM conversation_sessions WHERE id=?1",
            [legacy_active],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active_status, "active");
}

#[test]
fn summary_input_zero_disables_automatic_queueing_and_clears_pending() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();

    conn.execute(
        "UPDATE cognitive_role_policies SET summary_input_max_bytes=0 WHERE role='summary'",
        [],
    )
    .unwrap();

    let disabled = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(
        &mut conn,
        disabled,
        "SUMMARY-DISABLED-1",
        "Resposta",
    )
    .unwrap();
    assert!(conversation::close_session(&conn, disabled).unwrap());
    assert_eq!(
        conversation::history_session(&conn, disabled)
            .unwrap()
            .unwrap()
            .summary_status,
        "none"
    );
    assert!(conversation::claim_next_pending_summary(&mut conn)
        .unwrap()
        .is_none());

    conn.execute(
        "UPDATE cognitive_role_policies SET summary_input_max_bytes=32768 WHERE role='summary'",
        [],
    )
    .unwrap();
    let queued = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(
        &mut conn,
        queued,
        "SUMMARY-ENABLED-2",
        "Resposta",
    )
    .unwrap();
    assert!(conversation::close_session(&conn, queued).unwrap());
    assert_eq!(
        conversation::history_session(&conn, queued)
            .unwrap()
            .unwrap()
            .summary_status,
        "pending"
    );

    conn.execute(
        "UPDATE cognitive_role_policies SET summary_input_max_bytes=0 WHERE role='summary'",
        [],
    )
    .unwrap();
    assert_eq!(conversation::disable_pending_summaries(&conn).unwrap(), 1);
    assert_eq!(
        conversation::history_session(&conn, queued)
            .unwrap()
            .unwrap()
            .summary_status,
        "none"
    );
    assert!(conversation::claim_next_pending_summary(&mut conn)
        .unwrap()
        .is_none());
}

#[test]
fn restart_keeps_old_history_out_of_new_outbound_context() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let old = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(
        &mut conn,
        old,
        "HISTORICO-SECRETO-55",
        "Resposta antiga",
    )
    .unwrap();
    drop(conn);
    let mut conn = db.open().unwrap();
    assert_eq!(
        conversation::close_orphaned_product_sessions(&conn).unwrap(),
        1
    );
    assert!(conversation::history_session(&conn, old).unwrap().is_some());
    let current = conversation::create_session(&conn).unwrap();
    assert_ne!(old, current);
    conversation::append_exchange_to_session(&mut conn, current, "ATUAL-22", "Resposta atual")
        .unwrap();
    let outbound = conversation::outbound_history(
        &conn,
        current,
        conversation::OUTBOUND_HISTORY_MESSAGES,
        conversation::OUTBOUND_HISTORY_BYTES,
    )
    .unwrap();
    assert!(outbound.iter().any(|turn| turn.content == "ATUAL-22"));
    assert!(!outbound
        .iter()
        .any(|turn| turn.content.contains("HISTORICO-SECRETO-55")));
}
#[test]
fn summary_claim_recovery_completion_and_resume_preserve_messages() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let target = conversation::create_session(&conn).unwrap();
    let active = conversation::create_session(&conn).unwrap();
    let legacy = conversation::create_diagnostic(&mut conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, target, "SUMMARY-TARGET-71", "Resposta")
        .unwrap();
    conversation::append_exchange_to_session(&mut conn, active, "OTHER-SESSION-88", "Resposta")
        .unwrap();
    conversation::close_session(&conn, target).unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET summary_status='pending' WHERE id=?1",
        [legacy],
    )
    .unwrap();
    let before: String = conn
        .query_row(
            "SELECT updated_at FROM conversation_sessions WHERE id=?1",
            [target],
            |r| r.get(0),
        )
        .unwrap();
    let claimed = conversation::claim_next_pending_summary(&mut conn)
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id, target);
    assert_eq!(claimed.messages.len(), 2);
    assert!(conversation::claim_next_pending_summary(&mut conn)
        .unwrap()
        .is_none());
    assert_eq!(conversation::reset_interrupted_summaries(&conn).unwrap(), 1);
    drop(conn);
    let mut conn = db.open().unwrap();
    let reclaimed = conversation::claim_next_pending_summary(&mut conn)
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.id, target);
    assert!(conversation::fail_summary(&conn, target, true).unwrap());
    assert_eq!(
        conversation::claim_next_pending_summary(&mut conn)
            .unwrap()
            .unwrap()
            .id,
        target
    );
    assert!(
        conversation::complete_summary(&conn, target, "Título persistido", "Resumo factual")
            .unwrap()
    );
    assert!(!conversation::complete_summary(&conn, target, "Sobrescrito", "Outro").unwrap());
    let after: String = conn
        .query_row(
            "SELECT updated_at FROM conversation_sessions WHERE id=?1",
            [target],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(before, after);
    let item = conversation::list_history(&conn, 50)
        .unwrap()
        .into_iter()
        .find(|v| v.id == target)
        .unwrap();
    assert_eq!(item.title, "Título persistido");
    assert_eq!(item.preview, "Resumo factual");
    let resumed = conversation::resume_session(&mut conn, target, None).unwrap();
    assert_eq!(resumed.title.as_deref(), Some("Título persistido"));
    assert_eq!(resumed.summary_status, "none");
    assert!(resumed.summary.is_none());
    assert!(conversation::claim_next_pending_summary(&mut conn)
        .unwrap()
        .is_none());
    assert!(conversation::close_session(&conn, target).unwrap());
    assert_eq!(
        conversation::claim_next_pending_summary(&mut conn)
            .unwrap()
            .unwrap()
            .id,
        target
    );
    assert!(conversation::fail_summary(&conn, target, false).unwrap());
    assert!(conversation::claim_next_pending_summary(&mut conn)
        .unwrap()
        .is_none());
    assert_eq!(
        conversation::history_session(&conn, target)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
}
#[test]
fn completed_summary_does_not_reorder_history_or_change_messages() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let old = conversation::create_session(&conn).unwrap();
    let recent = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, old, "ALFA-A-11", "Resposta A").unwrap();
    conversation::append_exchange_to_session(&mut conn, recent, "BETA-B-22", "Resposta B").unwrap();
    conversation::close_session(&conn, old).unwrap();
    conversation::close_session(&conn, recent).unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET updated_at='2026-01-01T00:00:00Z' WHERE id=?1",
        [old],
    )
    .unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET updated_at='2026-01-02T00:00:00Z' WHERE id=?1",
        [recent],
    )
    .unwrap();
    assert_eq!(
        conversation::claim_next_pending_summary(&mut conn)
            .unwrap()
            .unwrap()
            .id,
        old
    );
    assert!(conversation::complete_summary(&conn, old, "Título A", "Resumo ALFA-A-11").unwrap());
    let items = conversation::list_history(&conn, 50).unwrap();
    assert_eq!(
        items.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![recent, old]
    );
    assert_eq!(items[1].updated_at, "2026-01-01T00:00:00Z");
    assert_eq!(items[1].preview, "Resumo ALFA-A-11");
    assert_eq!(
        conversation::history_session(&conn, old)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
}

#[test]
fn three_runs_require_explicit_resume_and_keep_sqlite_integrity() {
    let (db, _) = fixture();
    let mut run1 = db.open().unwrap();
    let a = conversation::create_session(&run1).unwrap();
    conversation::append_exchange_to_session(&mut run1, a, "ALFA-A-11", "Resposta A").unwrap();
    assert!(conversation::close_session(&run1, a).unwrap());
    assert_eq!(
        conversation::history_session(&run1, a)
            .unwrap()
            .unwrap()
            .summary_status,
        "pending"
    );
    drop(run1);

    let mut run2 = db.open().unwrap();
    assert_eq!(
        conversation::close_orphaned_product_sessions(&run2).unwrap(),
        0
    );
    assert_eq!(conversation::reset_interrupted_summaries(&run2).unwrap(), 0);
    assert!(!conversation::is_active_session(&run2, a).unwrap());
    assert_eq!(
        conversation::history_session(&run2, a)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
    conversation::resume_session(&mut run2, a, None).unwrap();
    conversation::append_exchange_to_session(&mut run2, a, "Continuação A", "Resposta nova")
        .unwrap();
    assert!(conversation::close_session(&run2, a).unwrap());
    assert_eq!(
        conversation::history_session(&run2, a)
            .unwrap()
            .unwrap()
            .summary_status,
        "pending"
    );
    drop(run2);

    let run3 = db.open().unwrap();
    assert_eq!(
        conversation::close_orphaned_product_sessions(&run3).unwrap(),
        0
    );
    assert!(!conversation::is_active_session(&run3, a).unwrap());
    assert_eq!(
        conversation::history_session(&run3, a)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        4
    );
    let integrity: String = run3
        .pragma_query_value(None, "integrity_check", |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let foreign_keys: i64 = run3
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(foreign_keys, 0);
}
#[test]
fn summary_claim_bounds_database_read_and_keeps_first_user() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let id = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, id, "FIRST-USER-😀", "Resposta").unwrap();
    for n in 0..300 {
        conn.execute(
            "INSERT INTO conversation_messages(session_id,role,content) VALUES (?1,'user',?2)",
            rusqlite::params![id, format!("RECENT-{n}")],
        )
        .unwrap();
    }
    conversation::close_session(&conn, id).unwrap();
    let claimed = conversation::claim_next_pending_summary(&mut conn)
        .unwrap()
        .unwrap();
    assert!(claimed.truncated);
    assert_eq!(claimed.messages.len(), 257);
    assert_eq!(claimed.messages.first().unwrap().content, "FIRST-USER-😀");
    assert_eq!(claimed.messages.last().unwrap().content, "RECENT-299");
}
#[test]
fn identity_versions_and_memory_import() {
    let (db, path) = fixture();
    write(&path, &data("v1"));
    assert_eq!(import_bootstrap(&db, &path).unwrap().memories_inserted, 2);
    assert_eq!(import_bootstrap(&db, &path).unwrap().memories_inserted, 0);
    let conn = db.open().unwrap();
    assert_eq!(
        identity::current_identity(&conn)
            .unwrap()
            .unwrap()
            .input
            .version,
        "v1"
    );
    let all = memory::list_active_memories(&conn, 10).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].title, "A");
    assert_eq!(
        memory::list_memories_by_domain(&conn, "tests", 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        memory::active_memories(
            &conn,
            memory::MemoryFilter {
                kind: Some("preference"),
                min_importance: Some(8),
                limit: 10,
                ..Default::default()
            }
        )
        .unwrap()
        .len(),
        1
    );
    drop(conn);
    let mut next = data("v2");
    next["memories"] = json!([]);
    write(&path, &next);
    assert!(import_bootstrap(&db, &path).unwrap().identity_inserted);
    let conn = db.open().unwrap();
    let current = identity::current_identity(&conn).unwrap().unwrap();
    assert_eq!(current.input.version, "v2");
    assert!(current.supersedes_id.is_some());
    let counts: (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*),SUM(is_current) FROM identity_snapshots",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (2, 1));
}
#[test]
fn conversation_and_task_survive_reopen() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let id = conversation::create_diagnostic(&mut conn).unwrap();
    assert_eq!(conversation::create_diagnostic(&mut conn).unwrap(), id);
    task_history::insert(
        &conn,
        &task_history::TaskRecord {
            task_id: 7,
            kind: "mock".into(),
            state: "completed".into(),
            started_at: "2026-01-01T00:00:00Z".into(),
            finished_at: "2026-01-01T00:00:01Z".into(),
            summary: None,
            error_code: None,
        },
    )
    .unwrap();
    drop(conn);
    let conn = db.open().unwrap();
    assert_eq!(
        conversation::recent(&conn).unwrap().unwrap().messages.len(),
        2
    );
    assert_eq!(task_history::max_id(&conn).unwrap(), 7);
}
#[test]
fn invalid_bootstrap_has_no_partial_import() {
    let (db, path) = fixture();
    let mut bad = data("bad");
    bad["memories"][1]["importance"] = json!(99);
    write(&path, &bad);
    assert!(import_bootstrap(&db, &path).is_err());
    let conn = db.open().unwrap();
    assert!(identity::current_identity(&conn).unwrap().is_none());
    assert!(memory::list_active_memories(&conn, 10).unwrap().is_empty());
}
#[test]
fn transaction_rolls_back_on_write_error() {
    let (db, path) = fixture();
    let conn = db.open().unwrap();
    conn.execute_batch("CREATE TRIGGER reject_memory BEFORE INSERT ON memory_records BEGIN SELECT RAISE(ABORT, 'synthetic rejection'); END;").unwrap();
    drop(conn);
    write(&path, &data("rollback"));
    assert!(import_bootstrap(&db, &path).is_err());
    let conn = db.open().unwrap();
    assert!(identity::current_identity(&conn).unwrap().is_none());
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
#[test]
fn conflicting_import_keys_are_rejected() {
    let (db, path) = fixture();
    write(&path, &data("v1"));
    import_bootstrap(&db, &path).unwrap();
    let mut changed = data("v1");
    changed["identity"]["canonicalName"] = json!("Changed");
    write(&path, &changed);
    assert!(matches!(
        import_bootstrap(&db, &path),
        Err(database::PersistenceError::Conflict)
    ));
    let mut changed = data("v1");
    changed["memories"][0]["summary"] = json!("Changed summary");
    write(&path, &changed);
    assert!(matches!(
        import_bootstrap(&db, &path),
        Err(database::PersistenceError::Conflict)
    ));
    let conn = db.open().unwrap();
    assert_eq!(
        identity::current_identity(&conn)
            .unwrap()
            .unwrap()
            .input
            .canonical_name,
        "Synthetic"
    );
}

#[test]
fn gemini_exchange_rolls_back_if_final_answer_cannot_be_saved() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    conn.execute_batch("CREATE TRIGGER reject_gemini_answer BEFORE INSERT ON conversation_messages WHEN NEW.role='assistant' BEGIN SELECT RAISE(ABORT, 'synthetic rejection'); END;").unwrap();
    assert!(
        conversation::append_gemini_exchange(&mut conn, "pergunta neutra", "resposta final")
            .is_err()
    );
    assert!(conversation::gemini_session(&conn).unwrap().is_none());
    conn.execute_batch("DROP TRIGGER reject_gemini_answer")
        .unwrap();
    conversation::append_gemini_exchange(&mut conn, "pergunta neutra", "resposta final").unwrap();
    let session = conversation::gemini_session(&conn).unwrap().unwrap();
    assert_eq!(session.messages.len(), 2);
    task_history::insert(
        &conn,
        &task_history::TaskRecord {
            task_id: 17,
            kind: "gemini_chat".into(),
            state: "completed".into(),
            started_at: "2026-01-01T00:00:00Z".into(),
            finished_at: "2026-01-01T00:00:01Z".into(),
            summary: None,
            error_code: None,
        },
    )
    .unwrap();
    task_history::mark_failed(&conn, 17, "channel_closed").unwrap();
    let (state, code): (String, String) = conn
        .query_row(
            "SELECT state,error_code FROM task_records WHERE task_id=17",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (state.as_str(), code.as_str()),
        ("failed", "channel_closed")
    );
}

#[test]
fn explicit_sessions_are_isolated_and_survive_restart_without_selection() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let a = conversation::create_session(&conn).unwrap();
    let b = conversation::create_session(&conn).unwrap();
    assert_ne!(a, b);
    conversation::append_exchange_to_session(&mut conn, a, "A pergunta", "A resposta").unwrap();
    conversation::append_exchange_to_session(&mut conn, b, "B pergunta", "B resposta").unwrap();
    assert_eq!(
        conversation::session(&conn, a).unwrap().unwrap().messages[0].content,
        "A pergunta"
    );
    assert_eq!(
        conversation::session(&conn, b).unwrap().unwrap().messages[0].content,
        "B pergunta"
    );
    let serialized =
        serde_json::to_value(conversation::session(&conn, a).unwrap().unwrap()).unwrap();
    assert_eq!(serialized["messages"][0]["sessionId"], json!(a));
    assert!(conversation::session(&conn, 0).unwrap().is_none());
    assert!(!conversation::is_active_session(&conn, i64::MAX).unwrap());
    assert!(conversation::append_exchange_to_session(&mut conn, i64::MAX, "x", "y").is_err());
    drop(conn);
    let conn = db.open().unwrap();
    assert_eq!(
        conversation::session(&conn, a)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
    assert_eq!(
        conversation::session(&conn, b)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
    // The database retains both sessions. The runtime starts without choosing either ID.
}

#[test]
fn outbound_history_is_bounded_and_close_preserves_messages() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let a = conversation::create_session(&conn).unwrap();
    let b = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, b, "SEGREDO-DA-SESSAO-B", "Entendido")
        .unwrap();
    for i in 0..6 {
        conversation::append_exchange_to_session(
            &mut conn,
            a,
            &format!("user-{i}"),
            &format!("assistant-{i}"),
        )
        .unwrap();
    }
    let history = conversation::outbound_history(
        &conn,
        a,
        conversation::OUTBOUND_HISTORY_MESSAGES,
        conversation::OUTBOUND_HISTORY_BYTES,
    )
    .unwrap();
    assert_eq!(history.len(), conversation::OUTBOUND_HISTORY_MESSAGES);
    assert_eq!(history.first().unwrap().content, "user-2");
    assert_eq!(history.last().unwrap().content, "assistant-5");
    assert!(!history
        .iter()
        .any(|message| message.content.contains("SEGREDO-DA-SESSAO-B")));
    assert!(
        history
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>()
            <= conversation::OUTBOUND_HISTORY_BYTES
    );
    assert!(conversation::close_session(&conn, a).unwrap());
    assert!(!conversation::close_session(&conn, a).unwrap());
    assert!(!conversation::is_active_session(&conn, a).unwrap());
    assert_eq!(
        conversation::session(&conn, a)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        12
    );
    assert!(conversation::outbound_history(
        &conn,
        a,
        conversation::OUTBOUND_HISTORY_MESSAGES,
        conversation::OUTBOUND_HISTORY_BYTES
    )
    .is_err());
    assert!(
        conversation::append_exchange_to_session(&mut conn, a, "later", "not allowed").is_err()
    );
    let c = conversation::create_session(&conn).unwrap();
    for i in 0..4 {
        conversation::append_exchange_to_session(
            &mut conn,
            c,
            &format!("turn-{i}"),
            &"x".repeat(4096),
        )
        .unwrap();
    }
    let bytes_limited = conversation::outbound_history(
        &conn,
        c,
        conversation::OUTBOUND_HISTORY_MESSAGES,
        conversation::OUTBOUND_HISTORY_BYTES,
    )
    .unwrap();
    assert!(bytes_limited.len() < conversation::OUTBOUND_HISTORY_MESSAGES);
    assert_eq!(bytes_limited.last().unwrap().content, "x".repeat(4096));
    assert!(
        bytes_limited
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>()
            <= conversation::OUTBOUND_HISTORY_BYTES
    );
}

#[test]
fn explicit_exchange_is_atomic_on_assistant_failure() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let id = conversation::create_session(&conn).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_answer BEFORE INSERT ON conversation_messages WHEN NEW.role='assistant' BEGIN SELECT RAISE(ABORT, 'synthetic'); END;").unwrap();
    assert!(conversation::append_exchange_to_session(&mut conn, id, "user", "assistant").is_err());
    assert!(conversation::session(&conn, id)
        .unwrap()
        .unwrap()
        .messages
        .is_empty());
}

#[test]
fn resume_validates_target_and_invalidates_summary() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let legacy = conversation::create_diagnostic(&mut conn).unwrap();
    let empty = conversation::create_session(&conn).unwrap();
    let active = conversation::create_session(&conn).unwrap();
    let b = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, b, "ORQUIDEA-71", "Entendido.").unwrap();
    assert!(conversation::close_session(&conn, b).unwrap());
    conn.execute("UPDATE conversation_sessions SET summary_status='completed',summary='old',summary_updated_at='2026-01-01' WHERE id=?1",[b]).unwrap();
    for id in [0, legacy, empty, active] {
        assert!(conversation::resume_session(&mut conn, id, None).is_err());
    }
    assert_eq!(
        conversation::resume_session(&mut conn, b, Some(b)).err(),
        Some("session_invalid")
    );
    conn.execute(
        "UPDATE conversation_sessions SET summary_status='running' WHERE id=?1",
        [b],
    )
    .unwrap();
    assert_eq!(
        conversation::resume_session(&mut conn, b, None).err(),
        Some("summary_busy")
    );
    conn.execute(
        "UPDATE conversation_sessions SET summary_status='completed' WHERE id=?1",
        [b],
    )
    .unwrap();
    let resumed = conversation::resume_session(&mut conn, b, None).unwrap();
    assert_eq!(resumed.status.as_deref(), Some("active"));
    assert_eq!(resumed.messages[0].content, "ORQUIDEA-71");
    let summary:(String,Option<String>,Option<String>)=conn.query_row("SELECT summary_status,summary,summary_updated_at FROM conversation_sessions WHERE id=?1",[b],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(summary, ("none".into(), None, None));
    assert!(conversation::resume_session(&mut conn, b, None).is_err());
}

#[test]
fn resume_swap_is_atomic_and_preserves_both_sessions() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let a = conversation::create_session(&conn).unwrap();
    let b = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, a, "ATUAL-A-11", "A resposta").unwrap();
    conversation::append_exchange_to_session(&mut conn, b, "ORQUIDEA-71", "Entendido.").unwrap();
    conversation::close_session(&conn, b).unwrap();
    let resumed = conversation::resume_session(&mut conn, b, Some(a)).unwrap();
    assert_eq!(resumed.messages.len(), 2);
    let a_state: (String, String) = conn
        .query_row(
            "SELECT status,summary_status FROM conversation_sessions WHERE id=?1",
            [a],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(a_state, ("closed".into(), "pending".into()));
    assert_eq!(
        conversation::session(&conn, a).unwrap().unwrap().messages[0].content,
        "ATUAL-A-11"
    );
    assert_eq!(
        conversation::outbound_history(
            &conn,
            b,
            conversation::OUTBOUND_HISTORY_MESSAGES,
            conversation::OUTBOUND_HISTORY_BYTES
        )
        .unwrap()[0]
            .content,
        "ORQUIDEA-71"
    );
    assert!(conversation::outbound_history(
        &conn,
        a,
        conversation::OUTBOUND_HISTORY_MESSAGES,
        conversation::OUTBOUND_HISTORY_BYTES
    )
    .is_err());
    let empty = conversation::create_session(&conn).unwrap();
    conversation::close_session(&conn, b).unwrap();
    conversation::resume_session(&mut conn, b, Some(empty)).unwrap();
    let empty_summary: String = conn
        .query_row(
            "SELECT summary_status FROM conversation_sessions WHERE id=?1",
            [empty],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(empty_summary, "none");
}

#[test]
fn failed_resume_rolls_back_closure_of_current_session() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let a = conversation::create_session(&conn).unwrap();
    let b = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, a, "A", "response").unwrap();
    conversation::append_exchange_to_session(&mut conn, b, "B", "response").unwrap();
    conversation::close_session(&conn, b).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_resume BEFORE UPDATE ON conversation_sessions WHEN NEW.status='active' AND OLD.status='closed' BEGIN SELECT RAISE(ABORT, 'synthetic'); END;").unwrap();
    assert_eq!(
        conversation::resume_session(&mut conn, b, Some(a)).err(),
        Some("write_failed")
    );
    assert!(conversation::is_active_session(&conn, a).unwrap());
    assert!(!conversation::is_active_session(&conn, b).unwrap());
    assert_eq!(
        conversation::session(&conn, a)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
    assert_eq!(
        conversation::session(&conn, b)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
}

#[test]
fn restart_requires_explicit_resume_again() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let a = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, a, "ORQUIDEA-71", "Entendido.").unwrap();
    drop(conn);
    let mut conn = db.open().unwrap();
    assert_eq!(
        conversation::close_orphaned_product_sessions(&conn).unwrap(),
        1
    );
    assert!(!conversation::is_active_session(&conn, a).unwrap());
    assert_eq!(
        conversation::history_session(&conn, a)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
    conversation::resume_session(&mut conn, a, None).unwrap();
    conversation::append_exchange_to_session(&mut conn, a, "Qual foi a palavra?", "ORQUIDEA-71")
        .unwrap();
    drop(conn);
    let mut conn = db.open().unwrap();
    assert_eq!(
        conversation::close_orphaned_product_sessions(&conn).unwrap(),
        1
    );
    assert!(!conversation::is_active_session(&conn, a).unwrap());
    assert_eq!(
        conversation::session(&conn, a)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        4
    );
    conversation::resume_session(&mut conn, a, None).unwrap();
    assert!(conversation::is_active_session(&conn, a).unwrap());
}

#[test]
fn migration_004_preserves_v3_policy_and_seeds_advanced_defaults() {
    use super::gemini_settings;
    use super::general_settings;
    use crate::cognition::policy::{self, CognitiveRole};
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "{} {} {} PRAGMA user_version=3;",
        include_str!("../../migrations/001_initial_persistence.sql"),
        include_str!("../../migrations/002_conversation_history.sql"),
        include_str!("../../migrations/003_cognitive_role_policy.sql")
    ))
    .unwrap();
    conn.execute("UPDATE cognitive_role_policies SET model='gemini-custom',max_provider_calls=4,max_output_tokens=NULL WHERE role='conversation'", []).unwrap();
    migrations::apply(&conn).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 12);
    let conversation = policy::load(&conn, CognitiveRole::Conversation).unwrap();
    let summary = policy::load(&conn, CognitiveRole::Summary).unwrap();
    assert_eq!(conversation.targets[0].model, "gemini-custom");
    assert_eq!(conversation.max_provider_calls, 4);
    assert_eq!(conversation.max_output_tokens, None);
    assert_eq!(
        (
            conversation.retry_enabled,
            conversation.max_retries,
            conversation.retry_backoff_ms,
            conversation.history_max_messages,
            conversation.history_max_bytes
        ),
        (true, 1, 1500, 8, 12288)
    );
    assert_eq!(
        (
            summary.retry_enabled,
            summary.max_retries,
            summary.retry_backoff_ms,
            summary.summary_input_max_bytes
        ),
        (false, 0, 1500, 32768)
    );
    assert_eq!(
        general_settings::load(&conn).unwrap(),
        general_settings::GeneralSettings {
            always_on_top: false,
            active_fps: 30,
            background_fps: 24
        }
    );
    assert_eq!(
        gemini_settings::load(&conn).unwrap(),
        gemini_settings::GeminiTimeouts {
            request_timeout_ms: 45_000,
            stream_idle_timeout_ms: 15_000
        }
    );
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(cognitive_role_policies)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!columns
        .iter()
        .any(|column| column.contains("key") || column.contains("secret")));
    let schema: String = conn
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    assert!(!schema.contains("api_key") && !schema.contains("gemini_key"));
    assert_sqlite_integrity(&conn);
}

#[test]
fn gemini_timeouts_persist_independently() {
    use super::gemini_settings::{self, GeminiTimeouts};
    let (db, _) = fixture();
    let conn = db.open().unwrap();
    let changed = GeminiTimeouts {
        request_timeout_ms: 60_000,
        stream_idle_timeout_ms: 20_000,
    };
    gemini_settings::save(&conn, &changed).unwrap();
    assert!(gemini_settings::save(
        &conn,
        &GeminiTimeouts {
            request_timeout_ms: 0,
            ..changed
        }
    )
    .is_err());
    drop(conn);
    assert_eq!(gemini_settings::load(&db.open().unwrap()).unwrap(), changed);
}

#[test]
fn migration_005_repairs_existing_v4_without_changing_preferences() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "{} {} {} {} PRAGMA user_version=4;",
        include_str!("../../migrations/001_initial_persistence.sql"),
        include_str!("../../migrations/002_conversation_history.sql"),
        include_str!("../../migrations/003_cognitive_role_policy.sql"),
        include_str!("../../migrations/004_cognitive_retry_and_general_settings.sql")
    ))
    .unwrap();
    conn.execute(
        "UPDATE general_settings SET always_on_top=1,active_fps=45,background_fps=20",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE cognitive_role_policies SET model='gemini-custom' WHERE role='conversation'",
        [],
    )
    .unwrap();
    migrations::apply(&conn).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 12);
    assert_eq!(super::general_settings::load(&conn).unwrap().active_fps, 45);
    assert_eq!(
        crate::cognition::policy::load(
            &conn,
            crate::cognition::policy::CognitiveRole::Conversation
        )
        .unwrap()
        .targets[0]
            .model,
        "gemini-custom"
    );
    assert_eq!(
        super::gemini_settings::load(&conn)
            .unwrap()
            .request_timeout_ms,
        45_000
    );
    assert_sqlite_integrity(&conn);
}

#[test]
fn migration_006_preserves_fixed_behavior_and_seeds_groq_fallback_config() {
    use crate::cognition::policy::{self, CognitiveRole, RoutingMode};
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "{} {} {} {} {} PRAGMA user_version=5;",
        include_str!("../../migrations/001_initial_persistence.sql"),
        include_str!("../../migrations/002_conversation_history.sql"),
        include_str!("../../migrations/003_cognitive_role_policy.sql"),
        include_str!("../../migrations/004_cognitive_retry_and_general_settings.sql"),
        include_str!("../../migrations/005_gemini_provider_timeouts.sql")
    ))
    .unwrap();
    conn.execute("UPDATE cognitive_role_policies SET model='gemini-custom',max_provider_calls=4 WHERE role='conversation'", []).unwrap();
    migrations::apply(&conn).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 12);
    let conversation = policy::load(&conn, CognitiveRole::Conversation).unwrap();
    let summary = policy::load(&conn, CognitiveRole::Summary).unwrap();
    assert_eq!(conversation.targets[0].model, "gemini-custom");
    assert_eq!(conversation.routing_mode, RoutingMode::Fixed);
    assert_eq!(summary.routing_mode, RoutingMode::Fixed);
    assert_eq!(summary.targets.len(), 1);
    assert_eq!(conversation.targets.len(), 1);
    assert_sqlite_integrity(&conn);
}

#[test]
fn migration_007_preserves_v6_policy_and_gemini_timeout() {
    use super::provider_timeouts;
    use crate::cognition::policy::{self, CognitiveRole, RoutingMode, ThinkingLevel};
    use crate::cognition::types::ProviderTimeouts;
    let (db, bootstrap_path) = fixture();
    let conn = rusqlite::Connection::open(bootstrap_path.with_file_name("test.sqlite3")).unwrap();
    conn.execute_batch(&format!(
        "{} {} {} {} {} {} PRAGMA user_version=6;",
        include_str!("../../migrations/001_initial_persistence.sql"),
        include_str!("../../migrations/002_conversation_history.sql"),
        include_str!("../../migrations/003_cognitive_role_policy.sql"),
        include_str!("../../migrations/004_cognitive_retry_and_general_settings.sql"),
        include_str!("../../migrations/005_gemini_provider_timeouts.sql"),
        include_str!("../../migrations/006_cognitive_routing.sql")
    ))
    .unwrap();
    conn.execute(
        "UPDATE gemini_provider_settings SET request_timeout_ms=64000,stream_idle_timeout_ms=17000",
        [],
    )
    .unwrap();
    conn.execute("UPDATE cognitive_role_policies SET routing_mode='preferred',model='custom-gemini',thinking_level='high',max_provider_calls=4 WHERE role='conversation'", []).unwrap();
    migrations::apply(&conn).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
    let before = policy::load(&conn, CognitiveRole::Conversation).unwrap();
    let summary = policy::load(&conn, CognitiveRole::Summary).unwrap();
    assert_eq!(before.targets[0].model, "custom-gemini");
    assert_eq!(before.max_provider_calls, 4);
    assert_eq!(before.routing_mode, RoutingMode::Preferred);
    assert_eq!(before.targets[0].thinking_level, Some(ThinkingLevel::High));
    assert_eq!(
        provider_timeouts::load(&conn, "gemini").unwrap(),
        ProviderTimeouts {
            request_timeout_ms: 64000,
            stream_idle_timeout_ms: 17000
        }
    );
    assert_eq!(
        provider_timeouts::load(&conn, "groq").unwrap(),
        ProviderTimeouts {
            request_timeout_ms: 45000,
            stream_idle_timeout_ms: 15000
        }
    );
    provider_timeouts::save(
        &conn,
        "groq",
        ProviderTimeouts {
            request_timeout_ms: 33000,
            stream_idle_timeout_ms: 11000,
        },
    )
    .unwrap();
    assert_eq!(
        provider_timeouts::load(&conn, "gemini")
            .unwrap()
            .request_timeout_ms,
        64000
    );
    migrations::apply(&conn).unwrap();
    assert_eq!(
        provider_timeouts::load(&conn, "groq")
            .unwrap()
            .request_timeout_ms,
        33000
    );
    assert_sqlite_integrity(&conn);
    drop(conn);
    let reopened = db.open().unwrap();
    assert_eq!(
        policy::load(&reopened, CognitiveRole::Conversation).unwrap(),
        before
    );
    assert_eq!(
        policy::load(&reopened, CognitiveRole::Summary).unwrap(),
        summary
    );
    assert_eq!(
        provider_timeouts::load(&reopened, "gemini")
            .unwrap()
            .request_timeout_ms,
        64000
    );
    assert_eq!(
        provider_timeouts::load(&reopened, "groq")
            .unwrap()
            .request_timeout_ms,
        33000
    );
    assert_sqlite_integrity(&reopened);
}

#[test]
fn provider_timeouts_survive_reopen_independently() {
    use super::provider_timeouts;
    use crate::cognition::types::ProviderTimeouts;
    let (db, _) = fixture();
    let conn = db.open().unwrap();
    let gemini = ProviderTimeouts {
        request_timeout_ms: 61000,
        stream_idle_timeout_ms: 21000,
    };
    let groq = ProviderTimeouts {
        request_timeout_ms: 29000,
        stream_idle_timeout_ms: 9000,
    };
    provider_timeouts::save(&conn, "gemini", gemini).unwrap();
    provider_timeouts::save(&conn, "groq", groq).unwrap();
    assert!(provider_timeouts::save(&conn, "unknown", groq).is_err());
    drop(conn);
    let reopened = db.open().unwrap();
    assert_eq!(
        provider_timeouts::load(&reopened, "gemini").unwrap(),
        gemini
    );
    assert_eq!(provider_timeouts::load(&reopened, "groq").unwrap(), groq);
    assert_sqlite_integrity(&reopened);
}

#[test]
fn general_settings_persist_validate_and_stay_independent_of_policy() {
    use super::general_settings::{self, GeneralSettings};
    use crate::cognition::policy::{self, CognitiveRole};
    let (db, _) = fixture();
    let conn = db.open().unwrap();
    let original_policy = policy::load(&conn, CognitiveRole::Conversation).unwrap();
    let changed = GeneralSettings {
        always_on_top: true,
        active_fps: 45,
        background_fps: 20,
    };
    general_settings::save(&conn, &changed).unwrap();
    assert_eq!(
        policy::load(&conn, CognitiveRole::Conversation).unwrap(),
        original_policy
    );
    assert!(general_settings::save(
        &conn,
        &GeneralSettings {
            active_fps: 0,
            ..changed.clone()
        }
    )
    .is_err());
    assert!(general_settings::save(
        &conn,
        &GeneralSettings {
            background_fps: 61,
            ..changed.clone()
        }
    )
    .is_err());
    drop(conn);
    let mut conn = db.open().unwrap();
    assert_eq!(general_settings::load(&conn).unwrap(), changed);
    let mut policy = original_policy;
    policy.max_provider_calls = 3;
    policy::save(&mut conn, &policy).unwrap();
    assert_eq!(general_settings::load(&conn).unwrap(), changed);
}

#[test]
fn outbound_history_limits_are_explicit_utf8_safe_and_session_scoped() {
    let (db, _) = fixture();
    let mut conn = db.open().unwrap();
    let session = conversation::create_session(&conn).unwrap();
    let other = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, session, "olá 😀", "resposta A").unwrap();
    conversation::append_exchange_to_session(&mut conn, session, "segunda 😀", "resposta B")
        .unwrap();
    conversation::append_exchange_to_session(&mut conn, other, "SEGREDO OUTRA SESSÃO", "isolado")
        .unwrap();
    assert!(conversation::outbound_history(&conn, session, 0, 1000)
        .unwrap()
        .is_empty());
    assert!(conversation::outbound_history(&conn, session, 10, 0)
        .unwrap()
        .is_empty());
    let two = conversation::outbound_history(&conn, session, 2, 1000).unwrap();
    assert_eq!(two.len(), 2);
    let tiny = conversation::outbound_history(&conn, session, 10, "resposta B".len()).unwrap();
    assert_eq!(tiny.len(), 1);
    assert_eq!(tiny[0].content, "resposta B");
    let all = conversation::outbound_history(&conn, session, 10, 1000).unwrap();
    assert_eq!(all.len(), 4);
    assert!(all.iter().all(|turn| !turn.content.contains("SEGREDO")));
    assert!(all.iter().any(|turn| turn.content.contains('😀')));
}
