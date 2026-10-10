use super::*;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::sync::Barrier;

struct FakeFactory {
    starts: AtomicUsize,
    stops: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    startup: Arc<tokio::sync::Semaphore>,
    work: Arc<tokio::sync::Semaphore>,
    fail: Option<&'static str>,
    stop_fail: Option<&'static str>,
}
struct FakeRuntime {
    stops: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    work: Arc<tokio::sync::Semaphore>,
    stop_fail: Option<&'static str>,
}
impl RuntimeFactory for FakeFactory {
    fn start(&self) -> RuntimeFuture<'_, Arc<dyn ManagedRuntime>> {
        Box::pin(async move {
            self.starts.fetch_add(1, Ordering::SeqCst);
            let p = self.startup.acquire().await.unwrap();
            p.forget();
            if let Some(e) = self.fail {
                return Err(e);
            }
            Ok(Arc::new(FakeRuntime {
                stops: self.stops.clone(),
                active: self.active.clone(),
                work: self.work.clone(),
                stop_fail: self.stop_fail,
            }) as Arc<dyn ManagedRuntime>)
        })
    }
}
impl ManagedRuntime for FakeRuntime {
    fn process_id(&self) -> Option<u32> {
        Some(123)
    }
    fn healthy(&self) -> bool {
        true
    }
    fn session<'a>(
        &'a self,
        i: &'a SessionInvocation,
        cancel: &'a Cancellation,
        progress: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> RuntimeFuture<'a, SessionReceipt> {
        Box::pin(async move {
            self.active.fetch_add(1, Ordering::SeqCst);
            progress("session_ready");
            tokio::select! {biased;_ = cancel.wait()=>{},p=self.work.acquire()=>{p.unwrap().forget();}}
            self.active.fetch_sub(1, Ordering::SeqCst);
            progress("session_detached");
            if cancel.is_cancelled() {
                return Err("cancelled");
            }
            Ok(SessionReceipt {
                provider_session_id: i
                    .provider_session_id
                    .clone()
                    .unwrap_or("fixture-session".into()),
                observation_gaps: 0,
                history_anchor: Some("fixture-anchor".into()),
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async move {
            assert_eq!(self.active.load(Ordering::SeqCst), 0);
            self.stops.fetch_add(1, Ordering::SeqCst);
            self.stop_fail.map_or(Ok(()), Err)
        })
    }
}
fn factory() -> Arc<FakeFactory> {
    Arc::new(FakeFactory {
        starts: AtomicUsize::new(0),
        stops: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        startup: Arc::new(tokio::sync::Semaphore::new(0)),
        work: Arc::new(tokio::sync::Semaphore::new(0)),
        fail: None,
        stop_fail: None,
    })
}
fn invocation() -> SessionInvocation {
    SessionInvocation {
        operation: AgentLifecycleOperation::Create,
        directory: PathBuf::from("fixture"),
        provider_session_id: None,
        expected_history_anchor: None,
    }
}
async fn until(f: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !f() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn spawn_run(
    s: Arc<AgentRuntimeSupervisor>,
    c: Arc<Cancellation>,
) -> tokio::task::JoinHandle<RuntimeOutcome> {
    tokio::spawn(async move { s.run(invocation(), c, Arc::new(|_| {})).await })
}
#[tokio::test]
async fn idle_queries_never_start_and_first_demand_starts() {
    let f = factory();
    let s = AgentRuntimeSupervisor::new(f.clone());
    for _ in 0..1000 {
        assert_eq!(s.snapshot().state, AgentRuntimeState::Dormant);
    }
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    let run = spawn_run(s.clone(), Arc::default());
    until(|| f.starts.load(Ordering::SeqCst) == 1).await;
    assert_eq!(s.snapshot().state, AgentRuntimeState::Starting);
    f.startup.add_permits(1);
    until(|| f.active.load(Ordering::SeqCst) == 1).await;
    assert_eq!(s.snapshot().state, AgentRuntimeState::Busy);
    f.work.add_permits(1);
    assert!(run.await.unwrap().cleanup_verified);
    assert_eq!(s.snapshot().state, AgentRuntimeState::Dormant);
    assert_eq!(f.stops.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn simultaneous_demands_share_startup_leases_and_last_release_stops() {
    let f = factory();
    let s = AgentRuntimeSupervisor::new(f.clone());
    let a = spawn_run(s.clone(), Arc::default());
    let b = spawn_run(s.clone(), Arc::default());
    until(|| f.starts.load(Ordering::SeqCst) == 1).await;
    f.startup.add_permits(1);
    until(|| f.active.load(Ordering::SeqCst) == 2).await;
    assert_eq!(s.snapshot().leases, 2);
    assert_eq!(f.starts.load(Ordering::SeqCst), 1);
    let c = spawn_run(s.clone(), Arc::default());
    assert_eq!(
        c.await.unwrap().result.unwrap_err(),
        "agent_concurrency_limit"
    );
    f.work.add_permits(1);
    until(|| s.snapshot().leases == 1).await;
    assert_eq!(f.stops.load(Ordering::SeqCst), 0);
    f.work.add_permits(1);
    let a = a.await.unwrap();
    let b = b.await.unwrap();
    assert!(a.result.is_ok() && b.result.is_ok());
    assert!(a.cleanup_verified || b.cleanup_verified);
    assert_eq!(f.stops.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn cancellation_during_startup_cleans_without_restart_or_session() {
    let f = factory();
    let s = AgentRuntimeSupervisor::new(f.clone());
    let c = Arc::new(Cancellation::default());
    let a = spawn_run(s.clone(), c.clone());
    until(|| f.starts.load(Ordering::SeqCst) == 1).await;
    c.cancel();
    f.startup.add_permits(1);
    let o = a.await.unwrap();
    assert_eq!(o.result.unwrap_err(), "cancelled");
    assert!(o.cleanup_verified);
    assert_eq!(f.active.load(Ordering::SeqCst), 0);
    assert_eq!(f.starts.load(Ordering::SeqCst), 1);
    assert_eq!(f.stops.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn cancellation_of_waiting_lease_does_not_stop_other_work() {
    let f = factory();
    let s = AgentRuntimeSupervisor::new(f.clone());
    let a = spawn_run(s.clone(), Arc::default());
    until(|| f.starts.load(Ordering::SeqCst) == 1).await;
    let c = Arc::new(Cancellation::default());
    let b = spawn_run(s.clone(), c.clone());
    c.cancel();
    assert_eq!(b.await.unwrap().result.unwrap_err(), "cancelled");
    assert_eq!(f.starts.load(Ordering::SeqCst), 1);
    f.startup.add_permits(1);
    f.work.add_permits(1);
    assert!(a.await.unwrap().result.is_ok());
}
#[tokio::test]
async fn cancellation_during_activity_and_dropped_client_future_are_cleaned() {
    let f = factory();
    let s = AgentRuntimeSupervisor::new(f.clone());
    f.startup.add_permits(1);
    let c = Arc::new(Cancellation::default());
    let a = spawn_run(s.clone(), c.clone());
    until(|| f.active.load(Ordering::SeqCst) == 1).await;
    a.abort();
    until(|| s.snapshot().cleanup_verified).await;
    assert!(c.is_cancelled());
    assert_eq!(f.active.load(Ordering::SeqCst), 0);
    assert_eq!(f.stops.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn cancel_completion_race_is_terminal_once_and_no_implicit_restart() {
    for _ in 0..32 {
        let f = factory();
        let s = AgentRuntimeSupervisor::new(f.clone());
        f.startup.add_permits(1);
        let c = Arc::new(Cancellation::default());
        let a = spawn_run(s.clone(), c.clone());
        until(|| f.active.load(Ordering::SeqCst) == 1).await;
        let barrier = Arc::new(Barrier::new(2));
        let b = barrier.clone();
        let control = c.clone();
        let cancel = tokio::spawn(async move {
            b.wait().await;
            control.cancel();
        });
        barrier.wait().await;
        f.work.add_permits(1);
        cancel.await.unwrap();
        let result = a.await.unwrap();
        assert!(result.result.is_ok() || result.result.unwrap_err() == "cancelled");
        assert!(result.cleanup_verified);
        assert_eq!(f.starts.load(Ordering::SeqCst), 1);
        assert_eq!(f.stops.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn concurrent_shutdown_cancels_drains_and_closes_admission() {
    let f = factory();
    let s = AgentRuntimeSupervisor::new(f.clone());
    f.startup.add_permits(1);
    let c = Arc::new(Cancellation::default());
    let a = spawn_run(s.clone(), c.clone());
    until(|| f.active.load(Ordering::SeqCst) == 1).await;
    s.close_admission();
    c.cancel();
    let (x, y) = tokio::join!(s.shutdown(), s.shutdown());
    assert!(x.is_ok() && y.is_ok());
    assert_eq!(a.await.unwrap().result.unwrap_err(), "cancelled");
    assert_eq!(f.stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        spawn_run(s.clone(), Arc::default())
            .await
            .unwrap()
            .result
            .unwrap_err(),
        "supervisor_stopping"
    );
}
#[tokio::test]
async fn startup_failure_is_shared_safe_and_explicit_new_demand_can_retry() {
    let mut f = factory();
    Arc::get_mut(&mut f).unwrap().fail = Some("cli_unavailable");
    f.startup.add_permits(2);
    let s = AgentRuntimeSupervisor::new(f.clone());
    for _ in 0..2 {
        assert_eq!(
            spawn_run(s.clone(), Arc::default())
                .await
                .unwrap()
                .result
                .unwrap_err(),
            "cli_unavailable"
        );
    }
    assert_eq!(f.starts.load(Ordering::SeqCst), 2);
    assert_eq!(s.snapshot().state, AgentRuntimeState::Faulted);
}
#[tokio::test]
async fn unverified_cleanup_blocks_reentry_and_failure_wins_over_cancel() {
    let mut f = factory();
    Arc::get_mut(&mut f).unwrap().stop_fail = Some("runtime_cleanup_incomplete");
    f.startup.add_permits(1);
    let s = AgentRuntimeSupervisor::new(f.clone());
    let c = Arc::new(Cancellation::default());
    let a = spawn_run(s.clone(), c.clone());
    until(|| f.active.load(Ordering::SeqCst) == 1).await;
    c.cancel();
    let o = a.await.unwrap();
    assert_eq!(o.result.unwrap_err(), "runtime_cleanup_incomplete");
    assert!(!o.cleanup_verified);
    assert_eq!(
        spawn_run(s.clone(), Arc::default())
            .await
            .unwrap()
            .result
            .unwrap_err(),
        "cleanup_not_verified"
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), 1);
}
fn service(d: &std::path::Path, f: Arc<dyn RuntimeFactory>) -> Arc<CopilotLifecycle> {
    CopilotLifecycle::new(
        Database::new(d.join("db")),
        Arc::new(TaskRegistry::default()),
        d.join("sessions"),
        f,
    )
    .unwrap()
}
#[tokio::test]
async fn adapter_and_registry_keep_tools_financial_gate_and_codex_planner_closed() {
    let d = tempfile::tempdir().unwrap();
    let f = factory();
    let service = service(d.path(), f.clone());
    let adapter = CopilotAgentAdapter { lifecycle: service };
    let mut registry = crate::agents::registry::AgentRegistry::production();
    registry
        .register(
            CopilotAgentAdapter::config(),
            Arc::new(CopilotAgentAdapter {
                lifecycle: adapter.lifecycle.clone(),
            }),
        )
        .unwrap();
    assert_eq!(registry.configs().len(), 2);
    assert!(!registry.get("copilot").unwrap().config.enabled);
    assert_eq!(
        registry.eligible(&AgentCapabilities {
            planning: true,
            structured_output: true,
            ..Default::default()
        })[0]
            .config
            .id,
        "codex"
    );
    let codex = registry.get("codex").unwrap().config.capabilities;
    assert!(
        !codex.tool_use && !codex.command_execution && !codex.file_write && !codex.repository_read
    );
    for cap in [
        AgentCapabilities {
            tool_use: true,
            ..Default::default()
        },
        AgentCapabilities {
            command_execution: true,
            ..Default::default()
        },
        AgentCapabilities {
            file_write: true,
            ..Default::default()
        },
        AgentCapabilities {
            repository_read: true,
            ..Default::default()
        },
    ] {
        assert_eq!(
            adapter
                .execute(
                    &AgentRequest {
                        objective: "run".into(),
                        required_capabilities: cap
                    },
                    &AtomicBool::new(false),
                    &mut |_| Ok(())
                )
                .await,
            Err(AgentError::UnsupportedCapability)
        );
    }
    assert_eq!(
        adapter
            .execute(
                &AgentRequest {
                    objective: "text".into(),
                    required_capabilities: Default::default()
                },
                &AtomicBool::new(false),
                &mut |_| Ok(())
            )
            .await,
        Err(AgentError::Unavailable)
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn detached_clients_do_not_cancel_task_and_durable_resume_is_explicit() {
    let d = tempfile::tempdir().unwrap();
    let f = factory();
    f.startup.add_permits(2);
    let s = service(d.path(), f.clone());
    let admitted = s.admit(AgentLifecycleOperation::Create, None).unwrap();
    let id = admitted["task_id"].as_u64().unwrap();
    let r: AgentSessionRef = serde_json::from_value(admitted["session_ref"].clone()).unwrap();
    let attachment = s.attach(&r).unwrap();
    assert_eq!(
        s.detach(attachment["attachment_id"].as_str().unwrap())["task_cancelled"],
        false
    );
    until(|| f.active.load(Ordering::SeqCst) == 1).await;
    assert_eq!(s.session(&r).unwrap()["state"], "creating");
    f.work.add_permits(1);
    until(|| s.controls.lock().unwrap().is_empty()).await;
    assert_eq!(
        store::task(&s.database.open().unwrap(), id).unwrap()["state"],
        "completed"
    );
    assert_eq!(s.session(&r).unwrap()["state"], "detached");
    assert!(s.cancel(id).unwrap()["already_terminal"].as_bool().unwrap());
    crate::server::mkdir(&d.path().join("sessions").join(&r.0).join("session-state")).unwrap();
    s.admit(AgentLifecycleOperation::Resume, Some(r.clone()))
        .unwrap();
    until(|| f.active.load(Ordering::SeqCst) == 1).await;
    f.work.add_permits(1);
    until(|| s.controls.lock().unwrap().is_empty()).await;
    assert_eq!(s.session(&r).unwrap()["state"], "detached");
    s.close(&r).unwrap();
    assert_eq!(
        s.admit(AgentLifecycleOperation::Resume, Some(r))
            .unwrap_err(),
        "session_not_resumable"
    );
    s.shutdown().await.unwrap();
    assert_eq!(f.starts.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn restart_recovers_uncertain_state_without_remote_replay_or_id_collision() {
    let d = tempfile::tempdir().unwrap();
    let db = Database::new(d.path().join("db"));
    let mut conn = db.open().unwrap();
    let r = AgentSessionRef(format!("cs-{}", "a".repeat(32)));
    crate::server::mkdir(&d.path().join("sessions")).unwrap();
    let dir = d.path().join("sessions").join(&r.0);
    crate::server::mkdir(&dir).unwrap();
    store::admit(
        &db,
        222,
        &r,
        AgentLifecycleOperation::Create,
        &dir,
        "fixture-correlation",
    )
    .unwrap();
    store::recover(&mut conn).unwrap();
    store::recover(&mut conn).unwrap();
    assert_eq!(store::task(&conn, 222).unwrap()["state"], "interrupted");
    assert_eq!(store::session(&conn, &r).unwrap()["state"], "interrupted");
    let f = factory();
    let s = service(d.path(), f.clone());
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.admit(AgentLifecycleOperation::Resume, Some(r))
            .unwrap_err(),
        "session_not_resumable"
    );
    let run = s.admit(AgentLifecycleOperation::Create, None).unwrap();
    assert!(run["task_id"].as_u64().unwrap() > 222);
    s.request_shutdown();
    s.shutdown().await.unwrap();
}
#[test]
fn refs_and_ipc_do_not_accept_raw_provider_ids_authority_or_profiles() {
    for value in ["provider-id", "cs-../bad", "cs-🦀"] {
        assert!(AgentSessionRef(value.into()).validate().is_err());
    }
    for command in [
        json!({"operation":"agent-session-create","authority":"HumanLocal"}),
        json!({"operation":"agent-session-create","profile":"explicit_yolo"}),
        json!({"operation":"agent-session-create","prompt":"hello"}),
    ] {
        assert!(serde_json::from_value::<crate::ipc::Command>(command).is_err());
    }
}
#[tokio::test]
async fn progress_observation_failure_is_a_gap_not_a_functional_failure() {
    let d = tempfile::tempdir().unwrap();
    let f = factory();
    let s = service(d.path(), f.clone());
    let admitted = s.admit(AgentLifecycleOperation::Create, None).unwrap();
    let id = admitted["task_id"].as_u64().unwrap();
    until(|| f.starts.load(Ordering::SeqCst) == 1).await;
    s.database
        .open()
        .unwrap()
        .execute_batch("DROP TABLE server_events;")
        .unwrap();
    f.startup.add_permits(1);
    f.work.add_permits(1);
    until(|| s.controls.lock().unwrap().is_empty()).await;
    let task = store::task(&s.database.open().unwrap(), id).unwrap();
    assert_eq!(task["state"], "completed");
    assert!(task["observation_gaps"].as_u64().unwrap() > 0);
    assert_eq!(task["cleanup_verified"], true);
}
#[tokio::test]
async fn actual_failure_is_preserved_when_observation_is_unavailable() {
    let d = tempfile::tempdir().unwrap();
    let mut f = factory();
    Arc::get_mut(&mut f).unwrap().fail = Some("runtime_died");
    let s = service(d.path(), f.clone());
    let admitted = s.admit(AgentLifecycleOperation::Create, None).unwrap();
    let id = admitted["task_id"].as_u64().unwrap();
    until(|| f.starts.load(Ordering::SeqCst) == 1).await;
    s.database
        .open()
        .unwrap()
        .execute_batch("DROP TABLE server_events;")
        .unwrap();
    f.startup.add_permits(1);
    until(|| s.controls.lock().unwrap().is_empty()).await;
    let task = store::task(&s.database.open().unwrap(), id).unwrap();
    assert_eq!(task["state"], "failed");
    assert_eq!(task["error_code"], "runtime_died");
    assert!(task["observation_gaps"].as_u64().unwrap() > 0);
}
#[tokio::test]
async fn admission_limits_attachments_and_close_are_fail_closed() {
    let d = tempfile::tempdir().unwrap();
    let f = factory();
    let s = service(d.path(), f.clone());
    let a = s.admit(AgentLifecycleOperation::Create, None).unwrap();
    let r: AgentSessionRef = serde_json::from_value(a["session_ref"].clone()).unwrap();
    s.admit(AgentLifecycleOperation::Create, None).unwrap();
    assert_eq!(
        s.admit(AgentLifecycleOperation::Create, None).unwrap_err(),
        "agent_concurrency_limit"
    );
    assert_eq!(s.close(&r).unwrap_err(), "agent_session_busy");
    for _ in 0..32 {
        s.attach(&r).unwrap();
    }
    assert_eq!(s.attach(&r).unwrap_err(), "agent_attachment_limit");
    s.request_shutdown();
    f.startup.add_permits(2);
    s.shutdown().await.unwrap();
    assert_eq!(s.attachments.lock().unwrap().len(), 0);
}
#[tokio::test]
async fn panicking_passive_observer_is_counted_without_abandoning_cleanup() {
    let f = factory();
    let s = AgentRuntimeSupervisor::new(f.clone());
    f.startup.add_permits(1);
    f.work.add_permits(1);
    let outcome = s
        .run(
            invocation(),
            Arc::default(),
            Arc::new(|_| panic!("fixture observer disconnected")),
        )
        .await;
    assert!(outcome.cleanup_verified);
    assert!(outcome.result.unwrap().observation_gaps > 0);
    assert_eq!(f.stops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn startup_failure_is_not_hidden_by_concurrent_cancellation() {
    let mut f = factory();
    Arc::get_mut(&mut f).unwrap().fail = Some("runtime_cleanup_incomplete");
    let supervisor = AgentRuntimeSupervisor::new(f.clone());
    let cancel = Arc::new(Cancellation::default());
    let running = spawn_run(supervisor.clone(), cancel.clone());
    until(|| f.starts.load(Ordering::SeqCst) == 1).await;
    cancel.cancel();
    f.startup.add_permits(1);
    let outcome = running.await.unwrap();
    assert_eq!(outcome.result.unwrap_err(), "runtime_cleanup_incomplete");
    assert!(!outcome.cleanup_verified);
    assert_eq!(
        supervisor.snapshot().last_error,
        Some("runtime_cleanup_incomplete")
    );
}
