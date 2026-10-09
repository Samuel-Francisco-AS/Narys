//! FIX-2: empty-session observations. No inference, retries or resume-to-create fallback.
use crate::{bounded, error_code, resume_config, session_config, shutdown, DEADLINE};
use github_copilot_sdk::{session::Session, Client, ClientOptions, SessionId};
use serde_json::{json, Value};
use std::path::{Component, Path};

/// Rebuild options for each owned CLI; ClientOptions in 1.0.17 is not Clone.
/// Minimal offline namespace; existing host authentication fails closed.
pub fn guarded_options(
    cli: &Path,
    workspace: &Path,
    state: &Path,
    sessions: &Path,
    existing_auth: bool,
) -> Result<ClientOptions, &'static str> {
    let owned = state.canonicalize().map_err(|_| "invalid_state_root")?;
    let storage = sessions.canonicalize().map_err(|_| "invalid_state_root")?;
    if !storage.starts_with(&owned)
        || sessions
            .symlink_metadata()
            .map_err(|_| "invalid_state_root")?
            .file_type()
            .is_symlink()
    {
        return Err("state_outside_owned_root");
    }
    if existing_auth {
        return Err("BLOCKED_AUTH_BOUNDARY");
    }
    crate::boundary::isolated_options(cli, workspace, state, sessions)
}

/// Inspect only a known invocation-owned session directory, without file contents.
/// Symlinks and path traversal fail closed. Never inspect the returned CLI path.
pub fn artifacts(root: &Path, id: &SessionId) -> Value {
    match root.symlink_metadata() {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        _ => return json!({"state":"unsafe_or_unavailable_storage_root"}),
    }
    let path = Path::new(id.as_ref());
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return json!({"state":"unsafe_session_id"});
    }
    let directory = root.join(path);
    match directory.symlink_metadata() {
        Ok(m) if !m.is_dir() || m.file_type().is_symlink() => {
            return json!({"state":"unsafe_storage_path"});
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return json!({"state":"absent","events_exists":false});
        }
        Err(_) => return json!({"state":"storage_unavailable"}),
        _ => {}
    }
    let mut result = json!({"state":"directory_present"});
    for (name, key) in [("events.jsonl", "events"), ("workspace.yaml", "workspace")] {
        match directory.join(name).symlink_metadata() {
            Ok(m) if m.is_file() && !m.file_type().is_symlink() => {
                result[format!("{key}_exists")] = json!(true);
                result[format!("{key}_bytes")] = json!(m.len());
            }
            Ok(_) => return json!({"state":"unsafe_storage_path"}),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                result[format!("{key}_exists")] = json!(false);
            }
            Err(_) => return json!({"state":"storage_unavailable"}),
        }
    }
    result
}

async fn metadata(client: &Client, id: &SessionId) -> Value {
    match bounded(client.get_session_metadata(id)).await {
        Ok(Some(m)) => json!({"state":"present","same_id":m.session_id == *id}),
        Ok(None) => json!({"state":"absent"}),
        Err(code) => json!({"state":"unavailable","code":code}),
    }
}

async fn create(
    client: &Client,
    workspace: &Path,
    id: Option<SessionId>,
    store: bool,
) -> Result<Session, &'static str> {
    let mut config = session_config(workspace);
    config.session_id = id;
    config.enable_session_store = Some(store);
    let prepared = client
        .prepare_session(config)
        .map_err(|_| "prepare_failed")?;
    let _events = prepared.subscribe();
    bounded(prepared.start()).await
}

pub async fn resume(client: &Client, workspace: &Path, id: &SessionId, store: bool) -> Value {
    let mut config = resume_config(id.clone(), workspace);
    config.enable_session_store = Some(store);
    let prepared = match client.prepare_resume_session(config) {
        Ok(p) => p,
        Err(_) => return json!({"state":"failed","code":"prepare_failed"}),
    };
    let _events = prepared.subscribe();
    match tokio::time::timeout(DEADLINE, prepared.start()).await {
        Ok(Ok(session)) => {
            let same = session.id() == id;
            let detach = bounded(session.disconnect()).await;
            json!({"state":if same {"resumed"} else {"failed"},
                   "same_id":same,"disconnect":detach.err().unwrap_or("acknowledged")})
        }
        Ok(Err(error)) => {
            // Numeric RPC codes are safe evidence. Never export upstream prose.
            let rpc_code = match error.kind() {
                github_copilot_sdk::ErrorKind::Rpc { code } => Some(*code),
                _ => None,
            };
            json!({"state":"failed","code":error_code(&error),"rpc_code":rpc_code})
        }
        Err(_) => json!({"state":"failed","code":"timeout"}),
    }
}

