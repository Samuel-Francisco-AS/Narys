use super::database::PersistenceError;
use rusqlite::{params, Connection};

#[derive(Debug)]
pub struct TaskRecord {
    pub task_id: u64,
    pub kind: String,
    pub state: String,
    pub started_at: String,
    pub finished_at: String,
    pub summary: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SubtaskRecord {
    pub root_task_id: u64,
    pub subtask_id: String,
    pub provider_id: Option<String>,
    pub state: String,
    pub started_at: Option<String>,
    pub finished_at: String,
    pub error_code: Option<String>,
}

fn valid_task_state(state: &str) -> bool {
    ["completed", "cancelled", "failed"].contains(&state)
}

fn valid_subtask_state(state: &str) -> bool {
    ["completed", "cancelled", "failed", "blocked"].contains(&state)
}

pub fn insert(conn: &Connection, record: &TaskRecord) -> Result<(), PersistenceError> {
    if !valid_task_state(&record.state) {
        return Err(PersistenceError::Write);
    }
    conn.execute(
        "INSERT INTO task_records(task_id,kind,state,started_at,finished_at,summary,error_code) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![record.task_id,record.kind,record.state,record.started_at,record.finished_at,record.summary,record.error_code],
    ).map_err(|_| PersistenceError::Write)?;
    Ok(())
}

pub fn insert_with_subtasks(
    conn: &mut Connection,
    record: &TaskRecord,
    subtasks: &[SubtaskRecord],
) -> Result<(), PersistenceError> {
    if !valid_task_state(&record.state)
        || subtasks.iter().any(|item| {
            item.root_task_id != record.task_id
                || item.subtask_id.trim().is_empty()
                || item.subtask_id.len() > 64
                || !valid_subtask_state(&item.state)
        })
    {
        return Err(PersistenceError::Write);
    }
    let tx = conn.transaction().map_err(|_| PersistenceError::Write)?;
    tx.execute(
        "INSERT INTO task_records(task_id,kind,state,started_at,finished_at,summary,error_code) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![record.task_id,record.kind,record.state,record.started_at,record.finished_at,record.summary,record.error_code],
    ).map_err(|_| PersistenceError::Write)?;
    for item in subtasks {
        tx.execute(
            "INSERT INTO task_subtask_records(root_task_id,subtask_id,provider_id,state,started_at,finished_at,error_code) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![item.root_task_id,item.subtask_id,item.provider_id,item.state,item.started_at,item.finished_at,item.error_code],
        ).map_err(|_| PersistenceError::Write)?;
    }
    tx.commit().map_err(|_| PersistenceError::Write)?;
    Ok(())
}

/// Canonical root high-water mark, including units committed before terminal
/// history. Invalid persisted identities prevent startup rather than reuse.
pub fn max_id(conn: &Connection) -> Result<u64, PersistenceError> {
    let maximum: u64 = conn.query_row(
        "SELECT MAX(id) FROM (SELECT COALESCE(MAX(task_id),0) AS id FROM main.task_records UNION ALL SELECT COALESCE(MAX(root_task_id),0) FROM main.cognitive_checkpoints)",
        [], |r| r.get(0),
    ).map_err(|_| PersistenceError::Read)?;
    if maximum > crate::cognitive_resources::MAX_HANDOFF_SEQUENCE {
        return Err(PersistenceError::Read);
    }
    Ok(maximum)
}

pub fn mark_failed(
    conn: &Connection,
    task_id: u64,
    error_code: &str,
) -> Result<(), PersistenceError> {
    let updated = conn
        .execute(
            "UPDATE task_records SET state='failed',error_code=?2 WHERE task_id=?1",
            params![task_id, error_code],
        )
        .map_err(|_| PersistenceError::Write)?;
    if updated == 1 {
        Ok(())
    } else {
        Err(PersistenceError::Write)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::database::Database;

    #[test]
    fn root_and_subtask_provenance_commit_atomically() {
        let dir = std::env::temp_dir().join(format!(
            "d3-task-history-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::for_test(dir.join("luna.sqlite3"));
        let mut conn = db.open().unwrap();
        let root = TaskRecord {
            task_id: 42,
            kind: "task_graph".into(),
            state: "completed".into(),
            started_at: "2026-10-01T00:00:00Z".into(),
            finished_at: "2026-10-01T00:00:01Z".into(),
            summary: Some("synthetic".into()),
            error_code: None,
        };
        let subtasks = vec![
            SubtaskRecord {
                root_task_id: 42,
                subtask_id: "a".into(),
                provider_id: Some("groq".into()),
                state: "completed".into(),
                started_at: Some("2026-10-01T00:00:00Z".into()),
                finished_at: "2026-10-01T00:00:01Z".into(),
                error_code: None,
            },
            SubtaskRecord {
                root_task_id: 42,
                subtask_id: "b".into(),
                provider_id: Some("cloudflare".into()),
                state: "completed".into(),
                started_at: Some("2026-10-01T00:00:00Z".into()),
                finished_at: "2026-10-01T00:00:01Z".into(),
                error_code: None,
            },
        ];
        insert_with_subtasks(&mut conn, &root, &subtasks).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=42",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        let providers: Vec<String> = conn
            .prepare(
                "SELECT provider_id FROM task_subtask_records WHERE root_task_id=42 ORDER BY subtask_id",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(providers, vec!["groq", "cloudflare"]);

        let invalid_root = TaskRecord {
            task_id: 43,
            ..root
        };
        let invalid = vec![SubtaskRecord {
            root_task_id: 999,
            subtask_id: "wrong-root".into(),
            provider_id: None,
            state: "blocked".into(),
            started_at: None,
            finished_at: "2026-10-01T00:00:01Z".into(),
            error_code: Some("synthetic".into()),
        }];
        assert!(insert_with_subtasks(&mut conn, &invalid_root, &invalid).is_err());
        let missing: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_records WHERE task_id=43",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(missing, 0);
        drop(conn);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
