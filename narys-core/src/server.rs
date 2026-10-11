use crate::{
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
pub async fn credentials(c: &Config) -> Value {
    crate::credentials::status(c.runtime.parent().unwrap()).await
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
            Command::AgentRuntimeStop {} => {
                self.services.copilot.shutdown().await?;
                Ok(self.services.copilot.status())
            }
            Command::AgentRuntimeRecover {} => {
                self.services.copilot.recover_runtime_ownership().await
            }
            Command::AgentStatus {} => Ok(self.services.copilot.status()),
            Command::AgentSessionCreate {} => self.services.copilot.admit(
                crate::agents::lifecycle::AgentLifecycleOperation::Create,
                None,
            ),
            Command::AgentSessionGet { session_ref } => self.services.copilot.session(&session_ref),
            Command::AgentSessionResume { session_ref } => self.services.copilot.admit(
                crate::agents::lifecycle::AgentLifecycleOperation::Resume,
                Some(session_ref),
            ),
            Command::AgentSessionAttach { session_ref } => {
                self.services.copilot.attach(&session_ref)
            }
            Command::AgentSessionDetach { attachment_id } => {
                Ok(self.services.copilot.detach(&attachment_id))
            }
            Command::AgentSessionClose { session_ref } => self.services.copilot.close(&session_ref),
            Command::Capabilities {} => Ok(
                json!({"protocol_version":ipc::VERSION,"authority":"narys-core","persistence":"core/db/luna.sqlite3","task_namespaces":["lr10a","product"],"implemented":["status","credentials","events","prepare","result","cancel","conversation","sessions","session-create","session-get","session-resume","session-close","providers","provider-configure","conversation-policy","task-get","task-cancel","tasks","models","agent-status","agent-runtime-recover","agent-runtime-stop","agent-session-create","agent-session-get","agent-session-resume","agent-session-attach","agent-session-detach","agent-session-close"],"conversation":true,"provider_configuration":true,"approvals":false,"agent_tools":false,"execution_authority_from_ipc":false}),
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
                let conn = self.config.db()?;
                if crate::copilot::store::contains(&conn, task.id)? {
                    crate::copilot::store::task(&conn, task.id)
                } else {
                    crate::persistence::conversation_runs::get(&conn, task.id)
                }
            }
            Command::TaskCancel { task } if matches!(task.namespace, TaskNamespace::Product) => {
                if crate::copilot::store::contains(&self.config.db()?, task.id)? {
                    return self.services.copilot.cancel(task.id);
                }
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
                json!({"core":"running","profile":"HOST_ASSISTED_NOT_SANDBOX","copilot":"on_demand","copilot_runtime":self.services.copilot.status(),"active_task":self.active.lock().await.as_ref().map(|x|x.0),"tools":0,"agent_execution_authority":false,"trace_events":self.trace.stats().published,"protocol_version":ipc::VERSION,"authority":"narys-core","database":"core/db/luna.sqlite3","conversation_integrated":true,"product_active_tasks":self.services.tasks.active_count(),"execution_workers":self.services.execution.worker_count(),"graphical_environment_present":std::env::var_os("DISPLAY").is_some()||std::env::var_os("WAYLAND_DISPLAY").is_some()}),
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
                Err("legacy_copilot_diagnostic_closed_use_agent_lifecycle")
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
            "submit" => Err("lr10a_authorization_closed"),
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
    crate::copilot::store::recover(&mut db)?;
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
    let services = crate::runtime::RuntimeServices::with_copilot_cli(
        config
            .database
            .get()
            .ok_or("runtime_database_missing")?
            .clone(),
        config.home.join(".local/share/br.com.assistente3d.app"),
        config.cli.clone(),
    )?;
    let _ = services.copilot.recover_runtime_ownership().await; // Fault only the specialist; keep the Core available.
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
    core.services.copilot.shutdown().await?;
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