async fn empty_case(
    client: &Client,
    workspace: &Path,
    root: &Path,
    id: Option<SessionId>,
    abort: bool,
    store: bool,
) -> (Value, Option<SessionId>) {
    let explicit = id.is_some();
    let session = match create(client, workspace, id.clone(), store).await {
        Ok(s) => s,
        Err(code) => return (json!({"create":"failed","code":code}), None),
    };
    let actual_id = session.id().clone();
    let mut row = json!({"create":"acknowledged","id_kind":if explicit {"explicit"} else {"sdk_generated"},
        "explicit_id_preserved":id.as_ref().is_none_or(|id| id == &actual_id),
        "workspace_path_reported":session.workspace_path().is_some(),
        "workspace_path_id_matches":session.workspace_path().and_then(Path::file_name).is_some_and(|n| n == actual_id.as_ref()),
        "store_enabled":store,"metadata_active":metadata(client,&actual_id).await,
        "artifacts_active":artifacts(root,&actual_id),"message_requests":0});
    row["timeline"] = match bounded(session.get_events()).await {
        Ok(events) => {
            json!({"count":events.len(),"user_messages":events.iter().filter(|e| e.event_type == "user.message").count(),
            "assistant_messages":events.iter().filter(|e| e.event_type == "assistant.message").count()})
        }
        Err(code) => json!({"state":"unavailable","code":code}),
    };
    row["abort_empty"] = if abort {
        json!(bounded(session.abort())
            .await
            .err()
            .unwrap_or("acknowledged"))
    } else {
        json!("not_requested")
    };
    match bounded(session.disconnect()).await {
        Ok(()) => row["disconnect"] = json!("acknowledged"),
        Err(code) => {
            row["disconnect"] = json!(code);
            return (row, Some(actual_id)); // Never disguise failed detach as resumed.
        }
    }
    drop(session);
    row["metadata_detached"] = metadata(client, &actual_id).await;
    row["artifacts_detached"] = artifacts(root, &actual_id);
    row["same_client_resume"] = resume(client, workspace, &actual_id, store).await;
    row["persistence_observation"] = json!(if row["artifacts_detached"]["events_exists"] == false
        && row["metadata_detached"]["state"] == "absent"
        && row["same_client_resume"]["code"] == "session_not_found"
    {
        "empty_session_not_persisted"
    } else {
        "see_artifacts_metadata_and_resume"
    });
    (row, Some(actual_id))
}

/// `root` is private host storage. `options` already carries the read-only guard.
/// Each restart is Client::start, therefore also a new owned CLI process.
pub async fn matrix(
    options: impl Fn(&Path) -> Result<ClientOptions, &'static str>,
    workspace: &Path,
    root: &Path,
) -> Value {
    let mut report = json!({"schema_version":1,"sdk":"1.0.17","inference_calls":0,
        "resume_fallback_create":false,"real_history_resume_gate":"BLOCKED_REAL",
        "cases":[],"shutdowns":[]});
    if !root
        .symlink_metadata()
        .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
    {
        report["configuration_error"] = json!("unsafe_or_unavailable_storage_root");
        return report;
    }
    let initial = match options(root) {
        Ok(o) => o,
        Err(code) => {
            report["configuration_error"] = json!(code);
            return report;
        }
    };
    let client = match bounded(Client::start(initial)).await {
        Ok(c) => c,
        Err(code) => {
            report["start_error"] = json!(code);
            return report;
        }
    };
    report["runtime"] = match bounded(client.get_status()).await {
        Ok(s) => json!({"version":s.version,"protocol_version":s.protocol_version}),
        Err(code) => json!({"code":code}),
    };
    report["auth"] = match bounded(client.get_auth_status()).await {
        Ok(a) => json!(if a.is_authenticated {
            "authenticated"
        } else {
            "authentication_required"
        }),
        Err(code) => json!(code),
    };
    let mut saved = Vec::new();
    for (case, explicit, abort, store) in [
        ("S1_explicit", true, false, true),
        ("S2_generated", false, false, true),
        ("S6_abort_empty", true, true, true),
        ("store_disabled_control", true, false, false),
    ] {
        let id = explicit.then(opaque_id);
        let (mut row, id) = empty_case(&client, workspace, root, id, abort, store).await;
        row["case"] = json!(case);
        report["cases"].as_array_mut().unwrap().push(row);
        if let Some(id) = id {
            saved.push((case, id, store));
        }
    }
    let missing = opaque_id();
    report["missing_session"] = resume(&client, workspace, &missing, true).await;
    report["shutdowns"]
        .as_array_mut()
        .unwrap()
        .push(json!(shutdown(&client).await));
    drop(client);
    // Preserve state across both Client and actual CLI restart. No sleep/retry.
    match start(&options, root).await {
        Ok(client) => {
            for (case, id, store) in &saved {
                report["cases"].as_array_mut().unwrap().push(
                    json!({"case":format!("S3_S4_{case}"),
                    "metadata_after_restart":metadata(&client,id).await,
                    "artifacts_after_restart":artifacts(root,id),
                    "resume":resume(&client,workspace,id,*store).await}),
                );
            }
            report["shutdowns"]
                .as_array_mut()
                .unwrap()
                .push(json!(shutdown(&client).await));
            drop(client);
        }
        Err(code) => report["restart_error"] = json!(code),
    }
    // Fresh owned state, keeping CLI/version/auth guard equivalent. Replace only
    // our overlay source; do not clear or recreate a user's session directory.
    let fresh = root
        .parent()
        .unwrap()
        .join("fix2-fresh-state/session-state");
    if std::fs::create_dir_all(&fresh).is_err() {
        report["fresh_state_error"] = json!("storage_unavailable");
    } else {
        match start(&options, &fresh).await {
            Ok(client) => {
                if let Some((_, id, store)) = saved.first() {
                    report["fresh_state_resume"] = resume(&client, workspace, id, *store).await;
                }
                report["shutdowns"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(shutdown(&client).await));
                drop(client);
            }
            Err(code) => report["fresh_start_error"] = json!(code),
        }
    }
    // Delete only IDs created by this invocation in the original owned state.
    match start(&options, root).await {
        Ok(client) => {
            for (case, id, store) in &saved {
                let existed = metadata(&client, id).await;
                let delete = bounded(client.delete_session(id)).await;
                let after = resume(&client, workspace, id, *store).await;
                report["cases"].as_array_mut().unwrap().push(json!({"case":format!("S7_delete_{case}"),
                    "metadata_before_delete":existed,"delete":delete.err().unwrap_or("acknowledged"),
                    "resume_deleted":after,"artifacts_after_delete":artifacts(root,id)}));
            }
            report["shutdowns"]
                .as_array_mut()
                .unwrap()
                .push(json!(shutdown(&client).await));
            drop(client);
        }
        Err(code) => report["cleanup_start_error"] = json!(code),
    }
    // Diagnostic controls only: these transcripts were authored by the fixture,
    // not persisted by session.create and never sent to a model. They cannot
    // satisfy the genuine SDK/provider persistence gate.
    report["synthetic_disk_controls"] = disk_controls(&options, workspace, root).await;
    report
}

