//! IPC adapter for the existing product Conversation, not another engine.
use crate::{ipc::Command, runtime::RuntimeServices};
use narys_domain::{
    cognition::{
        catalog,
        policy::{self, CognitiveRole},
    },
    luna::runtime::start_durable_conversation,
    persistence::{conversation, conversation_runs},
};
use serde_json::{json, Value};
impl RuntimeServices {
    pub async fn conversation_command(&self, command: Command) -> Result<Value, &'static str> {
        // Serialize session selection/admission/config mutations; workers own their snapshots.
        let _admission = self.conversation_admission.lock().await;
        let mut conn = self.database.open().map_err(|e| e.code())?;
        match command {
            Command::Conversation { session_id, text } => {
                if !conversation::is_active_session(&conn, session_id).map_err(|e| e.code())? {
                    return Err("session_invalid");
                }
                if self.tasks.has_foreground_provider_work() {
                    return Err("conversation_busy");
                }
                {
                    let mut selected = self
                        .sessions
                        .0
                        .lock()
                        .map_err(|_| "session_registry_failed")?;
                    selected.clear();
                    selected.insert(session_id);
                }
                drop(conn);
                let (tasks, db, providers, secrets, sessions) = (
                    self.tasks.clone(),
                    self.database.clone(),
                    self.providers.clone(),
                    self.secrets.clone(),
                    self.sessions.clone(),
                );
                let id = tokio::task::spawn_blocking(move || {
                    start_durable_conversation(
                        tasks, db, providers, secrets, sessions, session_id, text,
                    )
                })
                .await
                .map_err(|_| "conversation_worker_failed")?
                .map_err(|e| safe_error(&e))?;
                Ok(
                    json!({"task_id":id.0,"namespace":"product","session_id":session_id,"state":"pending","durable":true,"disconnect_cancels":false,"restart_replay":false}),
                )
            }
            Command::SessionCreate {} => {
                if self.tasks.has_foreground_provider_work() {
                    return Err("session_busy");
                }
                let tx = conn.transaction().map_err(|_| "write_failed")?;
                let id = conversation::create_session(&tx).map_err(|e| e.code())?;
                conversation_runs::event(
                    &tx,
                    None,
                    "session_created",
                    Some(json!({"session_id":id})),
                )?;
                tx.commit().map_err(|_| "write_failed")?;
                let mut selected = self
                    .sessions
                    .0
                    .lock()
                    .map_err(|_| "session_registry_failed")?;
                selected.clear();
                selected.insert(id);
                Ok(json!({"session_id":id,"status":"active"}))
            }
            Command::Sessions { after, limit } => {
                // Include diagnostic/legacy sessions and empty sessions: historical migration is observable.
                let mut query=conn.prepare("SELECT id,kind,status,title,created_at,updated_at,(SELECT count(*) FROM conversation_messages m WHERE m.session_id=s.id) FROM conversation_sessions s WHERE id>?1 ORDER BY id LIMIT ?2").map_err(|_|"read_failed")?;
                let mut rows=query.query_map(rusqlite::params![after,limit as u64+1],|r|Ok(json!({"session_id":r.get::<_,i64>(0)?,"kind":r.get::<_,String>(1)?,"status":r.get::<_,Option<String>>(2)?,"title":r.get::<_,Option<String>>(3)?,"created_at":r.get::<_,String>(4)?,"updated_at":r.get::<_,String>(5)?,"message_count":r.get::<_,i64>(6)?}))).map_err(|_|"read_failed")?.collect::<Result<Vec<_>,_>>().map_err(|_|"read_failed")?;
                let has_more = rows.len() > limit as usize;
                rows.truncate(limit as usize);
                let next = rows
                    .last()
                    .and_then(|r| r["session_id"].as_i64())
                    .unwrap_or(after);
                Ok(json!({"sessions":rows,"next_session":next,"has_more":has_more}))
            }
            Command::SessionGet {
                session_id,
                after_message,
                limit,
            } => {
                let meta=conn.query_row("SELECT kind,status,title,created_at,updated_at,summary_status,summary FROM conversation_sessions WHERE id=?1",[session_id],|r|Ok(json!({"session_id":session_id,"kind":r.get::<_,String>(0)?,"status":r.get::<_,Option<String>>(1)?,"title":r.get::<_,Option<String>>(2)?,"created_at":r.get::<_,String>(3)?,"updated_at":r.get::<_,String>(4)?,"summary_status":r.get::<_,String>(5)?,"summary":r.get::<_,Option<String>>(6)?}))).map_err(|_|"session_not_found")?;
                let mut query=conn.prepare("SELECT id,role,content,created_at FROM conversation_messages WHERE session_id=?1 AND id>?2 ORDER BY id LIMIT ?3").map_err(|_|"read_failed")?;
                let rows=query.query_map(rusqlite::params![session_id,after_message,limit as u64+1],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"role":r.get::<_,String>(1)?,"content":r.get::<_,String>(2)?,"created_at":r.get::<_,String>(3)?}))).map_err(|_|"read_failed")?;
                let mut messages = vec![];
                let mut bytes = serde_json::to_vec(&meta).map_err(|_| "read_failed")?.len();
                let mut has_more = false;
                for row in rows {
                    let row = row.map_err(|_| "read_failed")?;
                    bytes += serde_json::to_vec(&row).map_err(|_| "read_failed")?.len();
                    if messages.len() >= limit as usize || bytes > 192 * 1024 {
                        has_more = true;
                        break;
                    }
                    messages.push(row);
                }
                if messages.is_empty() && has_more {
                    return Err("response_limit");
                }
                let next = messages
                    .last()
                    .and_then(|r| r["id"].as_i64())
                    .unwrap_or(after_message);
                Ok(
                    json!({"session":meta,"messages":messages,"next_message":next,"has_more":has_more}),
                )
            }
            Command::SessionResume { session_id } => {
                if self.tasks.has_foreground_provider_work() {
                    return Err("session_busy");
                }
                // No implicit closure of another session in a server shared by clients.
                if conversation::is_active_session(&conn, session_id).map_err(|e| e.code())? {
                    let mut selected = self
                        .sessions
                        .0
                        .lock()
                        .map_err(|_| "session_registry_failed")?;
                    selected.clear();
                    selected.insert(session_id);
                } else {
                    let mut selected = self
                        .sessions
                        .0
                        .lock()
                        .map_err(|_| "session_registry_failed")?;
                    conversation::resume_session(&mut conn, session_id, None)?;
                    selected.clear();
                    selected.insert(session_id);
                }
                conversation_runs::event(
                    &conn,
                    None,
                    "session_resumed",
                    Some(json!({"session_id":session_id})),
                )?;
                Ok(json!({"session_id":session_id,"status":"active"}))
            }
            Command::SessionClose { session_id } => {
                if self
                    .tasks
                    .has_foreground_provider_work_for_session(session_id)
                {
                    return Err("session_busy");
                }
                let tx = conn.transaction().map_err(|_| "write_failed")?;
                let closed = conversation::close_session(&tx, session_id).map_err(|e| e.code())?;
                conversation_runs::event(
                    &tx,
                    None,
                    "session_closed",
                    Some(json!({"session_id":session_id,"changed":closed})),
                )?;
                tx.commit().map_err(|_| "write_failed")?;
                self.sessions
                    .0
                    .lock()
                    .map_err(|_| "session_registry_failed")?
                    .remove(&session_id);
                Ok(
                    json!({"session_id":session_id,"closed":closed,"automatic_summary_execution":false}),
                )
            }
            Command::Providers {} => {
                let status = self.providers.scheduler.status();
                let store = self.secrets.clone();
                let infos = tokio::task::spawn_blocking(move || catalog::infos(&status, &store))
                    .await
                    .map_err(|_| "credential_status_worker_failed")?;
                let permissions=conn.prepare("SELECT provider_id,enabled,free_tier_confirmed FROM server_provider_permissions").map_err(|_|"read_failed")?.query_map([],|r|Ok(json!({"provider_id":r.get::<_,String>(0)?,"enabled":r.get::<_,bool>(1)?,"free_tier_confirmed":r.get::<_,bool>(2)?}))).map_err(|_|"read_failed")?.collect::<Result<Vec<_>,_>>().map_err(|_|"read_failed")?;
                let mut provider_infos = serde_json::to_value(&infos.providers)
                    .map_err(|_| "provider_status_encode_failed")?;
                if let Some(items) = provider_infos.as_array_mut() {
                    for item in items {
                        let permission =
                            permissions.iter().find(|p| p["provider_id"] == item["id"]);
                        let enabled = permission.is_some_and(|p| p["enabled"] == true);
                        item["registered"] = json!(true);
                        item["enabled"] = json!(enabled);
                        item["local_state"] = json!(if !infos.credential_store_available {
                            "credential_store_unavailable"
                        } else if item["configured"] != true {
                            "not_configured"
                        } else if !enabled {
                            "free_account_confirmation_required"
                        } else {
                            "ready_local_quota_unverified"
                        });
                    }
                }
                let snapshot = crate::cognition::allocation_policy::load_role_runtime_policy(
                    &conn,
                    CognitiveRole::Conversation,
                )
                .map_err(|e| e.code())?;
                Ok(
                    json!({"providers":provider_infos,"credential_store_available":infos.credential_store_available,"credential_store_error_code":infos.credential_store_error_code,"permissions":permissions,"conversation_policy":snapshot.routing,"allocation_policy":snapshot.allocation.as_ref().map(|p|json!({"policy":p.policy(),"minimum_cognitive_tier":p.minimum_cognitive_tier()})),"admission":self.providers.scheduler.admission_snapshot(),"rate":self.providers.scheduler.rate_snapshot(),"resilience":self.providers.scheduler.resilience_snapshot(),"telemetry":self.providers.scheduler.telemetry_snapshot(),"automatic_summary_execution":false}),
                )
            }
            Command::ProviderConfigure {
                provider_id,
                enabled,
                free_tier_confirmed,
            } => {
                if self.tasks.has_foreground_provider_work() {
                    return Err("runtime_busy");
                }
                if enabled && !free_tier_confirmed {
                    return Err("free_provider_authorization_required");
                }
                if catalog::integration(&provider_id).is_none() {
                    return Err("provider_unavailable");
                }
                if enabled {
                    let (store, id) = (self.secrets.clone(), provider_id.clone());
                    let configured = tokio::task::spawn_blocking(move || {
                        catalog::configured_many(&store, &[&id])
                    })
                    .await
                    .map_err(|_| "credential_status_worker_failed")?
                    .map_err(|e| e.code())?;
                    if configured.get(&provider_id) != Some(&true) {
                        return Err("provider_not_configured");
                    }
                }
                let tx = conn.transaction().map_err(|_| "write_failed")?;
                tx.execute("UPDATE server_provider_permissions SET enabled=?2,free_tier_confirmed=?3,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE provider_id=?1",rusqlite::params![provider_id,enabled,free_tier_confirmed]).map_err(|_|"write_failed")?;
                conversation_runs::event(
                    &tx,
                    None,
                    "provider_permission_changed",
                    Some(
                        json!({"provider_id":provider_id,"enabled":enabled,"free_tier_confirmed":free_tier_confirmed}),
                    ),
                )?;
                tx.commit().map_err(|_| "write_failed")?;
                Ok(
                    json!({"provider_id":provider_id,"enabled":enabled,"free_tier_confirmed":free_tier_confirmed,"account_verified_by":"operator","overage_enabled_by_core":false}),
                )
            }
            Command::ConversationPolicy { policy: new_policy } => {
                if self.tasks.has_foreground_provider_work() {
                    return Err("runtime_busy");
                }
                catalog::validate_policy_registered(
                    &new_policy,
                    &self.providers.scheduler.status(),
                )?;
                let tx = conn.transaction().map_err(|_| "write_failed")?;
                policy::write_in_transaction(&tx, &new_policy).map_err(|e| e.code())?;
                conversation_runs::event(&tx, None, "conversation_policy_changed", None)?;
                tx.commit().map_err(|_| "write_failed")?;
                Ok(json!({"policy":new_policy}))
            }
            _ => Err("operation_not_allowed"),
        }
    }
}
fn safe_error(code: &str) -> &'static str {
    match code {
        "session_invalid" => "session_invalid",
        "conversation_busy" => "conversation_busy",
        "conversation_input_invalid" => "conversation_input_invalid",
        "runtime_shutting_down" => "server_stopping",
        "database_unavailable" => "database_unavailable",
        _ => "conversation_admission_failed",
    }
}
