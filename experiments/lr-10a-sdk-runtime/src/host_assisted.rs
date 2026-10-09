//! Separate HOST-ASSISTED preparation; no change to the isolated FIX1–4 profile.
use github_copilot_sdk::{CliProgram, Client, ClientOptions, LogLevel, Transport};
use narys_lr10a_poc::{bounded, quota_probe};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

pub const PROMPT: &str = "Responda somente o número da soma de alpha=2 e beta=3. Não utilize ferramentas, não execute comandos, não acesse arquivos e não faça outras solicitações.";

/// Metadata only: retain normal CLI credential resolution, never extract a token.
/// No base_directory here: changing COPILOT_HOME can change the selected account.
/// Therefore this client MUST NOT create/send a session in personal state.
pub fn preflight_options(cli: &Path, workspace: &Path, logs: &Path) -> ClientOptions {
    ClientOptions::new()
        .with_program(CliProgram::Path(cli.to_path_buf()))
        .with_transport(Transport::Stdio)
        .with_cwd(workspace)
        .with_use_logged_in_user(true)
        .with_log_level(LogLevel::None)
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
            logs.to_str().unwrap(),
        ])
}

pub async fn preflight(client: &Client) -> Value {
    let auth = match bounded(client.get_auth_status()).await {
        Ok(a) => json!({"authenticated": a.is_authenticated, "identity_omitted": true}),
        Err(code) => json!({"authenticated":null,"code":code}),
    };
    let models = match bounded(client.list_models()).await {
        Ok(models) => json!({"source":"models.list", "models":models.iter().map(|m| {
            let id = if m.id.len() <= 80 && m.id.bytes().all(|c| c.is_ascii_alphanumeric() || b"-._".contains(&c)) {Some(m.id.as_str())} else {None};
            json!({"id":id,"billing_multiplier":m.billing.as_ref().and_then(|b| b.multiplier),
                "token_pricing_present":m.billing.as_ref().is_some_and(|b| b.token_prices.is_some()),
                "policy_present":m.policy.is_some(), "capabilities_present":serde_json::to_value(&m.capabilities).ok().is_some_and(|v| v.as_object().is_some_and(|o| !o.is_empty()))})
        }).collect::<Vec<_>>()}),
        Err(code) => json!({"source":"models.list","models":null,"code":code}),
    };
    let mut quota = quota_probe(client).await;
    if let Some(rows) = quota["snapshots"].as_array_mut() {
        rows.retain(|r| {
            matches!(
                r["kind"].as_str(),
                Some("premium_interactions" | "chat" | "completions")
            )
        });
        for row in rows {
            if let Some(o) = row["snapshot"].as_object_mut() {
                o.remove("resetDate");
            }
        }
    }
    json!({"auth":auth,"catalog":models,"quota":quota})
}

/// Account quota is not model pricing or proof of paid-fallback enforcement.
/// This revision deliberately has no live-send admission: current billing is
/// credits/token based, while the pinned quota surface reports requests.
/// A verified pricing/enforcement contract must be implemented, never a boolean
/// supplied by an invocation, before a live sender can be linked into this binary.
pub fn blockers(observation: &Value) -> Vec<&'static str> {
    let mut result = Vec::new();
    if observation["auth"]["authenticated"] != true {
        result.push("authentication_unavailable");
    }
    let snapshots = observation["quota"]["snapshots"].as_array();
    let premium = snapshots.and_then(|s| s.iter().find(|s| s["kind"] == "premium_interactions"));
    match premium {
        Some(p) => {
            let q = &p["snapshot"];
            if p["state"] != "quota_available" {
                result.push("included_quota_unverified");
            }
            if q["overageAllowedWithExhaustedQuota"] != false
                || q["usageAllowedWithExhaustedQuota"] != false
            {
                result.push("overage_policy_unverified_or_enabled");
            }
        }
        None => result.push("included_quota_unverified"),
    }
    let models = observation["catalog"]["models"].as_array();
    if models.is_none_or(|m| m.is_empty()) {
        result.push("model_catalog_unavailable");
    } else if models.is_some_and(|m| m.iter().all(|m| m["id"] == "auto")) {
        result.push("auto_cost_unknown");
    }
    result.push("billing_units_and_maximum_cost_unverified");
    result.push("no_paid_fallback_enforcement_unverified");
    result.push("private_authenticated_session_state_unverified");
    result
}

/// Atomically burn the attempt before invoking send. Existing/corrupt entries
/// block without reading them. Anchor all opens to a verified directory FD.
/// A failure after creation leaves the marker in place: never retry.
pub fn claim_attempt(directory: &Path) -> Result<(), &'static str> {
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(0x10000 | 0x20000)
        .open(directory)
        .map_err(|_| "unsafe_marker_directory")?;
    let meta = dir.metadata().map_err(|_| "unsafe_marker_directory")?;
    let uid = std::fs::metadata("/proc/self")
        .map_err(|_| "marker_identity_unavailable")?
        .uid();
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
        return Err("unsafe_marker_directory");
    }
    let path = format!(
        "/proc/self/fd/{}/lr10a-a9-host-attempt.json",
        dir.as_raw_fd()
    );
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| "attempt_already_claimed_or_unavailable")?;
    marker.write_all(b"{\"state\":\"ATTEMPTED\",\"gate\":\"A9_HOST_ASSISTED\",\"sdk\":\"1.0.17\",\"max_sdk_send_calls\":1}\n").map_err(|_|"marker_write_failed")?;
    marker.sync_all().map_err(|_| "marker_sync_failed")?;
    File::sync_all(&dir).map_err(|_| "marker_directory_sync_failed")?;
    Ok(())
}
