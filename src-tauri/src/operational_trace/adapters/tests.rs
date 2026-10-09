use super::*;
use crate::{
    agents::{
        backend::{AgentBackend, MockAgentBackend},
        trace::{AgentTraceObservation, AgentTraceSink, NoopAgentTrace},
        types::{AgentCapabilities, AgentEvent, AgentRequest},
    },
    cognition::{
        policy::CognitiveRole,
        provider::{Provider, ProviderFuture},
        registry::ProviderRegistry,
        scheduler::{Scheduler, SchedulerEvent},
        types::*,
    },
    luna::task::{TaskEventKind, TaskId},
};
use std::{
    sync::{atomic::AtomicBool, Mutex},
    time::Instant,
};

pub(crate) struct RejectPublisher;
impl TracePublisher for RejectPublisher {
    fn publish(&self, _: EventDraft) -> Result<(), TraceError> {
        Err(TraceError::SequenceExhausted)
    }
}
pub(crate) struct NoopPublisher;
impl TracePublisher for NoopPublisher {
    fn publish(&self, _: EventDraft) -> Result<(), TraceError> {
        Ok(())
    }
}
pub(crate) fn events(bus: &OperationalTraceBus) -> Vec<Arc<OperationalEvent>> {
    let mut cursor = 0;
    let mut result = vec![];
    loop {
        let batch = bus.replay(cursor, BatchLimits::default()).unwrap();
        cursor = batch.next_after;
        result.extend(batch.events);
        if !batch.has_more {
            break;
        }
    }
    assert!(result.len() <= MAX_RETAINED_EVENTS);
    result
}
pub(crate) fn payloads(bus: &OperationalTraceBus) -> String {
    events(bus)
        .iter()
        .map(|e| match e.kind() {
            OperationalKind::State { code, detail, .. } => {
                format!("{} {}", code.as_str(), detail.as_str())
            }
            OperationalKind::Critical { code, message, .. } => {
                format!("{} {}", code.as_str(), message.as_str())
            }
            OperationalKind::TextDelta { text, .. } => text.as_str().into(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub(crate) fn code(e: &OperationalEvent) -> &str {
    match e.kind() {
        OperationalKind::State { code, .. } | OperationalKind::Critical { code, .. } => {
            code.as_str()
        }
        _ => "text",
    }
}
fn task_fixture() -> (Arc<OperationalTraceBus>, TaskTraceAdapter) {
    let bus = OperationalTraceBus::isolated();
    let task = TaskTraceAdapter::new(
        PassiveTracePublisher::new(bus.clone()),
        SourceType::Core,
        "core",
    );
    (bus, task)
}
#[test]
fn task_started_provenance_and_class() {
    let (bus, t) = task_fixture();
    t.observe(TaskId(17), &TaskEventKind::TaskStarted);
    let e = events(&bus).remove(0);
    assert_eq!(e.retention_class(), RetentionClass::State);
    assert_eq!(e.provenance().source.source_type, SourceType::Core);
    assert_eq!(e.provenance().source.id.as_str(), "core");
    assert_eq!(e.provenance().task_id, Some(TaskId(17)));
    assert_eq!(code(&e), "task_started");
}
#[test]
fn task_completed_cancelled_failed_are_safe_critical() {
    let (bus, t) = task_fixture();
    for k in [
        TaskEventKind::TaskCompleted,
        TaskEventKind::TaskCancelled,
        TaskEventKind::TaskFailed {
            detail: "ERROR-SECRET-RAW".repeat(1000),
        },
    ] {
        t.observe(TaskId(17), &k);
    }
    let e = events(&bus);
    assert_eq!(e.len(), 3);
    for (e, expected) in e.iter().zip([
        CriticalKind::Completed,
        CriticalKind::Cancelled,
        CriticalKind::Failed,
    ]) {
        assert_eq!(e.retention_class(), RetentionClass::Critical);
        assert!(
            matches!(e.kind(), OperationalKind::Critical {kind,message,..} if *kind==expected && message.as_str().is_empty())
        );
    }
    assert!(!payloads(&bus).contains("ERROR-SECRET"));
}
#[test]
fn context_counts_steps_pause_only() {
    let (bus, t) = task_fixture();
    t.observe(
        TaskId(17),
        &TaskEventKind::ContextBuilt {
            memory_count: 3,
            recent_message_count: 7,
        },
    );
    t.observe(
        TaskId(17),
        &TaskEventKind::StepStarted {
            step: crate::luna::task::TaskStep::Prepare,
        },
    );
    t.observe(
        TaskId(17),
        &TaskEventKind::StepCompleted {
            step: crate::luna::task::TaskStep::Verify,
        },
    );
    t.observe(
        TaskId(17),
        &TaskEventKind::TaskPaused {
            reason: crate::persistence::continuations::PauseReason::RecoveryRequired,
        },
    );
    assert_eq!(payloads(&bus),"context_built memories=3 recent_messages=7\nstep_started prepare\nstep_completed verify\ntask_paused ");
}
fn plan() -> crate::agents::planner::PlanV1 {
    crate::agents::planner::PlanV1::parse(r#"{"version":1,"objective":"PLAN-SECRET","steps":[{"id":"a","description":"PLAN-BODY-SECRET","requiredCapabilities":["planning"],"dependsOn":[]}],"risks":[],"needsUserInput":false,"questions":[]}"#).unwrap()
}
#[test]
fn result_markers_never_copy_task_plan_or_graph_bodies() {
    let (bus, t) = task_fixture();
    t.observe(
        TaskId(17),
        &TaskEventKind::TaskResultReady {
            result: TaskResult {
                text: "RESULT-SECRET".repeat(10000),
                provider_id: "p".into(),
                usage: SchedulerUsage::default(),
                context_metadata: ContextMetadata {
                    identity_version: "CONTEXT-SECRET".into(),
                    memory_count: 5,
                    recent_message_count: 8,
                },
            },
        },
    );
    t.observe(
        TaskId(17),
        &TaskEventKind::OrchestratorPlanReady {
            result: crate::cognition::orchestrator::OrchestratorResult {
                provider_id: "p".into(),
                plan: plan(),
                usage: SchedulerUsage::default(),
            },
        },
    );
    t.observe(
        TaskId(17),
        &TaskEventKind::TaskGraphResultReady {
            result: crate::cognition::task_graph::TaskGraphResult {
                planner_provider_id: "p".into(),
                planner_usage: SchedulerUsage::default(),
                plan: plan(),
                subtasks: vec![crate::cognition::task_graph::TaskGraphSubtaskResult {
                    subtask_id: "a".into(),
                    provider_id: "p".into(),
                    text: "SUBRESULT-SECRET".into(),
                    usage: SchedulerUsage::default(),
                }],
                consolidated_text: "CONSOLIDATED-SECRET".into(),
                worker_usage: SchedulerUsage::default(),
            },
        },
    );
    assert_eq!(
        events(&bus).iter().map(|e| code(e)).collect::<Vec<_>>(),
        ["result_ready"; 3]
    );
    assert!(!payloads(&bus).contains("SECRET"));
}
pub(crate) fn subtask_started() -> TaskEventKind {
    use crate::cognitive_resources::*;
    TaskEventKind::SubtaskStarted {
        subtask_id: "unit-a".into(),
        provider_id: "p".into(),
        unit_id: ExecutionUnitId::new(17, 1).unwrap(),
        allocation: AllocationVariant {
            resource_id: ResourceId::new("p").unwrap(),
            access_path: AccessPath::new("provider_runtime").unwrap(),
            billing_domain_id: BillingDomainId::new("p").unwrap(),
            model_id: ModelId::new("m").unwrap(),
            effort: None,
        },
        selection: crate::cognition::scheduler::AllocationSelection {
            mode: crate::cognition::policy::RoutingMode::Fixed,
            score: None,
        },
        handoff_reason: HandoffReason::FreshUnitAtConfirmedBoundary,
        transitions: vec![],
    }
}
#[test]
fn subtask_lifecycle_preserves_authorship_and_ids_without_handoff_debug() {
    use crate::cognitive_resources::*;
    let bus = OperationalTraceBus::isolated();
    let p = PassiveTracePublisher::new(bus.clone());
    let graph = TaskTraceAdapter::new(p.clone(), SourceType::TaskGraph, "task_graph");
    let worker = TaskTraceAdapter::new(p, SourceType::Worker, "worker");
    graph.observe(TaskId(17), &TaskEventKind::TaskPlanned { step_count: 4 });
    graph.observe(
        TaskId(17),
        &TaskEventKind::SubtaskWaiting {
            subtask_id: "unit-a".into(),
            depends_on: vec!["DEPENDENCY-SECRET".into()],
        },
    );
    worker.observe(TaskId(17), &subtask_started());
    graph.observe(
        TaskId(17),
        &TaskEventKind::SubtaskCompleted {
            subtask_id: "unit-a".into(),
            provider_id: "p".into(),
            checkpoint_id: CheckpointId::new(ExecutionUnitId::new(17, 1).unwrap(), 1).unwrap(),
        },
    );
    graph.observe(
        TaskId(17),
        &TaskEventKind::SubtaskFailed {
            subtask_id: "unit-a".into(),
            provider_id: None,
            error_code: "ERROR-SECRET".into(),
        },
    );
    let e = events(&bus);
    assert_eq!(e.len(), 5);
    for e in &e[1..] {
        assert_eq!(e.provenance().task_id, Some(TaskId(17)));
        assert_eq!(
            e.provenance().subtask_id.as_ref().unwrap().as_str(),
            "unit-a"
        );
    }
    assert_eq!(e[2].provenance().source.source_type, SourceType::Worker);
    assert!(e.iter().enumerate().filter(|(i, _)| *i != 2).all(|(_, e)| e
        .provenance()
        .source
        .source_type
        == SourceType::TaskGraph));
    assert!(!payloads(&bus).contains("SECRET"));
}
fn functional_events() -> Vec<TaskEventKind> {
    vec![
        TaskEventKind::ProviderQueued {
            provider_id: "p".into(),
            traffic_class: crate::cognition::admission::TrafficClass::ForegroundTask,
            queue_depth: 1,
        },
        TaskEventKind::ProviderAdmitted {
            provider_id: "p".into(),
            traffic_class: crate::cognition::admission::TrafficClass::ForegroundTask,
            queue_delay_ms: 1,
        },
        TaskEventKind::ProviderSelected {
            provider_id: "p".into(),
            model: "m".into(),
            attempt: 1,
            routing_reason: "fixed".into(),
            score: None,
        },
        TaskEventKind::ProviderChunk {
            provider_id: "p".into(),
            chunk: "EXPOSED".into(),
        },
        TaskEventKind::ProviderOutputObserved {
            provider_id: "p".into(),
        },
        TaskEventKind::ProviderRetry {
            provider_id: "p".into(),
            reason_code: "timeout".into(),
        },
        TaskEventKind::ProviderFallback {
            from_provider_id: "p".into(),
            to_provider_id: "q".into(),
            reason_code: "timeout".into(),
        },
        TaskEventKind::SubtaskRetry {
            subtask_id: "a".into(),
            provider_id: "p".into(),
            reason_code: "timeout".into(),
        },
        TaskEventKind::SubtaskOutputObserved {
            subtask_id: "a".into(),
            provider_id: "p".into(),
        },
    ]
}
#[test]
fn task_adapter_ignores_every_scheduler_projection() {
    let (bus, t) = task_fixture();
    for e in functional_events() {
        t.observe(TaskId(17), &e);
    }
    assert_eq!(bus.stats().published, 0);
}
#[test]
fn exact_utf8_fragmentation_above_eight_kib_and_boundaries() {
    for text in [
        "".into(),
        "x".repeat(8192),
        format!("{}😀á\n{}", "x".repeat(8191), "🦀".repeat(10000)),
    ] {
        let parts: Vec<_> = fragments(&text).collect();
        assert!(parts.iter().all(|s| s.len() <= 8192));
        assert_eq!(parts.concat(), text);
        for part in parts {
            assert!(TraceText::new(part).is_ok());
        }
    }
    let bus = OperationalTraceBus::isolated();
    let p = PassiveTracePublisher::new(bus.clone());
    let prov = provenance(
        SourceType::CognitiveProvider,
        "p",
        Some(TaskId(17)),
        None,
        Some(TraceId::new("call-1").unwrap()),
    );
    let text = format!("{}😀{}", "x".repeat(8191), "á".repeat(5000));
    p.text(&prov, TextChannel::ProviderText, &text);
    let e = events(&bus);
    assert!(e.len() > 1);
    assert!(e.iter().all(|e| e.provenance() == prov.as_ref().unwrap()));
    assert_eq!(
        e.iter()
            .map(|e| match e.kind() {
                OperationalKind::TextDelta { text, .. } => text.as_str(),
                _ => panic!(),
            })
            .collect::<String>(),
        text
    );
}
#[test]
fn invalid_ids_drafts_sequence_and_publisher_failure_have_no_functional_result() {
    let bus = OperationalTraceBus::isolated();
    let p = PassiveTracePublisher::new(bus.clone());
    TaskTraceAdapter::new(p.clone(), SourceType::Core, "bad source")
        .observe(TaskId(17), &TaskEventKind::TaskStarted);
    TaskTraceAdapter::new(p.clone(), SourceType::Core, "core")
        .observe(TaskId(0), &TaskEventKind::TaskStarted);
    assert_eq!(bus.stats().published, 0);
    bus.seed_test_sequence(u64::MAX);
    TaskTraceAdapter::new(p, SourceType::Core, "core")
        .observe(TaskId(17), &TaskEventKind::TaskCompleted);
    TaskTraceAdapter::new(
        PassiveTracePublisher::new(Arc::new(RejectPublisher)),
        SourceType::Core,
        "core",
    )
    .observe(TaskId(17), &TaskEventKind::TaskCompleted);
    assert_eq!(bus.stats().published, 0);
}
pub(crate) fn selected(attempt: u32) -> SchedulerEvent {
    SchedulerEvent::Selected {
        provider_id: "p".into(),
        model: "m".into(),
        attempt,
        routing_reason: "fixed",
        score: Some(42),
    }
}
fn scheduler_fixture(role: CognitiveRole) -> (Arc<OperationalTraceBus>, SchedulerTraceAdapter) {
    let b = OperationalTraceBus::isolated();
    let t = SchedulerTraceAdapter::new(
        PassiveTracePublisher::new(b.clone()),
        SchedulerTraceContext::new(Some(TaskId(17)), Some("unit-a"), role),
    );
    (b, t)
}
#[test]
fn scheduler_routing_admission_retry_fallback_attribution_and_dedup() {
    let (bus, mut s) = scheduler_fixture(CognitiveRole::Conversation);
    for e in [
        SchedulerEvent::Queued {
            provider_id: "p".into(),
            traffic_class: crate::cognition::admission::TrafficClass::ForegroundTask,
            queue_depth: 3,
        },
        SchedulerEvent::Admitted {
            provider_id: "p".into(),
            traffic_class: crate::cognition::admission::TrafficClass::ForegroundTask,
            queue_delay_ms: 4,
        },
        selected(1),
        SchedulerEvent::Retry {
            provider_id: "p".into(),
            reason_code: "/private/raw",
        },
        SchedulerEvent::Fallback {
            from: "p".into(),
            to: "q".into(),
            reason_code: "/private/raw",
        },
        SchedulerEvent::Chunk {
            provider_id: "p".into(),
            text: "EXPOSED".into(),
        },
    ] {
        s.observe(&e);
    }
    let task = TaskTraceAdapter::new(
        PassiveTracePublisher::new(bus.clone()),
        SourceType::Core,
        "core",
    );
    for e in functional_events() {
        task.observe(TaskId(17), &e);
    }
    let e = events(&bus);
    assert_eq!(
        e.iter().map(|e| code(e)).collect::<Vec<_>>(),
        [
            "provider_queued",
            "provider_admitted",
            "provider_selected",
            "provider_retry",
            "provider_fallback",
            "text"
        ]
    );
    assert!(e[..5].iter().all(
        |e| e.provenance().source.source_type == SourceType::Scheduler
            && e.provenance().source.id.as_str() == "scheduler"
    ));
    assert_eq!(
        e[5].provenance().source.source_type,
        SourceType::CognitiveProvider
    );
    assert_eq!(e[5].provenance().source.id.as_str(), "p");
    assert!(e.iter().all(|e| e.provenance().task_id == Some(TaskId(17))
        && e.provenance().subtask_id.as_ref().unwrap().as_str() == "unit-a"));
    assert!(!payloads(&bus).contains("/private/raw"));
}
#[test]
fn conversation_chunks_exact_and_in_order() {
    let (bus, mut s) = scheduler_fixture(CognitiveRole::Conversation);
    let text = format!("á\n{}😀tail", "x".repeat(19000));
    for t in [&text[..3], &text[3..]] {
        s.observe(&SchedulerEvent::Chunk {
            provider_id: "p".into(),
            text: t.into(),
        });
    }
    assert_eq!(
        events(&bus)
            .iter()
            .map(|e| match e.kind() {
                OperationalKind::TextDelta {
                    channel: TextChannel::ProviderText,
                    text,
                } => text.as_str(),
                _ => panic!(),
            })
            .collect::<String>(),
        text
    );
}
#[test]
fn all_internal_roles_fail_closed_and_coalesce_output_per_selection() {
    for role in [
        CognitiveRole::Orchestrator,
        CognitiveRole::Worker,
        CognitiveRole::Summary,
    ] {
        let bus = OperationalTraceBus::isolated();
        let mut context = SchedulerTraceContext::new(Some(TaskId(17)), Some("unit-a"), role);
        // Even accidental exposure opt-in cannot promote an internal role.
        context.exposure_policy = ExposurePolicy::ConversationOutput;
        let mut s = SchedulerTraceAdapter::new(PassiveTracePublisher::new(bus.clone()), context);
        for n in 1..=2 {
            s.observe(&selected(n));
            for _ in 0..3000 {
                s.observe(&SchedulerEvent::Chunk {
                    provider_id: "p".into(),
                    text: "INTERNAL-SECRET".into(),
                });
                s.observe(&SchedulerEvent::OutputObserved {
                    provider_id: "p".into(),
                });
            }
        }
        assert_eq!(bus.stats().published, 4);
        assert_eq!(
            events(&bus)
                .iter()
                .filter(|e| code(e) == "output_observed")
                .count(),
            2
        );
        assert!(!payloads(&bus).contains("INTERNAL-SECRET"));
        assert!(events(&bus)
            .iter()
            .all(|e| e.retention_class() == RetentionClass::State));
    }
}
#[test]
fn separate_invocations_tasks_subtasks_and_attempts_never_coalesce_together() {
    let bus = OperationalTraceBus::isolated();
    let p = PassiveTracePublisher::new(bus.clone());
    for id in [17, 18] {
        for subtask in ["a", "b"] {
            let mut s = SchedulerTraceAdapter::new(
                p.clone(),
                SchedulerTraceContext::new(
                    Some(TaskId(id)),
                    Some(subtask),
                    CognitiveRole::Conversation,
                ),
            );
            for n in [1, 2] {
                s.observe(&selected(n));
                s.observe(&SchedulerEvent::Chunk {
                    provider_id: "p".into(),
                    text: "x".into(),
                });
            }
        }
    }
    let keys: Vec<_> = events(&bus)
        .iter()
        .filter(|e| code(e) == "text")
        .map(|e| {
            (
                e.provenance().task_id,
                e.provenance().subtask_id.clone(),
                e.provenance().correlation_id.clone(),
                e.provenance().coalescing_key.clone(),
            )
        })
        .collect();
    assert_eq!(keys.len(), 8);
    for (i, k) in keys.iter().enumerate() {
        assert!(!keys[..i].contains(k));
    }
}
#[test]
fn agent_lifecycle_all_facts_and_safe_terminal_classes() {
    let b = OperationalTraceBus::isolated();
    let a = AgentTraceAdapter::new(
        PassiveTracePublisher::new(b.clone()),
        AgentTraceContext {
            source_id: "codex".into(),
            task_id: Some(TaskId(17)),
            subtask_id: None,
        },
    );
    for event in [
        AgentEvent::SessionReady,
        AgentEvent::WorkStarted,
        AgentEvent::OutputObserved,
        AgentEvent::CancellationRequested,
        AgentEvent::Completed,
        AgentEvent::Cancelled,
        AgentEvent::Failed,
    ] {
        a.observe(AgentTraceObservation::Lifecycle(&event));
    }
    let e = events(&b);
    assert_eq!(e.len(), 7);
    assert!(e[..4]
        .iter()
        .all(|e| e.retention_class() == RetentionClass::State));
    assert!(e[4..]
        .iter()
        .all(|e| e.retention_class() == RetentionClass::Critical));
    assert!(e.iter().all(
        |e| e.provenance().source.source_type == SourceType::SpecialistAgent
            && e.provenance().source.id.as_str() == "codex"
    ));
}
#[tokio::test]
async fn generic_mock_backend_passive_contract_is_reusable_and_failure_isolated() {
    let backend: Arc<dyn AgentBackend> = MockAgentBackend::new("Exposed 🦀".repeat(3000));
    let request = AgentRequest {
        objective: "INPUT-SECRET".into(),
        required_capabilities: AgentCapabilities::default(),
    };
    let cancel = AtomicBool::new(false);
    let absent = backend
        .execute_observed(&request, &cancel, &mut |_| Ok(()), Arc::new(NoopAgentTrace))
        .await
        .unwrap();
    for publisher in [
        PassiveTracePublisher::new(Arc::new(RejectPublisher)),
        PassiveTracePublisher::new(OperationalTraceBus::isolated()),
    ] {
        let a = Arc::new(AgentTraceAdapter::new(
            publisher,
            AgentTraceContext {
                source_id: "future-mock".into(),
                task_id: None,
                subtask_id: None,
            },
        ));
        let result = backend
            .execute_observed(&request, &cancel, &mut |_| Ok(()), a)
            .await
            .unwrap();
        assert_eq!(result, absent);
        assert!(!cancel.load(Ordering::Acquire));
    }
    let b = OperationalTraceBus::isolated();
    let a = Arc::new(AgentTraceAdapter::new(
        PassiveTracePublisher::new(b.clone()),
        AgentTraceContext {
            source_id: "future-mock".into(),
            task_id: None,
            subtask_id: None,
        },
    ));
    backend
        .execute_observed(&request, &cancel, &mut |_| Ok(()), a)
        .await
        .unwrap();
    assert_eq!(
        events(&b)
            .iter()
            .map(|e| match e.kind() {
                OperationalKind::TextDelta { text, .. } => text.as_str(),
                _ => panic!(),
            })
            .collect::<String>(),
        absent.output
    );
    assert!(!payloads(&b).contains("INPUT-SECRET"));
}

// Real Scheduler execution, in-memory providers, exact requests/accounting comparison.
struct FakeProvider {
    id: &'static str,
    burst: usize,
    retries: bool,
    requests: Arc<Mutex<Vec<(InvocationMode, serde_json::Value)>>>,
}
impl Provider for FakeProvider {
    fn execute<'a>(
        &'a self,
        r: &'a ProviderRequest,
        _: &'a AtomicBool,
        chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push((r.mode.clone(),serde_json::json!({"provider":self.id,"input":r.input,"instruction":r.internal_system_instruction,"history":r.history.iter().map(|h|serde_json::json!({"role":match h.role{ProviderRole::User=>"user",ProviderRole::Assistant=>"assistant"},"content":h.content})).collect::<Vec<_>>(),"context":r.context.identity,"metadata":r.context.metadata,"memories":r.context.relevant_memories.iter().map(|m|serde_json::json!({"id":m.id,"import_key":m.import_key,"kind":m.kind,"domains":m.domains,"state":m.state,"title":m.title,"summary":m.summary,"content":m.content,"retrieval_hint":m.retrieval_hint,"source_context":m.source_context,"importance":m.importance,"confidence":m.confidence,"event_date":m.event_date,"created_at":m.created_at,"updated_at":m.updated_at,"supersedes_id":m.supersedes_id})).collect::<Vec<_>>(),"recent":r.context.recent_messages.iter().map(|m|serde_json::json!({"id":m.id,"session_id":m.session_id,"role":m.role,"content":m.content,"created_at":m.created_at})).collect::<Vec<_>>(),"target":r.target,"output":r.max_output_tokens,"attempt":r.attempt})));
            if self.retries && self.id == "p" {
                return Err(if r.attempt == 1 {
                    ProviderError::Timeout
                } else {
                    ProviderError::Unavailable {
                        retry_after_ms: None,
                    }
                });
            }
            let text = "Exposed 🦀\n";
            for _ in 0..self.burst {
                chunk(ProviderChunk { text: text.into() })?;
            }
            Ok(ProviderResponse {
                text: String::new(),
                usage: ProviderUsage {
                    calls: 1,
                    input_tokens: 11,
                    output_tokens: 13,
                    total_tokens: Some(24),
                    thought_tokens: Some(2),
                    output_tokens_measured: true,
                },
            })
        })
    }
}
fn request() -> ProviderTaskRequest {
    let mut context = crate::cognition::orchestrator::technical_context();
    context.identity.canonical_name = "MEMORY-CONTEXT-SECRET".into();
    context
        .relevant_memories
        .push(crate::persistence::memory::MemoryRecord {
            id: 1,
            import_key: None,
            kind: "test".into(),
            domains: vec![],
            state: "active".into(),
            title: "MEMORY-TITLE-SECRET".into(),
            summary: "MEMORY-SECRET".into(),
            content: Some("CONTEXT-CONTENT-SECRET".into()),
            retrieval_hint: None,
            source_context: None,
            importance: 1,
            confidence: "test".into(),
            event_date: None,
            created_at: "test".into(),
            updated_at: "test".into(),
            supersedes_id: None,
        });
    context
        .recent_messages
        .push(crate::persistence::conversation::ConversationMessage {
            id: 1,
            session_id: 1,
            role: "user".into(),
            content: "RECENT-CONTEXT-SECRET".into(),
            created_at: "test".into(),
        });
    ProviderTaskRequest {
        allocation_policy: None,
        traffic_class: crate::cognition::admission::TrafficClass::ForegroundTask,
        mode: InvocationMode::default(),
        input: "USER-INPUT-SECRET".into(),
        internal_system_instruction: Some(
            "SYSTEM-INSTRUCTION-SECRET ENV-SECRET=TOPSECRET Authorization: Bearer API-KEY-SECRET"
                .into(),
        ),
        history: vec![ProviderMessage {
            role: ProviderRole::User,
            content: "HISTORY-SECRET".into(),
        }],
        context: Arc::new(context),
        max_output_tokens: Some(100),
        selection: ProviderSelection::Preferred,
        targets: ["p", "q"]
            .into_iter()
            .map(|id| ProviderTarget {
                provider_id: id.into(),
                invocation: ProviderInvocationConfig {
                    model: "m".into(),
                    thinking_level: None,
                    timeouts: None,
                },
            })
            .collect(),
        affinity_key: None,
        estimated_context_bytes: 0,
        required_capabilities: ProviderCapabilities::text_stream(),
    }
}
pub(crate) async fn provider_run(
    publisher: PassiveTracePublisher,
    role: CognitiveRole,
    task: Option<TaskId>,
    subtask: Option<&str>,
    burst: usize,
    retries: bool,
) -> (TaskResult, Vec<(InvocationMode, serde_json::Value)>, usize) {
    let requests = Arc::new(Mutex::new(vec![]));
    let mut registry = ProviderRegistry::default();
    for id in ["p", "q"] {
        registry
            .register(
                ProviderConfig {
                    id: id.into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                Arc::new(FakeProvider {
                    id,
                    burst,
                    retries,
                    requests: requests.clone(),
                }),
            )
            .unwrap();
    }
    let scheduler = Scheduler::new(registry);
    let mut t = SchedulerTraceAdapter::new(
        publisher.clone(),
        SchedulerTraceContext::new(task, subtask, role),
    );
    let task_adapter = TaskTraceAdapter::new(publisher, SourceType::Core, "core");
    let mut source_events = 0;
    let mut callback = |e: SchedulerEvent| -> Result<(), SchedulerError> {
        source_events += 1;
        t.observe(&e);
        // Same Conversation functional projection is deliberately observed too.
        if let Some(id) = task {
            let kind = match e {
                SchedulerEvent::Selected {
                    provider_id,
                    model,
                    attempt,
                    routing_reason,
                    score,
                } => Some(TaskEventKind::ProviderSelected {
                    provider_id,
                    model,
                    attempt,
                    routing_reason: routing_reason.into(),
                    score,
                }),
                SchedulerEvent::Chunk { provider_id, text } => Some(TaskEventKind::ProviderChunk {
                    provider_id,
                    chunk: text,
                }),
                SchedulerEvent::Retry {
                    provider_id,
                    reason_code,
                } => Some(TaskEventKind::ProviderRetry {
                    provider_id,
                    reason_code: reason_code.into(),
                }),
                SchedulerEvent::Fallback {
                    from,
                    to,
                    reason_code,
                } => Some(TaskEventKind::ProviderFallback {
                    from_provider_id: from,
                    to_provider_id: to,
                    reason_code: reason_code.into(),
                }),
                _ => None,
            };
            if let Some(k) = kind {
                task_adapter.observe(id, &k);
            }
        }
        Ok(())
    };
    let request = request();
    let budget = TaskBudget {
        max_provider_calls: 3,
        max_output_tokens: Some(100),
    };
    let retry = RetryPolicy {
        enabled: true,
        max_retries: 1,
        initial_backoff_ms: 0,
    };
    let cancelled = AtomicBool::new(false);
    let result = if role == CognitiveRole::Worker {
        scheduler
            .run_with_retry_conservative_output(request, budget, retry, &cancelled, &mut callback)
            .await
    } else {
        scheduler
            .run_with_retry(request, budget, retry, &cancelled, &mut callback)
            .await
    }
    .unwrap();
    let recorded = requests.lock().unwrap().clone();
    (result, recorded, source_events)
}
#[tokio::test]
async fn scheduler_zero_extra_inference_requests_attempts_usage_output_and_failures_gate() {
    let baseline = provider_run(
        PassiveTracePublisher::new(Arc::new(NoopPublisher)),
        CognitiveRole::Conversation,
        Some(TaskId(17)),
        None,
        3,
        true,
    )
    .await;
    for mode in 0..4 {
        let bus = OperationalTraceBus::isolated();
        let subscriber = if mode == 1 {
            Some(bus.subscribe().unwrap())
        } else {
            None
        };
        if mode == 3 {
            bus.seed_test_sequence(u64::MAX);
        }
        let p = if mode == 2 {
            PassiveTracePublisher::new(Arc::new(RejectPublisher))
        } else {
            PassiveTracePublisher::new(bus.clone())
        };
        let current = provider_run(
            p,
            CognitiveRole::Conversation,
            Some(TaskId(17)),
            None,
            3,
            true,
        )
        .await;
        assert!(baseline.1 == current.1);
        assert_eq!(current.0.text, baseline.0.text);
        assert_eq!(current.0.usage, baseline.0.usage);
        assert_eq!(current.2, baseline.2);
        assert_eq!(current.0.usage.provider_calls, 3);
        assert_eq!(current.0.usage.retries, 1);
        assert_eq!(current.0.usage.fallbacks, 1);
        assert_eq!(current.0.usage.input_tokens, 11);
        assert_eq!(current.0.usage.output_tokens, 13);
        if mode < 2 {
            let e = events(&bus);
            assert_eq!(
                e.iter().filter(|e| code(e) == "provider_selected").count(),
                3
            );
            assert_eq!(e.iter().filter(|e| code(e) == "provider_retry").count(), 1);
            assert_eq!(
                e.iter().filter(|e| code(e) == "provider_fallback").count(),
                1
            );
            assert_eq!(e.iter().filter(|e| code(e) == "text").count(), 3);
            assert!(!payloads(&bus).contains("SECRET"));
        }
        drop(subscriber);
    }
    println!("LR9D no-extra-call: baseline=active=3 calls, 1 retry, 1 fallback, input=11 output=13 accounted=13; requests/output/usage equal");
}

#[test]
fn multi_source_stress_and_reproducible_overhead_gate() {
    // Two Conversation tasks, four workers, Summary, and two actual Codex fakes.
    // Compare no-op / headless / full slow live queue. No timing pass/fail limit.
    let mut baseline = None;
    for mode in 0..3 {
        let bus = OperationalTraceBus::isolated();
        let subscriber = if mode == 2 {
            Some(bus.subscribe().unwrap())
        } else {
            None
        };
        let publisher = if mode == 0 {
            PassiveTracePublisher::new(Arc::new(NoopPublisher))
        } else {
            PassiveTracePublisher::new(bus.clone())
        };
        let barrier = Arc::new(std::sync::Barrier::new(10));
        let started = Instant::now();
        let outcomes = std::thread::scope(|scope| {
            let mut handles = vec![];
            for n in 0..7 {
                let (publisher, barrier) = (publisher.clone(), barrier.clone());
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    let (role, id, subtask) = if n < 2 {
                        (CognitiveRole::Conversation, Some(TaskId(17 + n)), None)
                    } else if n < 6 {
                        (
                            CognitiveRole::Worker,
                            Some(TaskId(17 + (n - 2) / 2)),
                            Some(format!("unit-{}", n - 2)),
                        )
                    } else {
                        (CognitiveRole::Summary, None, None)
                    };
                    let task = TaskTraceAdapter::new(
                        publisher.clone(),
                        if subtask.is_some() {
                            SourceType::Worker
                        } else {
                            SourceType::Core
                        },
                        if subtask.is_some() { "worker" } else { "core" },
                    );
                    let summary = SummaryTraceAdapter::new(publisher.clone());
                    let graph = TaskTraceAdapter::new(
                        publisher.clone(),
                        SourceType::TaskGraph,
                        "task_graph",
                    );
                    if let Some(id) = id {
                        if let Some(subtask) = &subtask {
                            let mut event = subtask_started();
                            if let TaskEventKind::SubtaskStarted {
                                subtask_id,
                                unit_id,
                                ..
                            } = &mut event
                            {
                                *subtask_id = subtask.clone();
                                *unit_id =
                                    crate::cognitive_resources::ExecutionUnitId::new(id.0, n)
                                        .unwrap();
                            }
                            task.observe(id, &event);
                        } else {
                            task.observe(id, &TaskEventKind::TaskStarted);
                            graph.observe(id, &TaskEventKind::TaskPlanned { step_count: 2 });
                        }
                    } else {
                        summary.observe(SummaryTraceState::Started);
                    }
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    let result = rt.block_on(provider_run(
                        publisher,
                        role,
                        id,
                        subtask.as_deref(),
                        3000,
                        true,
                    ));
                    if let Some(id) = id {
                        if let Some(subtask) = &subtask {
                            graph.observe(
                                id,
                                &TaskEventKind::SubtaskCompleted {
                                    subtask_id: subtask.clone(),
                                    provider_id: "q".into(),
                                    checkpoint_id: crate::cognitive_resources::CheckpointId::new(
                                        crate::cognitive_resources::ExecutionUnitId::new(id.0, n)
                                            .unwrap(),
                                        1,
                                    )
                                    .unwrap(),
                                },
                            );
                        } else {
                            task.observe(id, &TaskEventKind::TaskCompleted);
                        }
                    } else {
                        summary.observe(SummaryTraceState::Completed);
                    }
                    result
                }));
            }
            let (summary_publisher, summary_barrier) = (publisher.clone(), barrier.clone());
            let summary = scope.spawn(move || {
                summary_barrier.wait();
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(crate::cognition::summary::lr9d_tests::stress_summary(
                    summary_publisher,
                ))
            });
            let mut agents = vec![];
            for _ in 0..2 {
                let (publisher, barrier) = (publisher.clone(), barrier.clone());
                agents.push(scope.spawn(move || {
                    barrier.wait();
                    crate::agents::codex::backend::lifecycle_tests::lr9d_fake_trace_operation(
                        publisher, 1500,
                    )
                }));
            }
            let providers = handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>();
            let agents = agents.into_iter().map(|h| h.join().unwrap()).sum::<usize>();
            (providers, agents, summary.join().unwrap())
        });
        let signature: Vec<_> = outcomes
            .0
            .iter()
            .map(|(r, requests, count)| (r.text.clone(), r.usage.clone(), requests.clone(), *count))
            .collect();
        let comparison = (signature, outcomes.2 .1.clone());
        if let Some(expected) = &baseline {
            assert!(expected == &comparison);
        } else {
            baseline = Some(comparison);
        }
        assert_eq!(outcomes.1, 2);
        assert_eq!(outcomes.2 .0, 1);
        assert!(outcomes.2 .1[0].contains("SUMMARY-TRANSCRIPT-SECRET"));
        assert_eq!(
            outcomes
                .0
                .iter()
                .map(|(r, _, _)| r.usage.provider_calls)
                .sum::<u32>(),
            21
        );
        let stats = bus.stats();
        assert!(stats.retained_events <= MAX_RETAINED_EVENTS);
        assert!(stats.retained_bytes <= MAX_RETAINED_BYTES);
        if mode > 0 {
            assert!(stats.evicted.stream > 0);
            assert_eq!(stats.evicted.state, 0);
            assert_eq!(stats.evicted.critical, 0);
            assert_eq!(stats.dropped.state, 0);
            assert_eq!(stats.dropped.critical, 0);
            let e = events(&bus);
            assert_eq!(
                e.iter()
                    .filter(|e| e.retention_class() == RetentionClass::Critical)
                    .count(),
                6
            );
            assert_eq!(
                e.iter()
                    .filter(|e| code(e) == "output_observed"
                        && e.provenance().source.source_type == SourceType::CognitiveProvider)
                    .count(),
                6
            );
            for e in &e {
                let p = e.provenance();
                match p.source.source_type {
                    SourceType::CognitiveProvider => {
                        assert!(matches!(p.source.id.as_str(), "q" | "gemini"));
                        if let Some(subtask) = &p.subtask_id {
                            let n: u64 = subtask
                                .as_str()
                                .strip_prefix("unit-")
                                .unwrap()
                                .parse()
                                .unwrap();
                            assert_eq!(p.task_id, Some(TaskId(17 + n / 2)));
                            assert!(!matches!(e.kind(), OperationalKind::TextDelta { .. }));
                        }
                        if p.task_id.is_none() {
                            assert!(!matches!(e.kind(), OperationalKind::TextDelta { .. }));
                        }
                    }
                    SourceType::SpecialistAgent => assert!(p
                        .correlation_id
                        .as_ref()
                        .unwrap()
                        .as_str()
                        .starts_with("agent-call-")),
                    _ => {}
                }
            }
            assert!(!payloads(&bus).contains("SECRET"));
            assert!(!payloads(&bus).contains("/private/raw"));
            if mode == 2 {
                assert!(stats.live_delivery_dropped > 0);
            }
        }
        // Count adapter inputs, not duplicated TaskEvent projections or protocol
        // notifications without an observation. Each Codex contributes 3000
        // display deltas + four lifecycle events. The real structured Summary
        // reduces 3000 provider chunks to one Scheduler OutputObserved upstream:
        // three routing events + that fact + two Summary lifecycle events.
        let source_events = outcomes.0.iter().map(|(_, _, n)| *n).sum::<usize>()
            + 2 * (3000 + 4)
            + 2 * 3
            + 4 * 2
            + 2
            + 6;
        println!("LR9D overhead mode={mode} source_events={source_events} operational_events={} retained={} bytes={} evicted_stream={} dropped_stream={} live_dropped={} elapsed_ms={} provider_calls=22 agent_turn_starts=2",stats.published,stats.retained_events,stats.retained_bytes,stats.evicted.stream,stats.dropped.stream,stats.live_delivery_dropped,started.elapsed().as_millis());
        drop(subscriber);
        // Reopen sees only the bounded retained window; publishing remains valid.
        if mode > 0 {
            TaskTraceAdapter::new(
                PassiveTracePublisher::new(bus.clone()),
                SourceType::Core,
                "core",
            )
            .observe(TaskId(19), &TaskEventKind::TaskCompleted);
            assert_eq!(bus.stats().active_subscribers, 0);
            assert!(events(&bus).len() <= MAX_RETAINED_EVENTS);
        }
    }
}

