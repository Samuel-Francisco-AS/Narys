use github_copilot_sdk::Client;
use narys_lr10a_poc::*;
use serde_json::json;
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
    let workspace = tempfile::tempdir().expect("temporary fixture workspace");
    let state = tempfile::tempdir().expect("temporary runtime state");
    std::fs::write(
        workspace.path().join("fixture.txt"),
        "LR-10A read-only fixture: 2 + 3 = 5\n",
    )
    .unwrap();
    let start = Instant::now();
    if args[1].starts_with("sessions") {
        let session_storage = state.path().join("session-state");
        std::fs::create_dir(&session_storage).unwrap();
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
            workspace.path(),
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
    let mut opts = options(cli, workspace.path(), state.path());
    // Manual metadata-only probe against CLI's existing credential store. No login,
    // setters, session creation or config copies. Keep logs in the temporary dir.
    if args[1].ends_with("existing-auth") {
        opts.base_directory = None;
        opts.env_remove.push("COPILOT_HOME".into());
        // Existing auth can mutate CLI configuration during startup. Enforce a
        // read-only host view for this metadata-only experiment, with only our
        // disposable workspace/log dirs writable. This is not a product sandbox.
        let copilot = explicit_program(Path::new(&args[2])).unwrap();
        opts.program = github_copilot_sdk::CliProgram::Path("/usr/bin/bwrap".into());
        opts.prefix_args = vec![
            "--ro-bind".into(),
            "/".into(),
            "/".into(),
            "--dev-bind".into(),
            "/dev".into(),
            "/dev".into(),
            "--proc".into(),
            "/proc".into(),
            "--bind".into(),
            workspace.path().into(),
            workspace.path().into(),
            "--bind".into(),
            state.path().into(),
            state.path().into(),
            "--".into(),
            copilot.into_os_string(),
        ];
    }
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
