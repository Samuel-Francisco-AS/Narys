//! Explicit disposable OFFLINE Core mode. It cannot start a provider runtime.
//! Operator trust is established by exclusion from all executable namespaces,
//! never by UID/TTY/a challenge or an agent-provided boolean.
use super::*;
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::{
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::PathBuf,
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    process::{Child, ChildStdin, ChildStdout},
    sync::Semaphore,
};

const TOOL: &str = r#"import os,sys,json,hashlib,subprocess
from pathlib import Path
p=json.loads(sys.stdin.buffer.readline())
if p['kind']=='write':
 fd=os.open(p['path'],os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
 with os.fdopen(fd,'wb') as f:
  f.write(p['content'].encode('ascii')); f.flush(); os.fsync(f.fileno())
elif p['kind']=='command':
 r=subprocess.run([p['program']]+p['arguments'],capture_output=True,check=False)
 if r.returncode: sys.exit(r.returncode if r.returncode>0 else 1)
 if p['program']=='/usr/bin/sha256sum':
  print(json.dumps({'sha256':r.stdout.decode('ascii').split()[0]}),flush=True)
else: sys.exit(1)
"#;
const PREFLIGHT: &str = r#"import sys,json,os,socket,subprocess
from pathlib import Path
p=json.loads(sys.stdin.buffer.readline()); denied=[]
for name,action in [('operator-socket',lambda:socket.socket(socket.AF_UNIX).connect(p['socket'])),('private-state',lambda:Path(p['marker']).read_bytes()),('host-proc',lambda:Path('/proc/'+str(p['pid'])+'/environ').read_bytes()),('network',lambda:socket.create_connection(('127.0.0.1',p['port']),timeout=.2)),('workspace-write',lambda:Path('/workspace/unapproved').write_text('no'))]:
 try: action(); raise AssertionError(name+' escaped')
 except OSError: denied.append(name)
assert len(os.listdir('/proc/self/fd'))<=4
assert not os.environ.get('NARYS_PRIVATE_MARKER')
assert subprocess.run(['/usr/bin/unshare','-Ur','/usr/bin/true'],capture_output=True).returncode!=0
print(json.dumps({'denied':denied}),flush=True)
"#;
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum OperatorRequest {
    Status,
    StartSyntheticPeer {
        source: String,
        ttl_seconds: u64,
    },
    Approvals {
        #[serde(default)]
        after: u64,
        #[serde(default = "page_limit")]
        limit: u16,
        #[serde(default)]
        pending_only: bool,
    },
    Show {
        approval_id: String,
    },
    Approve {
        approval_id: String,
        digest: String,
    },
    Deny {
        approval_id: String,
    },
    Cancel {
        task_id: u64,
    },
    Task {
        task_id: u64,
    },
    Shutdown,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PeerIntent {
    Write {
        path: String,
        content: String,
    },
    Command {
        program: String,
        arguments: Vec<String>,
    },
}
fn page_limit() -> u16 {
    20
}
struct Pending {
    context: AgentOperationContext,
    authority: AgentAuthority,
}
struct Task {
    workspace: PathBuf,
    session: String,
    cancel: Arc<AtomicBool>,
    ttl: Duration,
}
struct LocalCore {
    authority: AuthorityService,
    root: PathBuf,
    socket: PathBuf,
    tasks: Mutex<BTreeMap<u64, Arc<Task>>>,
    pending: Mutex<BTreeMap<String, Pending>>,
    jobs: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    peer_slots: Arc<Semaphore>,
    stopping: AtomicBool,
    uncertain: AtomicBool,
}

// Owns PID 1 through a pidfd, not a reusable numeric PID. Kill-on-drop is a
// safety net; only wait + pidfd readiness may produce cleanup_verified.
struct Process {
    child: Child,
    init: File,
    input: Option<ChildStdin>,
    output: Option<ChildStdout>,
    workspace: File,
}
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.init.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            );
        }
    }
}
fn ready(fd: &File) -> bool {
    let mut p = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut p, 1, 0) > 0 && p.revents & libc::POLLIN != 0 }
}
impl Process {
    async fn spawn(workspace: &Path, code: &str, readonly: bool) -> Result<Self> {
        let (base, directory) =
            sandbox::pinned(workspace, "/usr/bin/python3", &["-I", "-c", code], readonly)?;
        let (info, writer) =
            std::os::unix::net::UnixStream::pair().map_err(|_| "sandbox_info_failed")?;
        info.set_nonblocking(true)
            .map_err(|_| "sandbox_info_failed")?;
        let info_fd = writer.as_raw_fd();
        let directory_fd = directory.as_raw_fd();
        let filter = super::seccomp::filter()?;
        let filter_fd = filter.as_raw_fd();
        let args: Vec<_> = base.get_args().map(|s| s.to_os_string()).collect();
        let mut command = std::process::Command::new(base.get_program());
        command.env_clear();
        let split = args
            .iter()
            .position(|s| s == "--")
            .ok_or("sandbox_args_invalid")?;
        command
            .args(&args[..split])
            .args([
                "--as-pid-1",
                "--info-fd",
                &info_fd.to_string(),
                "--seccomp",
                &filter_fd.to_string(),
            ])
            .args(&args[split..]);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(filter_fd, libc::F_SETFD, 0) < 0
                    || libc::fcntl(info_fd, libc::F_SETFD, 0) < 0
                    || libc::fcntl(directory_fd, libc::F_SETFD, 0) < 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                // Bound CPU/output/fd/process growth, without claiming cgroup memory containment.
                for (resource, limit) in [
                    (libc::RLIMIT_CPU, 40),
                    (libc::RLIMIT_FSIZE, 65536),
                    (libc::RLIMIT_NOFILE, 64),
                    (libc::RLIMIT_AS, 268435456),
                    (libc::RLIMIT_CORE, 0),
                ] {
                    let r = libc::rlimit {
                        rlim_cur: limit,
                        rlim_max: limit,
                    };
                    if libc::setrlimit(resource, &r) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| "sandbox_spawn_failed")?;
        drop(writer);
        drop(filter);
        let mut info = UnixStream::from_std(info).map_err(|_| "sandbox_info_failed")?;
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            let mut bytes = Vec::new();
            loop {
                let mut buf = [0; 512];
                let n = info
                    .read(&mut buf)
                    .await
                    .map_err(|_| "sandbox_info_failed")?;
                if n == 0 || bytes.len() + n > 4096 {
                    return Err("sandbox_info_failed");
                }
                bytes.extend_from_slice(&buf[..n]);
                if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                    let pid = v["child-pid"]
                        .as_i64()
                        .filter(|p| *p > 0 && *p <= i32::MAX as i64)
                        .ok_or("sandbox_pid_missing")?;
                    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as i32, 0) };
                    if fd < 0 {
                        return Err("sandbox_pidfd_unavailable");
                    }
                    return Ok(unsafe { File::from_raw_fd(fd as i32) });
                }
            }
        })
        .await;
        let init = match result {
            Ok(Ok(fd)) => fd,
            _ => {
                let _ = tokio::time::timeout(Duration::from_secs(3), child.kill()).await;
                let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
                return Err("sandbox_launch_cleanup_uncertain");
            }
        };
        let input = child.stdin.take();
        let output = child.stdout.take();
        Ok(Self {
            child,
            init,
            input,
            output,
            workspace: directory,
        })
    }
    async fn send(&mut self, v: &Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(v).map_err(|_| "peer_input_invalid")?;
        bytes.push(b'\n');
        self.input
            .as_mut()
            .ok_or("peer_input_closed")?
            .write_all(&bytes)
            .await
            .map_err(|_| "peer_input_closed")
    }
    async fn reap(&mut self, kill: bool) -> Result<i32> {
        if kill {
            unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    self.init.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                );
            }
            let _ = self.child.start_kill();
        }
        let status = tokio::time::timeout(Duration::from_secs(3), self.child.wait())
            .await
            .map_err(|_| "cleanup_uncertain")?
            .map_err(|_| "cleanup_uncertain")?;
        tokio::time::timeout(Duration::from_secs(3), async {
            while !ready(&self.init) {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .map_err(|_| "cleanup_uncertain")?;
        Ok(status.code().unwrap_or(-1))
    }
}
async fn read_line(reader: &mut BufReader<ChildStdout>, bytes: &mut Vec<u8>) -> Result<usize> {
    loop {
        let buf = reader.fill_buf().await.map_err(|_| "peer_output_closed")?;
        if buf.is_empty() {
            return Ok(bytes.len());
        }
        let end = buf.iter().position(|b| *b == b'\n').map(|n| n + 1);
        let n = end.unwrap_or(buf.len()).min(8193 - bytes.len());
        bytes.extend_from_slice(&buf[..n]);
        reader.consume(n);
        if bytes.len() > 8192 {
            return Err("peer_output_limit");
        }
        if end.is_some() {
            return Ok(bytes.len());
        }
    }
}
fn file_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn context(task_id: u64, task: &Task, intent: &PeerIntent) -> Result<AgentOperationContext> {
    let (tool, operation) = match intent {
        PeerIntent::Write { path, content }
            if file_name(path)
                && content.starts_with("NARYS_OFFLINE_TEST:")
                && content.len() <= 4096
                && content
                    .bytes()
                    .all(|b| b == b'\n' || b == b'\t' || (32..=126).contains(&b)) =>
        {
            if task.workspace.join(path).exists() {
                return Err("write_existing_file_denied");
            }
            (
                "narys.write",
                AgentOperation::Write {
                    relative_path: path.into(),
                    content: content.clone(),
                },
            )
        }
        PeerIntent::Command { program, arguments }
            if program == "/usr/bin/sleep"
                && arguments.len() == 1
                && arguments[0].parse::<u32>().is_ok_and(|n| n <= 30) =>
        {
            (
                "narys.command",
                AgentOperation::Command {
                    program: program.into(),
                    arguments: arguments.clone(),
                },
            )
        }
        PeerIntent::Command { program, arguments }
            if program == "/usr/bin/sha256sum"
                && arguments.len() == 1
                && arguments[0]
                    .strip_prefix("/workspace/")
                    .is_some_and(file_name) =>
        {
            validate_path(
                &task.workspace,
                Path::new(arguments[0].strip_prefix("/workspace/").unwrap()),
                false,
            )?;
            (
                "narys.command",
                AgentOperation::Command {
                    program: program.into(),
                    arguments: arguments.clone(),
                },
            )
        }
        _ => return Err("offline_tool_or_arguments_denied"),
    };
    Ok(AgentOperationContext {
        task_id,
        session_id: task.session.clone(),
        specialist_id: "synthetic-peer".into(),
        profile: AgentApprovalPolicy::Assisted,
        workspace: task.workspace.clone(),
        tool: tool.into(),
        operation,
        policy_version: POLICY_VERSION,
    })
}
fn payload(c: &AgentOperationContext) -> Value {
    match &c.operation {
        AgentOperation::Write {
            relative_path,
            content,
        } => json!({"kind":"write","path":relative_path,"content":content}),
        AgentOperation::Command { program, arguments } => {
            json!({"kind":"command","program":program,"arguments":arguments})
        }
        _ => Value::Null,
    }
}
fn preview(c: &AgentOperationContext) -> Result<Value> {
    Ok(
        json!({"context":c,"digest":binding(c)?,"effect":payload(c),"cwd":"/workspace","runner_program":"/usr/bin/python3","runner_arguments":["-I","-c",TOOL],"runner_sha256":hex(&Sha256::digest(TOOL.as_bytes())),"risk":"Only disposable workspace effects; no network, credentials, billing or host execution. Write is exclusive creation; commands limited to sleep/sha256sum.","requires_explicit_confirmation":true}),
    )
}
fn read_evidence(c: &AgentOperationContext, directory: &File) -> Result<Value> {
    let relative = match &c.operation {
        AgentOperation::Write { relative_path, .. } => Some(relative_path.clone()),
        AgentOperation::Command { program, arguments }
            if program == Path::new("/usr/bin/sha256sum") =>
        {
            Some(PathBuf::from(
                arguments[0]
                    .strip_prefix("/workspace/")
                    .ok_or("evidence_path_invalid")?,
            ))
        }
        _ => None,
    };
    if let Some(relative) = relative {
        let leaf = relative
            .to_str()
            .filter(|s| file_name(s))
            .ok_or("evidence_path_invalid")?;
        let name = std::ffi::CString::new(leaf).map_err(|_| "evidence_path_invalid")?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err("evidence_unavailable");
        }
        let mut f = unsafe { File::from_raw_fd(fd) };
        let m = f.metadata().map_err(|_| "evidence_unavailable")?;
        if !m.is_file() || m.nlink() != 1 || m.len() > 65536 {
            return Err("evidence_invalid");
        }
        let mut data = Vec::new();
        std::io::Read::by_ref(&mut f)
            .take(65537)
            .read_to_end(&mut data)
            .map_err(|_| "evidence_unavailable")?;
        if let AgentOperation::Write { content, .. } = &c.operation {
            if data != content.as_bytes() {
                return Err("effect_mismatch");
            }
        }
        Ok(
            json!({"path":relative,"bytes":data.len(),"sha256":hex(&Sha256::digest(&data)),"verified_by":"Core nofollow file handle, after process cleanup"}),
        )
    } else {
        Ok(json!({"verified_by":"supervised OS exit status, not peer text"}))
    }
}
impl LocalCore {
    async fn reap_jobs(&self) -> Result<()> {
        let retired = {
            let mut jobs = self.jobs.lock().map_err(|_| "local_faulted")?;
            let mut retired = Vec::new();
            let mut i = 0;
            while i < jobs.len() {
                if jobs[i].is_finished() {
                    retired.push(jobs.swap_remove(i));
                } else {
                    i += 1;
                }
            }
            retired
        };
        for j in retired {
            if j.await.is_err() {
                self.uncertain.store(true, Ordering::Release);
                return Err("local_job_cleanup_uncertain");
            }
        }
        Ok(())
    }
    fn conn(&self) -> Result<Connection> {
        connection(&self.authority.database).map_err(|e| {
            self.uncertain.store(true, Ordering::Release);
            e
        })
    }
    fn task(&self, id: u64) -> Result<Arc<Task>> {
        self.tasks
            .lock()
            .map_err(|_| "local_faulted")?
            .get(&id)
            .cloned()
            .ok_or("local_task_not_active")
    }
    fn audit(&self, id: u64, code: &str) -> Result<()> {
        crate::persistence::conversation_runs::event(&self.conn()?, Some(id), code, None)
    }
    async fn preflight(&self, workspace: &Path) -> Result<()> {
        let tcp = std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| "probe_unavailable")?;
        let mut p = Process::spawn(workspace, PREFLIGHT, true).await?;
        p.send(&json!({"socket":self.socket,"marker":self.root.join("private-marker"),"pid":std::process::id(),"port":tcp.local_addr().map_err(|_|"probe_unavailable")?.port()})).await?;
        p.input.take();
        let mut data = Vec::new();
        let out = p.output.take().ok_or("probe_unavailable")?;
        let read = tokio::time::timeout(
            Duration::from_secs(3),
            out.take(4097).read_to_end(&mut data),
        )
        .await;
        let status = p.reap(false).await?;
        if !matches!(read, Ok(Ok(_))) || status != 0 || data.len() > 4096 {
            return Err("sandbox_operational_probe_failed");
        }
        let v: Value =
            serde_json::from_slice(&data).map_err(|_| "sandbox_operational_probe_failed")?;
        // Both negative effects and successful OS exit + PID 1 exit are required.
        if v["denied"].as_array().map(Vec::len) != Some(5) {
            return Err("sandbox_operational_probe_failed");
        }
        Ok(())
    }
    async fn start(self: &Arc<Self>, source: String, ttl: u64) -> Result<Value> {
        if self.stopping.load(Ordering::Acquire) || self.uncertain.load(Ordering::Acquire) {
            return Err("local_admission_closed");
        }
        if source.len() > 16384 || source.contains('\0') || !(1..=300).contains(&ttl) {
            return Err("peer_input_limit");
        }
        // Reserve before any await/preflight. Counting registered jobs alone
        // lets concurrent admissions exceed the process boundary's limit.
        let slot = self
            .peer_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "local_concurrency_limit")?;
        self.reap_jobs().await?;
        let (id, workspace, session) = {
            let mut conn = self.conn()?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| "local_write_failed")?;
            if tx
                .query_row("SELECT count(*) FROM agent_local_tasks", [], |r| {
                    r.get::<_, u64>(0)
                })
                .map_err(|_| "local_read_failed")?
                >= 128
            {
                return Err("local_instance_task_limit");
            }
            let id:u64=tx.query_row("SELECT 1+max(n) FROM (SELECT coalesce(max(task_id),0) n FROM agent_local_tasks UNION ALL SELECT coalesce(max(task_id),0) FROM task_records UNION ALL SELECT coalesce(max(task_id),0) FROM agent_runs UNION ALL SELECT coalesce(max(task_id),0) FROM conversation_runs)",[],|r|r.get(0)).map_err(|_|"local_write_failed")?;
            let workspace = self.root.join("workspaces").join(id.to_string());
            crate::server::mkdir(&workspace)?;
            let session = format!("local-{}", hex(&random()?));
            tx.execute("INSERT INTO agent_local_tasks(task_id,session_id,workspace,epoch,state) VALUES(?1,?2,?3,?4,'active')",params![id,session,workspace.to_str(),self.authority.epoch]).map_err(|_|"local_write_failed")?;
            tx.commit().map_err(|_| "local_write_failed")?;
            (id, workspace, session)
        };
        if let Err(e) = self.preflight(&workspace).await {
            self.uncertain.store(true, Ordering::Release);
            self.conn()?
                .execute(
                    "UPDATE agent_local_tasks SET state='cleanup_uncertain' WHERE task_id=?1",
                    [id],
                )
                .map_err(|_| "local_write_failed")?;
            return Err(e);
        }
        if self.stopping.load(Ordering::Acquire) || self.uncertain.load(Ordering::Acquire) {
            self.conn()?.execute("UPDATE agent_local_tasks SET state='cancelled',peer_cleanup_verified=1 WHERE task_id=?1",[id]).map_err(|_|"local_write_failed")?;
            return Err("local_admission_closed");
        }
        let task = Arc::new(Task {
            workspace: workspace.clone(),
            session: session.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
            ttl: Duration::from_secs(ttl),
        });
        self.tasks
            .lock()
            .map_err(|_| "local_faulted")?
            .insert(id, task.clone());
        let core = self.clone();
        let job = tokio::spawn(async move {
            let _slot = slot;
            core.peer(id, task, source).await;
        });
        self.jobs.lock().map_err(|_| "local_faulted")?.push(job);
        Ok(
            json!({"task_id":id,"namespace":"product","session_id":session,"workspace":workspace,"peer":"synthetic Python, not Copilot","cost_authorized":false}),
        )
    }
    async fn peer(self: Arc<Self>, id: u64, task: Arc<Task>, source: String) {
        let (result, clean) = self.peer_inner(id, &task, &source).await;
        let tool_unsettled=self.conn().and_then(|conn|conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_tool_executions WHERE task_id=?1 AND (phase IN ('claimed','started','uncertain') OR cleanup_verified=0))",[id],|r|r.get::<_,bool>(0)).map_err(|_|"local_read_failed")).unwrap_or(true);
        let uncertain = !clean || tool_unsettled;
        if uncertain {
            self.uncertain.store(true, Ordering::Release);
        }
        let state = if uncertain {
            "cleanup_uncertain"
        } else if task.cancel.load(Ordering::Acquire) {
            "cancelled"
        } else if result.is_ok() {
            "completed"
        } else {
            "failed"
        };
        if let Ok(conn) = self.conn() {
            if conn.execute("UPDATE agent_local_tasks SET state=?2,peer_cleanup_verified=?3 WHERE task_id=?1",params![id,state,clean]).is_err(){self.uncertain.store(true,Ordering::Release);}
        } else {
            self.uncertain.store(true, Ordering::Release);
        }
        let _ = self.authority.cancel_task(id);
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, p| p.context.task_id != id);
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }
    async fn peer_inner(&self, id: u64, task: &Task, source: &str) -> (Result<()>, bool) {
        let code =
            format!("import sys,json\ninit=json.loads(sys.stdin.buffer.readline())\n{source}");
        let mut peer = match Process::spawn(&task.workspace, &code, true).await {
            Ok(p) => p,
            Err(e) => return (Err(e), false),
        };
        let result=async {
            peer.send(&json!({"task_id":id,"session_id":task.session,"workspace":"/workspace","approval_channel":"not exposed"})).await?;
            let mut out=BufReader::new(peer.output.take().ok_or("peer_output_closed")?);
            for _ in 0..8 {
                let mut bytes=Vec::new();
                let received=tokio::time::timeout(Duration::from_secs(35),async {
                    loop {
                        if task.cancel.load(Ordering::Acquire){return Err("authority_cancelled");}
                        tokio::select! {r=read_line(&mut out,&mut bytes)=>{return r;},_=tokio::time::sleep(Duration::from_millis(10))=>{}}
                    }
                }).await.map_err(|_|"peer_timeout")??;
                if received==0{return Ok(());}
                if bytes.len()>8192{return Err("peer_output_limit");}
                let outcome=match serde_json::from_slice::<PeerIntent>(&bytes) {
                    Ok(intent)=>self.operation(id,task,intent,&peer.init).await,
                    Err(_)=>Err("unknown_or_malformed_peer_tool"),
                };
                let successful=outcome.as_ref().is_ok_and(|v|v["phase"]=="completed");
                peer.send(&match outcome {Ok(v)=>json!({"ok":successful,"result":v}),Err(e)=>json!({"ok":false,"error_code":e})}).await?;
                if !successful {return Err("tool_operation_failed");}
            }
            Err("peer_operation_limit")
        }.await;
        let cleanup = peer.reap(true).await;
        match cleanup {
            Ok(_) => (result, true),
            Err(e) => (Err(e), false),
        }
    }
    async fn operation(
        &self,
        id: u64,
        task: &Task,
        intent: PeerIntent,
        peer_init: &File,
    ) -> Result<Value> {
        if task.cancel.load(Ordering::Acquire) || self.uncertain.load(Ordering::Acquire) {
            return Err("local_admission_closed");
        }
        let c = context(id, task, &intent)?;
        let digest = binding(&c)?;
        let deadline = Instant::now() + task.ttl;
        let boundary = ExecutionBoundary {
            binding: digest.clone(),
            deadline,
        };
        let financial = FinancialAdmission {
            binding: digest,
            deadline,
            paid_use_allowed: false,
        };
        // Only this offline executor constructs these proofs: sandbox + no
        // provider/credentials/network, zero inference/cost permission.
        let (approval_id, authority) =
            self.authority
                .request(c.clone(), task.ttl, Some(&boundary), Some(&financial))?;
        self.pending.lock().map_err(|_| "local_faulted")?.insert(
            approval_id.clone(),
            Pending {
                context: c.clone(),
                authority,
            },
        );
        self.audit(id, "local_tool_requested")?;
        loop {
            if ready(peer_init) {
                return Err("peer_exited_before_claim");
            }
            if task.cancel.load(Ordering::Acquire) {
                return Err("authority_cancelled");
            }
            let state = self.authority.get(&approval_id)?["state"]
                .as_str()
                .unwrap_or("invalid")
                .to_string();
            match state.as_str() {
                "approved" => break,
                "pending" => (),
                _ => return Err("approval_denied_expired_or_cancelled"),
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let pending = self
            .pending
            .lock()
            .map_err(|_| "local_faulted")?
            .remove(&approval_id)
            .ok_or("authority_not_live")?;
        let claim = self.authority.claim(&pending.authority, &c)?;
        // Durable claim journal must precede launch. If either SQL write fails,
        // the authority stays consumed and the task closes, never retries.
        let result = self.run_tool(&c, &claim).await;
        self.authority
            .state
            .lock()
            .map_err(|_| "authority_faulted")?
            .claims
            .remove(&claim.id);
        result
    }
    async fn run_tool(&self, c: &AgentOperationContext, claim: &ExecutionClaim) -> Result<Value> {
        if claim.cancelled.load(Ordering::Acquire) {
            return self.finish(
                claim,
                "cancelled",
                false,
                true,
                json!({"effect":"not_started"}),
            );
        }
        let mut process = match Process::spawn(&c.workspace, TOOL, false).await {
            Ok(p) => p,
            Err(e) => {
                self.uncertain.store(true, Ordering::Release);
                let _ = self.finish(claim, "uncertain", false, false, json!({"error_code":e}));
                return Err(e);
            }
        };
        let output = process.output.take().ok_or("tool_output_closed")?;
        let reader = tokio::spawn(async move {
            let mut data = Vec::new();
            let r = output.take(65537).read_to_end(&mut data).await;
            (r, data)
        });
        let mut started = false;
        let result=async {
            if claim.cancelled.load(Ordering::Acquire){return Err("authority_cancelled");}
            if Instant::now()>=claim.deadline{return Err("authority_expired");}
            let metadata=process.workspace.metadata().map_err(|_|"workspace_identity_changed")?;
            if (metadata.dev(),metadata.ino())!=claim.workspace_identity{return Err("workspace_identity_changed");}
            // Conservative started means effect MAY start after this commit.
            {
                let mut conn=self.conn()?;
                let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|_|"effect_start_journal_failed")?;
                if tx.execute("UPDATE agent_tool_executions SET phase='started',effect_started=1 WHERE approval_id=?1 AND phase='claimed'",[&claim.id]).map_err(|_|"effect_start_journal_failed")?!=1{return Err("effect_start_journal_failed");}
                tx.execute("INSERT INTO agent_execution_events(approval_id,phase) VALUES(?1,'started')",[&claim.id]).map_err(|_|"effect_start_journal_failed")?;
                tx.commit().map_err(|_|"effect_start_journal_failed")?;
            }
            started=true;
            process.send(&payload(c)).await?;process.input.take();
            let end=Instant::now()+Duration::from_secs(35);
            loop {
                if claim.cancelled.load(Ordering::Acquire){return Err("authority_cancelled");}
                if Instant::now()>=end{return Err("tool_timeout");}
                if Instant::now()>=claim.deadline{return Err("authority_expired");}
                if let Some(status)=process.child.try_wait().map_err(|_|"tool_wait_failed")? {return Ok(status.code().unwrap_or(-1));}
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }.await;
        let cleanup = process.reap(result.is_err()).await;
        let read = tokio::time::timeout(Duration::from_secs(3), reader).await;
        if cleanup.is_err() || read.is_err() {
            self.uncertain.store(true, Ordering::Release);
            return self.finish(
                claim,
                "uncertain",
                started,
                false,
                json!({"effect":"uncertain","error_code":"cleanup_unverified"}),
            );
        }
        let (read_result, data) = read
            .map_err(|_| "cleanup_uncertain")?
            .map_err(|_| "tool_output_failed")?;
        let output_ok = read_result.is_ok() && data.len() <= 65536;
        // Completion ordered with cancellation under short authority lock.
        // No lock was held while waiting for the process.
        let state = self
            .authority
            .state
            .lock()
            .map_err(|_| "authority_faulted")?;
        let cancelled = claim.cancelled.load(Ordering::Acquire);
        let (phase, evidence) = if cancelled {
            (
                "cancelled",
                json!({"effect":"may_have_occurred","success":false}),
            )
        } else if result == Ok(0) && output_ok {
            match read_evidence(c, &process.workspace) {
                Ok(v) => ("completed", v),
                Err(e) => ("failed", json!({"error_code":e})),
            }
        } else {
            (
                "failed",
                json!({"error_code":result.err().unwrap_or("tool_exit_failed"),"success":false}),
            )
        };
        let value=self.finish(claim,phase,started,true,json!({"evidence":evidence,"exit_code":cleanup.ok(),"stdout_sha256":hex(&Sha256::digest(&data)),"stdout_bytes":data.len()}));
        drop(state);
        value
    }
    fn finish(
        &self,
        claim: &ExecutionClaim,
        phase: &str,
        started: bool,
        clean: bool,
        result: Value,
    ) -> Result<Value> {
        let value = json!({"approval_id":claim.id,"phase":phase,"effect_started":started,"cancel_requested":claim.cancelled.load(Ordering::Acquire),"cleanup_verified":clean,"result":result});
        let persisted = (|| -> Result<()> {
            let mut conn = self.conn()?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| "result_persistence_uncertain")?;
            let changed=tx.execute("UPDATE agent_tool_executions SET phase=?2,effect_started=?3,cancel_requested=?4,cleanup_verified=?5,result_json=?6,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE approval_id=?1 AND phase IN ('claimed','started')",params![claim.id,phase,started,claim.cancelled.load(Ordering::Acquire),clean,value.to_string()]).map_err(|_|"result_persistence_uncertain")?;
            if changed != 1 {
                return Err("execution_state_uncertain");
            }
            tx.execute(
                "INSERT INTO agent_execution_events(approval_id,phase) VALUES(?1,?2)",
                params![claim.id, phase],
            )
            .map_err(|_| "result_persistence_uncertain")?;
            tx.commit().map_err(|_| "result_persistence_uncertain")
        })();
        if let Err(e) = persisted {
            self.uncertain.store(true, Ordering::Release);
            return Err(e);
        }
        Ok(value)
    }
    fn cancel(&self, id: u64) -> Result<Value> {
        let task = self.task(id)?;
        task.cancel.store(true, Ordering::Release);
        let revoked = self.authority.cancel_task(id);
        let persisted = (|| -> Result<()> {
            let mut conn = self.conn()?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| "cancel_persistence_failed")?;
            tx.execute(
                "UPDATE agent_local_tasks SET state='cancel_requested' WHERE task_id=?1",
                [id],
            )
            .map_err(|_| "cancel_persistence_failed")?;
            tx.execute("UPDATE agent_tool_executions SET cancel_requested=1 WHERE task_id=?1 AND phase IN ('claimed','started')",[id]).map_err(|_|"cancel_persistence_failed")?;
            tx.execute("INSERT INTO agent_execution_events(approval_id,phase) SELECT approval_id,'cancel_requested' FROM agent_tool_executions WHERE task_id=?1 AND phase IN ('claimed','started')",[id]).map_err(|_|"cancel_persistence_failed")?;
            tx.commit().map_err(|_| "cancel_persistence_failed")
        })();
        if revoked.is_err() || persisted.is_err() {
            self.uncertain.store(true, Ordering::Release);
        }
        revoked?;
        persisted?;
        Ok(json!({"task_id":id,"cancel_requested":true,"cleanup_verified":false}))
    }
    async fn dispatch(self: &Arc<Self>, r: OperatorRequest) -> Result<Value> {
        match r {
            OperatorRequest::Status => Ok(
                json!({"mode":"offline_local_boundary","native_copilot":"BLOCKED","isolated":"BLOCKED","yolo":"BLOCKED","cost_authorized":false,"admission_closed":self.uncertain.load(Ordering::Acquire),"trusted_operator":"namespace exclusion, not UID","socket":self.socket}),
            ),
            OperatorRequest::StartSyntheticPeer {
                source,
                ttl_seconds,
            } => self.start(source, ttl_seconds).await,
            OperatorRequest::Approvals {
                after,
                limit,
                pending_only,
            } => {
                if limit > 20 {
                    return Err("local_page_limit");
                }
                self.authority.list(after, limit, pending_only)
            }
            OperatorRequest::Show { approval_id } => {
                let details = {
                    let p = self.pending.lock().map_err(|_| "local_faulted")?;
                    p.get(&approval_id)
                        .map(|p| preview(&p.context))
                        .transpose()?
                };
                Ok(json!({"receipt":self.authority.get(&approval_id)?,"exact_preview":details}))
            }
            OperatorRequest::Approve {
                approval_id,
                digest,
            } => {
                if self.uncertain.load(Ordering::Acquire) || self.stopping.load(Ordering::Acquire) {
                    return Err("local_admission_closed");
                }
                let p = self.pending.lock().map_err(|_| "local_faulted")?;
                let pending = p.get(&approval_id).ok_or("approval_not_live")?;
                preview(&pending.context)?; // never approve an undisclosable intent
                self.authority.approve(
                    &approval_id,
                    &HumanChannel {
                        epoch: self.authority.epoch.clone(),
                    },
                    &digest,
                )?;
                Ok(json!({"approved_once":true,"approval_id":approval_id}))
            }
            OperatorRequest::Deny { approval_id } => self.authority.deny(&approval_id),
            OperatorRequest::Cancel { task_id } => self.cancel(task_id),
            OperatorRequest::Task { task_id } => {
                let conn = self.conn()?;
                let (session,workspace,state,cleanup):(String,String,String,bool)=conn.query_row("SELECT session_id,workspace,state,peer_cleanup_verified FROM agent_local_tasks WHERE task_id=?1",[task_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|_|"local_task_not_found")?;
                let mut s=conn.prepare("SELECT approval_id,phase,effect_started,cancel_requested,cleanup_verified,result_json FROM agent_tool_executions WHERE task_id=?1 ORDER BY rowid").map_err(|_|"local_read_failed")?;
                let rows=s.query_map([task_id],|r|Ok(json!({"approval_id":r.get::<_,String>(0)?,"phase":r.get::<_,String>(1)?,"effect_started":r.get::<_,bool>(2)?,"cancel_requested":r.get::<_,bool>(3)?,"cleanup_verified":r.get::<_,bool>(4)?,"result":r.get::<_,Option<String>>(5)?.and_then(|s|serde_json::from_str::<Value>(&s).ok())}))).map_err(|_|"local_read_failed")?.collect::<std::result::Result<Vec<_>,_>>().map_err(|_|"local_read_failed")?;
                Ok(
                    json!({"task_id":task_id,"session_id":session,"workspace":workspace,"state":state,"peer_cleanup_verified":cleanup,"executions":rows}),
                )
            }
            OperatorRequest::Shutdown => {
                self.stopping.store(true, Ordering::Release);
                for t in self.tasks.lock().map_err(|_| "local_faulted")?.values() {
                    t.cancel.store(true, Ordering::Release);
                }
                self.authority.revoke_all().map_err(|e| {
                    self.uncertain.store(true, Ordering::Release);
                    e
                })?;
                Ok(json!({"shutdown_requested":true,"cleanup_verified":false}))
            }
        }
    }
}

