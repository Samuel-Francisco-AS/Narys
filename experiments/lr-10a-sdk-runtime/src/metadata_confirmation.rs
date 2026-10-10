//! A9-FIX-4: exactly start/status/auth/shutdown; no session or billing APIs.
use github_copilot_sdk::{Client, ClientOptions};
use narys_lr10a_poc::{bounded, shutdown};
use serde_json::{json, Value};
use std::time::Instant;

fn phase(rows: &mut Vec<Value>, name: &str, observer: &mut impl FnMut() -> Value) -> bool {
    let observation = observer();
    let safe = observation["structural_check"] == "PASS_METADATA_ACCESS";
    rows.push(json!({"phase":name,"observation":observation}));
    safe
}

pub async fn confirm(options: ClientOptions, mut observer: impl FnMut() -> Value) -> Value {
    let mut rows = vec![];
    let mut report = json!({"sdk":"1.0.17","host_assisted_with_gui_not_sandbox":true,
        "authenticated":null,"start_calls":0,"status_calls":0,"auth_calls":0,
        "session_operations":0,"sdk_send_calls":0,"model_quota_calls":0,
        "attempt_marker_claimed":false,"shutdown":"NOT_STARTED"});
    if !phase(&mut rows, "before_start", &mut observer) {
        report["error_code"] = json!("structural_precondition_failed");
    } else {
        report["start_calls"] = json!(1);
        let started = Instant::now();
        match bounded(Client::start(options)).await {
            Err(code) => report["error_code"] = json!(code),
            Ok(client) => {
                report["start_ms"] = json!(started.elapsed().as_millis());
                if phase(&mut rows, "after_start", &mut observer) {
                    report["status_calls"] = json!(1);
                    match bounded(client.get_status()).await {
                        Err(code) => report["error_code"] = json!(code),
                        Ok(status) => {
                            // Only known version/protocol indicators, never arbitrary RPC prose.
                            report["runtime_identity_matches"] =
                                json!(status.version == "1.0.95" && status.protocol_version == 3);
                            if phase(&mut rows, "after_status", &mut observer)
                                && status.version == "1.0.95"
                                && status.protocol_version == 3
                            {
                                report["auth_calls"] = json!(1);
                                match bounded(client.get_auth_status()).await {
                                    Ok(auth) => {
                                        report["authenticated"] = json!(auth.is_authenticated)
                                    }
                                    Err(code) => report["error_code"] = json!(code),
                                }
                                phase(&mut rows, "after_auth", &mut observer);
                            }
                        }
                    }
                }
                let stopped = Instant::now();
                report["shutdown"] = json!(shutdown(&client).await);
                drop(client);
                report["stop_ms"] = json!(stopped.elapsed().as_millis());
                phase(&mut rows, "after_shutdown", &mut observer);
            }
        }
    }
    report["phases"] = json!(rows);
    report
}
