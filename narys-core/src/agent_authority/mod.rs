//! LR-10C: Core-only authority. Audit IDs and wire intents are never capabilities.
//! The native Copilot execution gate remains closed: see availability().
pub mod local;
mod sandbox;
mod seccomp;
#[cfg(test)]
mod tests;
use crate::agents::authority::*;
use crate::persistence::database::Database;
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const POLICY_VERSION: u64 = 1;
const MAX_TTL: Duration = Duration::from_secs(300);
type Result<T> = std::result::Result<T, &'static str>;

// Intentionally neither Deserialize, Serialize, Clone nor Debug. Never sent to
// the provider, IPC, environment, disk or observer. Receipt ID is a different value.
pub(crate) struct AgentAuthority([u8; 32]);
struct Grant {
    context: AgentOperationContext,
    digest: String,
    deadline: Instant,
    approval_id: String,
    cancelled: bool,
    financial_deadline: Instant,
    boundary_deadline: Instant,
    workspace_identity: (u64, u64),
}
// Independently issued, sealed Core proofs. The offline local issuer requires
// operational OS preflight; native Copilot has no admitted issuer.
struct ExecutionBoundary {
    binding: String,
    deadline: Instant,
}
struct FinancialAdmission {
    binding: String,
    deadline: Instant,
    paid_use_allowed: bool,
}
struct ExecutionClaim {
    id: String,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    workspace_identity: (u64, u64),
}
struct State {
    grants: BTreeMap<[u8; 32], Grant>,
    claims: BTreeMap<String, (u64, Arc<AtomicBool>)>,
    yolo: Option<YoloConsent>,
    cancelled_tasks: BTreeSet<u64>,
    admission_closed: bool,
}
/// Core-owned non-wire proof. Only the separate offline operator endpoint may
/// issue it after namespace exclusion is proved. Ordinary/native IPC never does.
struct HumanChannel {
    epoch: String,
}
struct YoloConsent {
    context: AgentOperationContext,
    deadline: Instant,
}
/// Core-only grants cannot be decoded or fabricated through a wire contract.
/// ```compile_fail
/// let _ = narys_core::agent_authority::AgentAuthority([0; 32]);
/// ```
/// ```compile_fail
/// let _: narys_core::agent_authority::AgentAuthority = serde_json::from_str("{}").unwrap();
/// ```
pub struct AuthorityService {
    database: Database,
    epoch: String,
    state: Mutex<State>,
}
fn random() -> Result<[u8; 32]> {
    let mut bytes = [0; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| "authority_entropy_unavailable")?;
    Ok(bytes)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn now() -> Result<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "authority_clock_unavailable")?
        .as_secs();
    i64::try_from(seconds).map_err(|_| "authority_clock_unavailable")
}
fn connection(database: &Database) -> Result<Connection> {
    database.open().map_err(|e| e.code())
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
}
fn validate_workspace(root: &Path) -> Result<(u64, u64)> {
    if !root.is_absolute()
        || root
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("workspace_invalid");
    }
    let canonical = root.canonicalize().map_err(|_| "workspace_unavailable")?;
    if canonical != root || root == Path::new("/") {
        return Err("workspace_alias_denied");
    }
    let metadata = std::fs::symlink_metadata(root).map_err(|_| "workspace_unavailable")?;
    if !metadata.is_dir() {
        return Err("workspace_invalid");
    }
    Ok((metadata.dev(), metadata.ino()))
}
fn validate_path(root: &Path, relative: &Path, allow_new: bool) -> Result<()> {
    let components: Vec<_> = relative.components().collect();
    if components.is_empty()
        || components.len() > 64
        || !components.iter().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("path_scope_denied");
    }
    let mut path = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        path.push(component.as_os_str());
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() || (m.is_file() && m.nlink() != 1) => {
                return Err("path_alias_denied")
            }
            Ok(m) if index + 1 < components.len() && !m.is_dir() => {
                return Err("path_scope_denied")
            }
            Ok(m) if index + 1 == components.len() && !m.is_file() => {
                return Err("path_scope_denied")
            }
            Ok(_) => (),
            Err(e)
                if allow_new
                    && index + 1 == components.len()
                    && e.kind() == std::io::ErrorKind::NotFound =>
            {
                ()
            }
            Err(_) => return Err("path_unavailable"),
        }
    }
    Ok(())
}
fn binding(context: &AgentOperationContext) -> Result<String> {
    if context.task_id == 0
        || context.task_id > i64::MAX as u64
        || !identifier(&context.session_id)
        || !identifier(&context.specialist_id)
        || context.policy_version != POLICY_VERSION
    {
        return Err("authority_context_invalid");
    }
    let identity = validate_workspace(&context.workspace)?;
    // These are Core-mediated tool contracts, not presumed native CLI names.
    match (&context.operation, context.tool.as_str()) {
        (AgentOperation::Read { relative_path }, "narys.read") => {
            validate_path(&context.workspace, relative_path, false)?
        }
        (
            AgentOperation::Write {
                relative_path,
                content,
            },
            "narys.write",
        ) => {
            if content.len() > 64 * 1024 {
                return Err("arguments_limit");
            }
            validate_path(&context.workspace, relative_path, true)?;
        }
        (AgentOperation::Delete { relative_path }, "narys.delete") => {
            validate_path(&context.workspace, relative_path, false)?
        }
        (AgentOperation::Command { program, arguments }, "narys.command") => {
            if !program.is_absolute()
                || arguments.len() > 128
                || arguments.iter().any(|a| a.len() > 4096 || a.contains('\0'))
            {
                return Err("arguments_invalid");
            }
        }
        (AgentOperation::Git { arguments }, "narys.git")
            if arguments.len() <= 128
                && arguments
                    .iter()
                    .all(|a| a.len() <= 4096 && !a.contains('\0')) =>
        {
            ()
        }
        (AgentOperation::Network { .. }, "narys.network") => {
            return Err("network_boundary_unavailable")
        }
        _ => return Err("unknown_or_mismatched_tool"),
    }
    let bytes =
        serde_json::to_vec(&(context, identity)).map_err(|_| "authority_context_invalid")?;
    if bytes.len() > 128 * 1024 {
        return Err("arguments_limit");
    }
    Ok(hex(&Sha256::digest(bytes)))
}
fn summary(context: &AgentOperationContext) -> Value {
    // No provider prose, argv, file contents, URLs or arbitrary risk descriptions.
    let (action, risk) = match context.operation {
        AgentOperation::Read { .. } => (
            "read workspace file",
            "private workspace data may be disclosed",
        ),
        AgentOperation::Write { .. } => ("write workspace file", "workspace contents will change"),
        AgentOperation::Delete { .. } => {
            ("delete workspace file", "workspace file will be removed")
        }
        AgentOperation::Command { .. } => (
            "run workspace command",
            "command and descendants may change workspace; network denied",
        ),
        AgentOperation::Git { .. } => (
            "run workspace Git operation",
            "repository state may change; network denied",
        ),
        AgentOperation::Network { .. } => ("network operation", "external communication"),
    };
    let workspace = context
        .workspace
        .to_string_lossy()
        .chars()
        .take(512)
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect::<String>();
    json!({"action":action,"tool":context.tool,"workspace":workspace,"effect_scope":"approved workspace only; exact arguments bound by digest","risk":risk,"arguments_disclosed":false})
}
fn event(conn: &Connection, id: &str, state: &str, reason: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO agent_approval_events(approval_id,state,reason) VALUES(?1,?2,?3)",
        params![id, state, reason],
    )
    .map_err(|_| "approval_persist_failed")?;
    conn.execute("INSERT INTO server_events(namespace,task_id,code,details_json) SELECT 'product',task_id,'agent_approval_decision',json_object('approval_id',approval_id,'state',?2,'reason',?3) FROM agent_approvals WHERE approval_id=?1", params![id,state,reason]).map_err(|_| "approval_persist_failed")?;
    conn.execute("DELETE FROM server_events WHERE sequence <= (SELECT coalesce(max(sequence),0)-4096 FROM server_events)", []).map_err(|_| "approval_persist_failed")?;
    Ok(())
}
fn expire(conn: &Connection) -> Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|_| "approval_persist_failed")?;
    transition_many(&tx, "expires_at<=?1", now()?, "expired", "approval_timeout")?;
    tx.commit().map_err(|_| "approval_persist_failed")
}
fn transition_many(
    conn: &Connection,
    condition: &'static str,
    value: i64,
    state: &'static str,
    reason: &'static str,
) -> Result<()> {
    // Conditions are private static SQL from Core, never wire/model strings.
    let mut statement=conn.prepare(&format!("SELECT approval_id FROM agent_approvals WHERE state IN ('pending','approved') AND ({condition})")).map_err(|_|"approval_persist_failed")?;
    let ids = statement
        .query_map([value], |r| r.get::<_, String>(0))
        .map_err(|_| "approval_persist_failed")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| "approval_persist_failed")?;
    drop(statement);
    for id in ids {
        conn.execute("UPDATE agent_approvals SET state=?2,reason=?3,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE approval_id=?1",params![id,state,reason]).map_err(|_|"approval_persist_failed")?;
        event(conn, &id, state, reason)?;
    }
    Ok(())
}

