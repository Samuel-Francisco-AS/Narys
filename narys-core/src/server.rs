use crate::{
    agents::plan_contract::{PlanCapability, PlanStepV1, PlanV1},
    agents::{
        backend::{AgentBackend, AgentFuture},
        registry::AgentRegistry,
        types::*,
    },
    cognition::task_graph::TaskGraph,
    ipc::{self, Command, Request, Response, TaskNamespace},
    operational_trace::*,
    persistence::database::Database,
    persistence::ownership::WriterLease,
    policy::{self, TaskInput},
};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::Mutex,
};

pub struct Config {
    pub home: PathBuf,
    pub state: PathBuf,
    pub runtime: PathBuf,
    pub root: PathBuf,
    pub cli: PathBuf,
    pub binary: PathBuf,
    pub database: OnceLock<Database>,
}
pub fn mkdir(path: &Path) -> Result<(), &'static str> {
    if !path.exists() {
        fs::create_dir(path).map_err(|_| "directory_create_failed")?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| "directory_permissions_failed")?;
    }
    policy::private_directory(path)
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<(), &'static str> {
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| "private_file_create_failed")?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| "private_file_write_failed")?;
    fs::File::open(path.parent().ok_or("private_file_parent_missing")?)
        .and_then(|d| d.sync_all())
        .map_err(|_| "private_directory_sync_failed")
}
impl Config {
    pub fn discover() -> Result<Self, &'static str> {
        let home = PathBuf::from(std::env::var("HOME").map_err(|_| "home_unavailable")?);
        let runtime =
            PathBuf::from(std::env::var("XDG_RUNTIME_DIR").map_err(|_| "user_runtime_required")?)
                .join("narys-core");
        let state = home.join(".local/state/narys");
        if !state.exists() {
            fs::create_dir_all(&state).map_err(|_| "state_unavailable")?;
            fs::set_permissions(&state, fs::Permissions::from_mode(0o700))
                .map_err(|_| "state_permissions_failed")?;
        }
        policy::private_directory(&state)?;
        if state.canonicalize().map_err(|_| "unsafe_state_path")? != state {
            return Err("unsafe_state_path");
        }
        let state = state.join("core");
        mkdir(&state)?;
        mkdir(&runtime)?;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cli=home.join(".local/share/fnm/node-versions/v24.18.0/installation/lib/node_modules/@github/copilot/node_modules/@github/copilot-linux-x64/copilot");
        Ok(Self {
            home,
            state,
            runtime,
            root,
            cli,
            binary: std::env::current_exe().map_err(|_| "binary_unavailable")?,
            database: OnceLock::new(),
        })
    }
    pub fn db(&self) -> Result<rusqlite::Connection, &'static str> {
        self.database
            .get_or_init(|| Database::new(self.state.join("db")))
            .open()
            .map_err(|e| e.code())
    }
}
fn agent_config() -> AgentConfig {
    AgentConfig {
        id: "copilot".into(),
        enabled: true,
        priority: 1,
        capabilities: AgentCapabilities {
            planning: true,
            ..Default::default()
        },
    }
}
struct CopilotBackend {
    dir: PathBuf,
    config: Arc<Config>,
}
struct CancelOnDrop(Option<PathBuf>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let Some(dir) = self.0.as_ref() else {
            return;
        };
        let _ = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.join("cancel"));
    }
}
impl AgentBackend for CopilotBackend {
    fn execute<'a>(
        &'a self,
        request: &'a AgentRequest,
        cancelled: &'a AtomicBool,
        on_event: &'a mut (dyn FnMut(AgentEvent) -> Result<(), AgentError> + Send),
    ) -> AgentFuture<'a> {
        Box::pin(async move {
            if cancelled.load(Ordering::Acquire) {
                return Err(AgentError::Cancelled);
            }
            if !agent_config()
                .capabilities
                .supports(&request.required_capabilities)
            {
                return Err(AgentError::UnsupportedCapability);
            }
            let mut guard = CancelOnDrop(Some(self.dir.clone()));
            on_event(AgentEvent::WorkStarted)?;
            let result = owned_run(&self.config, &self.dir, Some(cancelled))
                .await
                .map_err(|_| AgentError::BackendFailed)?;
            guard.0 = None; // Child lifetime ended; no spurious cancellation.
            private_write(
                &self.dir.join("runtime-evidence.json"),
                &serde_json::to_vec_pretty(&result).unwrap(),
            )
            .map_err(|_| AgentError::BackendFailed)?;
            if cancelled.load(Ordering::Acquire) {
                on_event(AgentEvent::Cancelled)?;
                return Err(AgentError::Cancelled);
            }
            if result["cleanup_complete"] != true
                || result["exit_code"] != 0
                || result["configuration_structural_error"] == true
                || result["sdk_report"]["shutdown"] != "graceful"
                || result["sdk_report"]["state"] != "response_received"
            {
                on_event(AgentEvent::Failed)?;
                return Err(AgentError::BackendFailed);
            }
            on_event(AgentEvent::OutputObserved)?;
            let output = result["sdk_report"]["output"]
                .as_str()
                .ok_or(AgentError::Protocol)?
                .to_owned();
            on_event(AgentEvent::Completed)?;
            Ok(AgentResult { output })
        })
    }
}
pub async fn credentials(c: &Config) -> Value {
    crate::credentials::status(c.runtime.parent().unwrap()).await
}
async fn owned_run(
    c: &Config,
    dir: &Path,
    cancelled: Option<&AtomicBool>,
) -> Result<Value, &'static str> {
    let child = tokio::process::Command::new("/usr/bin/python3")
        .env_clear()
        .env("HOME", &c.home)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("XDG_RUNTIME_DIR", c.runtime.parent().unwrap())
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}/bus", c.runtime.parent().unwrap().display()),
        )
        .arg(c.root.join("ops/owned_runtime.py"))
        .arg(&c.binary)
        .arg(dir)
        .arg(&c.cli)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "harness_launch_failed")?;
    // Do not kill the supervisor: its private subreaper must reclaim the CLI.
    let wait = child.wait_with_output();
    tokio::pin!(wait);
    let mut ticks = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            o=&mut wait=>{let o=o.map_err(|_|"harness_wait_failed")?;return serde_json::from_slice(&o.stdout).map_err(|_|"harness_evidence_invalid");},
            _=ticks.tick()=>{if cancelled.is_some_and(|x|x.load(Ordering::Acquire)){let _=fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(dir.join("cancel"));}}
        }
    }
}
fn trace(bus: &OperationalTraceBus, id: u64, code: &str) {
    let draft = EventDraft::new(
        Provenance {
            source: TraceSource {
                source_type: SourceType::SpecialistAgent,
                id: TraceId::new("copilot").unwrap(),
                instance: None,
            },
            task_id: Some(crate::TaskId(id)),
            subtask_id: Some(TraceId::new("specialist").unwrap()),
            correlation_id: None,
            coalescing_key: None,
        },
        OperationalKind::State {
            kind: StateKind::SubtaskLifecycle,
            code: TraceId::new(code).unwrap(),
            detail: TraceText::new("headless specialist lifecycle").unwrap(),
        },
    )
    .unwrap();
    let _ = bus.publish(draft);
}
struct Core {
    config: Arc<Config>,
    active: Mutex<Option<(u64, Arc<AtomicBool>)>>,
    busy: Arc<tokio::sync::Semaphore>,
    trace: Arc<OperationalTraceBus>,
    stopping: AtomicBool,
    services: crate::runtime::RuntimeServices,
}
impl Core {
    async fn dispatch(self: &Arc<Self>, command: Command) -> Result<Value, &'static str> {
        if self.stopping.load(Ordering::Acquire) {
            return Err("server_stopping");
        }
        match command {
            Command::Capabilities {} => Ok(
                json!({"protocol_version":ipc::VERSION,"authority":"narys-core","persistence":"core/db/luna.sqlite3","task_namespaces":["lr10a","product"],"implemented":["status","credentials","events","prepare","result","cancel","conversation","sessions","session-create","session-get","session-resume","session-close","providers","provider-configure","conversation-policy","task-get","task-cancel","tasks","models"],"conversation":true,"provider_configuration":true,"approvals":false,"agent_tools":false,"execution_authority_from_ipc":false}),
            ),
            Command::Models {} => {
                let status = self.services.providers.scheduler.status();
                let models: Vec<_> = crate::cognition::catalog::INTEGRATIONS.iter().map(|i| json!({"provider_id":i.id,"default_model":i.default_model,"registered":status.iter().any(|p|p.id==i.id),"remote_catalog_verified":false})).collect();
                Ok(
                    json!({"models":models,"source":"integrated_local_catalog","remote_probe":false}),
                )
            }
            Command::Tasks {
                namespace,
                after,
                limit,
            } => crate::operations::tasks(&self.config.db()?, namespace, after, limit),
            Command::Events { after, limit } => {
                let db = self.config.db()?;
                let min: Option<u64> = db
                    .query_row("SELECT min(sequence) FROM server_events", [], |r| r.get(0))
                    .map_err(|_| "event_read_failed")?;
                let mut query=db.prepare("SELECT sequence,namespace,task_id,code,created_at,details_json FROM server_events WHERE sequence>?1 ORDER BY sequence LIMIT ?2").map_err(|_|"event_read_failed")?;
                let rows=query.query_map(rusqlite::params![after,limit as u64+1],|r|Ok(json!({"sequence":r.get::<_,u64>(0)?,"namespace":r.get::<_,String>(1)?,"task_id":r.get::<_,Option<u64>>(2)?,"code":r.get::<_,String>(3)?,"created_at":r.get::<_,String>(4)?,"details":r.get::<_,Option<String>>(5)?.and_then(|s|serde_json::from_str::<Value>(&s).ok())}))).map_err(|_|"event_read_failed")?;
                let mut events = rows
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| "event_read_failed")?;
                let has_more = events.len() > limit as usize;
                events.truncate(limit as usize);
                let next = events
                    .last()
                    .and_then(|e| e["sequence"].as_u64())
                    .unwrap_or(after);
                Ok(
                    json!({"events":events,"next_sequence":next,"has_more":has_more,"complete":min.is_none_or(|m|after.saturating_add(1)>=m),"durable":true,"retention_events":4096}),
                )
            }
            Command::TaskGet { task } if matches!(task.namespace, TaskNamespace::Product) => {
                crate::persistence::conversation_runs::get(&self.config.db()?, task.id)
            }
            Command::TaskCancel { task } if matches!(task.namespace, TaskNamespace::Product) => {
                let _admission = self.services.conversation_admission.lock().await;
                let mut conn = self.config.db()?;
                let existing = crate::persistence::conversation_runs::get(&conn, task.id)?;
                let cancelled = self.services.tasks.cancel(crate::TaskId(task.id));
                if cancelled {
                    let tx = conn.transaction().map_err(|_| "write_failed")?;
                    crate::persistence::conversation_runs::event(
                        &tx,
                        Some(task.id),
                        "cancellation_requested",
                        None,
                    )?;
                    tx.commit().map_err(|_| "write_failed")?;
                }
                let terminal = matches!(
                    existing["state"].as_str(),
                    Some("completed" | "cancelled" | "failed" | "interrupted")
                );
                Ok(
                    json!({"task_id":task.id,"namespace":"product","state":existing["state"],"cancellation_requested":cancelled || existing["state"]=="cancelled","already_terminal":!cancelled && terminal,"commit_in_progress":!cancelled && !terminal}),
                )
            }
            Command::TaskGet { task } => {
                self.request(json!({"operation":"result","task_id":task.id}))
                    .await
            }
            Command::TaskCancel { task } => {
                self.request(json!({"operation":"cancel","task_id":task.id}))
                    .await
            }
            conversation @ (Command::Conversation { .. }
            | Command::Sessions { .. }
            | Command::SessionCreate {}
            | Command::SessionGet { .. }
            | Command::SessionResume { .. }
            | Command::SessionClose { .. }
            | Command::Providers {}
            | Command::ProviderConfigure { .. }
            | Command::ConversationPolicy { .. }) => {
                self.services.conversation_command(conversation).await
            }
            Command::Approval { .. } | Command::ToolRequest { .. } | Command::ToolResult { .. } => {
                Err("capability_not_integrated")
            }
            legacy => {
                self.request(serde_json::to_value(legacy).map_err(|_| "invalid_request")?)
                    .await
            }
        }
    }
    fn event(&self, id: Option<u64>, namespace: &str, code: &str) -> Result<(), &'static str> {
        let mut db = self.config.db()?;
        let tx = db.transaction().map_err(|_| "event_persist_failed")?;
        crate::storage::event(&tx, id, namespace, code)?;
        tx.commit().map_err(|_| "event_persist_failed")
    }
    async fn request(self: &Arc<Self>, v: Value) -> Result<Value, &'static str> {
        match v["operation"].as_str().ok_or("invalid_operation")? {
            "status" => Ok(
                json!({"core":"running","profile":"HOST_ASSISTED_NOT_SANDBOX","copilot":"on_demand","active_task":self.active.lock().await.as_ref().map(|x|x.0),"tools":0,"agent_execution_authority":false,"trace_events":self.trace.stats().published,"protocol_version":ipc::VERSION,"authority":"narys-core","database":"core/db/luna.sqlite3","conversation_integrated":true,"product_active_tasks":self.services.tasks.active_count(),"execution_workers":self.services.execution.worker_count(),"graphical_environment_present":std::env::var_os("DISPLAY").is_some()||std::env::var_os("WAYLAND_DISPLAY").is_some()}),
            ),
            "credentials" => Ok(credentials(&self.config).await),
            "stronghold" => {
                let _permit = self.busy.try_acquire().map_err(|_| "runtime_busy")?;
                if credentials(&self.config).await["login_unlocked"] != true {
                    return Err("manual_unlock_required");
                }
                let store = self.services.secrets.clone();
                let status = tokio::task::spawn_blocking(move || {
                    store
                        .secret_presence(&[narys_domain::security::secrets::SecretKey::GroqApiKey])
                        .map_err(|e| e.code())
                })
                .await
                .map_err(|_| "vault_worker_failed")?;
                status?;
                Ok(
                    json!({"existing_snapshot_opened":true,"writes":false,"migration":false,"secret_values_returned":false}),
                )
            }
            "copilot" | "session-check" | "resume-check" => {
                let _permit = self.busy.try_acquire().map_err(|_| "runtime_busy")?;
                if credentials(&self.config).await["login_unlocked"] != true {
                    return Err("manual_unlock_required");
                }
                let dir = self.create_job_dir()?;
                if v["operation"] == "resume-check" {
                    let id = v["task_id"].as_u64().ok_or("invalid_task_id")?;
                    let source:String=self.config.db()?.query_row("SELECT directory FROM headless_tasks WHERE id=?1 AND state='completed'",[id],|r|r.get(0)).map_err(|_|"completed_owned_task_required")?;
                    policy::private_directory(Path::new(&source))?;
                    // Fixed one-shot per completed task. Never recreate/resend.
                    private_write(
                        &Path::new(&source).join("resume-check-claimed.json"),
                        b"{\"state\":\"ATTEMPTED_RESUME_ONLY\"}",
                    )?;
                    private_write(
                        &dir.join("resume-check.json"),
                        &serde_json::to_vec(&json!({"source":source,"expected":"5"})).unwrap(),
                    )?;
                }
                if v["operation"] == "session-check" {
                    private_write(&dir.join("session-check.json"), b"{\"inference\":false}\n")?;
                }
                let evidence = owned_run(&self.config, &dir, None).await?;
                private_write(
                    &dir.join("metadata-evidence.json"),
                    &serde_json::to_vec_pretty(&evidence).unwrap(),
                )?;
                Ok(
                    json!({"metadata":evidence["sdk_report"],"cleanup_complete":evidence["cleanup_complete"],"evidence":dir.join("metadata-evidence.json"),"financial_admission":"AWAITING_HUMAN_FINANCIAL_REVIEW"}),
                )
            }
            "prepare" => {
                let input: TaskInput =
                    serde_json::from_value(v["task"].clone()).map_err(|_| "invalid_task")?;
                policy::validate_input(&input)?;
                let expected = v["expected"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 128)
                    .ok_or("expected_result_required")?;
                let dir = self.create_job_dir()?;
                private_write(&dir.join("task.json"), &serde_json::to_vec(&input).unwrap())?;
                let db = self.config.db()?;
                let tx = db
                    .unchecked_transaction()
                    .map_err(|_| "task_persist_failed")?;
                tx.execute("INSERT INTO headless_tasks(directory,objective,expected,state) VALUES(?1,?2,?3,'prepared')",rusqlite::params![dir.to_str(),input.objective,expected]).map_err(|_|"task_persist_failed")?;
                let id = tx.last_insert_rowid();
                crate::storage::event(&tx, Some(id as u64), "lr10a", "prepared")?;
                tx.commit().map_err(|_| "task_persist_failed")?;
                Ok(
                    json!({"task_id":id,"workspace":dir.join("workspace"),"state":"prepared","no_inference":true}),
                )
            }
            "submit" => {
                let permit = self
                    .busy
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| "runtime_busy")?;
                if credentials(&self.config).await["login_unlocked"] != true {
                    return Err("manual_unlock_required");
                }
                let id = v["task_id"]
                    .as_u64()
                    .filter(|x| *x > 0 && *x < 9_007_199_254_740_991)
                    .ok_or("invalid_task_id")?;
                let db = self.config.db()?;
                let (path,objective,expected):(String,String,String)=db.query_row("SELECT directory,objective,expected FROM headless_tasks WHERE id=?1 AND state='prepared'",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_|"task_not_prepared_or_already_attempted")?;
                let auth_root = crate::authorization::directory(&self.config.home);
                crate::authorization::check(&auth_root)?;
                let receipt = auth_root.join("receipts").join(format!("task-{id}.json"));
                policy::private_file(&receipt)?;
                let receipt: Value = serde_json::from_slice(
                    &fs::read(receipt).map_err(|_| "financial_review_required")?,
                )
                .map_err(|_| "financial_review_invalid")?;
                policy::reviewed_receipt(&receipt, id)?;
                if id <= 1
                    || receipt["authorization_scope"] != crate::authorization::SCOPE
                    || objective != crate::authorization::PROMPT
                    || expected != "5"
                {
                    return Err("final_consent_task_scope_invalid");
                }
                let dir = PathBuf::from(path);
                policy::private_directory(&dir)?;
                private_write(
                    &dir.join("financial-reviewed.json"),
                    &serde_json::to_vec(&receipt).unwrap(),
                )?;
                {
                    let tx = db
                        .unchecked_transaction()
                        .map_err(|_| "task_claim_failed")?;
                    if tx.execute("UPDATE headless_tasks SET state='running' WHERE id=?1 AND state='prepared'",[id]).map_err(|_|"task_claim_failed")?!=1{return Err("task_claim_failed");}
                    crate::storage::event(&tx, Some(id), "lr10a", "running")?;
                    tx.commit().map_err(|_| "task_claim_failed")?;
                }
                let cancelled = Arc::new(AtomicBool::new(false));
                *self.active.lock().await = Some((id, cancelled.clone()));
                let core = self.clone();
                tokio::spawn(async move {
                    let registry = AgentRegistry::default();
                    let mut registry = registry;
                    registry
                        .register(
                            agent_config(),
                            Arc::new(CopilotBackend {
                                dir: dir.clone(),
                                config: core.config.clone(),
                            }),
                        )
                        .unwrap();
                    let request = AgentRequest {
                        objective: objective.clone(),
                        required_capabilities: AgentCapabilities {
                            planning: true,
                            ..Default::default()
                        },
                    };
                    let plan = PlanV1 {
                        version: 1,
                        objective,
                        steps: vec![PlanStepV1 {
                            id: "specialist".into(),
                            description: "human-approved read-only specialist task".into(),
                            required_capabilities: vec![PlanCapability::Planning],
                            depends_on: vec![],
                        }],
                        risks: vec![],
                        needs_user_input: false,
                        questions: vec![],
                    };
                    let mut graph = TaskGraph::compile(&plan).unwrap();
                    graph.mark_running("specialist").unwrap();
                    let mut event = |e| {
                        let code = match e {
                            AgentEvent::WorkStarted => "started",
                            AgentEvent::Completed => "response_received",
                            AgentEvent::Cancelled => "cancelled",
                            AgentEvent::Failed => "failed",
                            _ => "lifecycle",
                        };
                        trace(&core.trace, id, code);
                        Ok(())
                    };
                    let result = registry
                        .eligible(&request.required_capabilities)
                        .into_iter()
                        .next()
                        .unwrap()
                        .backend
                        .execute(&request, &cancelled, &mut event)
                        .await;
                    let mut completion_guard = core.active.lock().await;
                    let result = if cancelled.load(Ordering::Acquire) {
                        Err(AgentError::Cancelled)
                    } else {
                        result
                    };
                    let (mut state, output, mut code) = match result {
                        Ok(r) if r.output.trim() == expected => ("completed", Some(r.output), None),
                        Ok(r) => {
                            graph.mark_failed("specialist").unwrap();
                            ("validation_failed", Some(r.output), Some("result_mismatch"))
                        }
                        Err(AgentError::Cancelled) => {
                            graph.cancel_unfinished();
                            ("cancelled", None, Some("cancelled"))
                        }
                        Err(e) => {
                            graph.mark_failed("specialist").unwrap();
                            ("failed", None, Some(e.code()))
                        }
                    };
                    if let Some(ref output) = output {
                        let artifact = dir.join("workspace/result.txt");
                        if private_write(&artifact, output.as_bytes()).is_err()
                            || policy::private_file(&artifact).is_err()
                            || fs::read(&artifact).ok().as_deref() != Some(output.as_bytes())
                        {
                            state = "failed";
                            code = Some("result_artifact_verification_failed");
                        }
                    }
                    if state == "completed" {
                        graph.mark_completed("specialist").unwrap();
                    } else if code == Some("result_artifact_verification_failed") {
                        let _ = graph.mark_failed("specialist");
                    }
                    let completion = json!({"task_id":id,"state":state,"subtask_state":graph.state("specialist"),"task_graph_all_completed":graph.all_completed(),"result_validated_against_human_expected":state=="completed","artifact_written_and_reread":output.is_some()&&code!=Some("result_artifact_verification_failed"),"execution_authority_granted":false});
                    if private_write(
                        &dir.join("completion-evidence.json"),
                        &serde_json::to_vec_pretty(&completion).unwrap(),
                    )
                    .is_err()
                    {
                        state = "failed";
                        code = Some("completion_evidence_persist_failed");
                    }
                    let persisted = (|| -> Result<(), &'static str> {
                        let mut db = core.config.db()?;
                        let tx = db.transaction().map_err(|_| "task_completion_failed")?;
                        if tx.execute("UPDATE headless_tasks SET state=?1,result=?2,error_code=?3 WHERE id=?4 AND state='running'",rusqlite::params![state,output,code,id]).map_err(|_|"task_completion_failed")?!=1{return Err("task_completion_failed");}
                        crate::storage::event(&tx, Some(id), "lr10a", state)?;
                        tx.commit().map_err(|_| "task_completion_failed")
                    })();
                    if persisted.is_err() {
                        state = "persistence_failed";
                    }
                    trace(&core.trace, id, state);
                    *completion_guard = None;
                    drop(permit);
                });
                Ok(
                    json!({"task_id":id,"namespace":"lr10a","state":"running","sdk_send_limit":1,"no_retry":true}),
                )
            }
            "cancel" => {
                let id = v["task_id"].as_u64().ok_or("invalid_task_id")?;
                let active = self.active.lock().await;
                if let Some((active_id, c)) = &*active {
                    if *active_id == id {
                        c.store(true, Ordering::Release);
                        self.event(Some(id), "lr10a", "cancellation_requested")?;
                        return Ok(
                            json!({"task_id":id,"namespace":"lr10a","cancellation_requested":true}),
                        );
                    }
                }
                let db = self.config.db()?;
                let state: String = db
                    .query_row("SELECT state FROM headless_tasks WHERE id=?1", [id], |r| {
                        r.get(0)
                    })
                    .map_err(|_| "task_not_found")?;
                if state == "prepared" {
                    let tx = db
                        .unchecked_transaction()
                        .map_err(|_| "task_cancel_failed")?;
                    tx.execute("UPDATE headless_tasks SET state='cancelled',error_code='cancelled_before_send' WHERE id=?1 AND state='prepared'",[id]).map_err(|_|"task_cancel_failed")?;
                    crate::storage::event(&tx, Some(id), "lr10a", "cancelled_before_send")?;
                    tx.commit().map_err(|_| "task_cancel_failed")?;
                    return Ok(
                        json!({"task_id":id,"namespace":"lr10a","state":"cancelled","cancellation_requested":true}),
                    );
                }
                Ok(
                    json!({"task_id":id,"namespace":"lr10a","state":state,"cancellation_requested":state=="cancelled","already_terminal":true}),
                )
            }
            "result" => {
                let id = v["task_id"].as_u64().ok_or("invalid_task_id")?;
                let db = self.config.db()?;
                db.query_row("SELECT state,result,error_code,directory FROM headless_tasks WHERE id=?1",[id],|r|Ok(json!({"task_id":id,"namespace":"lr10a","state":r.get::<_,String>(0)?,"result":r.get::<_,Option<String>>(1)?,"error_code":r.get::<_,Option<String>>(2)?,"private_directory":r.get::<_,String>(3)?}))).map_err(|_|"task_not_found")
            }
            _ => Err("operation_not_allowed"),
        }
    }
    fn create_job_dir(&self) -> Result<PathBuf, &'static str> {
        let dir = tempfile::Builder::new()
            .prefix("narys-task-")
            .tempdir()
            .map_err(|_| "workspace_create_failed")?
            .keep();
        policy::private_directory(&dir)?;
        for n in ["workspace", "logs", "session-state"] {
            mkdir(&dir.join(n))?;
        }
        Ok(dir)
    }
}
fn notify(message: &str) -> Result<(), &'static str> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixDatagram};
    let Some(path) = std::env::var_os("NOTIFY_SOCKET") else {
        return Ok(());
    };
    let socket = UnixDatagram::unbound().map_err(|_| "readiness_notify_failed")?;
    let bytes = path.as_encoded_bytes();
    let addr = if bytes.first() == Some(&b'@') {
        SocketAddr::from_abstract_name(&bytes[1..])
    } else {
        SocketAddr::from_pathname(Path::new(&path))
    }
    .map_err(|_| "readiness_notify_failed")?;
    socket
        .send_to_addr(message.as_bytes(), &addr)
        .map_err(|_| "readiness_notify_failed")?;
    Ok(())
}
pub async fn serve(config: Config) -> Result<(), &'static str> {
    // Must precede SQLite migration/recovery and stale socket cleanup.
    let _process_lease = WriterLease::acquire(&config.runtime.join("server.lock"))
        .map_err(|_| "core_already_running")?;
    // The pre-SERVER-1A daemon did not participate in the writer lease. Refuse
    // a live legacy socket before touching either database or recovery state.
    if UnixStream::connect(config.runtime.join("control.sock"))
        .await
        .is_ok()
    {
        return Err("core_already_running");
    }
    crate::storage::initialize(&config)?;
    let mut db = config.db()?;
    crate::persistence::conversation_runs::recover(&mut db)?;
    // No background summary, continuation resume, or provider call at boot.
    let tx = db
        .unchecked_transaction()
        .map_err(|_| "task_storage_failed")?;
    tx.execute("INSERT INTO server_events(namespace,task_id,code) SELECT 'lr10a',id,'restart_never_retries' FROM headless_tasks WHERE state='running'",[]).map_err(|_|"task_storage_failed")?;
    tx.execute("UPDATE headless_tasks SET state='interrupted',error_code='restart_never_retries' WHERE state='running'",[]).map_err(|_|"task_storage_failed")?;
    tx.commit().map_err(|_| "task_storage_failed")?;
    drop(db);
    let socket = config.runtime.join("control.sock");
    if let Ok(meta) = fs::symlink_metadata(&socket) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::geteuid() } {
            return Err("unsafe_stale_socket");
        }
        if UnixStream::connect(&socket).await.is_ok() {
            return Err("core_already_running");
        }
        fs::remove_file(&socket).map_err(|_| "stale_socket_cleanup_failed")?;
    }
    let listener = UnixListener::bind(&socket).map_err(|_| "local_socket_bind_failed")?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
        .map_err(|_| "socket_permissions_failed")?;
    let services = crate::runtime::RuntimeServices::new(
        config
            .database
            .get()
            .ok_or("runtime_database_missing")?
            .clone(),
        config.home.join(".local/share/br.com.assistente3d.app"),
    )?;
    let core = Arc::new(Core {
        config: Arc::new(config),
        active: Mutex::new(None),
        busy: Arc::new(tokio::sync::Semaphore::new(1)),
        trace: OperationalTraceBus::process_wide(),
        stopping: AtomicBool::new(false),
        services,
    });
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "shutdown_signal_unavailable")?;
    let slots = Arc::new(tokio::sync::Semaphore::new(ipc::MAX_CONNECTIONS));
    let mut connections = tokio::task::JoinSet::new();
    core.event(None, "runtime", "ready")?;
    notify("READY=1")?;
    eprintln!("narys_core_ready protocol_v1 local_same_uid zero_tools");
    loop {
        tokio::select! {
            _=term.recv()=>break,
            _=tokio::signal::ctrl_c()=>break,
            Some(_)=connections.join_next(), if !connections.is_empty()=>{},
            connection=listener.accept()=>{
                let (mut stream,_)=connection.map_err(|_|"socket_accept_failed")?;
                if stream.peer_cred().map_err(|_|"peer_identity_unavailable")?.uid()!=unsafe{libc::geteuid()}{continue;}
                let Ok(slot)=slots.clone().try_acquire_owned() else { continue; };
                let core=core.clone();connections.spawn(async move{
                    let _slot=slot;
                    let mut data=vec![];
                    let read=tokio::time::timeout(Duration::from_secs(5),(&mut stream).take((ipc::MAX_REQUEST_BYTES+1) as u64).read_to_end(&mut data)).await;
                    let mut id=String::new();
                    let result=if matches!(read,Ok(Ok(_)))&&data.len()<=ipc::MAX_REQUEST_BYTES {
                        match serde_json::from_slice::<Request>(&data) {
                            Ok(request)=>{
                                // Echo only validated public correlation IDs.
                                match request.validate() {
                                    Ok(())=>{id=request.request_id;core.dispatch(request.command).await},
                                    Err(code)=>Err(code),
                                }
                            }, Err(_)=>Err("invalid_request"),
                        }
                    }else{Err("request_limit_or_timeout")};
                    let mut bytes=serde_json::to_vec(&Response::new(&id,result)).unwrap();
                    if bytes.len()>ipc::MAX_RESPONSE_BYTES {bytes=serde_json::to_vec(&Response::new(&id,Err("response_limit"))).unwrap();}
                    let _=tokio::time::timeout(Duration::from_secs(5),stream.write_all(&bytes)).await;
                });
            }
        }
    }
    core.stopping.store(true, Ordering::Release);
    core.services.shutdown();
    drop(listener);
    notify("STOPPING=1")?;
    if let Some((_, cancel)) = &*core.active.lock().await {
        cancel.store(true, Ordering::Release);
    }
    while let Some(result) = connections.join_next().await {
        let _ = result;
    }
    while core.busy.available_permits() == 0
        || core.services.tasks.worker_count() > 0
        || core.services.tasks.active_count() > 0
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !core
        .services
        .execution
        .wait_shutdown(Duration::from_secs(10))
    {
        return Err("execution_shutdown_incomplete");
    }
    core.event(None, "runtime", "stopped")?;
    fs::remove_file(socket).map_err(|_| "socket_cleanup_failed")?;
    eprintln!("narys_core_stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(d: &Path) -> Arc<Config> {
        Arc::new(Config {
            home: d.into(),
            state: d.into(),
            runtime: d.into(),
            root: d.into(),
            cli: d.join("never-launch"),
            binary: d.join("never-launch"),
            database: OnceLock::new(),
        })
    }
    #[tokio::test]
    async fn specialist_never_inherits_shell_authority() {
        let d = tempfile::tempdir().unwrap();
        let backend = CopilotBackend {
            dir: d.path().into(),
            config: config(d.path()),
        };
        let req = AgentRequest {
            objective: "run shell".into(),
            required_capabilities: AgentCapabilities {
                command_execution: true,
                ..Default::default()
            },
        };
        let c = AtomicBool::new(false);
        let mut events = |_| panic!("no worker should start");
        assert!(matches!(
            backend.execute(&req, &c, &mut events).await,
            Err(AgentError::UnsupportedCapability)
        ));
    }
    #[tokio::test]
    async fn precancelled_task_has_no_side_effect_or_runtime() {
        let d = tempfile::tempdir().unwrap();
        let backend = CopilotBackend {
            dir: d.path().into(),
            config: config(d.path()),
        };
        let req = AgentRequest {
            objective: "compute".into(),
            required_capabilities: AgentCapabilities {
                planning: true,
                ..Default::default()
            },
        };
        let c = AtomicBool::new(true);
        let mut events = |_| panic!("no worker should start");
        assert!(matches!(
            backend.execute(&req, &c, &mut events).await,
            Err(AgentError::Cancelled)
        ));
        assert!(fs::read_dir(d.path()).unwrap().next().is_none());
    }
}
