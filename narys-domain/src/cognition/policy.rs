use crate::persistence::database::PersistenceError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CognitiveRole {
    Conversation,
    Summary,
    Orchestrator,
    Worker,
}
impl CognitiveRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Summary => "summary",
            Self::Orchestrator => "orchestrator",
            Self::Worker => "worker",
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    Fixed,
    Preferred,
    Auto,
}
impl RoutingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Preferred => "preferred",
            Self::Auto => "auto",
        }
    }
}

/// Finite route size bounds persistence, preflight and scoring work.
pub const MAX_TARGETS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CognitiveTargetPolicy {
    pub provider_id: String,
    pub model: String,
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CognitiveRolePolicy {
    pub role: CognitiveRole,
    pub routing_mode: RoutingMode,
    pub targets: Vec<CognitiveTargetPolicy>,
    pub max_output_tokens: Option<u32>,
    pub max_provider_calls: u32,
    pub retry_enabled: bool,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
    pub history_max_messages: u32,
    pub history_max_bytes: u32,
    pub summary_input_max_bytes: u32,
    pub context_max_bytes: u32,
}
impl CognitiveRolePolicy {
    pub fn retry_policy(&self) -> super::types::RetryPolicy {
        super::types::RetryPolicy {
            enabled: self.retry_enabled,
            max_retries: self.max_retries,
            initial_backoff_ms: self.retry_backoff_ms,
        }
    }

    fn valid_model(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 128
            && value.trim() == value
            && !value.chars().any(char::is_control)
    }

    fn valid_provider_id(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 64
            && value.trim() == value
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
            })
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.targets.is_empty() || self.targets.len() > MAX_TARGETS {
            return Err("target_count_invalid");
        }
        let mut ids = std::collections::HashSet::new();
        for target in &self.targets {
            if !Self::valid_provider_id(&target.provider_id) {
                return Err("provider_id_invalid");
            }
            if !Self::valid_model(&target.model) {
                return Err("model_invalid");
            }
            if !ids.insert(&target.provider_id) {
                return Err("target_duplicate");
            }
        }
        match self.routing_mode {
            RoutingMode::Fixed if self.targets.len() != 1 => return Err("target_count_invalid"),
            RoutingMode::Preferred | RoutingMode::Auto if self.targets.len() < 2 => {
                return Err("target_count_invalid")
            }
            _ => {}
        }
        if self.max_provider_calls == 0 {
            return Err("provider_calls_invalid");
        }
        if self.routing_mode != RoutingMode::Fixed && self.max_provider_calls < 2 {
            return Err("fallback_budget_invalid");
        }
        if self.max_output_tokens == Some(0) {
            return Err("output_limit_invalid");
        }
        if self.context_max_bytes == 0 {
            return Err("context_limit_invalid");
        }
        if self.retry_backoff_ms > i64::MAX as u64 {
            return Err("retry_backoff_invalid");
        }
        Ok(())
    }

    pub fn selection(&self) -> super::types::ProviderSelection {
        use super::types::ProviderSelection;
        match self.routing_mode {
            RoutingMode::Fixed => ProviderSelection::Fixed(self.targets[0].provider_id.clone()),
            RoutingMode::Preferred => ProviderSelection::Preferred,
            RoutingMode::Auto => ProviderSelection::Auto,
        }
    }

    pub fn provider_targets(
        &self,
        timeouts: &std::collections::HashMap<String, super::types::ProviderTimeouts>,
    ) -> Result<Vec<super::types::ProviderTarget>, &'static str> {
        self.validate()?;
        self.targets
            .iter()
            .map(|target| {
                Ok(super::types::ProviderTarget {
                    provider_id: target.provider_id.clone(),
                    invocation: super::types::ProviderInvocationConfig {
                        model: target.model.clone(),
                        thinking_level: target.thinking_level,
                        timeouts: Some(
                            *timeouts
                                .get(&target.provider_id)
                                .ok_or("provider_config_invalid")?,
                        ),
                    },
                })
            })
            .collect()
    }

    pub fn load_timeouts(
        &self,
        conn: &Connection,
    ) -> Result<std::collections::HashMap<String, super::types::ProviderTimeouts>, PersistenceError>
    {
        self.targets
            .iter()
            .map(|target| {
                Ok((
                    target.provider_id.clone(),
                    crate::persistence::provider_timeouts::load(conn, &target.provider_id)?,
                ))
            })
            .collect()
    }
}

fn parse_thinking(value: Option<String>) -> Result<Option<ThinkingLevel>, PersistenceError> {
    match value.as_deref() {
        None => Ok(None),
        Some("low") => Ok(Some(ThinkingLevel::Low)),
        Some("medium") => Ok(Some(ThinkingLevel::Medium)),
        Some("high") => Ok(Some(ThinkingLevel::High)),
        _ => Err(PersistenceError::Read),
    }
}

