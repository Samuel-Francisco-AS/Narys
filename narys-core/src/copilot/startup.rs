//! Write-ahead launch boundary inside the existing authoritative ownership row.
//! A preparing journal is positive control-flow evidence: SDK launch cannot be
//! invoked until launch_intent has committed. Missing/legacy journals prove nothing.
use super::supervisor::{StartupFailure, StartupSafety};
use crate::persistence::database::Database;
use rusqlite::params;
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn unresolved(db: &Database) -> Result<bool, &'static str> {
    let conn = db.open().map_err(|e| e.code())?;
    let mut q=conn.prepare("SELECT runtime_ref,private_directory,boot_id,state,cleanup_verified,owner_json,cleanup_json FROM agent_runtime_owners").map_err(|_|"runtime_ownership_read_failed")?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(|_| "runtime_ownership_read_failed")?;
    let mut pending = false;
    let mut inconsistent = Vec::new();
    for row in rows {
        let (reference, path, boot, state, verified, owner, proof) =
            row.map_err(|_| "runtime_ownership_read_failed")?;
        if !verified || state != "stopped" {
            pending = true;
            continue;
        }
        let valid = owner
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .zip(proof.and_then(|s| serde_json::from_str::<Value>(&s).ok()))
            .is_some_and(|(owner, proof)| {
                completed_proven(&owner, &proof, &reference, &boot, Path::new(&path))
            });
        if !valid {
            inconsistent.push(reference);
            pending = true;
        }
    }
    drop(q);
    drop(conn);
    for reference in inconsistent {
        mark_inconsistent(db, &reference)?;
    }
    Ok(pending)
}

pub(super) fn mark_inconsistent(db: &Database, reference: &str) -> Result<(), &'static str> {
    if db.open().map_err(|e|e.code())?.execute("UPDATE agent_runtime_owners SET state='faulted',cleanup_verified=0,error_code=coalesce(error_code,'runtime_terminal_evidence_invalid') WHERE runtime_ref=?1",[reference])
        .map_err(|_|"runtime_ownership_persist_failed")? != 1 {return Err("runtime_ownership_persist_failed");}
    Ok(())
}

pub(super) fn completed_proven(
    owner: &Value,
    proof: &Value,
    reference: &str,
    boot: &str,
    directory: &Path,
) -> bool {
    if !owner.is_object() || !super::sdk::valid_boot_id(boot) {
        return false;
    }
    if let Some(journal) = owner.get("startup") {
        if journal["version"] != 1
            || journal["runtime_ref"] != reference
            || journal["boot_id"] != boot
        {
            return false;
        }
    }
    if proof["no_process_launched"] == true {
        return matches!(
            proof["evidence"].as_str(),
            Some("sdk_launch_not_invoked" | "write_ahead_prelaunch_boundary")
        ) && prelaunch_proven(owner, reference, boot, directory);
    }
    if proof["evidence"] == "previous_kernel_boot" {
        return proof["recorded_boot_id"] == boot
            && proof["current_boot_id"].as_str().is_some_and(|observed| {
                observed != boot
                    && super::sdk::valid_boot_id(observed)
                    && super::sdk::valid_boot_id(boot)
            });
    }
    proof["cleanup_complete"] == true
        && proof["kernel_children_exhausted"] == true
        && owner.get("startup").is_none_or(|journal| {
            journal["launch_attempted"] == true
                && matches!(
                    journal["phase"].as_str(),
                    Some("ready" | "launch_intent" | "failed_after_launch")
                )
        })
        && owner["primary_pid"]
            .as_u64()
            .is_some_and(|pid| pid > 0 && pid <= u32::MAX as u64)
        && owner["primary_start_ticks"].as_u64().is_some()
}

