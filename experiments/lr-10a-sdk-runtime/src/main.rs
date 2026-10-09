use github_copilot_sdk::Client;
use narys_lr10a_poc::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::{path::Path, time::Instant};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let code = run().await;
    std::process::exit(code);
}

async fn run() -> i32 {
    if std::env::var_os("NARYS_LR10A_OWNED_HARNESS").is_none() {
        eprintln!("Use measure.py: owned process cleanup harness is required");
        return 2;
    }
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3
        || ![
            "metadata",
            "metadata-existing-auth",
            "sessions",
            "sessions-existing-auth",
        ]
        .contains(&args[1].as_str())
    {
        eprintln!(
            "Usage: narys-lr10a-poc metadata|metadata-existing-auth|sessions|sessions-existing-auth /absolute/path/to/copilot (no inference)"
        );
        return 2;
    }
    let cli = match explicit_program(Path::new(&args[2])) {
        Ok(p) => p,
        Err(code) => {
            println!("{}", json!({"error":code}));
            return 2;
        }
    };
    let job = tempfile::Builder::new()
        .prefix("narys-lr10a-fix3-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .expect("private experimental job");
    let workspace = tempfile::tempdir_in(job.path()).expect("temporary fixture workspace");
    let state = tempfile::tempdir_in(job.path()).expect("temporary runtime state");
    std::fs::create_dir(job.path().join("logs")).unwrap();
    let session_storage = state.path().join("session-state");
    std::fs::create_dir(&session_storage).unwrap();
    std::fs::write(
        workspace.path().join("fixture.txt"),
        "LR-10A read-only fixture: 2 + 3 = 5\n",
    )
    .unwrap();
    let start = Instant::now();
    if args[1].starts_with("sessions") {
        let report = persistence::matrix(
            |root| {
                persistence::guarded_options(
                    &cli,
                    workspace.path(),
                    state.path(),
                    root,
                    args[1].ends_with("existing-auth"),
                )
            },
            Path::new("/fixture"),
            &session_storage,
        )
        .await;
        println!(
            "{}",
            json!({"probe":args[1],"sdk":"1.0.17","inference_calls":0,
            "result":report,"elapsed_ms":start.elapsed().as_millis()})
        );
        // Matrix observations do not turn the blocked real-history gate into PASS.
        return 1;
    }
    let opts = match persistence::guarded_options(
        &cli,
        workspace.path(),
        state.path(),
        &session_storage,
        args[1].ends_with("existing-auth"),
    ) {
        Ok(opts) => opts,
        Err(code) => {
            println!(
                "{}",
                json!({"configuration_error":code,"inference_calls":0})
            );
            return 2;
        }
    };
    let client = match bounded(Client::start(opts)).await {
        Ok(c) => c,
        Err(code) => {
            println!(
                "{}",
                json!({"start_error":code,"elapsed_ms":start.elapsed().as_millis(),"inference_calls":0})
            );
            return 1;
        }
    };
    let start_ms = start.elapsed().as_millis();
    let pid = client.pid();
    let result = metadata(&client).await;
    let stop_start = Instant::now();
    let stop = shutdown(&client).await;
    drop(client);
    // SDK force_stop is a request, not proof of reaping; independently inspect /proc.
    let deadline = Instant::now() + std::time::Duration::from_secs(2);
    while pid.is_some_and(|p| Path::new(&format!("/proc/{p}")).exists())
        && Instant::now() < deadline
    {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let pid_gone = pid.is_some_and(|p| !Path::new(&format!("/proc/{p}")).exists());
    println!(
        "{}",
        json!({"probe":args[1],"sdk":"1.0.17","start_ms":start_ms,
        "stop_ms":stop_start.elapsed().as_millis(),"pid":pid,"owned_pid_gone":pid_gone,
        "shutdown":stop,"inference_calls":0,"result":result})
    );
    if !pid_gone || stop != "graceful" || result.get("lifecycle_error").is_some() {
        return 1;
    }
    0
}
