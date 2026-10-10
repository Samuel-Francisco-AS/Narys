use std::{collections::HashSet, sync::{Arc, Mutex}};
use crate::{luna::runtime::TaskRegistry, persistence::{database::Database, conversation::{self, ConversationSession}}};
#[derive(Clone, Default)]
pub struct CurrentRunSessions(pub Arc<Mutex<HashSet<i64>>>);
impl CurrentRunSessions {
    pub fn selected(&self) -> Result<Option<i64>, String> {
        let sessions = self.0.lock().map_err(|_| "session_registry_failed")?;
        if sessions.len() > 1 { return Err("ambiguous_product_session".into()); }
        Ok(sessions.iter().next().copied())
    }
}


pub fn resume_registered_session(
    db: &Database,
    sessions: &CurrentRunSessions,
    registry: &TaskRegistry,
    target_session_id: i64,
    current_session_id: Option<i64>,
) -> Result<ConversationSession, String> {
    if target_session_id <= 0
        || current_session_id.is_some_and(|id| id <= 0 || id == target_session_id)
    {
        return Err("session_invalid".into());
    }
    let mut current_run = sessions.0.lock().map_err(|_| "session_registry_failed")?;
    if current_run.len() > 1 { return Err("ambiguous_product_session".into()); }
    if current_session_id.is_none() && !current_run.is_empty() {
        return Err("session_invalid".into());
    }
    if current_session_id.is_some_and(|id| !current_run.contains(&id)) {
        return Err("session_invalid".into());
    }
    if current_session_id.is_some_and(|id| registry.has_foreground_provider_work_for_session(id)) {
        return Err("session_busy".into());
    }
    let mut conn = db.open().map_err(|e| e.code())?;
    let resumed = conversation::resume_session(&mut conn, target_session_id, current_session_id)
        .map_err(str::to_owned)?;
    if let Some(id) = current_session_id {
        current_run.remove(&id);
    }
    current_run.insert(target_session_id);
    Ok(resumed)
}