/// Detect missing ownership rows without adopting or signalling any process.
pub(super) fn check_artifacts(db: &Database, root: &Path) -> Result<(), &'static str> {
    if !root.exists() {
        return Ok(());
    }
    crate::policy::private_directory(root)?;
    let conn = db.open().map_err(|e| e.code())?;
    for entry in std::fs::read_dir(root).map_err(|_| "runtime_artifact_read_failed")? {
        let path = entry.map_err(|_| "runtime_artifact_read_failed")?.path();
        crate::policy::private_directory(&path)?;
        let registered: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_runtime_owners WHERE private_directory=?1)",
                [path.to_str()],
                |r| r.get(0),
            )
            .map_err(|_| "runtime_ownership_read_failed")?;
        if !registered {
            return Err("runtime_unregistered_artifact");
        }
    }
    Ok(())
}

pub(super) struct StartupJournal {
    db: Database,
    pub reference: String,
    value: Value,
    armed: bool,
}
impl StartupJournal {
    pub fn begin(db: Database, directory: &Path, boot: &str) -> Result<Self, StartupFailure> {
        let reference = directory
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| StartupFailure::no_process("runtime_path_invalid"))?
            .to_owned();
        let value = json!({"startup":{"version":1,"runtime_ref":reference,"boot_id":boot,
            "phase":"preparing","launch_attempted":false}});
        let persisted = (|| {
            let mut conn = db.open().map_err(|e| e.code())?;
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|_| "runtime_ownership_persist_failed")?;
            let pending: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_runtime_owners WHERE cleanup_verified=0 OR state!='stopped')", [], |r| r.get(0),
            ).map_err(|_| "runtime_ownership_read_failed")?;
            if pending {
                return Err("runtime_previous_ownership_unresolved");
            }
            tx.execute("INSERT INTO agent_runtime_owners(runtime_ref,private_directory,boot_id,state,owner_json) VALUES(?1,?2,?3,'starting',?4)",
                params![reference,directory.to_str(),boot,value.to_string()]).map_err(|_| "runtime_ownership_persist_failed")?;
            tx.commit().map_err(|_| "runtime_ownership_persist_failed")
        })();
        if let Err(code) = persisted {
            let mut failure = StartupFailure::persistence(code);
            failure.runtime_ref = Some(reference);
            return Err(failure);
        }
        Ok(Self {
            db,
            reference,
            value,
            armed: true,
        })
    }
    #[cfg(test)]
    pub fn leave_pending_for_restart(mut self) {
        // Crash fixture: keep the already committed preparing row unchanged.
        self.armed = false;
    }
    pub fn launch_intent(&mut self) -> Result<(), &'static str> {
        self.value["startup"]["phase"] = json!("launch_intent");
        self.value["startup"]["launch_attempted"] = json!(true);
        self.write("starting", false, None, None)
    }
    pub fn ready(&mut self, owner: Value) -> Result<(), &'static str> {
        self.merge_owner(owner);
        self.value["startup"]["phase"] = json!("ready");
        self.write("ready", false, None, None)?;
        self.armed = false;
        Ok(())
    }
    fn merge_owner(&mut self, owner: Value) {
        if let Some(fields) = owner.as_object() {
            for (key, value) in fields {
                self.value[key] = value.clone();
            }
        }
    }
    fn write(
        &self,
        state: &str,
        verified: bool,
        error: Option<&str>,
        proof: Option<Value>,
    ) -> Result<(), &'static str> {
        let mut conn = self.db.open().map_err(|e| e.code())?;
        let tx = conn
            .transaction()
            .map_err(|_| "runtime_ownership_persist_failed")?;
        let count = tx.execute("UPDATE agent_runtime_owners SET state=?2,cleanup_verified=?3,error_code=?4,owner_json=?5,cleanup_json=?6,finished_at=CASE WHEN ?2 IN ('stopped','faulted') THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') ELSE NULL END WHERE runtime_ref=?1",
            params![self.reference,state,verified,error,self.value.to_string(),proof.map(|p|p.to_string())])
            .map_err(|_| "runtime_ownership_persist_failed")?;
        if count != 1 {
            return Err("runtime_ownership_missing");
        }
        if verified {
            tx.execute(
                "UPDATE agent_runs SET cleanup_verified=1 WHERE runtime_ref=?1",
                [&self.reference],
            )
            .map_err(|_| "runtime_ownership_persist_failed")?;
        }
        tx.commit().map_err(|_| "runtime_ownership_persist_failed")
    }
    pub fn fail_before_launch(
        &mut self,
        code: &'static str,
        persistence_fault: bool,
    ) -> StartupFailure {
        self.value["startup"]["phase"] = json!("failed_before_launch");
        self.value["startup"]["launch_attempted"] = json!(false);
        self.value["startup"]["original_error"] = json!(code);
        let proof = json!({"no_process_launched":true,"evidence":"sdk_launch_not_invoked"});
        let result = self.write(
            if persistence_fault {
                "faulted"
            } else {
                "stopped"
            },
            !persistence_fault,
            Some(code),
            Some(proof),
        );
        self.armed = result.is_err();
        StartupFailure {
            code,
            safety: if persistence_fault || result.is_err() {
                StartupSafety::PersistenceUncertain
            } else {
                StartupSafety::NoProcessLaunched
            },
            safety_error: result
                .err()
                .or(persistence_fault.then_some("runtime_ownership_persist_failed")),
            runtime_ref: Some(self.reference.clone()),
        }
    }
    pub fn fail_after_launch(
        &mut self,
        code: &'static str,
        cleanup: Result<(), &'static str>,
        persistence_fault: bool,
        owner: Option<Value>,
        proof: Option<Value>,
        stop_error: Option<&'static str>,
    ) -> StartupFailure {
        if let Some(owner) = owner {
            self.merge_owner(owner);
        }
        let verified = cleanup.is_ok() && !persistence_fault;
        self.value["startup"]["phase"] = json!("failed_after_launch");
        self.value["startup"]["launch_attempted"] = json!(true);
        self.value["startup"]["original_error"] = json!(code);
        self.value["startup"]["cleanup_error"] =
            json!(cleanup.as_ref().err().copied().or(stop_error));
        let persisted = self.write(
            if verified { "stopped" } else { "faulted" },
            verified,
            Some(code),
            proof,
        );
        self.armed = persisted.is_err();
        StartupFailure {
            code,
            safety: if persistence_fault || persisted.is_err() {
                StartupSafety::PersistenceUncertain
            } else if verified {
                StartupSafety::CleanupVerified
            } else {
                StartupSafety::CleanupUnverified
            },
            safety_error: persisted
                .err()
                .or(cleanup.err())
                .or(stop_error)
                .or(persistence_fault.then_some("runtime_ownership_persist_failed")),
            runtime_ref: Some(self.reference.clone()),
        }
    }
}
impl Drop for StartupJournal {
    fn drop(&mut self) {
        if self.armed {
            // Never bless cleanup in a destructor. Preserve the last write-ahead
            // phase for explicit recovery, including panics/cancelled futures.
            let _ = self.write("faulted", false, Some("startup_owner_dropped"), None);
        }
    }
}

pub(super) fn prelaunch_proven(
    owner: &Value,
    reference: &str,
    boot: &str,
    directory: &Path,
) -> bool {
    let journal = &owner["startup"];
    journal["version"] == 1
        && journal["runtime_ref"] == reference
        && journal["boot_id"] == boot
        && matches!(
            journal["phase"].as_str(),
            Some("preparing" | "failed_before_launch")
        )
        && journal["launch_attempted"] == false
        && [
            "primary-owner.json",
            "owner.json",
            "cleanup.json",
            "primary-cleanup.json",
        ]
        .iter()
        .all(|file| !directory.join(file).exists())
        && ["primary_pid", "guardian_pid", "runtime_pid"]
            .iter()
            .all(|key| owner[*key].is_null())
}
