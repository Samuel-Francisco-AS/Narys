//! Core's SpecialistAgent adapter and durable lifecycle service (LR-10B).
pub mod sdk;
pub mod sdk_policy;
pub mod store;
pub mod supervisor;
use crate::{
    agents::{
        backend::{AgentBackend, AgentFuture},
        lifecycle::*,
        types::*,
    },
    cognition::task_graph::TaskGraph,
    luna::runtime::TaskRegistry,
    persistence::database::Database,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use supervisor::*;

struct RunControl {
    cancel: Arc<Cancellation>,
    terminal: Mutex<bool>,
    gaps: AtomicU64,
}
pub struct CopilotAgentAdapter {
    pub lifecycle: Arc<CopilotLifecycle>,
}
impl CopilotAgentAdapter {
    pub fn config() -> AgentConfig {
        // AgentRegistry's legacy capability set describes planner routing. A
        // textual Copilot session is not silently promoted to a PlanV1 planner.
        AgentConfig {
            id: "copilot".into(),
            enabled: false,
            priority: 2,
            capabilities: AgentCapabilities::default(),
        }
    }
    pub fn capabilities() -> SpecialistCapabilities {
        SpecialistCapabilities {
            textual: true,
            tools: false,
            permissions_integrated: false,
            inference_admission_integrated: false,
        }
    }
}
impl AgentBackend for CopilotAgentAdapter {
    fn execute<'a>(
        &'a self,
        request: &'a AgentRequest,
        cancelled: &'a AtomicBool,
        _events: &'a mut (dyn FnMut(AgentEvent) -> Result<(), AgentError> + Send),
    ) -> AgentFuture<'a> {
        Box::pin(async move {
            if cancelled.load(Ordering::Acquire) {
                return Err(AgentError::Cancelled);
            }
            if !Self::config()
                .capabilities
                .supports(&request.required_capabilities)
            {
                return Err(AgentError::UnsupportedCapability);
            }
            if request.objective.trim().is_empty() || request.objective.len() > 4096 {
                return Err(AgentError::InvalidRequest);
            }
            // No wire flag, old LR-10A receipt, profile or text grants a new send.
            Err(AgentError::Unavailable)
        })
    }
}
pub struct CopilotLifecycle {
    pub supervisor: Arc<AgentRuntimeSupervisor>,
    database: Database,
    tasks: Arc<TaskRegistry>,
    root: PathBuf,
    controls: Mutex<BTreeMap<u64, Arc<RunControl>>>,
    attachments: Mutex<BTreeMap<String, AgentSessionRef>>,
    closed: AtomicBool,
    recovering: AtomicBool,
}
fn opaque() -> Result<String, &'static str> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| "agent_id_unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn graph(operation: AgentLifecycleOperation) -> TaskGraph {
    use crate::agents::plan_contract::*;
    TaskGraph::compile(&PlanV1 {
        version: 1,
        objective: format!("{operation:?} specialist session lifecycle"),
        steps: vec![PlanStepV1 {
            id: "lifecycle".into(),
            description: "SDK session operation; no inference or tools".into(),
            required_capabilities: vec![PlanCapability::Planning],
            depends_on: vec![],
        }],
        risks: vec![],
        needs_user_input: false,
        questions: vec![],
    })
    .expect("fixed lifecycle graph")
}
impl CopilotLifecycle {
    pub fn new(
        database: Database,
        tasks: Arc<TaskRegistry>,
        root: PathBuf,
        factory: Arc<dyn RuntimeFactory>,
    ) -> Result<Arc<Self>, &'static str> {
        tasks.seed_next_id(store::max_id(&database.open().map_err(|e| e.code())?)?);
        Ok(Arc::new(Self {
            supervisor: AgentRuntimeSupervisor::new(factory),
            database,
            tasks,
            root,
            controls: Mutex::new(BTreeMap::new()),
            attachments: Mutex::new(BTreeMap::new()),
            closed: AtomicBool::new(false),
            recovering: AtomicBool::new(false),
        }))
    }
    pub fn status(&self) -> Value {
        json!({"specialist_id":"copilot","runtime":self.supervisor.snapshot(),
        "capabilities":CopilotAgentAdapter::capabilities(),"registered":true,"planner_routing_enabled":false,
        "process_observation":sdk::process_snapshot(&self.database),
        "active_tasks":self.controls.lock().unwrap().len(),"attachments":self.attachments.lock().unwrap().len(),
        "inference_gate":"not_integrated_no_send","tools_gate":"LR-10C_required"})
    }
    pub fn session(&self, reference: &AgentSessionRef) -> Result<Value, &'static str> {
        let mut v = store::session(&self.database.open().map_err(|e| e.code())?, reference)?;
        v["attachments"] = json!(self
            .attachments
            .lock()
            .unwrap()
            .values()
            .filter(|r| *r == reference)
            .count());
        Ok(v)
    }
    pub fn attach(&self, reference: &AgentSessionRef) -> Result<Value, &'static str> {
        reference.validate()?;
        self.session(reference)?;
        let mut attachments = self.attachments.lock().unwrap();
        if attachments.len() >= 32 {
            return Err("agent_attachment_limit");
        }
        let id = format!("ca-{}", opaque()?);
        attachments.insert(id.clone(), reference.clone());
        Ok(json!({"attachment_id":id,"session_ref":reference,"task_lifetime_independent":true}))
    }
    pub fn detach(&self, id: &str) -> Value {
        let detached = self.attachments.lock().unwrap().remove(id).is_some();
        json!({"detached":detached,"task_cancelled":false})
    }
    pub fn close(&self, reference: &AgentSessionRef) -> Result<Value, &'static str> {
        let conn = self.database.open().map_err(|e| e.code())?;
        let session = store::session(&conn, reference)?;
        if session["active_task_id"].is_u64() {
            return Err("agent_session_busy");
        }
        conn.execute("UPDATE agent_sessions SET state='closed',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE session_ref=?1 AND active_task_id IS NULL",[&reference.0]).map_err(|_|"agent_write_failed")?;
        self.attachments
            .lock()
            .unwrap()
            .retain(|_, r| r != reference);
        self.session(reference)
    }
    pub fn admit(
        self: &Arc<Self>,
        operation: AgentLifecycleOperation,
        reference: Option<AgentSessionRef>,
    ) -> Result<Value, &'static str> {
        let mut controls = self.controls.lock().unwrap();
        if self.closed.load(Ordering::Acquire) {
            return Err("supervisor_stopping");
        }
        if self.recovering.load(Ordering::Acquire) {
            return Err("agent_runtime_recovering");
        }
        if controls.len() >= 2 {
            return Err("agent_concurrency_limit");
        }
        let id = self
            .tasks
            .reserve_background_id()
            .map_err(|_| "task_id_exhausted")?
            .0;
        let (reference, directory) = match operation {
            AgentLifecycleOperation::Create => {
                if reference.is_some() {
                    return Err("invalid_session_invocation");
                }
                let reference = AgentSessionRef(format!("cs-{}", opaque()?));
                crate::server::mkdir(&self.root)?;
                let directory = self.root.join(&reference.0);
                crate::server::mkdir(&directory)?;
                (reference, directory)
            }
            AgentLifecycleOperation::Resume => {
                let reference = reference.ok_or("agent_session_ref_required")?;
                reference.validate()?;
                let directory =
                    store::directory(&self.database.open().map_err(|e| e.code())?, &reference)?;
                if directory.parent() != Some(self.root.as_path())
                    || directory.file_name() != Some(std::ffi::OsStr::new(&reference.0))
                {
                    return Err("session_directory_mismatch");
                }
                (reference, directory)
            }
        };
        let correlation = format!("copilot-{id}-{}", opaque()?);
        let (provider, anchor) = store::admit(
            &self.database,
            id,
            &reference,
            operation,
            &directory,
            &correlation,
        )?;
        let control = Arc::new(RunControl {
            cancel: Arc::default(),
            terminal: Mutex::new(false),
            gaps: AtomicU64::new(0),
        });
        controls.insert(id, control.clone());
        drop(controls);
        let this = self.clone();
        let corr = correlation.clone();
        tokio::spawn(async move {
            this.execute(id, operation, directory, provider, anchor, corr, control)
                .await;
        });
        Ok(
            json!({"task_id":id,"namespace":"product","specialist_id":"copilot","session_ref":reference,
            "correlation_id":correlation,"state":"pending","inference":false,"tools":false}),
        )
    }
    fn publish_trace(&self, id: u64, correlation: &str, code: &'static str, control: &RunControl) {
        use crate::operational_trace::*;
        let draft = EventDraft::new(
            Provenance {
                source: TraceSource {
                    source_type: SourceType::SpecialistAgent,
                    id: TraceId::new("copilot").unwrap(),
                    instance: None,
                },
                task_id: Some(crate::TaskId(id)),
                subtask_id: Some(TraceId::new("lifecycle").unwrap()),
                correlation_id: TraceId::new(correlation).ok(),
                coalescing_key: None,
            },
            OperationalKind::State {
                kind: StateKind::SubtaskLifecycle,
                code: TraceId::new(code).unwrap(),
                detail: TraceText::new("specialist lifecycle; no inference/tools").unwrap(),
            },
        );
        if draft
            .and_then(|draft| OperationalTraceBus::process_wide().publish(draft))
            .is_err()
        {
            control.gaps.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn observe(&self, id: u64, correlation: &str, code: &'static str, control: &RunControl) {
        self.publish_trace(id, correlation, code, control);
        if matches!(code, "runtime_ready" | "session_starting") {
            let reference = self.supervisor.snapshot().runtime_ref;
            if self
                .database
                .open()
                .map_err(|e| e.code())
                .and_then(|conn| {
                    conn.execute(
                        "UPDATE agent_runs SET runtime_ref=?2 WHERE task_id=?1",
                        rusqlite::params![id, reference],
                    )
                    .map_err(|_| "agent_write_failed")
                })
                .is_err()
            {
                control.gaps.fetch_add(1, Ordering::Relaxed);
            }
        }
        if self
            .database
            .open()
            .map_err(|e| e.code())
            .and_then(|conn| store::event(&conn, id, correlation, code))
            .is_err()
        {
            control.gaps.fetch_add(1, Ordering::Relaxed);
        }
    }
    async fn execute(
        self: Arc<Self>,
        id: u64,
        operation: AgentLifecycleOperation,
        directory: PathBuf,
        provider: Option<String>,
        anchor: Option<String>,
        correlation: String,
        control: Arc<RunControl>,
    ) {
        let mut graph = graph(operation);
        graph.mark_running("lifecycle").unwrap();
        let running=self.database.open().map_err(|e|e.code()).and_then(|conn|
            conn.execute("UPDATE agent_runs SET state='running',graph_state='running' WHERE task_id=?1 AND state='pending'",[id]).map_err(|_|"agent_write_failed"));
        let outcome = if running.is_err() {
            RuntimeOutcome {
                result: Err("agent_write_failed"),
                cleanup_verified: true,
                runtime_ref: None,
            }
        } else {
            let observer = self.clone();
            let c = control.clone();
            let corr = correlation.clone();
            self.supervisor
                .run(
                    SessionInvocation {
                        operation,
                        directory,
                        provider_session_id: provider,
                        expected_history_anchor: anchor,
                    },
                    control.cancel.clone(),
                    Arc::new(move |code| observer.observe(id, &corr, code, &c)),
                )
                .await
        };
        // A single lock linearizes cancel versus terminal commit. Cleanup errors
        // remain failures even when a cancellation arrived concurrently.
        let mut terminal = control.terminal.lock().unwrap();
        let mut result = outcome.result;
        if control.cancel.is_cancelled() && result.is_ok() {
            result = Err("cancelled");
        }
        let (state, code, provider, gaps, anchor) = match result {
            Ok(receipt) => {
                graph.mark_completed("lifecycle").unwrap();
                (
                    "completed",
                    None,
                    Some(receipt.provider_session_id),
                    receipt.observation_gaps,
                    receipt.history_anchor,
                )
            }
            Err("cancelled") => {
                graph.cancel_unfinished();
                ("cancelled", Some("cancelled"), None, 0, None)
            }
            Err(code) => {
                graph.mark_failed("lifecycle").unwrap();
                ("failed", Some(code), None, 0, None)
            }
        };
        let graph_state = match state {
            "completed" => "completed",
            "cancelled" => "cancelled",
            _ => "failed",
        };
        let persisted = store::finish(
            &self.database,
            id,
            state,
            code,
            provider.as_deref(),
            outcome.cleanup_verified,
            gaps + control.gaps.load(Ordering::Relaxed),
            graph_state,
            &correlation,
            outcome.runtime_ref.as_deref(),
            anchor.as_deref(),
        );
        *terminal = true;
        drop(terminal);
        if persisted.is_ok() {
            let before = control.gaps.load(Ordering::Relaxed);
            self.publish_trace(
                id,
                &correlation,
                match state {
                    "completed" => "agent_completed",
                    "cancelled" => "agent_cancelled",
                    _ => "agent_failed",
                },
                &control,
            );
            let extra = control.gaps.load(Ordering::Relaxed).saturating_sub(before);
            if extra > 0 {
                let _=self.database.open().map(|conn|conn.execute("UPDATE agent_runs SET observation_gaps=observation_gaps+?2 WHERE task_id=?1",rusqlite::params![id,extra]));
            }
        }
        if persisted.is_err() {
            // Preserve uncertain authoritative running state for restart recovery;
            // close admission rather than duplicate an operation or claim success.
            self.closed.store(true, Ordering::Release);
            self.supervisor.close_admission();
        }
        self.controls.lock().unwrap().remove(&id);
    }
    pub fn cancel(&self, id: u64) -> Result<Value, &'static str> {
        let controls = self.controls.lock().unwrap();
        if let Some(control) = controls.get(&id) {
            let terminal = control.terminal.lock().unwrap();
            if !*terminal {
                if !control.cancel.is_cancelled() {
                    control.cancel.cancel();
                    let correlation = self
                        .database
                        .open()
                        .ok()
                        .and_then(|conn| store::task(&conn, id).ok())
                        .and_then(|v| v["correlation_id"].as_str().map(str::to_owned));
                    if let Some(correlation) = correlation {
                        self.observe(id, &correlation, "agent_cancellation_requested", control);
                    } else {
                        control.gaps.fetch_add(1, Ordering::Relaxed);
                    }
                }
                return Ok(
                    json!({"task_id":id,"namespace":"product","cancellation_requested":true,"already_terminal":false}),
                );
            }
        }
        let task = store::task(&self.database.open().map_err(|e| e.code())?, id)?;
        Ok(
            json!({"task_id":id,"namespace":"product","state":task["state"],"already_terminal":true,"cancellation_requested":task["state"]=="cancelled"}),
        )
    }
    pub async fn recover_runtime_ownership(&self) -> Result<Value, &'static str> {
        // Excludes admission for this bounded reconciliation. It is not a restart.
        {
            let controls = self.controls.lock().unwrap();
            if !controls.is_empty() {
                return Err("agent_runtime_busy");
            }
            if self.recovering.swap(true, Ordering::AcqRel) {
                return Err("agent_runtime_recovering");
            }
        }
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = Reset(&self.recovering);
        match tokio::time::timeout(
            std::time::Duration::from_secs(7),
            sdk::recover(&self.database),
        )
        .await
        .unwrap_or(Err("runtime_recovery_timeout"))
        {
            Ok(()) => {
                self.supervisor.acknowledge_recovered_cleanup()?;
                Ok(self.status())
            }
            Err(code) => {
                self.supervisor.fault_cleanup(code);
                Err(code)
            }
        }
    }
    pub fn request_shutdown(&self) {
        let controls = self.controls.lock().unwrap();
        self.closed.store(true, Ordering::Release);
        self.supervisor.close_admission();
        for control in controls.values() {
            control.cancel.cancel();
        }
    }
    pub async fn shutdown(&self) -> Result<(), &'static str> {
        self.request_shutdown();
        self.supervisor.shutdown().await?;
        while !self.controls.lock().unwrap().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        self.attachments.lock().unwrap().clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
