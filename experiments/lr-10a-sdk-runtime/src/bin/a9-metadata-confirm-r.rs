//! Independent, one-invocation diagnostic. Never links a session/send path.
#[path = "../host_assisted.rs"]
#[allow(dead_code)]
mod host_assisted;
#[path = "../metadata_confirmation.rs"]
mod metadata_confirmation;
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::{Command, Stdio},
};

fn snapshot() -> Value {
    let output = Command::new("/usr/bin/python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/metadata_policy.py"))
        .arg("--snapshot")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(p) => serde_json::from_slice(&p.stdout)
            .unwrap_or_else(|_| json!({"structural_check":"BLOCKED"})),
        Err(_) => json!({"structural_check":"BLOCKED"}),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3
        || args[1] != "confirm"
        || std::env::var_os("NARYS_LR10A_OWNED_HARNESS").is_none()
    {
        std::process::exit(2);
    }
    // Separate FIX-4R diagnostic reservation, NOT the A9 inference marker.
    // Remains after crash/timeout. No path/flag for a second invocation.
    let mut reservation = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(0x20000)
        .mode(0o600)
        .open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/evidence/a9-fix-4r-runtime-reservation.json"
        )) {
        Ok(f) => f,
        Err(_) => std::process::exit(2),
    };
    if reservation
        .write_all(b"{\"scope\":\"A9_FIX4R_METADATA_ONLY\",\"max_client_invocations\":1}\n")
        .is_err()
        || reservation.sync_all().is_err()
    {
        std::process::exit(2);
    }
    let job = tempfile::Builder::new()
        .prefix("narys-a9-confirm-r-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = job.path().join("workspace");
    let logs = job.path().join("logs");
    for p in [&workspace, &logs] {
        std::fs::create_dir(p).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let options = host_assisted::preflight_options(Path::new(&args[2]), &workspace, &logs);
    let report = metadata_confirmation::confirm(options, snapshot).await;
    println!("{}", report);
}
