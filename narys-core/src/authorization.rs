//! Fixed LR-10A final human consent. Reservations count even if a crash makes
//! delivery uncertain. No retry, reset, caller-selected budget or task replay.
use crate::policy;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
};
pub const SCOPE: &str = "lr10a-final-20261010-three-attempts";
pub const PROMPT: &str = "Responda somente o número da soma de alpha=2 e beta=3. Não utilize ferramentas, não execute comandos, não acesse arquivos e não faça outras solicitações.";
pub fn directory(home: &Path) -> PathBuf {
    home.join(".local/state/narys/core/lr10a-final-authorization")
}
pub fn objective_hash() -> String {
    format!("{:x}", Sha256::digest(PROMPT.as_bytes()))
}
pub fn check(root: &Path) -> Result<(), &'static str> {
    policy::private_directory(root)?;
    if root
        .canonicalize()
        .map_err(|_| "authorization_path_unavailable")?
        != root
    {
        return Err("unsafe_authorization_path");
    }
    policy::private_file(&root.join("consent.json"))?;
    let c: Value = serde_json::from_slice(
        &fs::read(root.join("consent.json")).map_err(|_| "consent_unavailable")?,
    )
    .map_err(|_| "consent_invalid")?;
    if c["scope"] != SCOPE
        || c["max_attempts"] != 3
        || c["human_explicit_consent"] != true
        || c["provider_additional_usage_disabled"] != true
        || c["max_additional_usd"] != 0
        || c["billing_unit_uncertainty_explicitly_accepted"] != true
        || c["objective_sha256"] != objective_hash()
    {
        return Err("consent_scope_invalid");
    }
    if fs::symlink_metadata(root.join("closed.json")).is_ok() {
        return Err("authorization_closed");
    }
    Ok(())
}
/// Durable global slot + unique task, protected by flock, O_EXCL and fsync.
/// A partially written/corrupt reservation blocks all further sends.
pub fn claim(root: &Path, task_id: u64, receipt: &Value) -> Result<u8, &'static str> {
    check(root)?;
    policy::reviewed_receipt(receipt, task_id)?;
    if task_id <= 1
        || receipt["authorization_scope"] != SCOPE
        || receipt["objective_sha256"] != objective_hash()
    {
        return Err("attempt_scope_invalid");
    }
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.join("lock"))
        .map_err(|_| "attempt_lock_unavailable")?;
    policy::private_file(&root.join("lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err("attempt_lock_failed");
    }
    check(root)?;
    let mut free = None;
    for slot in 1..=3u8 {
        let path = root.join(format!("attempt-{slot}.json"));
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if free.is_none() {
                    free = Some(slot);
                }
            }
            Err(_) => return Err("attempt_identity_unavailable"),
            Ok(_) => {
                policy::private_file(&path)?;
                let v: Value =
                    serde_json::from_slice(&fs::read(&path).map_err(|_| "attempt_unavailable")?)
                        .map_err(|_| "attempt_corrupt")?;
                if v["scope"] != SCOPE
                    || v["slot"] != slot
                    || v["state"] != "ATTEMPTED"
                    || v["task_id"].as_u64().is_none()
                {
                    return Err("attempt_corrupt");
                }
                if v["task_id"] == task_id {
                    return Err("task_attempt_already_claimed");
                }
            }
        }
    }
    let slot = free.ok_or("global_attempt_limit_exhausted")?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.join(format!("attempt-{slot}.json")))
        .map_err(|_| "attempt_claim_failed")?;
    let value = json!({"scope":SCOPE,"slot":slot,"task_id":task_id,"state":"ATTEMPTED","uncertain_delivery_counts":true,"max_additional_usd":0});
    file.write_all(serde_json::to_string(&value).unwrap().as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| "attempt_sync_failed")?;
    fs::File::open(root)
        .and_then(|d| d.sync_all())
        .map_err(|_| "attempt_sync_failed")?;
    Ok(slot)
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    pub fn fixture(root: &Path) {
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("consent.json"),serde_json::to_vec(&json!({"scope":SCOPE,"max_attempts":3,"human_explicit_consent":true,"provider_additional_usage_disabled":true,"max_additional_usd":0,"billing_unit_uncertainty_explicitly_accepted":true,"objective_sha256":objective_hash()})).unwrap()).unwrap();
        fs::set_permissions(root.join("consent.json"), fs::Permissions::from_mode(0o600)).unwrap();
    }
    pub fn receipt(id: u64) -> Value {
        json!({"authorization_scope":SCOPE,"authorized_task_id":id,"objective_sha256":objective_hash(),"max_additional_usd":0,"provider_additional_usage_disabled":true,"billing_unit_uncertainty_explicitly_accepted":true,"max_sdk_send_calls":1,"human_reviewed_at_unix":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64()})
    }
    #[test]
    fn durable_cap_replay_and_crash() {
        let d = tempfile::tempdir().unwrap();
        fixture(d.path());
        assert_eq!(claim(d.path(), 2, &receipt(2)), Ok(1));
        assert_eq!(
            claim(d.path(), 2, &receipt(2)),
            Err("task_attempt_already_claimed")
        );
        assert_eq!(claim(d.path(), 3, &receipt(3)), Ok(2));
        assert_eq!(claim(d.path(), 4, &receipt(4)), Ok(3));
        assert_eq!(
            claim(d.path(), 5, &receipt(5)),
            Err("global_attempt_limit_exhausted")
        );
        assert!(claim(d.path(), 1, &receipt(1)).is_err());
    }
    #[test]
    fn race_allows_only_three_distinct_tasks() {
        let d = tempfile::tempdir().unwrap();
        fixture(d.path());
        let root = d.path().to_path_buf();
        let jobs = (2..10)
            .map(|id| {
                let root = root.clone();
                std::thread::spawn(move || claim(&root, id, &receipt(id)))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            jobs.into_iter()
                .filter_map(|j| j.join().unwrap().ok())
                .count(),
            3
        );
    }
    #[test]
    fn corruption_closure_wrong_scope_and_symlink_fail_closed() {
        let d = tempfile::tempdir().unwrap();
        fixture(d.path());
        assert!(claim(d.path(), 2, &json!({"force":true})).is_err());
        fs::write(d.path().join("attempt-1.json"), b"{").unwrap();
        fs::set_permissions(
            d.path().join("attempt-1.json"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert_eq!(claim(d.path(), 2, &receipt(2)), Err("attempt_corrupt"));
        fs::remove_file(d.path().join("attempt-1.json")).unwrap(); // synthetic only
        fs::write(d.path().join("closed.json"), b"closed").unwrap();
        assert_eq!(claim(d.path(), 2, &receipt(2)), Err("authorization_closed"));
    }
}
