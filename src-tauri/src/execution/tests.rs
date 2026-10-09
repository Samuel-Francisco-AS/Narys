//! LR-9B real Linux processes/PTYs. No network, UI reader or external provider.
use super::*;
use crate::{luna::task::TaskId, operational_trace::TraceId};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(15);
const PYTHON: &str = "/usr/bin/python3";

struct Runtime(Arc<ExecutionBroker>);
impl Runtime {
    fn new() -> Self {
        Self(ExecutionBroker::isolated())
    }
    fn idle(&self) {
        let deadline = Instant::now() + WAIT;
        while !self.0.stopped() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(self.0.active_count(), 0);
        assert_eq!(self.0.active_sessions(), 0);
        assert_eq!(self.0.worker_count(), 0);
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.0.request_shutdown();
        assert!(
            self.0.wait_shutdown(SHUTDOWN_DEADLINE),
            "fixture cleanup exceeded deadline"
        );
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static IDS: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "narys-lr9b-{}-{}",
            std::process::id(),
            IDS.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn request(program: &str, args: &[&str]) -> ExecutionRequest {
    ExecutionRequest {
        program: program.into(),
        args: args.iter().map(OsString::from).collect(),
        cwd: std::env::temp_dir(),
        origin: ExecutionOrigin::TestFixture,
        task_id: None,
        correlation: None,
        workspace: None,
        environment: EnvironmentPolicy::Controlled(vec![]),
        timeout: WAIT,
        capture: CapturePolicy::default(),
    }
}
fn python(script: &str) -> ExecutionRequest {
    request(PYTHON, &["-c", script])
}
fn submit(rt: &Runtime, r: &ExecutionRequest) -> ExecutionHandle {
    rt.0.submit(r, &ExecutionAuthority::fixture(&r.program))
        .unwrap()
}
fn done(h: &ExecutionHandle) -> Arc<ExecutionResult> {
    h.wait(WAIT).expect("execution did not finish")
}
fn reaped(r: &ExecutionResult) {
    assert!(r.reaped && !r.cleanup_pending, "{r:?}");
    let pid = r.process_id.unwrap();
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "direct child not reaped"
    );
    // waitid observes only direct children, therefore ECHILD proves actual reap.
    assert_eq!(
        os::exited(pid).unwrap_err().raw_os_error(),
        Some(libc::ECHILD)
    );
}
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "condition deadline");
        thread::sleep(Duration::from_millis(5));
    }
}
fn running(h: &ExecutionHandle) {
    until(|| h.state() == ExecutionState::Running);
}
fn dimensions() -> PtyDimensions {
    PtyDimensions { rows: 24, cols: 80 }
}
fn human_request(program: &str, args: &[&str]) -> ExecutionRequest {
    let mut r = request(program, args);
    r.origin = ExecutionOrigin::Human;
    r
}
fn shell(rt: &Runtime, cwd: &Path) -> PtySession {
    let mut r = human_request("/bin/sh", &["-i"]);
    r.cwd = cwd.into();
    r.environment = EnvironmentPolicy::Controlled(vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("PS1".into(), "".into()),
        ("ENV".into(), "/dev/null".into()),
    ]);
    let s =
        rt.0.open_pty(&r, dimensions(), &ExecutionAuthority::human_local())
            .unwrap();
    until(|| s.state() == ExecutionState::Running);
    input(&s, "stty -echo; printf '%s%s\\n' LR9B_ READY\n");
    wait_text(&s, b"LR9B_READY");
    s
}
fn input(s: &PtySession, command: &str) {
    s.send_input(
        &ExecutionOrigin::Human,
        &ExecutionAuthority::human_local(),
        command.as_bytes(),
    )
    .unwrap();
}
fn retained(s: &PtySession) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut cursor = 0;
    loop {
        let b = s.replay(cursor, MAX_BATCH_CHUNKS, MAX_BATCH_BYTES).unwrap();
        bytes.extend(b.chunks.iter().flat_map(|c| c.bytes.iter()).copied());
        cursor = b.next_after;
        if !b.has_more {
            return bytes;
        }
        assert!(bytes.len() <= PTY_RETAIN_BYTES);
    }
}
fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}
fn wait_text(s: &PtySession, text: &[u8]) {
    until(|| contains(&retained(s), text));
}
fn close(s: &PtySession) -> Arc<ExecutionResult> {
    assert!(s.close(&ExecutionAuthority::human_local()).unwrap());
    s.wait(WAIT).unwrap()
}