impl AuthorityService {
    pub fn new(database: Database) -> Result<Self> {
        Ok(Self {
            database,
            epoch: hex(&random()?),
            state: Mutex::new(State {
                grants: BTreeMap::new(),
                claims: BTreeMap::new(),
                yolo: None,
                cancelled_tasks: BTreeSet::new(),
                admission_closed: false,
            }),
        })
    }
    pub fn availability(&self) -> Value {
        let consent_live = self
            .state
            .lock()
            .map(|s| s.yolo.as_ref().is_some_and(|y| y.deadline > Instant::now()))
            .unwrap_or(false);
        json!({"policy_version":POLICY_VERSION,"default_profile":"assisted","authority":"opaque_core_memory_only","approvals_query":true,"approve_once":"BLOCKED","human_channel":"unavailable_native_runtime_separation_not_proven","assisted_execution":"BLOCKED","isolated_execution":"BLOCKED","isolated_reason":"native_copilot_transport_credentials_and_full_tool_boundary_not_verified","sandbox_diagnostics":"explicit_local_synthetic_tests_only","yolo_contract":"implemented_execution_disabled","yolo_consent_live":consent_live,"yolo_execution":"BLOCKED","financial_authorization":false,"native_tools":false,"broker_mediation":false})
    }
    // Core admission only. Neither proof has a wire deserializer or an agent
    // constructor. No production issuer opens these gates in C.
    fn request(
        &self,
        context: AgentOperationContext,
        ttl: Duration,
        boundary: Option<&ExecutionBoundary>,
        financial_admission: Option<&FinancialAdmission>,
    ) -> Result<(String, AgentAuthority)> {
        let mut state = self.state.lock().map_err(|_| "authority_faulted")?;
        if state.admission_closed || state.cancelled_tasks.contains(&context.task_id) {
            return Err("authority_cancelled");
        }
        state
            .grants
            .retain(|_, g| !g.cancelled && g.deadline > Instant::now());
        let boundary = boundary.ok_or("execution_boundary_unavailable")?;
        let financial = financial_admission.ok_or("financial_admission_required")?;
        if context.profile != AgentApprovalPolicy::Assisted {
            return Err("autonomy_profile_unavailable");
        }
        if ttl < Duration::from_secs(1) || ttl > MAX_TTL || state.grants.len() >= 128 {
            return Err("authority_limit");
        }
        let digest = binding(&context)?;
        let workspace_identity = validate_workspace(&context.workspace)?;
        if boundary.binding != digest || boundary.deadline <= Instant::now() {
            return Err("execution_boundary_unavailable");
        }
        if financial.binding != digest
            || financial.deadline <= Instant::now()
            || financial.paid_use_allowed
        {
            return Err("financial_admission_required");
        }
        let id = format!("ap-{}", hex(&random()?));
        let authority = AgentAuthority(random()?);
        let mut conn = connection(&self.database)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "approval_persist_failed")?;
        tx.execute("INSERT INTO agent_approvals(approval_id,epoch,task_id,session_id,specialist_id,profile,policy_version,binding_digest,summary_json,expires_at,state,reason) VALUES(?1,?2,?3,?4,?5,'assisted',?6,?7,?8,?9,'pending','human_approval_required')", params![id,self.epoch,context.task_id,context.session_id,context.specialist_id,context.policy_version,digest,summary(&context).to_string(),now()?.checked_add(ttl.as_secs() as i64).ok_or("authority_clock_unavailable")?]).map_err(|_| "approval_persist_failed")?;
        event(&tx, &id, "pending", "human_approval_required")?;
        tx.commit().map_err(|_| "approval_persist_failed")?;
        state.grants.insert(
            authority.0,
            Grant {
                context,
                digest,
                deadline: Instant::now() + ttl,
                approval_id: id.clone(),
                cancelled: false,
                financial_deadline: financial.deadline,
                boundary_deadline: boundary.deadline,
                workspace_identity,
            },
        );
        Ok((id, authority))
    }
    fn approve(&self, id: &str, channel: &HumanChannel, expected_digest: &str) -> Result<()> {
        let state = self.state.lock().map_err(|_| "authority_faulted")?;
        if channel.epoch != self.epoch {
            return Err("untrusted_approval_channel");
        }
        let grant = state
            .grants
            .values()
            .find(|g| g.approval_id == id)
            .ok_or("approval_not_live")?;
        if grant.cancelled || grant.deadline <= Instant::now() {
            return Err("approval_not_live");
        }
        if grant.digest != expected_digest || binding(&grant.context)? != expected_digest {
            return Err("approval_context_mismatch");
        }
        let mut conn = connection(&self.database)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "approval_persist_failed")?;
        if tx.execute("UPDATE agent_approvals SET state='approved',reason='human_approve_once',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE approval_id=?1 AND epoch=?2 AND binding_digest=?3 AND state='pending' AND expires_at>?4", params![id,self.epoch,expected_digest,now()?]).map_err(|_| "approval_persist_failed")? != 1 { return Err("approval_not_pending"); }
        event(&tx, id, "approved", "human_approve_once")?;
        tx.commit().map_err(|_| "approval_persist_failed")
    }
    // Test adapter only: operational effects use claim + supervised local runner.
    #[cfg(test)]
    fn execute<T>(
        &self,
        authority: &AgentAuthority,
        context: &AgentOperationContext,
        effect: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let claim = self.claim(authority, context)?;
        if claim.cancelled.load(Ordering::Acquire) {
            return Err("authority_cancelled");
        }
        let result = effect();
        self.state
            .lock()
            .map_err(|_| "authority_faulted")?
            .claims
            .remove(&claim.id);
        if claim.cancelled.load(Ordering::Acquire) {
            Err("authority_cancelled")
        } else {
            result
        }
    }
    fn claim(
        &self,
        authority: &AgentAuthority,
        context: &AgentOperationContext,
    ) -> Result<ExecutionClaim> {
        let mut state = self.state.lock().map_err(|_| "authority_faulted")?;
        let grant = state.grants.get(&authority.0).ok_or("authority_unknown")?;
        if grant.cancelled {
            return Err("authority_cancelled");
        }
        if grant.deadline <= Instant::now() {
            return Err("authority_expired");
        }
        if grant.boundary_deadline <= Instant::now() || grant.financial_deadline <= Instant::now() {
            return Err("authority_gate_closed");
        }
        if &grant.context != context
            || grant.digest != binding(context)?
            || validate_workspace(&context.workspace)? != grant.workspace_identity
        {
            return Err("authority_context_mismatch");
        }
        let mut conn = connection(&self.database)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "approval_persist_failed")?;
        if tx.execute("UPDATE agent_approvals SET state='consumed',reason='execution_claimed',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE approval_id=?1 AND epoch=?2 AND binding_digest=?3 AND state='approved' AND expires_at>?4", params![grant.approval_id,self.epoch,grant.digest,now()?]).map_err(|_| "approval_persist_failed")? != 1 { return Err("approval_not_approved_or_consumed"); }
        event(&tx, &grant.approval_id, "consumed", "execution_claimed")?;
        let local:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_local_tasks WHERE task_id=?1 AND session_id=?2 AND workspace=?3 AND epoch=?4 AND state='active')",params![context.task_id,context.session_id,context.workspace.to_str(),self.epoch],|r|r.get(0)).map_err(|_|"claim_journal_failed")?;
        if local {
            tx.execute("INSERT INTO agent_tool_executions(approval_id,task_id,phase) VALUES(?1,?2,'claimed')",params![grant.approval_id,context.task_id]).map_err(|_|"claim_journal_failed")?;
            tx.execute(
                "INSERT INTO agent_execution_events(approval_id,phase) VALUES(?1,'claimed')",
                [&grant.approval_id],
            )
            .map_err(|_| "claim_journal_failed")?;
        }
        tx.commit().map_err(|_| "approval_persist_failed")?;
        let deadline = grant
            .deadline
            .min(grant.boundary_deadline)
            .min(grant.financial_deadline);
        let workspace_identity = grant.workspace_identity;
        let id = grant.approval_id.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        state.grants.remove(&authority.0);
        state
            .claims
            .insert(id.clone(), (context.task_id, cancelled.clone()));
        // No global mutex survives the claim. Consumed != started/success.
        Ok(ExecutionClaim {
            id,
            cancelled,
            deadline,
            workspace_identity,
        })
    }
    pub fn deny(&self, id: &str) -> Result<Value> {
        validate_id(id)?;
        let mut guard = self.state.lock().map_err(|_| "authority_faulted")?;
        let mut conn = connection(&self.database)?;
        expire(&conn)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "approval_persist_failed")?;
        let changed = tx.execute("UPDATE agent_approvals SET state='denied',reason='explicit_deny',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE approval_id=?1 AND state IN ('pending','approved')", [id]).map_err(|_| "approval_persist_failed")?;
        if changed == 1 {
            event(&tx, id, "denied", "explicit_deny")?;
        }
        tx.commit().map_err(|_| "approval_persist_failed")?;
        guard.grants.retain(|_, g| g.approval_id != id);
        self.get_unlocked(id)
    }
    pub fn approve_from_ipc(&self, _id: &str) -> Result<Value> {
        // Same UID, TTY, SSH or a declared origin cannot mint HumanChannel.
        Err("human_approval_channel_unavailable")
    }
    pub fn cancel_task(&self, task: u64) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| "authority_faulted")?;
        if state.cancelled_tasks.len() >= 4096 {
            state.admission_closed = true;
        } else {
            state.cancelled_tasks.insert(task);
        }
        for g in state
            .grants
            .values_mut()
            .filter(|g| g.context.task_id == task)
        {
            g.cancelled = true;
        }
        state.grants.retain(|_, g| g.context.task_id != task);
        for (_, flag) in state.claims.values().filter(|(t, _)| *t == task) {
            flag.store(true, Ordering::Release);
        }
        if state
            .yolo
            .as_ref()
            .is_some_and(|y| y.context.task_id == task)
        {
            state.yolo = None;
        }
        let mut conn = connection(&self.database)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "approval_persist_failed")?;
        transition_many(
            &tx,
            "task_id=?1",
            i64::try_from(task).map_err(|_| "authority_context_invalid")?,
            "cancelled",
            "task_cancelled",
        )?;
        tx.commit().map_err(|_| "approval_persist_failed")
    }
    pub fn get(&self, id: &str) -> Result<Value> {
        let _guard = self.state.lock().map_err(|_| "authority_faulted")?;
        self.get_unlocked(id)
    }
    fn get_unlocked(&self, id: &str) -> Result<Value> {
        validate_id(id)?;
        let conn = connection(&self.database)?;
        expire(&conn)?;
        conn.query_row("SELECT sequence,task_id,session_id,specialist_id,profile,policy_version,binding_digest,summary_json,expires_at,state,reason FROM agent_approvals WHERE approval_id=?1", [id], |r| {
            let summary: String = r.get(7)?;
            Ok(json!({"approval_id":id,"sequence":r.get::<_,i64>(0)?,"task_id":r.get::<_,i64>(1)?,"session_id":r.get::<_,String>(2)?,"specialist_id":r.get::<_,String>(3)?,"profile":r.get::<_,String>(4)?,"policy_version":r.get::<_,i64>(5)?,"binding_digest":r.get::<_,String>(6)?,"action":serde_json::from_str::<Value>(&summary).unwrap_or(Value::Null),"expires_at":r.get::<_,i64>(8)?,"state":r.get::<_,String>(9)?,"reason":r.get::<_,String>(10)?,"authority":false}))
        }).map_err(|_| "approval_not_found")
    }
    pub fn list(&self, after: u64, limit: u16, pending_only: bool) -> Result<Value> {
        if after > i64::MAX as u64 || limit == 0 || limit > 100 {
            return Err("invalid_approval_page");
        }
        let _guard = self.state.lock().map_err(|_| "authority_faulted")?;
        let conn = connection(&self.database)?;
        expire(&conn)?;
        let mut statement = conn.prepare("SELECT approval_id,sequence FROM agent_approvals WHERE sequence>?1 AND (?3=0 OR state='pending') ORDER BY sequence LIMIT ?2").map_err(|_| "approval_read_failed")?;
        let rows = statement
            .query_map(params![after, limit as u32 + 1, pending_only], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
            })
            .map_err(|_| "approval_read_failed")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| "approval_read_failed")?;
        let has_more = rows.len() > limit as usize;
        let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();
        let next = rows.last().map_or(after, |r| r.1);
        let approvals = rows
            .iter()
            .map(|r| self.get_unlocked(&r.0))
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"approvals":approvals,"next_sequence":next,"has_more":has_more,"human_channel":"unavailable","history_replays_effects":false}),
        )
    }
    fn activate_yolo(
        &self,
        context: AgentOperationContext,
        channel: &HumanChannel,
        ttl: Duration,
        acknowledge_unisolated: bool,
        managed_requires_approval: bool,
    ) -> Result<()> {
        if channel.epoch != self.epoch || !acknowledge_unisolated {
            return Err("explicit_human_yolo_consent_required");
        }
        let mut state = self.state.lock().map_err(|_| "authority_faulted")?;
        if state.admission_closed || state.cancelled_tasks.contains(&context.task_id) {
            return Err("authority_cancelled");
        }
        if ttl.is_zero() || ttl > MAX_TTL || context.profile != AgentApprovalPolicy::ExplicitYolo {
            return Err("yolo_scope_invalid");
        }
        if managed_requires_approval {
            return Err("managed_policy_requires_approval");
        }
        binding(&context)?;
        state.yolo = Some(YoloConsent {
            context,
            deadline: Instant::now() + ttl,
        });
        Ok(())
    }
    pub fn revoke_all(&self) -> Result<()> {
        let tasks: Vec<_> = {
            let mut state = self.state.lock().map_err(|_| "authority_faulted")?;
            state.admission_closed = true;
            state.yolo = None;
            // Signal every live effect before any fallible persistence. A
            // failed first revocation must not leave later tools running.
            for (_, flag) in state.claims.values() {
                flag.store(true, Ordering::Release);
            }
            for grant in state.grants.values_mut() {
                grant.cancelled = true;
            }
            state
                .grants
                .values()
                .map(|g| g.context.task_id)
                .chain(state.claims.values().map(|(id, _)| *id))
                .collect()
        };
        let mut failure = None;
        for task in tasks {
            if let Err(e) = self.cancel_task(task) {
                failure.get_or_insert(e);
            }
        }
        failure.map_or(Ok(()), Err)
    }
    pub fn revoke_yolo(&self) -> Result<()> {
        self.state.lock().map_err(|_| "authority_faulted")?.yolo = None;
        Ok(())
    }
}
pub fn validate_id(id: &str) -> Result<()> {
    if id.len() != 67
        || !id.starts_with("ap-")
        || !id[3..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("invalid_approval_id");
    }
    Ok(())
}
pub fn recover(conn: &mut Connection) -> Result<()> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "approval_persist_failed")?;
    transition_many(&tx, "?1=0", 0, "interrupted", "restart_revokes_authority")?;
    tx.commit().map_err(|_| "approval_persist_failed")
}
