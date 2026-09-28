use crate::persistence::database::PersistenceError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CognitiveRole {
    Conversation,
    Summary,
}
impl CognitiveRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Summary => "summary",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel {
    Low,
    Medium,
    High,
}
impl ThinkingLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CognitiveRolePolicy {
    pub role: CognitiveRole,
    pub provider_id: String,
    pub model: String,
    pub thinking_level: Option<ThinkingLevel>,
    pub max_output_tokens: Option<u32>,
    pub max_provider_calls: u32,
    pub retry_enabled: bool,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
    pub history_max_messages: u32,
    pub history_max_bytes: u32,
    pub summary_input_max_bytes: u32,
}
impl CognitiveRolePolicy {
    pub fn retry_policy(&self) -> super::types::RetryPolicy {
        super::types::RetryPolicy {
            enabled: self.retry_enabled,
            max_retries: self.max_retries,
            initial_backoff_ms: self.retry_backoff_ms,
        }
    }
    fn validate_integrity(&self) -> Result<(), &'static str> {
        if self.model.is_empty()
            || self.model.len() > 128
            || self.model.trim() != self.model
            || self.model.chars().any(|c| c.is_control())
        {
            return Err("model_invalid");
        }
        if self.max_output_tokens == Some(0) {
            return Err("output_limit_invalid");
        }
        if self.max_provider_calls == 0 {
            return Err("provider_calls_invalid");
        }
        if self.retry_backoff_ms > i64::MAX as u64 {
            return Err("retry_backoff_invalid");
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        self.validate_integrity()?;
        if self.provider_id != "gemini" {
            return Err("provider_unavailable");
        }
        Ok(())
    }
}
pub fn load(
    conn: &Connection,
    role: CognitiveRole,
) -> Result<CognitiveRolePolicy, PersistenceError> {
    let raw: (String, String, Option<String>, Option<i64>, i64, i64, i64, i64, i64, i64, i64) = conn.query_row(
        "SELECT provider_id,model,thinking_level,max_output_tokens,max_provider_calls,retry_enabled,max_retries,retry_backoff_ms,history_max_messages,history_max_bytes,summary_input_max_bytes FROM cognitive_role_policies WHERE role=?1",
        [role.as_str()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?,row.get(10)?))
    ).map_err(|_| PersistenceError::Read)?;
    let thinking_level = match raw.2.as_deref() {
        None => None,
        Some("low") => Some(ThinkingLevel::Low),
        Some("medium") => Some(ThinkingLevel::Medium),
        Some("high") => Some(ThinkingLevel::High),
        _ => return Err(PersistenceError::Read),
    };
    let max_output_tokens = raw
        .3
        .map(|v| u32::try_from(v).map_err(|_| PersistenceError::Read))
        .transpose()?;
    let max_provider_calls = u32::try_from(raw.4).map_err(|_| PersistenceError::Read)?;
    let policy = CognitiveRolePolicy {
        role,
        provider_id: raw.0,
        model: raw.1,
        thinking_level,
        max_output_tokens,
        max_provider_calls,
        retry_enabled: match raw.5 {
            0 => false,
            1 => true,
            _ => return Err(PersistenceError::Read),
        },
        max_retries: u32::try_from(raw.6).map_err(|_| PersistenceError::Read)?,
        retry_backoff_ms: u64::try_from(raw.7).map_err(|_| PersistenceError::Read)?,
        history_max_messages: u32::try_from(raw.8).map_err(|_| PersistenceError::Read)?,
        history_max_bytes: u32::try_from(raw.9).map_err(|_| PersistenceError::Read)?,
        summary_input_max_bytes: u32::try_from(raw.10).map_err(|_| PersistenceError::Read)?,
    };
    policy
        .validate_integrity()
        .map_err(|_| PersistenceError::Read)?;
    Ok(policy)
}
pub fn save(
    conn: &mut Connection,
    policy: &CognitiveRolePolicy,
) -> Result<CognitiveRolePolicy, PersistenceError> {
    policy.validate().map_err(|_| PersistenceError::Write)?;
    let tx = conn.transaction().map_err(|_| PersistenceError::Write)?;
    let changed = tx.execute("UPDATE cognitive_role_policies SET provider_id=?2,model=?3,thinking_level=?4,max_output_tokens=?5,max_provider_calls=?6,retry_enabled=?7,max_retries=?8,retry_backoff_ms=?9,history_max_messages=?10,history_max_bytes=?11,summary_input_max_bytes=?12,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE role=?1",
    params![policy.role.as_str(), policy.provider_id, policy.model, policy.thinking_level.map(ThinkingLevel::as_str), policy.max_output_tokens, policy.max_provider_calls, policy.retry_enabled, policy.max_retries, policy.retry_backoff_ms, policy.history_max_messages, policy.history_max_bytes, policy.summary_input_max_bytes]).map_err(|_| PersistenceError::Write)?;
    if changed != 1 {
        return Err(PersistenceError::Write);
    }
    tx.commit().map_err(|_| PersistenceError::Write)?;
    Ok(policy.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::database::Database;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[test]
    fn migration_seed_independent_updates_null_roundtrip_and_restart() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("uip6a-policy-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::for_test(dir.join("test.sqlite3"));
        let mut conn = db.open().unwrap();
        let mut conversation = load(&conn, CognitiveRole::Conversation).unwrap();
        let mut summary = load(&conn, CognitiveRole::Summary).unwrap();
        assert_eq!(
            (
                conversation.provider_id.as_str(),
                conversation.model.as_str(),
                conversation.thinking_level,
                conversation.max_output_tokens,
                conversation.max_provider_calls
            ),
            (
                "gemini",
                "gemini-3.8-flash",
                Some(ThinkingLevel::Low),
                Some(4096),
                2
            )
        );
        assert_eq!(
            (
                summary.thinking_level,
                summary.max_output_tokens,
                summary.max_provider_calls
            ),
            (Some(ThinkingLevel::Low), Some(1024), 1)
        );
        conversation.model = "gemini-new-model".into();
        conversation.thinking_level = None;
        conversation.max_output_tokens = None;
        conversation.max_provider_calls = 3;
        save(&mut conn, &conversation).unwrap();
        assert_eq!(load(&conn, CognitiveRole::Summary).unwrap(), summary);
        summary.max_output_tokens = Some(512);
        save(&mut conn, &summary).unwrap();
        assert_eq!(
            load(&conn, CognitiveRole::Conversation).unwrap(),
            conversation
        );
        drop(conn);
        let conn = db.open().unwrap();
        assert_eq!(
            load(&conn, CognitiveRole::Conversation).unwrap(),
            conversation
        );
        assert_eq!(load(&conn, CognitiveRole::Summary).unwrap(), summary);
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
    }
    #[test]
    fn invalid_values_are_rejected() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("uip6a-invalid-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::for_test(dir.join("test.sqlite3"));
        let mut conn = db.open().unwrap();
        let original = load(&conn, CognitiveRole::Conversation).unwrap();
        let mut bad = original.clone();
        bad.provider_id = "unknown".into();
        assert!(save(&mut conn, &bad).is_err());
        bad = original.clone();
        bad.model = "\u{0007}".into();
        assert!(save(&mut conn, &bad).is_err());
        bad = original.clone();
        bad.max_output_tokens = Some(0);
        assert!(save(&mut conn, &bad).is_err());
        bad = original.clone();
        bad.max_provider_calls = 0;
        assert!(save(&mut conn, &bad).is_err());
        assert_eq!(load(&conn, CognitiveRole::Conversation).unwrap(), original);
    }
}
