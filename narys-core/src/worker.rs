//! Fixed SDK worker: host-assisted, zero tools, private per-session state.
use crate::policy::{self, TaskInput};
use github_copilot_sdk::{CliProgram, Client, ClientOptions, LogLevel, Transport};
use narys_lr10a_poc::{bounded, session_config, shutdown};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path, sync::Arc, time::Duration};

pub fn validate_cli(cli: &Path) -> Result<(), &'static str> {
    let mut image = fs::File::open(cli).map_err(|_| "cli_unavailable")?;
    let mut hash = Sha256::new();
    std::io::copy(&mut image, &mut hash).map_err(|_| "cli_read_failed")?;
    if format!("{:x}", hash.finalize()) != policy::CLI_SHA {
        return Err("cli_pin_mismatch");
    }
    Ok(())
}
fn options(cli: &Path, dir: &Path) -> ClientOptions {
    ClientOptions::new()
        .with_program(CliProgram::Path(cli.into()))
        .with_transport(Transport::Stdio)
        .with_cwd(dir.join("workspace"))
        .with_use_logged_in_user(true)
        .with_log_level(LogLevel::None)
        // Local IPv6 route is unusable for provider endpoints. Affect only this
        // runtime's glibc DNS lookups; keep TLS verification and host untouched.
        .with_env([("RES_OPTIONS", "no-aaaa")])
        .with_env_remove([
            "COPILOT_HOME",
            "COPILOT_SDK_AUTH_TOKEN",
            "COPILOT_GITHUB_TOKEN",
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "DISPLAY",
            "WAYLAND_DISPLAY",
        ])
        .with_extra_args([
            "--disable-builtin-mcps",
            "--log-dir",
            dir.join("logs").to_str().unwrap(),
        ])
}
async fn preflight(client: &Client) -> Value {
    let auth = bounded(client.get_auth_status())
        .await
        .map(|a| json!({"authenticated":a.is_authenticated,"identity_omitted":true}))
        .unwrap_or_else(|c| json!({"authenticated":null,"code":c}));
    // One metadata request with an explicit network deadline; no retry.
    let catalog = match tokio::time::timeout(Duration::from_secs(30), client.list_models()).await {
        Ok(Ok(models)) => {
            json!({"source":"models.list","models":models.into_iter().filter(|m|m.id.len()<=80&&m.id.bytes().all(|b|b.is_ascii_alphanumeric()||b"-._".contains(&b))).map(|m|json!({"id":m.id,"billing_multiplier":m.billing.as_ref().and_then(|b|b.multiplier),"token_pricing_present":m.billing.as_ref().is_some_and(|b|b.token_prices.is_some())})).collect::<Vec<_>>()})
        }
        Ok(Err(e)) => {
            json!({"source":"models.list","models":null,"code":narys_lr10a_poc::error_code(&e)})
        }
        Err(_) => json!({"source":"models.list","models":null,"code":"timeout"}),
    };
    let mut quota = narys_lr10a_poc::quota_probe(client).await;
    if let Some(rows) = quota["snapshots"].as_array_mut() {
        rows.retain(|r| {
            matches!(
                r["kind"].as_str(),
                Some("premium_interactions" | "chat" | "completions")
            )
        });
        for r in rows {
            if let Some(s) = r["snapshot"].as_object_mut() {
                s.remove("resetDate");
            }
        }
    }
    json!({"auth":auth,"catalog":catalog,"quota":quota})
}
pub fn claim_task(directory: &Path) -> Result<(), &'static str> {
    policy::private_directory(directory)?;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join("send-attempt.json"))
        .map_err(|_| "attempt_already_claimed")?;
    f.write_all(b"{\"state\":\"ATTEMPTED\",\"max_sdk_send_calls\":1}\n")
        .map_err(|_| "attempt_write_failed")?;
    f.sync_all().map_err(|_| "attempt_sync_failed")?;
    fs::File::open(directory)
        .and_then(|d| d.sync_all())
        .map_err(|_| "attempt_sync_failed")
}
pub async fn run(dir: &Path, cli: &Path) -> Value {
    let mut report = json!({"sdk":"1.0.17","host_assisted_not_sandbox":true,"sdk_send_calls":0,"tools_executed":0,"shutdown":"NOT_STARTED","state":"blocked"});
    let result = operation(dir, cli, &mut report).await;
    if let Err(code) = result {
        report["error_code"] = json!(code);
    }
    report
}
async fn operation(dir: &Path, cli: &Path, report: &mut Value) -> Result<(), &'static str> {
    policy::private_directory(dir)?;
    validate_cli(cli)?;
    for name in ["workspace", "logs", "session-state"] {
        policy::private_directory(&dir.join(name))?;
    }
    let client = bounded(Client::start(options(cli, dir))).await?;
    run_client(client, dir, report).await
}
async fn run_client(client: Client, dir: &Path, report: &mut Value) -> Result<(), &'static str> {
    let outcome=async {
        report["phase"]=json!("runtime_status");
        let s=bounded(client.get_status()).await?;
        if s.version!="1.0.95" || s.protocol_version!=3 {return Err("runtime_protocol_mismatch");}
        report["runtime"]=json!({"version":s.version,"protocol_version":s.protocol_version});
        report["phase"]=json!("metadata_preflight");
        let preflight=preflight(&client).await;
        report["preflight"]=preflight.clone();
        if !dir.join("task.json").exists() {report["state"]=json!("metadata_observed"); return Ok(());}
        policy::private_file(&dir.join("task.json"))?;
        let input:TaskInput=serde_json::from_slice(&fs::read(dir.join("task.json")).map_err(|_|"task_read_failed")?).map_err(|_|"invalid_task")?;
        policy::validate_input(&input)?; policy::financial_preflight(&preflight)?;
        // Separate trusted human financial admission, not derived from authentication.
        policy::private_file(&dir.join("financial-reviewed.json"))?;
        let review:Value=serde_json::from_slice(&fs::read(dir.join("financial-reviewed.json")).map_err(|_|"financial_review_unavailable")?).map_err(|_|"financial_review_invalid")?;
        let id=review["authorized_task_id"].as_u64().ok_or("financial_review_invalid")?;
        policy::reviewed_receipt(&review,id)?;
        let objective_hash=format!("{:x}",Sha256::digest(input.objective.as_bytes()));
        if review["objective_sha256"]!=objective_hash{return Err("reviewed_objective_changed");}
        let mut cfg=session_config(&dir.join("workspace")).with_permission_handler(Arc::new(github_copilot_sdk::handler::DenyAllHandler));
        cfg.config_directory=Some(dir.join("session-state"));
        cfg.enable_session_store=Some(false);
        let mut limit=github_copilot_sdk::session_events::SessionLimitsConfig::default(); limit.max_ai_credits=Some(0.5); cfg.session_limits=Some(limit);
        cfg.infinite_sessions=Some(github_copilot_sdk::types::InfiniteSessionConfig::new().with_enabled(false));
        let prepared=client.prepare_session(cfg).map_err(|_|"prepare_failed")?;
        let mut events=prepared.subscribe();
        // Prepared.start includes session.create and the SDK's post-create
        // options patch. Do not claim an exact RPC method from this phase.
        report["phase"]=json!("session_start");
        let session=match tokio::time::timeout(narys_lr10a_poc::DEADLINE,prepared.start()).await {
            Ok(Ok(session))=>session,
            Ok(Err(error))=>{
                if let github_copilot_sdk::ErrorKind::Rpc{code}=error.kind(){report["rpc_error_code"]=json!(code);}
                return Err(narys_lr10a_poc::error_code(&error));
            },
            Err(_)=>return Err("timeout"),
        };
        report["session_created"]=json!(true);
        if session.workspace_path().is_some_and(|p|!p.starts_with(dir)){let _=bounded(session.disconnect()).await;return Err("session_storage_outside_private_root");}
        let inference=async {
            if dir.join("cancel").exists() {return Err("cancelled_before_send");}
            claim_task(dir)?;
            report["phase"]=json!("single_send");
            report["sdk_send_calls"]=json!(1);
            let deadline=if cfg!(test){Duration::from_millis(300)}else{Duration::from_secs(120)};
            let send=session.send_and_wait(github_copilot_sdk::types::MessageOptions::new(input.objective.clone()).with_wait_timeout(deadline));
            tokio::pin!(send);
            let mut interval=tokio::time::interval(Duration::from_millis(100));
            let mut seen=std::collections::BTreeMap::<String,u32>::new();
            loop {
                tokio::select! {
                    result=&mut send=> {
                        report["events"]=json!(seen);
                        let message=result.map_err(|e|narys_lr10a_poc::error_code(&e))?.ok_or("no_final_response")?;
                        let output=message.data.get("content").and_then(Value::as_str).ok_or("invalid_final_response")?;
                        if output.trim().is_empty() || output.len()>16384 {return Err("invalid_final_response");}
                        report["output"]=json!(output); report["state"]=json!("response_received");
                        return Ok(());
                    },
                    event=events.recv()=> {
                        let e=event.map_err(|_|"event_stream_incomplete")?;
                        let kind=e.event_type.to_string();
                        // Fixed event labels, no raw payload, hidden reasoning or IDs.
                        if matches!(kind.as_str(),"assistant.turn_start"|"assistant.turn_end"|"assistant.message"|"assistant.usage"|"session.idle"|"session.error"|"tool.execution_start") {
                            *seen.entry(kind.clone()).or_default()+=1;
                        }
                        if kind=="assistant.usage" {
                            let mut usage=serde_json::Map::new();
                            for key in ["inputTokens","outputTokens","cacheReadTokens","cacheWriteTokens","cost","duration"]{if e.data[key].is_number(){usage.insert(key.into(),e.data[key].clone());}}
                            report["provider_usage_numeric_fields"]=Value::Object(usage);
                            report["cost_field_unit"]=json!("model_multiplier_as_sdk_schema_not_USD");
                        }
                        if kind=="tool.execution_start" {return Err("unexpected_tool_execution");}
                        if kind=="session.error" {return Err("session_error_observed");}
                    },
                    _=interval.tick()=>{if dir.join("cancel").exists(){return Err("cancelled");}}
                }
            }
        }.await;
        if inference.is_err() {let _=bounded(session.abort()).await;}
        report["disconnect"]=json!(bounded(session.disconnect()).await.err().unwrap_or("acknowledged"));
        drop(session);
        report["quota_after"]=narys_lr10a_poc::quota_probe(&client).await;
        if let Some(rows)=report["quota_after"]["snapshots"].as_array_mut(){rows.retain(|r|matches!(r["kind"].as_str(),Some("premium_interactions"|"chat"|"completions")));for r in rows{if let Some(s)=r["snapshot"].as_object_mut(){s.remove("resetDate");}}}
        inference
    }.await;
    report["shutdown"] = json!(shutdown(&client).await);
    drop(client);
    if report["shutdown"] != "graceful" {
        return Err("sdk_shutdown_incomplete");
    }
    outcome
}
#[cfg(test)]
mod tests {
    use super::*;
    async fn synthetic(mode: &str) -> (Result<(), &'static str>, Value, Value) {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
        for n in ["workspace", "logs", "session-state"] {
            fs::create_dir(d.path().join(n)).unwrap();
        }
        let input = json!({"objective":"Responda 5.","model":"auto","included_only_approval":true});
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let review = json!({"authorized_task_id":1,"max_additional_usd":0,"provider_additional_usage_disabled":true,"billing_unit_uncertainty_explicitly_accepted":true,"max_sdk_send_calls":1,"human_reviewed_at_unix":now,"objective_sha256":format!("{:x}",Sha256::digest(b"Responda 5."))});
        for (name, value) in [("task.json", input), ("financial-reviewed.json", review)] {
            fs::write(d.path().join(name), serde_json::to_vec(&value).unwrap()).unwrap();
            fs::set_permissions(d.path().join(name), fs::Permissions::from_mode(0o600)).unwrap();
        }
        let opts = ClientOptions::new()
            .with_program(CliProgram::Path("/usr/bin/python3".into()))
            .with_prefix_args([concat!(env!("CARGO_MANIFEST_DIR"), "/tests/sdk_peer.py")])
            .with_env([
                ("SYNTHETIC_ROOT", d.path().to_str().unwrap()),
                ("SYNTHETIC_MODE", mode),
            ]);
        let client = Client::start(opts).await.unwrap();
        let pid = client.pid().unwrap();
        let mut report = json!({"sdk_send_calls":0});
        let result = run_client(client, d.path(), &mut report).await;
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        if report["sdk_send_calls"] == 1 {
            assert_eq!(claim_task(d.path()), Err("attempt_already_claimed"));
        } else {
            assert!(!d.path().join("send-attempt.json").exists());
        }
        let summary =
            serde_json::from_slice(&fs::read(d.path().join("summary.json")).unwrap()).unwrap();
        (result, report, summary)
    }
    #[tokio::test]
    async fn synthetic_send_validates_response_without_retry() {
        let (r, e, s) = synthetic("normal").await;
        assert_eq!(r, Ok(()));
        assert_eq!(e["output"], "5");
        assert_eq!(e["shutdown"], "graceful");
        assert_eq!(s["send"], 1);
        assert_eq!(s["detach"], 1);
        assert_eq!(s["zero_tools"], true);
    }
    #[tokio::test]
    async fn synthetic_cancel_and_timeout_abort_disconnect_without_resend() {
        for mode in ["cancel", "timeout"] {
            let (r, e, s) = synthetic(mode).await;
            assert!(r.is_err());
            assert_eq!(e["shutdown"], "graceful");
            assert_eq!(s["send"], 1);
            assert_eq!(s["abort"], 1);
            assert_eq!(s["detach"], 1);
        }
    }
    #[tokio::test]
    async fn session_create_error_keeps_numeric_code_without_secret_or_send() {
        let (r, e, s) = synthetic("create_error").await;
        assert_eq!(r, Err("rpc_error_unknown"));
        assert_eq!(e["phase"], "session_start");
        assert_eq!(e["rpc_error_code"], -32602);
        assert_eq!(e["sdk_send_calls"], 0);
        assert_eq!(e["shutdown"], "graceful");
        assert_eq!(s["send"], 0);
        assert!(!e.to_string().contains("synthetic-private-secret"));
    }
    #[tokio::test]
    async fn post_create_patch_error_is_not_a_completed_session_or_inference() {
        let (r, e, s) = synthetic("options_error").await;
        assert_eq!(r, Err("rpc_error_unknown"));
        assert_eq!(e["phase"], "session_start");
        assert_eq!(e["rpc_error_code"], -32602);
        assert_eq!(e["sdk_send_calls"], 0);
        assert_eq!(s["send"], 0);
        assert_eq!(s["detach"], 1);
        assert_eq!(e["shutdown"], "graceful");
    }
    #[test]
    fn attempt_claim_is_single_use() {
        let d = tempfile::tempdir().unwrap();
        std::fs::set_permissions(
            d.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        claim_task(d.path()).unwrap();
        assert_eq!(claim_task(d.path()), Err("attempt_already_claimed"));
    }
    #[test]
    fn unknown_cli_fails_before_start() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bad");
        fs::write(&p, b"fake").unwrap();
        assert_eq!(validate_cli(&p), Err("cli_pin_mismatch"));
    }
    #[test]
    fn deny_all_configuration_keeps_private_state() {
        let d = tempfile::tempdir().unwrap();
        let c = session_config(d.path())
            .with_permission_handler(Arc::new(github_copilot_sdk::handler::DenyAllHandler));
        assert_eq!(c.available_tools, Some(vec![]));
        assert!(c.permission_handler.is_some());
        assert_eq!(c.enable_skills, Some(false));
        assert_eq!(c.hooks, Some(false));
    }
}