/// Policy row and ordered targets must come from one SQLite read snapshot.
pub fn load(
    conn: &Connection,
    role: CognitiveRole,
) -> Result<CognitiveRolePolicy, PersistenceError> {
    if !conn.is_autocommit() {
        return load_snapshot(conn, role);
    }
    let tx = conn
        .unchecked_transaction()
        .map_err(|_| PersistenceError::Read)?;
    let policy = load_snapshot(&tx, role)?;
    tx.commit().map_err(|_| PersistenceError::Read)?;
    Ok(policy)
}

fn load_snapshot(
    conn: &Connection,
    role: CognitiveRole,
) -> Result<CognitiveRolePolicy, PersistenceError> {
    let mut policy = conn.query_row(
        "SELECT routing_mode,max_output_tokens,max_provider_calls,retry_enabled,max_retries,retry_backoff_ms,history_max_messages,history_max_bytes,summary_input_max_bytes,context_max_bytes FROM cognitive_role_policies WHERE role=?1",
        [role.as_str()], |row| {
            let mode: String = row.get(0)?;
            let routing_mode = match mode.as_str() {
                "fixed" => RoutingMode::Fixed, "preferred" => RoutingMode::Preferred, "auto" => RoutingMode::Auto,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(CognitiveRolePolicy { role, routing_mode, targets: vec![],
                max_output_tokens: row.get(1)?, max_provider_calls: row.get(2)?, retry_enabled: match row.get::<_, i64>(3)? { 0 => false, 1 => true, _ => return Err(rusqlite::Error::InvalidQuery) },
                max_retries: row.get(4)?, retry_backoff_ms: row.get(5)?, history_max_messages: row.get(6)?,
                history_max_bytes: row.get(7)?, summary_input_max_bytes: row.get(8)?, context_max_bytes: row.get(9)?,
            })
        }).map_err(|_| PersistenceError::Read)?;
    let mut stmt = conn.prepare("SELECT position,provider_id,model,thinking_level FROM cognitive_role_targets WHERE role=?1 ORDER BY position")
        .map_err(|_| PersistenceError::Read)?;
    let rows = stmt
        .query_map([role.as_str()], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|_| PersistenceError::Read)?;
    for row in rows {
        let (position, provider_id, model, thinking) = row.map_err(|_| PersistenceError::Read)?;
        if position != policy.targets.len() {
            return Err(PersistenceError::Read);
        }
        policy.targets.push(CognitiveTargetPolicy {
            provider_id,
            model,
            thinking_level: parse_thinking(thinking)?,
        });
    }
    policy.validate().map_err(|_| PersistenceError::Read)?;
    Ok(policy)
}

#[cfg(any(test, feature = "desktop-tests"))]
pub fn save(
    conn: &mut Connection,
    policy: &CognitiveRolePolicy,
) -> Result<CognitiveRolePolicy, PersistenceError> {
    policy.validate().map_err(|_| PersistenceError::Write)?;
    let tx = conn.transaction().map_err(|_| PersistenceError::Write)?;
    write_in_transaction(&tx, policy)?;
    tx.commit().map_err(|_| PersistenceError::Write)?;
    Ok(policy.clone())
}

pub fn write_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    policy: &CognitiveRolePolicy,
) -> Result<(), PersistenceError> {
    policy.validate().map_err(|_| PersistenceError::Write)?;
    let changed = tx.execute(
        "UPDATE cognitive_role_policies SET routing_mode=?2,max_output_tokens=?3,max_provider_calls=?4,retry_enabled=?5,max_retries=?6,retry_backoff_ms=?7,history_max_messages=?8,history_max_bytes=?9,summary_input_max_bytes=?10,context_max_bytes=?11,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE role=?1",
        params![policy.role.as_str(),policy.routing_mode.as_str(),policy.max_output_tokens,policy.max_provider_calls,
            policy.retry_enabled,policy.max_retries,policy.retry_backoff_ms,policy.history_max_messages,
            policy.history_max_bytes,policy.summary_input_max_bytes,policy.context_max_bytes],
    ).map_err(|_| PersistenceError::Write)?;
    if changed != 1 {
        return Err(PersistenceError::Write);
    }
    tx.execute(
        "DELETE FROM cognitive_role_targets WHERE role=?1",
        [policy.role.as_str()],
    )
    .map_err(|_| PersistenceError::Write)?;
    for (position, target) in policy.targets.iter().enumerate() {
        tx.execute("INSERT INTO cognitive_role_targets(role,position,provider_id,model,thinking_level) VALUES(?1,?2,?3,?4,?5)",
            params![policy.role.as_str(),position,target.provider_id,target.model,target.thinking_level.map(ThinkingLevel::as_str)])
            .map_err(|_| PersistenceError::Write)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::database::Database;

    fn fixture() -> (Database, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "d2-policy-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (Database::for_test(dir.join("policy.sqlite3")), dir)
    }
    fn target(id: &str) -> CognitiveTargetPolicy {
        CognitiveTargetPolicy {
            provider_id: id.into(),
            model: format!("{id}-model"),
            thinking_level: None,
        }
    }
    #[test]
    fn v8_to_v11_preserves_existing_roles_and_adds_worker_defaults() {
        let (db, dir) = fixture();
        let conn = Connection::open(dir.join("policy.sqlite3")).unwrap();
        conn.execute_batch(concat!(
            include_str!("../../migrations/001_initial_persistence.sql"),
            include_str!("../../migrations/002_conversation_history.sql"),
            include_str!("../../migrations/003_cognitive_role_policy.sql"),
            include_str!("../../migrations/004_cognitive_retry_and_general_settings.sql"),
            include_str!("../../migrations/005_gemini_provider_timeouts.sql"),
            include_str!("../../migrations/006_cognitive_routing.sql"),
            include_str!("../../migrations/007_provider_timeouts.sql"),
            include_str!("../../migrations/008_orchestrator_role.sql"),
            "PRAGMA user_version=8;"
        ))
        .unwrap();
        conn.execute_batch("UPDATE cognitive_role_policies SET updated_at='preserved',max_provider_calls=4,max_retries=2,retry_backoff_ms=77,retry_enabled=0,max_output_tokens=777,history_max_messages=11,history_max_bytes=3333,summary_input_max_bytes=4444,context_max_bytes=5555;
            UPDATE cognitive_role_policies SET routing_mode='preferred',provider_id='groq',model='custom-groq',thinking_level='high',fallback_provider_id='gemini',fallback_model='custom-gemini',fallback_thinking_level='medium' WHERE role='conversation';
            UPDATE cognitive_role_policies SET provider_id='groq',model='summary-own',thinking_level=NULL,fallback_provider_id='gemini',fallback_model='dormant' WHERE role='summary';
            UPDATE cognitive_role_policies SET model='planner-own',thinking_level='medium' WHERE role='orchestrator';").unwrap();
        drop(conn);
        let conn = db.open().unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            23
        );
        let conversation = load(&conn, CognitiveRole::Conversation).unwrap();
        assert_eq!(conversation.routing_mode, RoutingMode::Preferred);
        assert_eq!(
            conversation.targets,
            vec![
                CognitiveTargetPolicy {
                    provider_id: "groq".into(),
                    model: "custom-groq".into(),
                    thinking_level: Some(ThinkingLevel::High)
                },
                CognitiveTargetPolicy {
                    provider_id: "gemini".into(),
                    model: "custom-gemini".into(),
                    thinking_level: Some(ThinkingLevel::Medium)
                },
            ]
        );
        let summary = load(&conn, CognitiveRole::Summary).unwrap();
        assert_eq!(summary.routing_mode, RoutingMode::Fixed);
        assert_eq!(
            summary.targets,
            vec![CognitiveTargetPolicy {
                provider_id: "groq".into(),
                model: "summary-own".into(),
                thinking_level: None
            }]
        );
        let planner = load(&conn, CognitiveRole::Orchestrator).unwrap();
        assert_eq!(planner.targets.len(), 1);
        assert_eq!(planner.targets[0].model, "planner-own");
        assert_eq!(
            planner.targets[0].thinking_level,
            Some(ThinkingLevel::Medium)
        );
        for role in [
            CognitiveRole::Conversation,
            CognitiveRole::Summary,
            CognitiveRole::Orchestrator,
        ] {
            let p = load(&conn, role).unwrap();
            assert_eq!(
                (p.max_provider_calls, p.max_retries, p.retry_backoff_ms),
                (4, 2, 77)
            );
            assert_eq!(
                (
                    p.retry_enabled,
                    p.max_output_tokens,
                    p.history_max_messages,
                    p.history_max_bytes,
                    p.summary_input_max_bytes,
                    p.context_max_bytes
                ),
                (false, Some(777), 11, 3333, 4444, 5555)
            );
            assert_eq!(
                conn.query_row(
                    "SELECT updated_at FROM cognitive_role_policies WHERE role=?1",
                    [role.as_str()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "preserved"
            );
        }
        let worker = load(&conn, CognitiveRole::Worker).unwrap();
        assert_eq!(worker.routing_mode, RoutingMode::Preferred);
        assert_eq!(worker.max_provider_calls, 4);
        assert_eq!(worker.max_retries, 1);
        assert_eq!(worker.retry_backoff_ms, 750);
        assert_eq!(worker.max_output_tokens, Some(4096));
        assert_eq!(worker.context_max_bytes, 16384);
        assert_eq!(
            worker
                .targets
                .iter()
                .map(|target| target.provider_id.as_str())
                .collect::<Vec<_>>(),
            vec!["groq", "cloudflare"]
        );
        drop(conn);
        let reopened = db.open().unwrap();
        assert_eq!(
            load(&reopened, CognitiveRole::Conversation).unwrap(),
            conversation
        );
        assert_eq!(load(&reopened, CognitiveRole::Summary).unwrap(), summary);
        assert_eq!(
            load(&reopened, CognitiveRole::Orchestrator).unwrap(),
            planner
        );
        assert_eq!(load(&reopened, CognitiveRole::Worker).unwrap(), worker);
        assert_eq!(
            reopened
                .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert_eq!(
            reopened
                .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                    .get::<_, u32>(
                    0
                ))
                .unwrap(),
            0
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn all_roles_roundtrip_auto_order_and_independent_configuration() {
        let (db, dir) = fixture();
        let mut conn = db.open().unwrap();
        let roles = [
            CognitiveRole::Conversation,
            CognitiveRole::Orchestrator,
            CognitiveRole::Summary,
            CognitiveRole::Worker,
        ];
        let mut policies = vec![];
        for role in roles {
            let untouched: Vec<_> = roles
                .iter()
                .copied()
                .filter(|other| *other != role)
                .map(|other| (other, load(&conn, other).unwrap()))
                .collect();
            let mut policy = load(&conn, role).unwrap();
            policy.routing_mode = RoutingMode::Auto;
            policy.targets = vec![target("b"), target("a"), target("c")];
            policy.targets[1].thinking_level = Some(ThinkingLevel::High);
            policy.max_provider_calls = 2; // Chain may intentionally exceed call budget.
            policy.max_output_tokens = None;
            save(&mut conn, &policy).unwrap();
            assert_eq!(load(&conn, role).unwrap(), policy);
            for (other, previous) in untouched {
                assert_eq!(load(&conn, other).unwrap(), previous);
            }
            policies.push(policy);
        }
        drop(conn);
        let conn = db.open().unwrap();
        for p in policies {
            assert_eq!(load(&conn, p.role).unwrap(), p);
        }
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(cognitive_role_policies)")
            .unwrap()
            .query_map([], |r| r.get(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for old in [
            "provider_id",
            "model",
            "thinking_level",
            "fallback_provider_id",
        ] {
            assert!(!columns.contains(&old.to_string()));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn cardinality_duplicates_invalid_target_and_budget_fail_closed() {
        let (db, dir) = fixture();
        let mut conn = db.open().unwrap();
        let original = load(&conn, CognitiveRole::Conversation).unwrap();
        for mode in [
            RoutingMode::Fixed,
            RoutingMode::Preferred,
            RoutingMode::Auto,
        ] {
            for count in 0..=9 {
                let mut p = original.clone();
                p.routing_mode = mode;
                p.targets = (0..count).map(|n| target(&format!("p{n}"))).collect();
                assert_eq!(
                    p.validate().is_ok(),
                    if mode == RoutingMode::Fixed {
                        count == 1
                    } else {
                        (2..=8).contains(&count)
                    }
                );
            }
        }
        let mut p = original.clone();
        p.routing_mode = RoutingMode::Auto;
        p.targets = vec![target("a"), target("a")];
        assert_eq!(p.validate(), Err("target_duplicate"));
        p.targets[1] = target("b");
        p.max_provider_calls = 1;
        assert_eq!(p.validate(), Err("fallback_budget_invalid"));
        p.max_provider_calls = 2;
        for invalid in ["", "A", "a b", "x\n", &"x".repeat(65)] {
            p.targets[1].provider_id = invalid.into();
            assert!(save(&mut conn, &p).is_err());
        }
        p.targets[1] = target("b");
        for invalid in ["", " ", "bad\n", &"x".repeat(129)] {
            p.targets[1].model = invalid.into();
            assert!(save(&mut conn, &p).is_err());
        }
        assert!(serde_json::from_str::<CognitiveTargetPolicy>(
            r#"{"providerId":"a","model":"m","thinkingLevel":"extreme"}"#
        )
        .is_err());
        assert_eq!(load(&conn, CognitiveRole::Conversation).unwrap(), original);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn corrupt_persisted_thinking_order_and_cardinality_are_rejected() {
        let (db, dir) = fixture();
        let conn = db.open().unwrap();
        assert!(conn
            .execute(
                "INSERT INTO cognitive_role_targets VALUES('conversation',1,'gemini','m',NULL)",
                []
            )
            .is_err());
        assert!(conn
            .execute(
                "UPDATE cognitive_role_targets SET thinking_level='invalid'",
                []
            )
            .is_err());
        conn.execute(
            "UPDATE cognitive_role_targets SET position=1 WHERE role='conversation'",
            [],
        )
        .unwrap();
        assert!(load(&conn, CognitiveRole::Conversation).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
