//! FIX-4 offline real CLI registration/metadata only; no credentials or inference.
#[path = "../auth_network.rs"]
mod auth_network;
use github_copilot_sdk::Client;
use narys_lr10a_poc::{boundary::isolated_options, bounded, metadata, shutdown};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Instant};
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    if std::env::var_os("NARYS_LR10A_OWNED_HARNESS").is_none()
        || args.len() != 3
        || args[1] != "metadata"
    {
        std::process::exit(2);
    }
    let job = tempfile::Builder::new()
        .prefix("narys-fix4-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = job.path().join("workspace");
    let state = job.path().join("state");
    let sessions = state.join("session-state");
    let logs = job.path().join("logs");
    for p in [&workspace, &sessions, &logs] {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(workspace.join("fixture.txt"), "synthetic metadata fixture").unwrap();
    let mut opts = match isolated_options(Path::new(&args[2]), &workspace, &state, &sessions) {
        Ok(o) => o,
        Err(c) => {
            println!("{}", json!({"configuration_error":c,"inference_calls":0}));
            return;
        }
    };
    let gateway = Arc::new(auth_network::FixtureGateway::new(None, None));
    opts.request_handler = Some(gateway.clone());
    let start = Instant::now();
    let client = match bounded(Client::start(opts)).await {
        Ok(c) => c,
        Err(code) => {
            println!(
                "{}",
                json!({"start_error":code,"handler_registered":false,"inference_calls":0})
            );
            return;
        }
    };
    let startup_ms = start.elapsed().as_millis();
    if let Err(code) = auth_network::require_registered(&client).await {
        let outcome = shutdown(&client).await;
        println!(
            "{}",
            json!({"handler_registered":false,"registration_error":code,
            "shutdown":outcome,"inference_calls":0,"metadata_not_admitted":true})
        );
        return;
    }
    let status = bounded(client.get_status()).await;
    let metadata = metadata(&client).await;
    let stop = Instant::now();
    let outcome = shutdown(&client).await;
    println!(
        "{}",
        json!({"handler_registered":true,"registration_positive_ack_checked":true,"startup_ms":startup_ms,
        "runtime_version":status.as_ref().map(|s|s.version.as_str()).ok(),
        "protocol_version":status.as_ref().map(|s|s.protocol_version).ok(),
        "metadata":metadata,"gateway_attempts":gateway.calls(),
        "shutdown":outcome,"shutdown_ms":stop.elapsed().as_millis(),"inference_calls":0,
        "auth_boundary":"BLOCKED_AUTH_BOUNDARY","network_boundary":"BLOCKED_NETWORK_BOUNDARY"})
    );
}
