//! Explicit projection of already-authorized OperationalEvent fields, no Debug
//! serialization and no ExecutionRequest/Result or environment projection.
use crate::operational_trace::*;
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TraceDto {
    pub sequence: String,
    pub last_sequence: String,
    pub fragments: usize,
    pub observed_at_unix_ms: u64,
    pub last_observed_at_unix_ms: u64,
    pub class: &'static str,
    pub source_type: &'static str,
    pub source_id: String,
    pub source_instance: Option<String>,
    pub task_id: Option<u64>,
    pub subtask_id: Option<String>,
    pub correlation_id: Option<String>,
    pub kind: &'static str,
    pub code: Option<String>,
    pub channel: Option<&'static str>,
    pub text: String,
}
fn source_name(s: SourceType) -> &'static str {
    match s {
        SourceType::Core => "core",
        SourceType::Scheduler => "scheduler",
        SourceType::TaskGraph => "task_graph",
        SourceType::CognitiveProvider => "cognitive_provider",
        SourceType::Worker => "worker",
        SourceType::SpecialistAgent => "specialist_agent",
        SourceType::ExecutionBroker => "execution_broker",
        SourceType::TerminalProcess => "terminal_process",
        SourceType::Human => "human",
    }
}
fn channel_name(c: TextChannel) -> &'static str {
    match c {
        TextChannel::Stdout => "stdout",
        TextChannel::Stderr => "stderr",
        TextChannel::ProviderText => "provider_text",
        TextChannel::AgentMessage => "agent_message",
        TextChannel::Progress => "progress",
        TextChannel::DisplayReasoningSummary => "display_reasoning_summary",
    }
}
impl TraceDto {
    fn base(p: &Provenance, first: u64, last: u64, at: u64, last_at: u64) -> Self {
        Self {
            sequence: first.to_string(),
            last_sequence: last.to_string(),
            fragments: 1,
            observed_at_unix_ms: at,
            last_observed_at_unix_ms: last_at,
            class: "STREAM",
            source_type: source_name(p.source.source_type),
            source_id: p.source.id.as_str().into(),
            source_instance: p.source.instance.as_ref().map(|id| id.as_str().into()),
            task_id: p.task_id.map(|id| id.0),
            subtask_id: p.subtask_id.as_ref().map(|id| id.as_str().into()),
            correlation_id: p.correlation_id.as_ref().map(|id| id.as_str().into()),
            kind: "text_delta",
            code: None,
            channel: None,
            text: String::new(),
        }
    }
    pub fn event(e: &OperationalEvent) -> Self {
        let mut d = Self::base(
            e.provenance(),
            e.sequence(),
            e.sequence(),
            e.observed_at_unix_ms(),
            e.observed_at_unix_ms(),
        );
        match e.kind() {
            OperationalKind::TextDelta { channel, text } => {
                d.channel = Some(channel_name(*channel));
                d.text = text.as_str().into();
            }
            OperationalKind::State { kind, code, detail } => {
                d.class = "STATE";
                d.kind = match kind {
                    StateKind::Started => "started",
                    StateKind::Planning => "planning",
                    StateKind::Routing => "routing",
                    StateKind::Fallback => "fallback",
                    StateKind::Retry => "retry",
                    StateKind::SubtaskLifecycle => "subtask_lifecycle",
                    StateKind::CommandLifecycle => "command_lifecycle",
                    StateKind::ToolLifecycle => "tool_lifecycle",
                    StateKind::Checkpoint => "checkpoint",
                };
                d.code = Some(code.as_str().into());
                d.text = detail.as_str().into();
            }
            OperationalKind::Critical {
                kind,
                code,
                message,
            } => {
                d.class = "CRITICAL";
                d.kind = match kind {
                    CriticalKind::ApprovalRequired => "approval_required",
                    CriticalKind::PolicyBlocked => "policy_blocked",
                    CriticalKind::Failed => "failed",
                    CriticalKind::Cancelled => "cancelled",
                    CriticalKind::Completed => "completed",
                };
                d.code = Some(code.as_str().into());
                d.text = message.as_str().into();
            }
        }
        d
    }
    pub fn item(item: CoalescedItem) -> Self {
        match item {
            CoalescedItem::Event(e) => Self::event(&e),
            CoalescedItem::Text(t) => {
                let mut d = Self::base(
                    &t.provenance,
                    t.first_sequence,
                    t.last_sequence,
                    t.first_observed_at_unix_ms,
                    t.last_observed_at_unix_ms,
                );
                d.fragments = t.fragments;
                d.channel = Some(channel_name(t.channel));
                d.text = t.text().into();
                d
            }
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TraceBatchDto {
    pub cursor: String,
    pub delivery_epoch: String,
    pub missing_events: String,
    pub live_delivery_dropped: String,
    pub replay_complete: bool,
    pub events: Vec<TraceDto>,
}
pub(crate) fn trace_batch(after: u64, b: ReplayBatch, live_dropped: u64) -> TraceBatchDto {
    // Sequence holes, including sparse priority retention and a lost tail,
    // are metadata. Never synthesize content or infer a source/class for holes.
    let missing = b
        .next_after
        .saturating_sub(after)
        .saturating_sub(b.events.len() as u64);
    TraceBatchDto {
        cursor: b.next_after.to_string(),
        delivery_epoch: "0".into(),
        missing_events: missing.to_string(),
        live_delivery_dropped: live_dropped.to_string(),
        replay_complete: b.replay_complete,
        events: coalesce_batch(&b.events)
            .expect("bus returns bounded batches")
            .into_iter()
            .map(TraceDto::item)
            .collect(),
    }
}
/// Raw frame: LE u64 cursor, missing chunks, cumulative dropped bytes; then
/// exact concatenated PTY bytes. JS decodes counters as BigInt/decimal string.
pub(crate) fn pty_frame(after: u64, b: &crate::execution::PtyReplay) -> Vec<u8> {
    let missing = b
        .next_after
        .saturating_sub(after)
        .saturating_sub(b.chunks.len() as u64);
    let mut bytes = Vec::with_capacity(24 + b.chunks.iter().map(|c| c.bytes.len()).sum::<usize>());
    for n in [b.next_after, missing, b.dropped_bytes] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    for c in &b.chunks {
        bytes.extend_from_slice(&c.bytes);
    }
    bytes
}