#[tokio::test]
async fn invalid_scheduler_correlation_never_changes_provider_execution() {
    let baseline = provider_run(
        PassiveTracePublisher::new(Arc::new(NoopPublisher)),
        CognitiveRole::Conversation,
        Some(TaskId(17)),
        None,
        3,
        true,
    )
    .await;
    let bus = OperationalTraceBus::isolated();
    let invalid = provider_run(
        PassiveTracePublisher::new(bus.clone()),
        CognitiveRole::Conversation,
        Some(TaskId(0)),
        Some("invalid subtask"),
        3,
        true,
    )
    .await;
    assert!(baseline.1 == invalid.1);
    assert_eq!(baseline.0.usage, invalid.0.usage);
    assert_eq!(baseline.0.text, invalid.0.text);
    assert_eq!(bus.stats().published, 0);
}
#[test]
fn taskgraph_dedup_keeps_lifecycle_and_scheduler_facts_once() {
    let bus = OperationalTraceBus::isolated();
    let p = PassiveTracePublisher::new(bus.clone());
    let worker = TaskTraceAdapter::new(p.clone(), SourceType::Worker, "worker");
    let graph = TaskTraceAdapter::new(p.clone(), SourceType::TaskGraph, "task_graph");
    worker.observe(TaskId(17), &subtask_started());
    let mut scheduler = SchedulerTraceAdapter::new(
        p,
        SchedulerTraceContext::new(Some(TaskId(17)), Some("unit-a"), CognitiveRole::Worker),
    );
    scheduler.observe(&selected(1));
    for _ in 0..3000 {
        scheduler.observe(&SchedulerEvent::Chunk {
            provider_id: "p".into(),
            text: "HIDDEN-SECRET".into(),
        });
        graph.observe(
            TaskId(17),
            &TaskEventKind::SubtaskOutputObserved {
                subtask_id: "unit-a".into(),
                provider_id: "p".into(),
            },
        );
    }
    graph.observe(
        TaskId(17),
        &TaskEventKind::SubtaskFailed {
            subtask_id: "unit-a".into(),
            provider_id: Some("p".into()),
            error_code: "ERROR-SECRET".into(),
        },
    );
    assert_eq!(
        events(&bus).iter().map(|e| code(e)).collect::<Vec<_>>(),
        [
            "subtask_started",
            "provider_selected",
            "output_observed",
            "subtask_failed"
        ]
    );
    assert!(!payloads(&bus).contains("SECRET"));
}

#[tokio::test]
async fn conservative_worker_accounting_and_requests_match_noop_trace() {
    let baseline = provider_run(
        PassiveTracePublisher::new(Arc::new(NoopPublisher)),
        CognitiveRole::Worker,
        Some(TaskId(17)),
        Some("unit-a"),
        3,
        true,
    )
    .await;
    let bus = OperationalTraceBus::isolated();
    let active = provider_run(
        PassiveTracePublisher::new(bus.clone()),
        CognitiveRole::Worker,
        Some(TaskId(17)),
        Some("unit-a"),
        3,
        true,
    )
    .await;
    assert!(baseline.1 == active.1);
    assert_eq!(baseline.0.usage, active.0.usage);
    assert_eq!(baseline.0.text, active.0.text);
    assert_eq!(active.0.usage.provider_calls, 3);
    assert_eq!(active.0.usage.retries, 1);
    assert_eq!(active.0.usage.fallbacks, 1);
    assert_eq!(active.0.usage.output_tokens, 13);
    assert_eq!(active.0.usage.output_tokens_accounted, 80);
    assert!(events(&bus)
        .iter()
        .all(|e| !matches!(e.kind(), OperationalKind::TextDelta { .. })));
}