pub async fn serve(root: PathBuf) -> Result<()> {
    // No deployment, HOME/credential access or coexistence with native runtime
    // in this Core instance. The root must be an explicit disposable location.
    if root.parent() != Some(Path::new("/tmp"))
        || !root
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("narys-boundary-") && file_name(s))
    {
        return Err("disposable_boundary_root_required");
    }
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0
        || unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) } != 0
    {
        return Err("core_process_protection_unavailable");
    }
    crate::server::mkdir(&root)?;
    validate_workspace(&root)?;
    crate::server::mkdir(&root.join("workspaces"))?;
    let database = Database::new(root.join("db"));
    let mut conn = connection(&database)?;
    recover(&mut conn)?;
    // Recovery never reuses workspaces/grants. Unknown cleanup remains a closed
    // admission latch, including a crash before the first durable tool journal.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "local_recovery_failed")?;
    tx.execute("INSERT INTO agent_execution_events(approval_id,phase) SELECT approval_id,'uncertain_after_restart' FROM agent_tool_executions WHERE phase IN ('claimed','started')",[]).map_err(|_|"local_recovery_failed")?;
    tx.execute("UPDATE agent_tool_executions SET phase='uncertain',cleanup_verified=0 WHERE phase IN ('claimed','started')",[]).map_err(|_|"local_recovery_failed")?;
    tx.execute("UPDATE agent_local_tasks SET state='cleanup_uncertain' WHERE state IN ('active','cancel_requested') AND peer_cleanup_verified=0",[]).map_err(|_|"local_recovery_failed")?;
    let uncertain:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_local_tasks WHERE state='cleanup_uncertain') OR EXISTS(SELECT 1 FROM agent_tool_executions WHERE phase='uncertain')",[],|r|r.get(0)).map_err(|_|"local_recovery_failed")?;
    tx.commit().map_err(|_| "local_recovery_failed")?;
    let socket = root.join("operator.sock");
    if socket.exists() {
        let m = std::fs::symlink_metadata(&socket).map_err(|_| "unsafe_operator_socket")?;
        if !std::os::unix::fs::FileTypeExt::is_socket(&m.file_type()) {
            return Err("unsafe_operator_socket");
        }
        std::fs::remove_file(&socket).map_err(|_| "operator_socket_cleanup_failed")?;
    }
    let listener = UnixListener::bind(&socket).map_err(|_| "operator_bind_failed")?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| "operator_permissions_failed")?;
    let marker = root.join("private-marker");
    if !marker.exists() {
        std::fs::write(&marker, "SYNTHETIC_OFFLINE_PRIVATE_MARKER")
            .map_err(|_| "probe_unavailable")?;
    }
    let core = Arc::new(LocalCore {
        authority: AuthorityService::new(database)?,
        root,
        socket: socket.clone(),
        tasks: Mutex::new(BTreeMap::new()),
        pending: Mutex::new(BTreeMap::new()),
        jobs: Mutex::new(vec![]),
        peer_slots: Arc::new(Semaphore::new(4)),
        stopping: AtomicBool::new(false),
        uncertain: AtomicBool::new(uncertain),
    });
    println!(
        "{}",
        json!({"mode":"offline_local_boundary","operator_socket":socket,"native_copilot":"BLOCKED"})
    );
    let limit = Arc::new(Semaphore::new(8));
    let mut handlers = tokio::task::JoinSet::new();
    loop {
        if core.stopping.load(Ordering::Acquire) {
            break;
        }
        tokio::select! {
            r=listener.accept()=>{let (mut stream,_)=r.map_err(|_|"operator_accept_failed")?;let Ok(permit)=limit.clone().try_acquire_owned()else{continue};let c=core.clone();handlers.spawn(async move{let _permit=permit;let mut bytes=Vec::new();let read=tokio::time::timeout(Duration::from_secs(3),(&mut stream).take(20001).read_to_end(&mut bytes)).await;let result=if matches!(read,Ok(Ok(_)))&&bytes.len()<=20000{match serde_json::from_slice(&bytes){Ok(r)=>c.dispatch(r).await,Err(_)=>Err("invalid_operator_request")}}else{Err("operator_request_limit")};let response=match result{Ok(v)=>json!({"ok":true,"result":v}),Err(e)=>json!({"ok":false,"error_code":e})};let _=stream.write_all(response.to_string().as_bytes()).await;});},
            _=tokio::signal::ctrl_c()=>{core.stopping.store(true,Ordering::Release);},
            _=tokio::time::sleep(Duration::from_millis(20))=>{}
        }
        while let Some(r) = handlers.try_join_next() {
            if r.is_err() {
                core.uncertain.store(true, Ordering::Release);
            }
        }
        let _ = core.reap_jobs().await;
    }
    for t in core.tasks.lock().map_err(|_| "local_faulted")?.values() {
        t.cancel.store(true, Ordering::Release);
    }
    if core.authority.revoke_all().is_err() {
        core.uncertain.store(true, Ordering::Release);
    }
    // Let bounded admission/probe handlers certify cleanup. Aborting them here
    // would lose a process launched before its registration in jobs.
    while let Some(r) = handlers.join_next().await {
        if r.is_err() {
            core.uncertain.store(true, Ordering::Release);
        }
    }
    for task in core.tasks.lock().map_err(|_| "local_faulted")?.values() {
        task.cancel.store(true, Ordering::Release);
    }
    let jobs = std::mem::take(&mut *core.jobs.lock().map_err(|_| "local_faulted")?);
    for j in jobs {
        if j.await.is_err() {
            core.uncertain.store(true, Ordering::Release);
        }
    }
    std::fs::remove_file(socket).map_err(|_| "operator_socket_cleanup_failed")?;
    if core.uncertain.load(Ordering::Acquire) {
        return Err("local_cleanup_or_persistence_uncertain");
    }
    Ok(())
}