#[test]
fn ids_concurrent_monotonic_zero_invalid_exhaustion_without_wrap() {
    let ids = Arc::new(contract::IdAllocator::default());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let ids = ids.clone();
            thread::spawn(move || {
                (0..128)
                    .map(|_| ids.next().unwrap().get())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut found: Vec<_> = threads
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    found.sort_unstable();
    assert_eq!(found, (1..=1024).collect::<Vec<_>>());
    ids.0.store(u64::MAX - 1, Ordering::Relaxed);
    assert_eq!(ids.next().unwrap().get(), u64::MAX);
    assert_eq!(ids.next(), Err(ExecutionError::IdExhausted));
    assert_eq!(ids.next(), Err(ExecutionError::IdExhausted));
}
#[test]
fn request_limits_and_paths_reject_before_admission() {
    let rt = Runtime::new();
    let base = python("pass");
    let mut cases = Vec::new();
    let mut r = base.clone();
    r.args = vec!["x".into(); MAX_ARGS + 1];
    cases.push(r);
    let mut r = base.clone();
    r.args = vec!["x".repeat(MAX_ARG_BYTES + 1).into()];
    cases.push(r);
    let mut r = base.clone();
    r.args = vec!["bad\0arg".into()];
    cases.push(r);
    let mut r = base.clone();
    r.program = "x".repeat(MAX_PATH_BYTES + 1).into();
    cases.push(r);
    let mut r = base.clone();
    r.program = "".into();
    cases.push(r);
    let mut r = base.clone();
    r.cwd = "x".repeat(MAX_PATH_BYTES + 1).into();
    cases.push(r);
    for timeout in [Duration::ZERO, MAX_TIMEOUT + Duration::from_secs(1)] {
        let mut r = base.clone();
        r.timeout = timeout;
        cases.push(r);
    }
    let mut r = base.clone();
    r.capture.stdout_bytes = MAX_CAPTURE_BYTES + 1;
    cases.push(r);
    let mut r = base.clone();
    r.capture.stderr_bytes = MAX_CAPTURE_BYTES + 1;
    cases.push(r);
    for id in [0, 9_007_199_254_740_992] {
        let mut r = base.clone();
        r.task_id = Some(TaskId(id));
        cases.push(r);
    }
    for env in [
        vec![("".into(), "x".into())],
        vec![("x=y".into(), "x".into())],
        vec![("key".into(), "bad\0value".into())],
        vec![("k".into(), "x".repeat(MAX_ENV_BYTES).into())],
        vec![("k".into(), "v".into()); MAX_ENV_ENTRIES + 1],
    ] {
        let mut r = base.clone();
        r.environment = EnvironmentPolicy::Controlled(env);
        cases.push(r);
    }
    for r in cases {
        assert!(rt
            .0
            .submit(&r, &ExecutionAuthority::fixture(&r.program))
            .is_err());
    }
    assert_eq!(rt.0.active_count(), 0);
    let mut boundary = base;
    boundary.args = vec!["x".repeat(MAX_ARG_BYTES).into()];
    assert!(boundary.validate().is_ok());
    boundary.args = vec![OsString::new(); MAX_ARGS];
    assert!(boundary.validate().is_ok());
    assert!(WorkspaceScope::confined(&[]).is_err());
    assert!(WorkspaceScope::confined(&vec![std::env::temp_dir(); MAX_SCOPE_ROOTS + 1]).is_err());
}
#[test]
fn structured_executable_argv_is_literal_bytes_without_implicit_shell() {
    let rt = Runtime::new();
    let r = request("/usr/bin/printf", &["%s", "a b;$(touch impossible)\n'\"$"]);
    let result = done(&submit(&rt, &r));
    assert_eq!(result.stdout.bytes, b"a b;$(touch impossible)\n'\"$");
    assert_eq!(result.state, ExecutionState::Completed);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.request.args, r.args);
    assert_eq!(result.mode, ExecutionMode::Structured);
    reaped(&result);
    rt.idle();
}
#[test]
fn cwd_canonical_scope_valid_and_symlink_escape_rejected() {
    let rt = Runtime::new();
    let root = Directory::new();
    let outside = Directory::new();
    std::fs::create_dir(root.0.join("inner")).unwrap();
    std::os::unix::fs::symlink(&outside.0, root.0.join("escape")).unwrap();
    let mut r = python("import os; print(os.getcwd())");
    r.workspace = Some(WorkspaceScope::confined(&[root.0.clone()]).unwrap());
    r.cwd = root.0.join("inner/..");
    let result = done(&submit(&rt, &r));
    assert_eq!(result.request.cwd, root.0.canonicalize().unwrap());
    assert_eq!(
        String::from_utf8(result.stdout.bytes.clone())
            .unwrap()
            .trim(),
        root.0.to_str().unwrap()
    );
    reaped(&result);
    r.cwd = root.0.join("escape");
    assert!(matches!(
        rt.0.submit(&r, &ExecutionAuthority::fixture(&r.program)),
        Err(ExecutionError::CwdOutsideScope)
    ));
    r.cwd = root.0.join("missing");
    assert_eq!(r.validate().unwrap_err(), ExecutionError::InvalidCwd);
    // A validated initial cwd is intentionally NOT a filesystem sandbox.
    r.cwd = root.0.clone();
    r.args = vec![
        "-c".into(),
        "import os; print(os.path.isdir('/usr'))".into(),
    ];
    assert_eq!(done(&submit(&rt, &r)).stdout.bytes, b"True\n");
    rt.idle();
}
#[test]
fn stdout_stderr_binary_exit_and_nonzero_are_factual() {
    let rt = Runtime::new();
    let r = python(
        "import os; os.write(1,b'out\\x00\\xff'); os.write(2,b'err\\xfe'); raise SystemExit(7)",
    );
    let result = done(&submit(&rt, &r));
    assert_eq!(result.stdout.bytes, b"out\0\xff");
    assert_eq!(result.stderr.bytes, b"err\xfe");
    assert_eq!(result.exit_code, Some(7));
    assert_eq!(result.state, ExecutionState::Failed);
    assert!(!result.spawn_failed && !result.runtime_error);
    reaped(&result);
    rt.idle();
}
#[test]
fn spawn_failure_returns_terminal_without_child() {
    let rt = Runtime::new();
    let r = request("/no/such/narys-lr9b-program", &[]);
    let result = done(&submit(&rt, &r));
    assert!(result.spawn_failed);
    assert_eq!(result.state, ExecutionState::Failed);
    assert!(!result.reaped);
    assert!(result.process_id.is_none());
    assert!(!result.cleanup_pending);
    rt.idle();
}
const FLOOD: &str = "import os,threading\ndef flood(fd):\n for i in range(768): os.write(fd,b'x'*8192)\na=threading.Thread(target=flood,args=(1,)); b=threading.Thread(target=flood,args=(2,)); a.start(); b.start(); a.join(); b.join(); os.write(1,b'END')";
#[test]
fn simultaneous_stdout_stderr_overflow_drains_without_deadlock() {
    let rt = Runtime::new();
    let r = python(FLOOD);
    let result = done(&submit(&rt, &r));
    assert_eq!(result.state, ExecutionState::Completed);
    assert_eq!(result.exit_code, Some(0));
    for (out, total) in [
        (&result.stdout, 768 * 8192 + 3),
        (&result.stderr, 768 * 8192),
    ] {
        assert_eq!(out.total_bytes, total);
        assert_eq!(out.captured_bytes(), MAX_CAPTURE_BYTES);
        assert_eq!(out.dropped_bytes, total - MAX_CAPTURE_BYTES as u64);
        assert!(out.truncated);
        assert!(!out.read_error && !out.incomplete);
    }
    reaped(&result);
    rt.idle();
}
#[test]
fn zero_capture_still_drains_and_accounts_every_byte() {
    let rt = Runtime::new();
    let mut r = python(FLOOD);
    r.capture = CapturePolicy {
        stdout_bytes: 0,
        stderr_bytes: 31,
    };
    let result = done(&submit(&rt, &r));
    assert_eq!(result.state, ExecutionState::Completed);
    assert!(result.stdout.bytes.is_empty());
    assert_eq!(result.stdout.dropped_bytes, result.stdout.total_bytes);
    assert_eq!(result.stderr.bytes.len(), 31);
    assert_eq!(result.stderr.dropped_bytes + 31, result.stderr.total_bytes);
    reaped(&result);
    rt.idle();
}
#[test]
fn signal_exit_is_distinct_from_exit_code() {
    let rt = Runtime::new();
    let result = done(&submit(
        &rt,
        &python("import os,signal; os.kill(os.getpid(),signal.SIGTERM)"),
    ));
    assert_eq!(result.state, ExecutionState::Failed);
    assert_eq!(result.signal.as_deref(), Some("15"));
    assert_eq!(result.exit_code, None);
    reaped(&result);
    rt.idle();
}
#[test]
fn timeout_is_sticky_even_if_term_handler_exits_zero() {
    let rt = Runtime::new();
    let mut r=python("import signal,time; signal.signal(signal.SIGTERM,lambda *_:exit(0)); print('ready',flush=True); time.sleep(60)");
    r.timeout = Duration::from_millis(500);
    let h = submit(&rt, &r);
    let result = done(&h);
    assert_eq!(result.state, ExecutionState::TimedOut);
    assert!(!h.cancel());
    reaped(&result);
    rt.idle();
}
#[test]
fn cancellation_term_grace_kill_and_reap() {
    let rt = Runtime::new();
    let dir = Directory::new();
    let ready = dir.0.join("ready");
    let script=format!("import signal,time,pathlib; signal.signal(signal.SIGTERM,signal.SIG_IGN); pathlib.Path({:?}).touch(); time.sleep(60)",ready.to_str().unwrap());
    let h = submit(&rt, &python(&script));
    until(|| ready.exists());
    assert!(h.cancel());
    assert!(!h.cancel());
    let result = done(&h);
    assert_eq!(result.state, ExecutionState::Cancelled);
    assert_eq!(result.signal.as_deref(), Some("9"));
    reaped(&result);
    rt.idle();
}
#[test]
fn process_group_cleanup_kills_ordinary_descendant() {
    let rt = Runtime::new();
    let dir = Directory::new();
    let ready = dir.0.join("child");
    let script=format!("import os,signal,time,pathlib\npid=os.fork()\nif pid==0:\n signal.signal(signal.SIGTERM,signal.SIG_IGN); pathlib.Path({:?}).write_text(str(os.getpid())); time.sleep(60)\nelse:\n signal.signal(signal.SIGTERM,lambda *_:None)\n os.waitpid(pid,0)",ready.to_str().unwrap());
    let h = submit(&rt, &python(&script));
    until(|| {
        ready.exists()
            && std::fs::read_to_string(&ready)
                .unwrap_or_default()
                .parse::<u32>()
                .is_ok()
    });
    let child: u32 = std::fs::read_to_string(&ready).unwrap().parse().unwrap();
    assert!(h.cancel());
    let r = done(&h);
    assert_eq!(r.state, ExecutionState::Cancelled);
    reaped(&r);
    // Grandchildren are reaped by their parent/init, not waitable by this broker.
    until(|| {
        std::fs::read_to_string(format!("/proc/{child}/stat")).map_or(true, |s| {
            s.rsplit_once(')').unwrap().1.trim_start().starts_with('Z')
        })
    });
    rt.idle();
}
#[test]
fn max_active_admission_and_shutdown_are_bounded() {
    let rt = Runtime::new();
    let r = python("import time; time.sleep(60)");
    let handles: Vec<_> = (0..MAX_ACTIVE_EXECUTIONS)
        .map(|_| submit(&rt, &r))
        .collect();
    assert!(matches!(
        rt.0.submit(&r, &ExecutionAuthority::fixture(&r.program)),
        Err(ExecutionError::ActiveLimit)
    ));
    rt.0.request_shutdown();
    for h in handles {
        let r = done(&h);
        assert_eq!(r.state, ExecutionState::Cancelled);
        if r.process_id.is_some() {
            reaped(&r);
        }
    }
    rt.idle();
    assert!(matches!(
        rt.0.submit(&r, &ExecutionAuthority::fixture(&r.program)),
        Err(ExecutionError::ShuttingDown)
    ));
}
#[test]
fn agent_provenance_never_grants_execution_or_human_pty_input() {
    let rt = Runtime::new();
    let human = ExecutionAuthority::human_local();
    let fixture = ExecutionAuthority::fixture(Path::new(PYTHON));
    for origin in [
        ExecutionOrigin::SpecialistAgent(TraceId::new("Codex").unwrap()),
        ExecutionOrigin::SpecialistAgent(TraceId::new("Copilot").unwrap()),
        ExecutionOrigin::Worker(TraceId::new("worker").unwrap()),
        ExecutionOrigin::CognitiveProvider(TraceId::new("provider").unwrap()),
    ] {
        let mut r = python("pass");
        r.origin = origin.clone();
        assert!(matches!(
            rt.0.submit(&r, &human),
            Err(ExecutionError::AuthorityDenied)
        ));
        assert!(matches!(
            rt.0.submit(&r, &fixture),
            Err(ExecutionError::AuthorityDenied)
        ));
        assert!(matches!(
            rt.0.open_pty(&r, dimensions(), &human),
            Err(ExecutionError::AuthorityDenied)
        ));
        assert_eq!(
            human.authorize_input(&origin),
            Err(ExecutionError::AuthorityDenied)
        );
    }
    let s = shell(&rt, &std::env::temp_dir());
    assert_eq!(
        s.send_input(
            &ExecutionOrigin::SpecialistAgent(TraceId::new("Codex").unwrap()),
            &human,
            b"exit\n"
        ),
        Err(ExecutionError::AuthorityDenied)
    );
    assert_eq!(s.close(&fixture), Err(ExecutionError::AuthorityDenied));
    assert_eq!(
        s.resize(dimensions(), &fixture),
        Err(ExecutionError::AuthorityDenied)
    );
    close(&s);
    rt.idle();
}
#[test]
fn fixture_authority_is_scoped_to_program_and_controlled_environment() {
    let rt = Runtime::new();
    let mut r = python("pass");
    let authority = ExecutionAuthority::fixture(Path::new("/bin/sh"));
    assert!(matches!(
        rt.0.submit(&r, &authority),
        Err(ExecutionError::AuthorityDenied)
    ));
    r.environment = EnvironmentPolicy::HumanInherited;
    assert!(matches!(
        rt.0.submit(&r, &ExecutionAuthority::fixture(&r.program)),
        Err(ExecutionError::AuthorityDenied)
    ));
}
#[test]
fn controlled_environment_does_not_inherit_artificial_secret_human_can_opt_in() {
    const KEY: &str = "NARYS_LR9B_SYNTHETIC_ENV_SECRET";
    struct Env;
    impl Drop for Env {
        fn drop(&mut self) {
            std::env::remove_var(KEY);
        }
    }
    assert!(std::env::var_os(KEY).is_none());
    std::env::set_var(KEY, "artificial-only");
    let _env = Env;
    let rt = Runtime::new();
    let r=python(&format!("import os; print(os.environ.get('{KEY}','ABSENT')); print(os.environ.get('EXPLICIT','ABSENT'))"));
    let result = done(&submit(&rt, &r));
    assert_eq!(result.stdout.bytes, b"ABSENT\nABSENT\n");
    let mut explicit = r.clone();
    explicit.environment =
        EnvironmentPolicy::Controlled(vec![("EXPLICIT".into(), "present".into())]);
    assert_eq!(
        done(&submit(&rt, &explicit)).stdout.bytes,
        b"ABSENT\npresent\n"
    );
    let mut human = r;
    human.origin = ExecutionOrigin::Human;
    human.environment = EnvironmentPolicy::HumanInherited;
    assert_eq!(
        done(
            &rt.0
                .submit(&human, &ExecutionAuthority::human_local())
                .unwrap()
        )
        .stdout
        .bytes,
        b"artificial-only\nABSENT\n"
    );
    rt.idle();
}
#[test]
fn trace_rejection_and_absent_ui_do_not_control_execution() {
    let rt = Runtime::new();
    rt.0.reject_trace.store(true, Ordering::Release);
    let registry = crate::luna::runtime::TaskRegistry::default();
    assert!(registry.suspend_ui_if_safe());
    registry.events.detach_main();
    let h = submit(&rt, &python("import os; os.write(1,b'headless')"));
    let r = done(&h);
    assert_eq!(r.state, ExecutionState::Completed);
    assert_eq!(r.stdout.bytes, b"headless");
    reaped(&r);
    registry.resume_ui();
    rt.idle();
}
#[test]
fn pty_real_tty_shell_input_pwd_cd_command_and_simple_interaction() {
    let rt = Runtime::new();
    let dir = Directory::new();
    std::fs::create_dir(dir.0.join("inner")).unwrap();
    let s = shell(&rt, &dir.0);
    assert_eq!(s.origin(), &ExecutionOrigin::Human);
    assert_eq!(s.request().cwd, dir.0);
    input(&s,"test -t 0 && printf '%s%s\\n' REAL_ TTY; pwd; cd inner; pwd; printf '%s%s\\n' SIMPLE_ COMMAND; read answer; printf 'ANSWER:%s\\n' \"$answer\"\n");
    wait_text(&s, b"REAL_TTY");
    wait_text(&s, dir.0.join("inner").to_str().unwrap().as_bytes());
    wait_text(&s, b"SIMPLE_COMMAND");
    input(&s, "hello world\n");
    wait_text(&s, b"ANSWER:hello world");
    input(&s, "exit 0\n");
    let r = s.wait(WAIT).unwrap();
    assert_eq!(r.state, ExecutionState::Completed);
    assert_eq!(r.exit_code, Some(0));
    reaped(&r);
    rt.idle();
}
#[test]
fn pty_raw_input_and_non_utf8_output_are_bytes() {
    let rt = Runtime::new();
    let r=human_request(PYTHON,&["-c","import os,tty; tty.setraw(0); os.write(1,b'RAW_READY'); data=os.read(0,3); os.write(1,b'RAW:'+data+b'\\xff')"]);
    let s =
        rt.0.open_pty(&r, dimensions(), &ExecutionAuthority::human_local())
            .unwrap();
    wait_text(&s, b"RAW_READY");
    s.send_input(
        &ExecutionOrigin::Human,
        &ExecutionAuthority::human_local(),
        b"\0\xfeX",
    )
    .unwrap();
    let result = s.wait(WAIT).unwrap();
    assert_eq!(result.state, ExecutionState::Completed);
    assert!(contains(&retained(&s), b"RAW:\0\xfeX\xff"));
    reaped(&result);
    rt.idle();
}
#[test]
fn pty_resize_reaches_real_slave_stty_and_rejects_invalid_dimensions() {
    let rt = Runtime::new();
    let s = shell(&rt, &std::env::temp_dir());
    let human = ExecutionAuthority::human_local();
    for d in [
        PtyDimensions { rows: 0, cols: 80 },
        PtyDimensions { rows: 24, cols: 0 },
        PtyDimensions {
            rows: MAX_DIMENSION + 1,
            cols: 80,
        },
    ] {
        assert_eq!(s.resize(d, &human), Err(ExecutionError::InvalidDimensions));
    }
    s.resize(
        PtyDimensions {
            rows: 41,
            cols: 123,
        },
        &human,
    )
    .unwrap();
    input(&s, "stty size\n");
    wait_text(&s, b"41 123");
    assert_eq!(
        s.replay(0, MAX_BATCH_CHUNKS, MAX_BATCH_BYTES)
            .unwrap()
            .dimensions
            .rows,
        41
    );
    reaped(&close(&s));
    assert_eq!(
        s.resize(dimensions(), &human),
        Err(ExecutionError::NotRunning)
    );
    rt.idle();
}
#[test]
fn pty_explicit_close_reaps_and_rejects_later_input() {
    let rt = Runtime::new();
    let s = shell(&rt, &std::env::temp_dir());
    let r = close(&s);
    assert_eq!(r.state, ExecutionState::Cancelled);
    reaped(&r);
    assert_eq!(
        s.send_input(
            &ExecutionOrigin::Human,
            &ExecutionAuthority::human_local(),
            b"late"
        ),
        Err(ExecutionError::NotRunning)
    );
    assert!(!s.close(&ExecutionAuthority::human_local()).unwrap());
    rt.idle();
}
#[test]
fn pty_timeout_and_spawn_failure_are_factual() {
    let rt = Runtime::new();
    let mut r = human_request(PYTHON, &["-c", "import time; time.sleep(60)"]);
    r.timeout = Duration::from_millis(300);
    let s =
        rt.0.open_pty(&r, dimensions(), &ExecutionAuthority::human_local())
            .unwrap();
    let result = s.wait(WAIT).unwrap();
    assert_eq!(result.state, ExecutionState::TimedOut);
    reaped(&result);
    let r = human_request("/no/such/narys-lr9b-shell", &[]);
    let s =
        rt.0.open_pty(&r, dimensions(), &ExecutionAuthority::human_local())
            .unwrap();
    let result = s.wait(WAIT).unwrap();
    assert!(result.spawn_failed);
    assert_eq!(result.state, ExecutionState::Failed);
    assert!(!result.reaped);
    rt.idle();
}
#[test]
fn pty_session_limits_ids_human_owner_and_shutdown_readers_stop() {
    let rt = Runtime::new();
    let mut sessions = Vec::new();
    for _ in 0..MAX_PTY_SESSIONS {
        sessions.push(shell(&rt, &std::env::temp_dir()));
    }
    assert!(sessions
        .windows(2)
        .all(|w| w[0].id().get() < w[1].id().get()));
    let r = human_request("/bin/sh", &["-i"]);
    assert!(matches!(
        rt.0.open_pty(&r, dimensions(), &ExecutionAuthority::human_local()),
        Err(ExecutionError::SessionLimit)
    ));
    rt.0.request_shutdown();
    for s in sessions {
        let r = s.wait(WAIT).unwrap();
        assert_eq!(r.state, ExecutionState::Cancelled);
        reaped(&r);
    }
    rt.idle();
}
#[test]
fn pty_batch_cursor_limits_and_input_budget() {
    let rt = Runtime::new();
    let s = shell(&rt, &std::env::temp_dir());
    let human = ExecutionAuthority::human_local();
    assert_eq!(
        s.send_input(
            &ExecutionOrigin::Human,
            &human,
            &vec![0; MAX_INPUT_BYTES + 1]
        ),
        Err(ExecutionError::InputTooLarge)
    );
    for (chunks, bytes) in [
        (0, MAX_BATCH_BYTES),
        (MAX_BATCH_CHUNKS + 1, MAX_BATCH_BYTES),
        (1, READ_CHUNK_BYTES - 1),
        (1, MAX_BATCH_BYTES + 1),
    ] {
        assert!(matches!(
            s.replay(0, chunks, bytes),
            Err(ExecutionError::InvalidBatch)
        ));
    }
    assert!(matches!(
        s.replay(u64::MAX, 1, READ_CHUNK_BYTES),
        Err(ExecutionError::FutureCursor)
    ));
    let b = s.replay(0, 1, READ_CHUNK_BYTES).unwrap();
    assert!(!b.gap);
    assert!(b.chunks.len() <= 1);
    let next = s
        .replay(b.next_after, MAX_BATCH_CHUNKS, MAX_BATCH_BYTES)
        .unwrap();
    assert!(next.chunks.iter().all(|c| c.sequence > b.next_after));
    close(&s);
    rt.idle();
}
#[test]
fn structured_multi_process_stress_gate() {
    let rt = Runtime::new();
    let success = submit(&rt, &python(FLOOD));
    let second = submit(&rt, &python(FLOOD));
    let cancelled = submit(&rt, &python("import time; time.sleep(60)"));
    running(&cancelled);
    let mut timeout = python("import time; time.sleep(60)");
    timeout.timeout = Duration::from_millis(300);
    let timed = submit(&rt, &timeout);
    assert!(cancelled.cancel());
    for h in [success, second] {
        let r = done(&h);
        assert_eq!(r.state, ExecutionState::Completed);
        assert!(r.stdout.truncated && r.stderr.truncated);
        reaped(&r);
    }
    for (h, state) in [
        (cancelled, ExecutionState::Cancelled),
        (timed, ExecutionState::TimedOut),
    ] {
        let r = done(&h);
        assert_eq!(r.state, state);
        reaped(&r);
    }
    rt.idle();
    eprintln!("LR-9B structured stress: 4 processes, 2 dual 6 MiB bursts, cancelled + timed out, all reaped; active=0 readers=0");
}
#[test]
fn pty_no_subscriber_overflow_cursor_resize_then_command_exit_stress_gate() {
    let rt = Runtime::new();
    let s = shell(&rt, &std::env::temp_dir());
    // The observer is not read at all until the child has emitted > retention
    // and written its completion marker on the filesystem.
    let dir = Directory::new();
    let marker = dir.0.join("flood-finished");
    input(&s,&format!("/usr/bin/python3 -c \"import os,pathlib; [os.write(1,b'x'*8192) for _ in range(768)]; pathlib.Path('{}').touch()\"\n",marker.display()));
    until(|| marker.exists());
    input(&s, "printf '%s%s\\n' OVERFLOW_ ALIVE\n");
    wait_text(&s, b"OVERFLOW_ALIVE");
    assert_eq!(s.state(), ExecutionState::Running);
    let b = s.replay(0, MAX_BATCH_CHUNKS, MAX_BATCH_BYTES).unwrap();
    assert!(b.gap && b.dropped_bytes > 0 && b.dropped_chunks > 0);
    assert!(b.retained_bytes <= PTY_RETAIN_BYTES && b.retained_chunks <= PTY_RETAIN_CHUNKS);
    assert_eq!(b.total_bytes, b.retained_bytes as u64 + b.dropped_bytes);
    assert_eq!(
        b.latest_sequence,
        b.retained_chunks as u64 + b.dropped_chunks
    );
    assert_eq!(b.retained_range.unwrap().0, b.dropped_chunks + 1);
    assert!(b.has_more);
    assert_eq!(b.next_after, b.chunks.last().unwrap().sequence);
    let next = s
        .replay(b.next_after, MAX_BATCH_CHUNKS, MAX_BATCH_BYTES)
        .unwrap();
    assert!(!next.gap);
    assert!(next.chunks.iter().all(|c| c.sequence > b.next_after));
    s.resize(
        PtyDimensions {
            rows: 37,
            cols: 101,
        },
        &ExecutionAuthority::human_local(),
    )
    .unwrap();
    input(&s, "stty size; printf '%s%s\\n' AFTER_ RESIZE\n");
    wait_text(&s, b"37 101");
    wait_text(&s, b"AFTER_RESIZE");
    input(&s, "exit\n");
    let result = s.wait(WAIT).unwrap();
    assert_eq!(result.state, ExecutionState::Completed);
    reaped(&result);
    rt.idle();
    let b = s.replay(0, MAX_BATCH_CHUNKS, MAX_BATCH_BYTES).unwrap();
    eprintln!("LR-9B PTY stress: bytes={} retained={} chunks={} dropped={} dropped_chunks={} gap={} active=0 readers=0",b.total_bytes,b.retained_bytes,b.retained_chunks,b.dropped_bytes,b.dropped_chunks,b.gap);
}
#[test]
fn presentation_detach_does_not_shutdown_execution_plane_or_pty() {
    let rt = Runtime::new();
    let s = shell(&rt, &std::env::temp_dir());
    let registry = crate::luna::runtime::TaskRegistry::default();
    registry.events.detach_main();
    registry.detach_ui_bound();
    assert!(registry.suspend_ui_if_safe());
    input(&s, "printf '%s%s\\n' HEADLESS_ PTY\n");
    wait_text(&s, b"HEADLESS_PTY");
    assert_eq!(s.state(), ExecutionState::Running);
    registry.resume_ui();
    close(&s);
    rt.idle();
    // Structural check of the actual host: the native close path has no broker
    // teardown; only explicit Quit and Exit do. No fake WebView test is claimed.
    let host = include_str!("../presentation.rs");
    let close_path = host
        .split("pub fn close(app:")
        .nth(1)
        .unwrap()
        .split("pub fn destroy_window")
        .next()
        .unwrap();
    assert!(!close_path.contains("ExecutionBroker") && !close_path.contains("request_shutdown"));
    assert!(host
        .split("pub fn request_quit")
        .nth(1)
        .unwrap()
        .contains("ExecutionBroker"));
    for source in [
        include_str!("mod.rs"),
        include_str!("broker.rs"),
        include_str!("pty.rs"),
        include_str!("contract.rs"),
    ] {
        for forbidden in [
            "tauri::ipc",
            "tauri::command",
            "crate::presentation",
            "crate::agents",
            "crate::cognition",
        ] {
            assert!(!source.contains(forbidden));
        }
    }
}

#[test]
fn human_shell_resolution_and_native_open_have_human_owner() {
    let rt = Runtime::new();
    let resolved = resolve_human_shell().unwrap();
    assert!(resolved.is_absolute() && resolved.is_file());
    let s =
        rt.0.open_human_shell(
            &std::env::temp_dir(),
            dimensions(),
            &ExecutionAuthority::human_local(),
        )
        .unwrap();
    until(|| s.state() == ExecutionState::Running);
    assert_eq!(s.origin(), &ExecutionOrigin::Human);
    assert_eq!(s.request().program, resolved);
    reaped(&close(&s));
    rt.idle();
}
#[test]
fn timeout_wins_over_late_cancel_in_terminal_control() {
    // Direct state-machine proof avoids racing against wall-clock scheduling.
    let rt = Runtime::new();
    let h =
        rt.0.launch(ExecutionMode::Structured, |h, b| {
            assert_eq!(
                h.control.stop_cause(Some(ExecutionState::TimedOut)),
                Some(ExecutionState::TimedOut)
            );
            assert!(!h.cancel());
            let mut r = broker::initial_result(&h, python("pass"), ExecutionMode::Structured);
            r.state = ExecutionState::Completed;
            broker::finish(&h, &b, r);
        })
        .unwrap();
    assert_eq!(done(&h).state, ExecutionState::TimedOut);
    rt.idle();
}
#[test]
fn lifecycle_trace_has_mode_identity_correlation_and_no_process_content() {
    use crate::operational_trace::{BatchLimits, OperationalKind, OperationalTraceBus, SourceType};
    let rt = Runtime::new();
    let correlation = TraceId::new("lr9b-lifecycle-proof").unwrap();
    let mut r = python("print('PRIVATE_SYNTHETIC_PROCESS_CONTENT')");
    r.correlation = Some(correlation.clone());
    r.task_id = Some(TaskId(93));
    let result = done(&submit(&rt, &r));
    rt.idle();
    let bus = OperationalTraceBus::process_wide();
    let mut cursor = 0;
    let mut codes = Vec::new();
    loop {
        let batch = bus.replay(cursor, BatchLimits::default()).unwrap();
        for e in batch.events {
            if e.provenance().correlation_id.as_ref() == Some(&correlation) {
                assert_eq!(
                    e.provenance().source.source_type,
                    SourceType::ExecutionBroker
                );
                assert_eq!(e.provenance().task_id, Some(TaskId(93)));
                assert_eq!(
                    e.provenance().source.instance.as_ref().unwrap().as_str(),
                    format!("execution:{}", result.id.get())
                );
                let (code, text) = match e.kind() {
                    OperationalKind::State { code, detail, .. } => (code, detail.as_str()),
                    OperationalKind::Critical { code, message, .. } => (code, message.as_str()),
                    _ => panic!("process content published"),
                };
                assert!(!text.contains("PRIVATE"));
                codes.push(code.as_str().to_owned());
            }
        }
        cursor = batch.next_after;
        if !batch.has_more {
            break;
        }
    }
    assert_eq!(codes, ["exec_starting", "exec_running", "exec_completed"]);
}
#[test]
fn pty_close_escalates_to_kill_for_term_ignoring_child() {
    let rt = Runtime::new();
    let r=human_request(PYTHON,&["-c","import os,signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); os.write(1,b'IGNORE_TERM_READY'); time.sleep(60)"]);
    let s =
        rt.0.open_pty(&r, dimensions(), &ExecutionAuthority::human_local())
            .unwrap();
    wait_text(&s, b"IGNORE_TERM_READY");
    let result = close(&s);
    assert_eq!(result.state, ExecutionState::Cancelled);
    assert!(result.signal.is_some());
    reaped(&result);
    rt.idle();
}

#[test]
fn singleton_and_deferred_reap_preserve_terminal_cause_and_cleanup_facts() {
    use std::os::unix::process::ExitStatusExt;
    assert!(Arc::ptr_eq(
        &ExecutionBroker::process_wide(),
        &ExecutionBroker::process_wide()
    ));
    let rt = Runtime::new();
    let h =
        rt.0.launch(ExecutionMode::Structured, |h, b| {
            let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
            let pid = child.id();
            h.control.running(pid);
            assert!(h.cancel());
            // Force only the deferred publication path; no kernel-stuck child or
            // elapsed-time assertion is required to prove retained-child reap.
            let mut r = broker::initial_result(
                &h,
                request("/usr/bin/true", &[]),
                ExecutionMode::Structured,
            );
            r.cleanup_pending = true;
            r.process_id = Some(pid);
            broker::finish(&h, &b, r);
            os::deferred_reap(
                pid,
                || {
                    child
                        .wait()
                        .map(|s| (s.code(), s.signal().map(|n| n.to_string())))
                },
                &h,
            );
        })
        .unwrap();
    done(&h);
    rt.idle();
    let result = h.result().unwrap();
    assert_eq!(result.state, ExecutionState::Cancelled);
    assert_eq!(result.exit_code, Some(0));
    reaped(&result);
}

#[test]
fn pty_job_control_cleanup_preserves_separate_structured_process() {
    let rt = Runtime::new();
    let dir = Directory::new();
    let ready = dir.0.join("job-pid");
    let separate = submit(&rt, &python("import time; time.sleep(60)"));
    running(&separate);
    let s = shell(&rt, &dir.0);
    input(&s,&format!("/usr/bin/python3 -c \"import os,signal,pathlib,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); pathlib.Path('{}').write_text(str(os.getpid())); time.sleep(60)\" &\n",ready.display()));
    until(|| {
        std::fs::read_to_string(&ready)
            .unwrap_or_default()
            .parse::<u32>()
            .is_ok()
    });
    let job: u32 = std::fs::read_to_string(&ready).unwrap().parse().unwrap();
    // SAFETY: read-only kernel query of the fixture's published PID.
    assert_ne!(
        unsafe { libc::getpgid(job as i32) },
        s.process_id().unwrap() as i32
    );
    let result = close(&s);
    assert_eq!(result.state, ExecutionState::Cancelled);
    reaped(&result);
    until(|| {
        std::fs::read_to_string(format!("/proc/{job}/stat")).map_or(true, |stat| {
            stat.rsplit_once(')')
                .unwrap()
                .1
                .trim_start()
                .starts_with('Z')
        })
    });
    assert_eq!(separate.state(), ExecutionState::Running);
    assert!(separate.cancel());
    reaped(&done(&separate));
    rt.idle();
}

// Native-only bridge for the LR-9E integrated test. No IPC constructor.
pub(crate) fn lr9e_submit(
    broker: &Arc<ExecutionBroker>,
    request: &ExecutionRequest,
) -> ExecutionHandle {
    broker
        .submit(request, &ExecutionAuthority::human_local())
        .unwrap()
}

#[test]
fn lr9e_replaced_workspace_root_fails_closed_and_ids_never_grant_authority() {
    let root = Directory::new();
    let outside = Directory::new();
    let scope = WorkspaceScope::confined(&[root.0.clone()]).unwrap();
    let original = root.0.with_extension("original");
    std::fs::rename(&root.0, &original).unwrap();
    std::os::unix::fs::symlink(&outside.0, &root.0).unwrap();
    assert_eq!(
        scope.validate(&root.0).unwrap_err(),
        ExecutionError::CwdOutsideScope
    );
    std::fs::remove_file(&root.0).unwrap();
    std::fs::rename(&original, &root.0).unwrap();
    let human = ExecutionAuthority::human_local();
    let mut r = human_request("/usr/bin/python3", &["-c", "pass"]);
    r.task_id = Some(crate::luna::task::TaskId(1));
    r.correlation = Some(TraceId::new("agent-call-1").unwrap());
    assert_eq!(human.authorize(&r, ExecutionMode::Structured), Ok(()));
    for origin in [
        ExecutionOrigin::SpecialistAgent(TraceId::new("codex").unwrap()),
        ExecutionOrigin::Worker(TraceId::new("worker").unwrap()),
        ExecutionOrigin::CognitiveProvider(TraceId::new("provider").unwrap()),
    ] {
        r.origin = origin;
        assert_eq!(
            human.authorize(&r, ExecutionMode::Structured),
            Err(ExecutionError::AuthorityDenied)
        );
    }
}
