use super::*;
use crate::{
    cognition::types::{ContextMetadata, TaskResult},
    persistence::database::Database,
};
fn result() -> TaskResult {
    TaskResult {
        text: "synthetic-answer".into(),
        provider_id: "groq".into(),
        usage: Default::default(),
        context_metadata: ContextMetadata {
            identity_version: "test".into(),
            memory_count: 0,
            recent_message_count: 0,
        },
    }
}
#[test]
fn admission_history_and_completion_are_atomic_and_restart_never_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().into());
    let mut conn = db.open().unwrap();
    let session = conversation::create_session(&conn).unwrap();
    conversation::append_exchange_to_session(&mut conn, session, "old-user", "old-answer").unwrap();
    admit(&mut conn, 900, session, "new-user").unwrap();
    assert!(admit(&mut conn, 901, session, "must-roll-back").is_err());
    assert_eq!(
        conn.query_row("SELECT count(*) FROM conversation_messages", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    let before: i64 = conn
        .query_row(
            "SELECT user_message_id FROM conversation_runs WHERE task_id=900",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let history = conversation::outbound_history_before(&conn, session, 8, 12288, before).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content, "old-user");
    finish(&mut conn, 900, "completed", None, Some(&result())).unwrap();
    let persisted = get(&conn, 900).unwrap();
    assert_eq!(persisted["result"]["text"], "synthetic-answer");
    assert_eq!(persisted["result"]["providerId"], "groq");
    drop(conn);
    let mut conn = db.open().unwrap();
    assert_eq!(recover(&mut conn).unwrap(), 0);
    assert_eq!(
        conversation::session(&conn, session)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        4
    );
    assert_eq!(task_history::max_id(&conn).unwrap(), 900);
    assert!(finish(&mut conn, 900, "completed", None, Some(&result())).is_err());
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM task_records WHERE task_id=900",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}
#[test]
fn uncertain_running_and_pending_recover_once_with_original_input_and_ids() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().into());
    let mut conn = db.open().unwrap();
    for (id, state) in [(501, "pending"), (502, "running")] {
        let session = conversation::create_session(&conn).unwrap();
        admit(&mut conn, id, session, "keep-user").unwrap();
        conn.execute(
            "UPDATE conversation_runs SET state=?2 WHERE task_id=?1",
            params![id, state],
        )
        .unwrap();
    }
    assert_eq!(task_history::max_id(&conn).unwrap(), 502);
    drop(conn);
    let mut conn = db.open().unwrap();
    assert_eq!(recover(&mut conn).unwrap(), 2);
    assert_eq!(recover(&mut conn).unwrap(), 0);
    for id in [501, 502] {
        let task = get(&conn, id).unwrap();
        assert_eq!(task["state"], "interrupted");
        assert_eq!(task["error_code"], "restart_never_retries");
        assert!(task["result"].is_null());
    }
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM conversation_messages WHERE role='assistant'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM server_events WHERE code='restart_never_retries'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}
#[test]
fn failed_completion_rolls_back_assistant_history_and_state_together() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().into());
    let mut conn = db.open().unwrap();
    let session = conversation::create_session(&conn).unwrap();
    admit(&mut conn, 77, session, "keep-input").unwrap();
    conn.execute_batch("CREATE TRIGGER fail_history BEFORE INSERT ON task_records BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(finish(&mut conn, 77, "completed", None, Some(&result())).is_err());
    assert_eq!(get(&conn, 77).unwrap()["state"], "pending");
    assert_eq!(
        conversation::session(&conn, session)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        1
    );
    conn.execute_batch("DROP TRIGGER fail_history;").unwrap();
    finish(&mut conn, 77, "failed", Some("write_failed"), None).unwrap();
    assert_eq!(get(&conn, 77).unwrap()["error_code"], "write_failed");
}
#[test]
fn event_provenance_is_durable_without_raw_chunks_or_errors_and_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().into());
    let mut conn = db.open().unwrap();
    let session = conversation::create_session(&conn).unwrap();
    admit(&mut conn, 5, session, "private-user").unwrap();
    use crate::luna::task::{TaskId, TaskState};
    observe(
        &mut conn,
        &TaskEvent {
            task_id: TaskId(5),
            sequence: 1,
            state: TaskState::Running,
            kind: TaskEventKind::ProviderSelected {
                provider_id: "groq".into(),
                model: "openai/gpt-oss-20b".into(),
                attempt: 1,
                routing_reason: "fixed".into(),
                score: None,
            },
        },
    )
    .unwrap();
    observe(
        &mut conn,
        &TaskEvent {
            task_id: TaskId(5),
            sequence: 2,
            state: TaskState::Running,
            kind: TaskEventKind::ProviderChunk {
                provider_id: "groq".into(),
                chunk: "SECRET-CHUNK".into(),
            },
        },
    )
    .unwrap();
    observe(
        &mut conn,
        &TaskEvent {
            task_id: TaskId(5),
            sequence: 3,
            state: TaskState::Failed,
            kind: TaskEventKind::TaskFailed {
                detail: "UNSAFE-REMOTE-ERROR".into(),
            },
        },
    )
    .unwrap();
    let data: String = conn
        .query_row(
            "SELECT details_json FROM server_events WHERE code='provider_selected'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(data.contains("openai/gpt-oss-20b"));
    let events: Vec<String> = conn
        .prepare("SELECT coalesce(details_json,'') FROM server_events")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!format!("{events:?}").contains("SECRET"));
    assert!(!format!("{events:?}").contains("UNSAFE"));
    for _ in 0..4100 {
        event(&conn, Some(5), "test", None).unwrap();
    }
    assert_eq!(
        conn.query_row("SELECT count(*) FROM server_events", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        4096
    );
}