pub async fn request(socket: &Path, request: Value) -> Result<Value> {
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(|_| "boundary_connect_failed")?;
    stream
        .write_all(request.to_string().as_bytes())
        .await
        .map_err(|_| "boundary_write_failed")?;
    stream
        .shutdown()
        .await
        .map_err(|_| "boundary_write_failed")?;
    let mut bytes = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(8),
        stream.take(65537).read_to_end(&mut bytes),
    )
    .await
    .map_err(|_| "boundary_timeout")?
    .map_err(|_| "boundary_read_failed")?;
    if bytes.len() > 65536 {
        return Err("boundary_response_limit");
    }
    serde_json::from_slice(&bytes).map_err(|_| "boundary_response_invalid")
}
pub async fn cli(args: &[String]) -> Result<()> {
    let socket = Path::new(args.get(0).ok_or("operator_socket_required")?);
    let command = args
        .get(1)
        .map(String::as_str)
        .ok_or("boundary_command_required")?;
    let id = || args.get(2).ok_or("boundary_id_required");
    let v = match command {
        "status" | "approvals" | "shutdown" if args.len() == 2 => json!({"operation":command}),
        "approvals" if args.len() == 3 => {
            json!({"operation":"approvals","after":id()?.parse::<u64>().map_err(|_|"invalid_cursor")?})
        }
        "start-synthetic-peer" if args.len() == 4 => {
            let source =
                std::fs::read_to_string(&args[2]).map_err(|_| "synthetic_peer_read_failed")?;
            json!({"operation":"start_synthetic_peer","source":source,"ttl_seconds":args[3].parse::<u64>().map_err(|_|"invalid_ttl")?})
        }
        "show" | "deny" if args.len() == 3 => json!({"operation":command,"approval_id":id()?}),
        "task" | "cancel" if args.len() == 3 => {
            json!({"operation":command,"task_id":id()?.parse::<u64>().map_err(|_|"invalid_task_id")?})
        }
        "approve" if args.len() == 3 => {
            let shown = request(socket, json!({"operation":"show","approval_id":id()?})).await?;
            if shown["ok"] != true {
                return Err("approval_not_live");
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&shown).map_err(|_| "preview_invalid")?
            );
            let digest = shown["result"]["exact_preview"]["digest"]
                .as_str()
                .ok_or("exact_preview_unavailable")?;
            println!(
                "Confirme a operação exata digitando: approve {} {}",
                id()?,
                digest
            );
            let mut text = String::new();
            std::io::stdin()
                .lock()
                .take(200)
                .read_line(&mut text)
                .map_err(|_| "confirmation_read_failed")?;
            if text.trim_end() != format!("approve {} {}", id()?, digest) {
                return Err("explicit_confirmation_required");
            }
            json!({"operation":"approve","approval_id":id()?,"digest":digest})
        }
        _ => return Err("invalid_boundary_command"),
    };
    let response = request(socket, v).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response).map_err(|_| "boundary_response_invalid")?
    );
    if response["ok"] == true {
        Ok(())
    } else {
        Err("boundary_operation_refused")
    }
}
