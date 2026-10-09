use super::*;
use std::{thread, time::Instant};
fn until(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
}
struct Runtime {
    broker: Arc<ExecutionBroker>,
    human: Arc<HumanTerminal>,
    hub: Arc<SurfaceHub>,
}
impl Runtime {
    fn new() -> Self {
        let broker = ExecutionBroker::isolated();
        Self {
            human: Arc::new(HumanTerminal::new(broker.clone())),
            broker,
            hub: Arc::new(SurfaceHub::default()),
        }
    }
    fn shell(&self) -> PtySession {
        let dto = self.human.open().unwrap();
        let s = self.human.find(&dto.session_id).unwrap();
        until(|| s.state() == ExecutionState::Running);
        s
    }
    fn idle(&self) {
        until(|| self.broker.stopped() && self.hub.worker_count() == 0);
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.hub.detach_main();
        self.broker.request_shutdown();
        assert!(self.broker.wait_shutdown(SHUTDOWN_DEADLINE));
        until(|| self.hub.worker_count() == 0);
    }
}
fn tail(s: &PtySession) -> Vec<u8> {
    let mut cursor = 0;
    let mut bytes = Vec::new();
    loop {
        let b = s
            .replay(cursor, MAX_BATCH_CHUNKS, crate::execution::MAX_BATCH_BYTES)
            .unwrap();
        for c in b.chunks {
            bytes.extend_from_slice(&c.bytes);
        }
        cursor = b.next_after;
        if !b.has_more {
            return bytes;
        }
    }
}
fn marker(s: &PtySession, text: &str) {
    until(|| tail(s).windows(text.len()).any(|w| w == text.as_bytes()));
}
fn stream(source: &str, text: &str) -> EventDraft {
    EventDraft::new(
        Provenance {
            source: TraceSource {
                source_type: SourceType::Worker,
                id: TraceId::new(source).unwrap(),
                instance: None,
            },
            task_id: None,
            subtask_id: None,
            correlation_id: None,
            coalescing_key: Some(TraceId::new("delta").unwrap()),
        },
        OperationalKind::TextDelta {
            channel: TextChannel::Stdout,
            text: TraceText::new(text).unwrap(),
        },
    )
    .unwrap()
}
#[test]
fn registry_product_boundary_reattach_same_id_pid_and_native_directory() {
    let rt = Runtime::new();
    let s = rt.shell();
    let first = rt.human.open().unwrap();
    assert_eq!(first.session_id, s.id().get().to_string());
    assert_eq!(
        rt.human.find(&first.session_id).unwrap().process_id(),
        s.process_id()
    );
    assert_eq!(s.origin(), &ExecutionOrigin::Human);
    assert_eq!(
        s.request().cwd,
        crate::execution::human::starting_directory()
    );
    assert!(s.request().args.is_empty());
    assert!(matches!(
        s.request().environment,
        EnvironmentPolicy::HumanInherited
    ));
    assert!(rt.human.find("arbitrary-pid").is_err());
}
#[test]
fn human_interactive_policy_does_not_expire_at_artificial_hour_structured_policy_remains_bounded() {
    use crate::execution::pty::PtyLifecycle;
    let elapsed = MAX_TIMEOUT + Duration::from_secs(1);
    assert!(!PtyLifecycle::HumanInteractive.expired(elapsed, MAX_TIMEOUT));
    assert!(PtyLifecycle::Bounded.expired(elapsed, MAX_TIMEOUT));
    assert!(include_str!("../execution/broker.rs").contains("Instant::now() >= deadline"));
}
#[test]
fn attachment_drop_and_disconnect_preserve_running_pty() {
    let rt = Runtime::new();
    let s = rt.shell();
    let live = s.subscribe().unwrap();
    drop(live);
    let a = rt.hub.create(Some(s.id().get().to_string())).unwrap();
    let live = s.subscribe().unwrap();
    let aa = a.clone();
    let ss = s.clone();
    rt.hub
        .spawn(a, move || {
            run_pty_bridge(&aa, ss, live, |_| false, |_| true)
        })
        .unwrap();
    until(|| rt.hub.worker_count() == 0);
    assert_eq!(s.state(), ExecutionState::Running);
    assert_eq!(s.subscriber_count(), 0);
    rt.human
        .input(
            &s.id().get().to_string(),
            b"printf '%s%s\\n' AFTER_ DISCONNECT\n",
        )
        .unwrap();
    marker(&s, "AFTER_DISCONNECT");
}
#[test]
fn repeated_attach_detach_bounded_workers_and_subscribers() {
    let rt = Runtime::new();
    let s = rt.shell();
    for _ in 0..20 {
        let a = rt.hub.create(Some(s.id().get().to_string())).unwrap();
        let aa = a.clone();
        let live = s.subscribe().unwrap();
        let ss = s.clone();
        rt.hub
            .spawn(a, move || {
                run_pty_bridge(
                    &aa,
                    ss,
                    live,
                    |b| {
                        aa.ack(
                            0,
                            &u64::from_le_bytes(b[..8].try_into().unwrap()).to_string(),
                        )
                        .unwrap();
                        true
                    },
                    |_| true,
                )
            })
            .unwrap();
        rt.hub.detach_main();
        until(|| rt.hub.worker_count() == 0);
        assert_eq!(s.subscriber_count(), 0);
    }
    assert_eq!(s.state(), ExecutionState::Running);
}
#[test]
fn slow_live_subscriber_is_bounded_and_reader_recovers_after_overflow() {
    let rt = Runtime::new();
    let s = rt.shell();
    let slow = s.subscribe().unwrap();
    assert_eq!(
        slow.cursor,
        s.replay(0, 1, READ_CHUNK_BYTES).unwrap().latest_sequence
    );
    rt.human.input(&s.id().get().to_string(), b"stty -echo; /usr/bin/python3 -c \"import os; [os.write(1,b'x'*8192) for _ in range(768)]\"; printf '%s%s\\n' STRESS_ ALIVE\n").unwrap();
    marker(&s, "STRESS_ALIVE");
    let b = s
        .replay(0, MAX_BATCH_CHUNKS, crate::execution::MAX_BATCH_BYTES)
        .unwrap();
    assert!(b.gap && b.dropped_bytes > 2 * 1024 * 1024);
    assert!(b.retained_bytes <= PTY_RETAIN_BYTES && b.retained_chunks <= PTY_RETAIN_CHUNKS);
    assert!(slow.lost_notifications() > 0);
    assert!(slow.wait(Duration::ZERO).is_some());
    assert!(slow.wait(Duration::ZERO).is_none());
    assert_eq!(s.state(), ExecutionState::Running);
    drop(slow);
    // New real bridge receives bounded replay and factual gap; raw bytes exact.
    let a = rt.hub.create(Some(s.id().get().to_string())).unwrap();
    let aa = a.clone();
    let live = s.subscribe().unwrap();
    let ss = s.clone();
    let frames = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let captured = frames.clone();
    rt.hub
        .spawn(a, move || {
            run_pty_bridge(
                &aa,
                ss,
                live,
                |b| {
                    let cursor = u64::from_le_bytes(b[..8].try_into().unwrap()).to_string();
                    captured.lock().unwrap().push(b);
                    aa.ack(0, &cursor).unwrap();
                    true
                },
                |_| true,
            )
        })
        .unwrap();
    until(|| {
        frames
            .lock()
            .unwrap()
            .iter()
            .any(|b| b.windows(13).any(|w| w == b"STRESS_ALIVE\r"))
    });
    assert!(u64::from_le_bytes(frames.lock().unwrap()[0][8..16].try_into().unwrap()) > 0);
    rt.human.resize(&s.id().get().to_string(), 39, 111).unwrap();
    rt.human
        .input(
            &s.id().get().to_string(),
            b"stty size; printf '%s%s\\n' INPUT_ AFTER\n",
        )
        .unwrap();
    marker(&s, "39 111");
    marker(&s, "INPUT_AFTER");
    rt.hub.detach_main();
    rt.human
        .input(&s.id().get().to_string(), b"exit\n")
        .unwrap();
    assert!(s.wait(Duration::from_secs(10)).unwrap().reaped);
    rt.idle();
    assert_eq!(s.subscriber_count(), 0);
    eprintln!(
        "LR-9C PTY stress: total={} dropped={} retained={} workers=0",
        b.total_bytes, b.dropped_bytes, b.retained_bytes
    );
}
#[test]
fn close_prunes_completed_handle_and_quit_reaps_product_session() {
    let rt = Runtime::new();
    let s = rt.shell();
    let id = s.id().get().to_string();
    assert!(rt.human.close(&id).unwrap());
    let result = s.wait(Duration::from_secs(10)).unwrap();
    assert!(result.reaped);
    assert_eq!(rt.human.status().unwrap().state, "cancelled");
    let next = rt.shell();
    assert_ne!(next.id(), s.id());
    assert!(rt.human.find(&id).is_err());
    let pid = next.process_id().unwrap();
    rt.broker.request_shutdown();
    rt.idle();
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
}
#[test]
fn attachment_acks_scoped_ordered_and_detach_revokes_input() {
    let hub = SurfaceHub::default();
    let a = hub.create(Some("9007199254740993".into())).unwrap();
    assert_eq!(
        session_for_attachment(&hub, &a.id).unwrap(),
        "9007199254740993"
    );
    assert!(a.prepare(0, "9007199254740993".into()));
    assert!(!a.prepare(0, "other".into()));
    assert!(a.ack(0, "9007199254740992").is_err());
    assert!(a.ack(1, "9007199254740993").is_err());
    assert!(a.ack(0, "9007199254740993").is_ok());
    hub.detach_main();
    assert!(session_for_attachment(&hub, &a.id).is_err());
}
#[test]
fn stalled_attachment_does_not_accumulate_channel_batches() {
    let rt = Runtime::new();
    let s = rt.shell();
    let a = rt.hub.create(Some(s.id().get().to_string())).unwrap();
    let aa = a.clone();
    let ss = s.clone();
    let live = s.subscribe().unwrap();
    let sends = Arc::new(AtomicUsize::new(0));
    let count = sends.clone();
    rt.hub
        .spawn(a.clone(), move || {
            run_pty_bridge(
                &aa,
                ss,
                live,
                |_| {
                    count.fetch_add(1, Ordering::Relaxed);
                    true
                },
                |_| true,
            )
        })
        .unwrap();
    until(|| sends.load(Ordering::Relaxed) == 1);
    rt.human
        .input(
            &s.id().get().to_string(),
            b"printf '%s%s\\n' CORE_ PROGRESS\n",
        )
        .unwrap();
    marker(&s, "CORE_PROGRESS");
    assert_eq!(sends.load(Ordering::Relaxed), 1);
    until(|| a.stopped());
    until(|| rt.hub.worker_count() == 0);
    assert_eq!(s.state(), ExecutionState::Running);
}
#[test]
fn trace_dto_exact_large_identity_and_no_request_environment() {
    let bus = OperationalTraceBus::isolated();
    bus.publish(stream("a", "á\n")).unwrap();
    bus.publish(stream("a", "🦀  ")).unwrap();
    let mut dto = trace_batch(0, bus.replay(0, BatchLimits::default()).unwrap(), 0);
    assert_eq!(dto.events.len(), 1);
    assert_eq!(dto.events[0].text, "á\n🦀  ");
    assert_eq!(dto.events[0].last_sequence, "2");
    dto.events[0].sequence = u64::MAX.to_string();
    dto.cursor = "9007199254740993".into();
    let json = serde_json::to_value(dto).unwrap();
    assert_eq!(json["events"][0]["sequence"], "18446744073709551615");
    assert_eq!(json["cursor"], "9007199254740993");
    for absent in ["environment", "request", "args", "program", "authority"] {
        assert!(!json.to_string().contains(absent));
    }
}
#[test]
fn trace_bridge_replay_live_gap_reattach_and_dead_channel_publishers_progress() {
    let bus = OperationalTraceBus::isolated();
    bus.publish(stream("a", "initial ")).unwrap();
    let hub = SurfaceHub::default();
    let a = hub.create(None).unwrap();
    let aa = a.clone();
    let bb = bus.clone();
    let live = bus.subscribe().unwrap();
    let batches = Arc::new(Mutex::new(Vec::new()));
    let found = batches.clone();
    hub.spawn(a, move || {
        run_trace_bridge(&aa, bb, live, |b| {
            aa.ack(1, &b.cursor).unwrap();
            found.lock().unwrap().push(b);
            true
        })
    })
    .unwrap();
    until(|| !batches.lock().unwrap().is_empty());
    bus.publish(stream("b", "live")).unwrap();
    until(|| {
        batches
            .lock()
            .unwrap()
            .iter()
            .flat_map(|b| &b.events)
            .any(|e| e.text == "live")
    });
    hub.detach_main();
    until(|| hub.worker_count() == 0);
    assert_eq!(bus.stats().active_subscribers, 0);
    for i in 0..5000 {
        bus.publish(stream(if i % 2 == 0 { "a" } else { "b" }, "flood"))
            .unwrap();
    }
    let replay = trace_batch(0, bus.replay(0, BatchLimits::default()).unwrap(), 0);
    assert!(!replay.replay_complete);
    assert!(replay.missing_events.parse::<u64>().unwrap() > 0);
    let a = hub.create(None).unwrap();
    let aa = a.clone();
    let live = bus.subscribe().unwrap();
    let bb = bus.clone();
    hub.spawn(a, move || run_trace_bridge(&aa, bb, live, |_| false))
        .unwrap();
    until(|| hub.worker_count() == 0);
    assert!(bus.publish(stream("a", "after death")).is_ok());
    assert_eq!(bus.stats().active_subscribers, 0);
}
#[test]
fn trace_slow_attachment_multi_source_stress_preserves_state_critical_without_webview_ack() {
    let bus = OperationalTraceBus::isolated();
    let hub = SurfaceHub::default();
    let a = hub.create(None).unwrap();
    let aa = a.clone();
    let bb = bus.clone();
    let live = bus.subscribe().unwrap();
    let sent = Arc::new(AtomicUsize::new(0));
    let count = sent.clone();
    hub.spawn(a, move || {
        run_trace_bridge(&aa, bb, live, |_| {
            count.fetch_add(1, Ordering::Relaxed);
            true
        })
    })
    .unwrap();
    until(|| sent.load(Ordering::Relaxed) == 1);
    for i in 0..6000 {
        let source = format!("source-{}", i % 4);
        let d = stream(&source, "á exact delta\n");
        let p = d.clone();
        bus.publish(p).unwrap();
        if i % 100 == 0 {
            let mut provenance = Provenance {
                source: TraceSource {
                    source_type: SourceType::Core,
                    id: TraceId::new("fixture").unwrap(),
                    instance: None,
                },
                task_id: None,
                subtask_id: None,
                correlation_id: None,
                coalescing_key: None,
            };
            provenance.task_id = Some(crate::luna::task::TaskId(i + 1));
            bus.publish(
                EventDraft::new(
                    provenance.clone(),
                    OperationalKind::State {
                        kind: StateKind::Checkpoint,
                        code: TraceId::new("fixed").unwrap(),
                        detail: TraceText::new("state").unwrap(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
            bus.publish(
                EventDraft::new(
                    provenance,
                    OperationalKind::Critical {
                        kind: CriticalKind::Completed,
                        code: TraceId::new("fixed").unwrap(),
                        message: TraceText::new("critical").unwrap(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
    let stats = bus.stats();
    assert_eq!(stats.published, 6120);
    assert!(stats.live_delivery_dropped > 0);
    assert_eq!(stats.evicted.state, 0);
    assert_eq!(stats.evicted.critical, 0);
    assert_eq!(sent.load(Ordering::Relaxed), 1);
    hub.detach_main();
    until(|| hub.worker_count() == 0);
    let mut cursor = 0;
    let mut state = 0;
    let mut critical = 0;
    loop {
        let b = bus.replay(cursor, BatchLimits::default()).unwrap();
        state += b
            .events
            .iter()
            .filter(|e| e.retention_class() == RetentionClass::State)
            .count();
        critical += b
            .events
            .iter()
            .filter(|e| e.retention_class() == RetentionClass::Critical)
            .count();
        cursor = b.next_after;
        if !b.has_more {
            break;
        }
    }
    assert_eq!((state, critical), (60, 60));
    assert_eq!(bus.stats().active_subscribers, 0);
    eprintln!("LR-9C trace stress: published={} retained={} live_dropped={} state={} critical={} in_flight=1",stats.published,stats.retained_events,stats.live_delivery_dropped,state,critical);
}
#[test]
fn security_boundary_no_generic_executor_provider_or_agent_path() {
    let source = include_str!("mod.rs");
    for forbidden in [
        "execute_command",
        "ExecutionAuthority",
        "crate::agents",
        "crate::cognition",
        "ExecutionRequest",
        "EnvironmentPolicy",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    let main: serde_json::Value =
        serde_json::from_str(include_str!("../../capabilities/main-window.json")).unwrap();
    assert_eq!(main["windows"], serde_json::json!(["main"]));
    for settings in [
        include_str!("../../capabilities/settings-general.json"),
        include_str!("../../capabilities/settings-ai.json"),
    ] {
        for forbidden in [
            "human-terminal",
            "terminal-input",
            "terminal-surface",
            "resize-terminal",
        ] {
            assert!(!settings.contains(forbidden));
        }
    }
}

#[test]
fn actual_session_and_bus_ids_cross_serialization_above_js_safe_integer() {
    let rt = Runtime::new();
    rt.broker.seed_test_id(9_007_199_254_740_992);
    let s = rt.shell();
    let dto = serde_json::to_value(rt.human.status().unwrap()).unwrap();
    assert_eq!(dto["sessionId"], "9007199254740993");
    assert_eq!(s.id().get(), 9_007_199_254_740_993);
    let bus = OperationalTraceBus::isolated();
    bus.seed_test_sequence(9_007_199_254_740_992);
    bus.publish(stream("exact", "native fact")).unwrap();
    let projected = trace_batch(
        9_007_199_254_740_992,
        bus.replay(9_007_199_254_740_992, BatchLimits::default())
            .unwrap(),
        0,
    );
    let json = serde_json::to_value(projected).unwrap();
    assert_eq!(json["events"][0]["sequence"], "9007199254740993");
    assert_eq!(json["cursor"], "9007199254740993");
    let raw = pty_frame(0, &s.replay(0, 1, READ_CHUNK_BYTES).unwrap());
    assert!(raw.len() >= 24);
}

#[test]
fn simultaneous_worker_admission_never_exceeds_bridge_budget() {
    let hub = Arc::new(SurfaceHub::default());
    let a = Arc::new(Attachment::new("fixture".into(), None));
    let start = Arc::new(std::sync::Barrier::new(16));
    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let hub = hub.clone();
            let a = a.clone();
            let start = start.clone();
            let release = release.clone();
            thread::spawn(move || {
                start.wait();
                hub.spawn(a, move || {
                    let (lock, wake) = &*release;
                    let _guard = wake
                        .wait_while(lock.lock().unwrap(), |released| !*released)
                        .unwrap();
                })
                .is_ok()
            })
        })
        .collect();
    let admitted = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(admitted, MAX_BRIDGE_WORKERS);
    assert!(hub.worker_count() <= MAX_BRIDGE_WORKERS);
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    until(|| hub.worker_count() == 0);
}

#[cfg(target_os = "linux")]
#[test]
fn activity_fixture_uses_real_adapters_and_existing_lr9c_dto() {
    use crate::agents::{
        trace::{AgentTraceObservation, AgentTraceSink},
        types::AgentEvent,
    };
    use crate::cognition::{policy::CognitiveRole, scheduler::SchedulerEvent};
    use crate::luna::task::{TaskEventKind, TaskId};
    use crate::operational_trace::adapters::tests::{events, selected, subtask_started};
    use crate::operational_trace::adapters::*;
    let b = OperationalTraceBus::isolated();
    let p = PassiveTracePublisher::new(b.clone());
    TaskTraceAdapter::new(p.clone(), SourceType::Core, "core")
        .observe(TaskId(17), &TaskEventKind::TaskStarted);
    let mut s = SchedulerTraceAdapter::new(
        p.clone(),
        SchedulerTraceContext::new(Some(TaskId(17)), None, CognitiveRole::Conversation),
    );
    s.observe(&selected(1));
    s.observe(&SchedulerEvent::Chunk {
        provider_id: "p".into(),
        text: "Saída pública 🦀".into(),
    });
    TaskTraceAdapter::new(p.clone(), SourceType::TaskGraph, "task_graph")
        .observe(TaskId(17), &TaskEventKind::TaskPlanned { step_count: 2 });
    TaskTraceAdapter::new(p.clone(), SourceType::Worker, "worker")
        .observe(TaskId(17), &subtask_started());
    let a = AgentTraceAdapter::new(
        p,
        AgentTraceContext {
            source_id: "codex".into(),
            task_id: Some(TaskId(17)),
            subtask_id: None,
        },
    );
    a.observe(AgentTraceObservation::Lifecycle(&AgentEvent::SessionReady));
    a.observe(AgentTraceObservation::AgentMessage(
        "Mensagem natural do planner",
    ));
    a.observe(AgentTraceObservation::DisplayReasoningSummary(
        "Resumo explicitamente exibível",
    ));
    a.observe(AgentTraceObservation::Lifecycle(&AgentEvent::Completed));
    let dto: Vec<_> = events(&b)
        .iter()
        .map(|e| crate::terminal_surface::dto::TraceDto::event(e))
        .collect();
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(dto.len(), 9);
    if let Ok(path) = std::env::var("NARYS_LR9D_ACTIVITY_FIXTURE") {
        std::fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    }
    assert!(json
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["sourceType"] == "specialist_agent" && e["sourceId"] == "codex"));
}

/// Native deterministic matrix: OS effects and real Scheduler/Summary/Codex
/// fakes overlap. Consumer callbacks deliberately reenter locks to prove sends
/// happen after locks have been released. No commercial network or generic IPC.
#[test]
fn lr9e_integrated_concurrency_fault_and_hygiene_matrix() {
    use crate::cognition::policy::CognitiveRole;
    use crate::operational_trace::adapters::{tests as fixtures, PassiveTracePublisher};
    struct RejectAfterPublish(Arc<OperationalTraceBus>);
    impl crate::operational_trace::adapters::TracePublisher for RejectAfterPublish {
        fn publish(&self, draft: EventDraft) -> Result<(), crate::operational_trace::TraceError> {
            self.0.publish(draft)?;
            Err(crate::operational_trace::TraceError::SequenceExhausted)
        }
    }
    let bus = OperationalTraceBus::isolated();
    let broker = ExecutionBroker::isolated_with_trace(bus.clone());
    let rt = Runtime {
        human: Arc::new(HumanTerminal::new(broker.clone())),
        broker,
        hub: Arc::new(SurfaceHub::default()),
    };
    let s = rt.shell();
    let sid = s.id().get().to_string();
    let pid = s.process_id().unwrap();
    let slow = bus.subscribe().unwrap();
    let request = |script: &str, timeout| ExecutionRequest {
        program: "/usr/bin/python3".into(),
        args: vec!["-c".into(), script.into()],
        cwd: std::env::temp_dir(),
        origin: ExecutionOrigin::Human,
        task_id: Some(crate::luna::task::TaskId(91)),
        correlation: Some(TraceId::new("matrix-exec").unwrap()),
        workspace: None,
        environment: EnvironmentPolicy::Controlled(vec![(
            "LR9E_ENV".into(),
            "ENVIRONMENT-PRIVATE-MARKER".into(),
        )]),
        timeout,
        capture: CapturePolicy::default(),
    };
    // Native human boundary, never serialized or passed to agents/providers.
    // Access via execution test helper: fixed controlled request only.
    let a = rt.hub.create(Some(sid.clone())).unwrap();
    let aa = a.clone();
    let bb = bus.clone();
    let trace_live = bus.subscribe().unwrap();
    rt.hub
        .spawn(a.clone(), move || {
            run_trace_bridge(&aa, bb.clone(), trace_live, |_| {
                // Would deadlock if either producer or Attachment lock crossed send.
                bb.stats();
                aa.stopped();
                true // intentionally no ACK
            })
        })
        .unwrap();
    let aa = a.clone();
    let ss = s.clone();
    let live = s.subscribe().unwrap();
    rt.hub
        .spawn(a, move || {
            run_pty_bridge(
                &aa,
                ss.clone(),
                live,
                |_| {
                    ss.state();
                    aa.stopped();
                    true // pending PTY frame, detach wakes deadline
                },
                |_| true,
            )
        })
        .unwrap();
    let graph_fixture = crate::cognition::task_graph_runtime_tests::lr9e_graph_fixture();
    let barrier = Arc::new(std::sync::Barrier::new(9));
    let (graph_release, graph_wait) = std::sync::mpsc::channel();
    let started = Instant::now();
    let (normal, cancel, timed, pids) = std::thread::scope(|scope| {
        let mut producers = Vec::new();
        for n in 0..4 {
            let (b, gate) = (bus.clone(), barrier.clone());
            producers.push(scope.spawn(move || {
                gate.wait();
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let publisher = if n == 3 {
                    PassiveTracePublisher::new(Arc::new(fixtures::RejectPublisher))
                } else {
                    PassiveTracePublisher::new(b)
                };
                runtime.block_on(fixtures::provider_run(
                    publisher,
                    if n == 0 || n == 3 {
                        CognitiveRole::Conversation
                    } else {
                        CognitiveRole::Worker
                    },
                    Some(crate::luna::task::TaskId(51 + n)),
                    if n == 0 { None } else { Some("matrix-worker") },
                    1500,
                    true,
                ))
            }));
        }
        let (b, gate) = (bus.clone(), barrier.clone());
        let summary = scope.spawn(move || {
            gate.wait();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(crate::cognition::summary::lr9d_tests::stress_summary(
                PassiveTracePublisher::new(b),
            ))
        });
        let mut agents = Vec::new();
        for n in 0..2 {
            let (b, gate) = (bus.clone(), barrier.clone());
            agents.push(scope.spawn(move || {
                gate.wait();
                crate::agents::codex::backend::lifecycle_tests::lr9d_fake_trace_operation(
                    if n == 0 {
                        PassiveTracePublisher::new(b)
                    } else {
                        PassiveTracePublisher::new(Arc::new(RejectAfterPublish(b)))
                    },
                    750,
                )
            }));
        }
        let gate = barrier.clone();
        let graph = scope.spawn(move || graph_fixture(gate, graph_wait));
        barrier.wait();
        crate::luna::runtime::lr9d_trace_publication_is_independent_of_failed_functional_channel();
        let normal = crate::execution::tests::lr9e_submit(
            &rt.broker,
            &request(
                "import os,time; time.sleep(.3); os.write(1,b'EXEC-ALLOWED')",
                Duration::from_secs(10),
            ),
        );
        let cancel = crate::execution::tests::lr9e_submit(
            &rt.broker,
            &request("import time; time.sleep(30)", Duration::from_secs(10)),
        );
        let timed = crate::execution::tests::lr9e_submit(
            &rt.broker,
            &request("import time; time.sleep(30)", Duration::from_millis(180)),
        );
        until(|| {
            normal.process_id().is_some()
                && cancel.process_id().is_some()
                && timed.process_id().is_some()
        });
        let pids = [
            normal.process_id().unwrap(),
            cancel.process_id().unwrap(),
            timed.process_id().unwrap(),
        ];
        rt.human.input(&sid,b"stty -echo; /usr/bin/python3 -c \"import os; [os.write(1,b'x'*8192) for _ in range(768)]\"; printf '%s%s\\n' PTY_PRIVATE_ MARKER\n").unwrap();
        assert!(cancel.cancel());
        rt.hub.detach_main(); // both batches pending during multi-source burst
        marker(&s, "PTY_PRIVATE_MARKER");
        graph_release.send(()).unwrap();
        let results: Vec<_> = producers.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            results
                .iter()
                .map(|(r, _, _)| r.usage.provider_calls)
                .sum::<u32>(),
            12
        );
        assert_eq!(
            results.iter().map(|(r, _, _)| r.usage.retries).sum::<u32>(),
            4
        );
        assert_eq!(
            results
                .iter()
                .map(|(r, _, _)| r.usage.fallbacks)
                .sum::<u32>(),
            4
        );
        assert_eq!(results[0].0.text, results[3].0.text);
        assert_eq!(results[0].0.usage, results[3].0.usage);
        assert_eq!(results[0].1, results[3].1); // passive rejection during real concurrent load
        assert_eq!(graph.join().unwrap(), 3);
        assert_eq!(summary.join().unwrap().0, 1);
        assert_eq!(
            agents.into_iter().map(|h| h.join().unwrap()).sum::<usize>(),
            2
        );
        (normal, cancel, timed, pids)
    });
    marker(&s, "PTY_PRIVATE_MARKER");
    assert_eq!(s.process_id(), Some(pid));
    assert_eq!(s.state(), ExecutionState::Running);
    let replay = s.replay(0, 1, READ_CHUNK_BYTES).unwrap();
    assert!(replay.total_bytes > 6 * 1024 * 1024 && replay.gap);
    rt.human.resize(&sid, 37, 109).unwrap();
    rt.human
        .input(&sid, b"stty size; printf '%s%s\\n' AFTER_OVERFLOW_ INPUT\n")
        .unwrap();
    marker(&s, "37 109");
    marker(&s, "AFTER_OVERFLOW_INPUT");
    for (h, expected) in [
        (&normal, ExecutionState::Completed),
        (&cancel, ExecutionState::Cancelled),
        (&timed, ExecutionState::TimedOut),
    ] {
        let result = h.wait(Duration::from_secs(10)).unwrap();
        assert_eq!(result.state, expected);
        assert!(result.reaped && !result.cleanup_pending);
    }
    assert_eq!(s.state(), ExecutionState::Running); // Exec cleanup preserved PTY
    for p in pids {
        assert!(!std::path::Path::new(&format!("/proc/{p}")).exists());
    }
    let stats = bus.stats();
    assert!(stats.live_delivery_dropped > 0);
    assert!(
        stats.retained_events <= MAX_RETAINED_EVENTS && stats.retained_bytes <= MAX_RETAINED_BYTES
    );
    assert_eq!(
        stats.evicted.state + stats.evicted.critical + stats.dropped.state + stats.dropped.critical,
        0
    );
    let payload = fixtures::payloads(&bus);
    assert!(!payload.contains("SECRET"));
    for marker in [
        "USER-INPUT-SECRET",
        "MEMORY-SECRET",
        "CONTEXT-CONTENT-SECRET",
        "RECENT-CONTEXT-SECRET",
        "SUMMARY-TRANSCRIPT-SECRET",
        "SUMMARY-INTERNAL-SECRET",
        "ENVIRONMENT-PRIVATE-MARKER",
        "PRIVATE-REASONING-SECRET",
        "FORBIDDEN-SECRET",
        "PTY_PRIVATE_MARKER",
    ] {
        assert!(!payload.contains(marker), "{marker}");
    }
    drop(slow);
    until(|| rt.hub.worker_count() == 0);
    assert_eq!(bus.stats().active_subscribers, 0);
    // Replay/live cycles during ongoing PTY: every consumer tears down exactly.
    for _ in 0..10 {
        let a = rt.hub.create(Some(sid.clone())).unwrap();
        let aa = a.clone();
        let b = bus.clone();
        let live = b.subscribe().unwrap();
        rt.hub
            .spawn(a, move || {
                run_trace_bridge(&aa, b.clone(), live, |batch| {
                    b.stats();
                    aa.ack(1, &batch.cursor).is_ok()
                })
            })
            .unwrap();
        rt.hub.detach_main();
        until(|| rt.hub.worker_count() == 0);
        assert_eq!(bus.stats().active_subscribers, 0);
    }
    rt.human.close(&sid).unwrap();
    assert!(s.wait(Duration::from_secs(10)).unwrap().reaped);
    rt.idle();
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    println!(
        "LR9E_MATRIX {}",
        serde_json::json!({"pass":true,"elapsedMs":started.elapsed().as_millis(),"providerCalls":16,"schedulerCalls":12,"summaryCalls":1,"graphCalls":3,"codexTurnStarts":2,"execStates":["completed","cancelled","timed_out"],"ptyBytes":replay.total_bytes,"ptyDroppedBytes":replay.dropped_bytes,"retainedEvents":stats.retained_events,"retainedBytes":stats.retained_bytes,"liveDrops":stats.live_delivery_dropped,"surfaceWorkers":rt.hub.worker_count(),"brokerActive":rt.broker.active_count(),"brokerWorkers":rt.broker.worker_count(),"activeSubscribers":bus.stats().active_subscribers,"reconnectCycles":10,"remainingManagedPids":[]})
    );
}

#[test]
fn lr9e_activity_suspend_replays_without_changing_human_attachment_or_blocking_quit() {
    let rt = Runtime::new();
    let s = rt.shell();
    let sid = s.id().get().to_string();
    let pid = s.process_id();
    let bus = OperationalTraceBus::isolated();
    bus.publish(stream("activity", "initial")).unwrap();
    let a = rt.hub.create(Some(sid.clone())).unwrap();
    let sent = Arc::new(AtomicUsize::new(0));
    let auto_ack = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let first = Arc::new(Mutex::new(None));
    let (aa, bb, ss, ack, ff) = (
        a.clone(),
        bus.clone(),
        sent.clone(),
        auto_ack.clone(),
        first.clone(),
    );
    let live = bus.subscribe().unwrap();
    rt.hub
        .spawn(a.clone(), move || {
            run_trace_bridge(&aa, bb.clone(), live, |b| {
                bb.stats();
                aa.stopped();
                ss.fetch_add(1, Ordering::AcqRel);
                if ack.load(Ordering::Acquire) {
                    let _ = aa.ack_trace(Some(&b.delivery_epoch), &b.cursor);
                    true // send success; a concurrent mode transition may reject its ACK
                } else {
                    *ff.lock().unwrap() = Some((b.delivery_epoch, b.cursor, b.missing_events));
                    true
                }
            })
        })
        .unwrap();
    until(|| sent.load(Ordering::Acquire) > 0);
    assert_eq!(a.set_trace_enabled(false).unwrap(), "1");
    until(|| bus.stats().active_subscribers == 0);
    assert!(!a.stopped());
    assert_eq!(session_for_attachment(&rt.hub, &a.id).unwrap(), sid);
    rt.human
        .input(&sid, b"printf '%s%s\\n' COLLAPSED_ ALIVE\n")
        .unwrap();
    marker(&s, "COLLAPSED_ALIVE");
    let before = sent.load(Ordering::Acquire);
    for _ in 0..6000 {
        bus.publish(stream("activity", "bounded invisible delta"))
            .unwrap();
    }
    assert_eq!(sent.load(Ordering::Acquire), before);
    assert_eq!(bus.stats().active_subscribers, 0);
    assert_eq!(a.set_trace_enabled(true).unwrap(), "2");
    until(|| first.lock().unwrap().as_ref().is_some_and(|v| v.0 == "2"));
    let (_, cursor, missing) = first.lock().unwrap().clone().unwrap();
    assert!(missing.parse::<u64>().unwrap() > 0);
    assert_eq!(
        a.ack_trace(Some("0"), &cursor),
        Err("stale_trace_ack".into())
    );
    assert!(!a.stopped());
    auto_ack.store(true, Ordering::Release);
    a.ack_trace(Some("2"), &cursor).unwrap();
    for _ in 0..10 {
        a.set_trace_enabled(false).unwrap();
        until(|| bus.stats().active_subscribers == 0);
        a.set_trace_enabled(true).unwrap();
        until(|| bus.stats().active_subscribers == 1);
        assert_eq!(s.process_id(), pid);
        assert_eq!(session_for_attachment(&rt.hub, &a.id).unwrap(), sid);
    }
    a.set_trace_enabled(false).unwrap();
    until(|| bus.stats().active_subscribers == 0);
    // The consumer parks without polling; stop must wake it and reap the worker.
    rt.hub.detach_main();
    until(|| rt.hub.worker_count() == 0);
    assert_eq!(bus.stats().active_subscribers, 0);
    rt.human.close(&sid).unwrap();
    assert!(s.wait(Duration::from_secs(10)).unwrap().reaped);
    rt.idle();
}
#[test]
fn lr9e_activity_epoch_exhaustion_fails_closed_without_wrapping_or_affecting_pty_ack() {
    let a = Attachment::new("main-attachment".into(), Some("human-session".into()));
    a.delivery.lock().unwrap().trace_epoch = u64::MAX;
    assert_eq!(
        a.set_trace_enabled(false),
        Err("trace_epoch_exhausted".into())
    );
    assert_eq!(a.trace_mode(), (true, u64::MAX));
    assert!(a.prepare(0, "9".into()));
    assert_eq!(a.ack(0, "9"), Ok(()));
    assert!(!a.stopped());
}
