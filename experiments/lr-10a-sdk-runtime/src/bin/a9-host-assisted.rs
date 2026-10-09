//! Live metadata only in this candidate: financial admission is BLOCKED_PRE_SEND.
#[path = "../host_assisted.rs"]
#[allow(dead_code)]
mod host_assisted;
use github_copilot_sdk::Client;
use narys_lr10a_poc::{bounded, explicit_program, shutdown};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, path::Path, time::Instant};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3
        || args[1] != "preflight"
        || std::env::var_os("NARYS_LR10A_OWNED_HARNESS").is_none()
    {
        println!(
            "{}",
            json!({"state":"BLOCKED_PRE_SEND","code":"owned_preflight_only","sdk_send_calls":0})
        );
        std::process::exit(2);
    }
    let code = run(Path::new(&args[2])).await;
    std::process::exit(code);
}
async fn run(path: &Path) -> i32 {
    let cli = match explicit_program(path) {
        Ok(p) => p,
        Err(code) => {
            println!(
                "{}",
                json!({"state":"BLOCKED_PRE_SEND","code":code,"sdk_send_calls":0})
            );
            return 2;
        }
    };
    let job = match tempfile::Builder::new()
        .prefix("narys-a9-host-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
    {
        Ok(j) => j,
        Err(_) => return 2,
    };
    let workspace = job.path().join("workspace");
    let logs = job.path().join("logs");
    for p in [&workspace, &logs] {
        if std::fs::create_dir(p).is_err()
            || std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).is_err()
        {
            return 2;
        }
    }
    let fixture = workspace.join("fixture.txt");
    let content = "alpha=2\nbeta=3\n";
    if std::fs::write(&fixture, content).is_err()
        || std::fs::set_permissions(&fixture, std::fs::Permissions::from_mode(0o400)).is_err()
    {
        return 2;
    }
    let started = Instant::now();
    let client = match bounded(Client::start(host_assisted::preflight_options(
        &cli, &workspace, &logs,
    )))
    .await
    {
        Ok(c) => c,
        Err(code) => {
            println!(
                "{}",
                json!({"state":"BLOCKED_PRE_SEND","start_code":code,"sdk_send_calls":0,"gui_started":false})
            );
            return 1;
        }
    };
    let start_ms = started.elapsed().as_millis();
    let status = match bounded(client.get_status()).await {
        Ok(s) => json!({"version":s.version,"protocol_version":s.protocol_version}),
        Err(code) => json!({"code":code}),
    };
    let result = host_assisted::preflight(&client).await;
    let blocked = host_assisted::blockers(&result);
    let stop_start = Instant::now();
    let stop = shutdown(&client).await;
    drop(client);
    println!(
        "{}",
        json!({"gate":"A9_HOST_ASSISTED","state":"BLOCKED_PRE_SEND","sdk":"1.0.17","runtime":status,
        "preflight":result,"blockers":blocked,"model_selected":null,"sdk_send_calls":0,"response_received":false,
        "task_result_validated":false,"persistence":"NOT_RUN_pre_send_blocked","attempt_marker_claimed":false,
        "workspace_private":true,"fixture_unchanged":std::fs::read(&fixture).ok().as_deref()==Some(content.as_bytes()),
        "host_assisted_not_sandbox":true,"gui_started":false,"start_ms":start_ms,"shutdown":stop,"stop_ms":stop_start.elapsed().as_millis()})
    );
    1
}