pub fn opaque_id() -> SessionId {
    SessionId::from(uuid::Uuid::new_v4().to_string())
}

async fn disk_controls(
    options: &impl Fn(&Path) -> Result<ClientOptions, &'static str>,
    workspace: &Path,
    root: &Path,
) -> Value {
    let mut rows = Vec::new();
    for corrupt in [false, true] {
        let id = opaque_id();
        let directory = root.join(id.as_ref());
        if std::fs::create_dir(&directory).is_err() {
            rows.push(json!({"state":"fixture_storage_failed"}));
            continue;
        }
        let event = json!({"id":uuid::Uuid::new_v4().to_string(),"parentId":null,
            "timestamp":"2026-10-09T00:00:00Z","type":"session.start","data":{
                "sessionId":id,"version":1,"producer":"copilot-agent","copilotVersion":"fixture",
                "startTime":"2026-10-09T00:00:00Z","selectedModel":"auto",
                "context":{"cwd":workspace}}});
        let bytes = if corrupt {
            b"{synthetic-corrupt-fixture}\n".to_vec()
        } else {
            format!("{event}\n").into_bytes()
        };
        let transcript = directory.join("events.jsonl");
        if std::fs::write(&transcript, &bytes).is_err() {
            rows.push(json!({"state":"fixture_storage_failed"}));
            continue;
        }
        let mut row = json!({"case":if corrupt {"S9_synthetic_corrupt"} else {"S9_synthetic_start_only"},
            "source":"fixture_authored_not_sdk_persistence","message_requests":0});
        match start(options, root).await {
            Ok(client) => {
                row["resume"] = resume(&client, workspace, &id, false).await;
                row["transcript_bytes_unchanged"] =
                    json!(std::fs::read(&transcript).ok().as_ref() == Some(&bytes));
                row["metadata"] = metadata(&client, &id).await;
                row["delete"] = json!(bounded(client.delete_session(&id))
                    .await
                    .err()
                    .unwrap_or("acknowledged"));
                row["resume_deleted"] = resume(&client, workspace, &id, false).await;
                row["shutdown"] = json!(shutdown(&client).await);
                drop(client);
            }
            Err(code) => row["start_error"] = json!(code),
        }
        rows.push(row);
    }
    json!(rows)
}

async fn start(
    options: &impl Fn(&Path) -> Result<ClientOptions, &'static str>,
    root: &Path,
) -> Result<Client, &'static str> {
    bounded(Client::start(options(root)?)).await
}
