//! Shared IPC v1 client. One attempt per request; uncertain mutations are never replayed.
use crate::ipc;
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub fn socket() -> Result<PathBuf, &'static str> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })));
    crate::policy::private_directory(&runtime)?;
    let directory = runtime.join("narys-core");
    crate::policy::private_directory(&directory)?;
    Ok(directory.join("control.sock"))
}
pub async fn request(command: ipc::Command) -> Result<Value, &'static str> {
    let request = ipc::Request {
        version: ipc::VERSION,
        request_id: format!("cli-{}", std::process::id()),
        command,
    };
    request.validate()?;
    let bytes = serde_json::to_vec(&request).map_err(|_| "invalid_command")?;
    if bytes.len() > ipc::MAX_REQUEST_BYTES {
        return Err("request_limit_or_timeout");
    }
    let mut stream = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::UnixStream::connect(socket()?),
    )
    .await
    .map_err(|_| "connect_timeout")?
    .map_err(|_| "core_service_unavailable")?;
    if stream
        .peer_cred()
        .map_err(|_| "peer_identity_unavailable")?
        .uid()
        != unsafe { libc::geteuid() }
    {
        return Err("server_identity_mismatch");
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        stream
            .write_all(&bytes)
            .await
            .map_err(|_| "request_write_failed")?;
        stream
            .shutdown()
            .await
            .map_err(|_| "request_shutdown_failed")
    })
    .await
    .map_err(|_| "request_timeout_outcome_unknown")??;
    let mut out = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(265),
        stream
            .take((ipc::MAX_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut out),
    )
    .await
    .map_err(|_| "response_timeout_outcome_unknown")?
    .map_err(|_| "response_read_failed_outcome_unknown")?;
    if out.len() > ipc::MAX_RESPONSE_BYTES {
        return Err("response_limit");
    }
    let typed: ipc::Response = serde_json::from_slice(&out).map_err(|_| "response_invalid")?;
    let v = serde_json::to_value(typed).map_err(|_| "response_invalid")?;
    if v["version"] != ipc::VERSION || v["request_id"] != request.request_id {
        return Err("response_correlation_mismatch");
    }
    Ok(v)
}
