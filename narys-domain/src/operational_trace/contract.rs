use crate::luna::task::TaskId;

pub const MAX_IDENTIFIER_BYTES: usize = 64;
pub const MAX_TEXT_BYTES: usize = 8 * 1024;
/// Deterministic charge, not serialized size or allocator/RSS measurement.
/// Includes fixed metadata, plus UTF-8 lengths of every owned string.
pub const EVENT_BASE_ESTIMATED_BYTES: usize = 256;
pub const MAX_EVENT_ESTIMATED_BYTES: usize =
    EVENT_BASE_ESTIMATED_BYTES + 6 * MAX_IDENTIFIER_BYTES + MAX_TEXT_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceError {
    InvalidIdentifier,
    TextTooLarge,
    InvalidTaskId,
    InvalidBatchLimits,
    FutureCursor,
    SubscriberLimit,
    SequenceExhausted,
}

/// Machine identifier, not a label, path, command or arbitrary provenance text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceId(Box<str>);
impl TraceId {
    pub fn new(value: &str) -> Result<Self, TraceError> {
        if value.is_empty()
            || value.len() > MAX_IDENTIFIER_BYTES
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':' | b'/'))
        {
            return Err(TraceError::InvalidIdentifier);
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact UTF-8 content. Rejection happens before allocation into the contract;
/// there is no truncation and no heuristic secret detection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceText(Box<str>);
impl TraceText {
    pub fn new(value: &str) -> Result<Self, TraceError> {
        if value.len() > MAX_TEXT_BYTES {
            return Err(TraceError::TextTooLarge);
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceType {
    Core,
    Scheduler,
    TaskGraph,
    CognitiveProvider,
    Worker,
    SpecialistAgent,
    ExecutionBroker,
    TerminalProcess,
    Human,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceSource {
    pub source_type: SourceType,
    pub id: TraceId,
    pub instance: Option<TraceId>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Provenance {
    pub source: TraceSource,
    pub task_id: Option<TaskId>,
    pub subtask_id: Option<TraceId>,
    pub correlation_id: Option<TraceId>,
    pub coalescing_key: Option<TraceId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RetentionClass {
    Stream,
    State,
    Critical,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CriticalKind {
    ApprovalRequired,
    PolicyBlocked,
    Failed,
    Cancelled,
    Completed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateKind {
    Started,
    Planning,
    Routing,
    Fallback,
    Retry,
    SubtaskLifecycle,
    CommandLifecycle,
    ToolLifecycle,
    Checkpoint,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextChannel {
    Stdout,
    Stderr,
    ProviderText,
    AgentMessage,
    Progress,
    /// Only summaries explicitly exposed and authorized for display by a backend.
    /// There is deliberately no hidden/private reasoning channel.
    DisplayReasoningSummary,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationalKind {
    Critical {
        kind: CriticalKind,
        code: TraceId,
        message: TraceText,
    },
    State {
        kind: StateKind,
        code: TraceId,
        detail: TraceText,
    },
    TextDelta {
        channel: TextChannel,
        text: TraceText,
    },
}
impl OperationalKind {
    pub fn retention_class(&self) -> RetentionClass {
        match self {
            Self::Critical { .. } => RetentionClass::Critical,
            Self::State { .. } => RetentionClass::State,
            Self::TextDelta { .. } => RetentionClass::Stream,
        }
    }
    fn payload_bytes(&self) -> usize {
        match self {
            Self::Critical { code, message, .. } => code.0.len() + message.0.len(),
            Self::State { code, detail, .. } => code.0.len() + detail.0.len(),
            Self::TextDelta { text, .. } => text.0.len(),
        }
    }
}

/// No producer-supplied sequence, timestamp or priority flag. Typed fields
/// cannot contain oversized strings, including after cloning. No Deserialize
/// escape hatch; a future IPC adapter must validate its own boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventDraft {
    provenance: Provenance,
    kind: OperationalKind,
}
impl EventDraft {
    pub fn new(provenance: Provenance, kind: OperationalKind) -> Result<Self, TraceError> {
        // Preserve the Core's current JS-safe TaskId contract, without importing
        // scheduler/provider/TaskEvent execution semantics.
        if provenance
            .task_id
            .is_some_and(|id| id.0 == 0 || id.0 > 9_007_199_254_740_991)
        {
            return Err(TraceError::InvalidTaskId);
        }
        Ok(Self { provenance, kind })
    }
    pub fn estimated_bytes(&self) -> usize {
        let p = &self.provenance;
        EVENT_BASE_ESTIMATED_BYTES
            + p.source.id.0.len()
            + [
                &p.source.instance,
                &p.subtask_id,
                &p.correlation_id,
                &p.coalescing_key,
            ]
            .iter()
            .map(|id| id.as_ref().map_or(0, |id| id.0.len()))
            .sum::<usize>()
            + self.kind.payload_bytes()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct OperationalEvent {
    sequence: u64,
    /// Milliseconds since UNIX epoch, sampled by the bus at publication. Wall
    /// time can move backwards; sequence alone defines the total order.
    observed_at_unix_ms: u64,
    draft: EventDraft,
}
impl OperationalEvent {
    pub(super) fn observed(sequence: u64, observed_at_unix_ms: u64, draft: EventDraft) -> Self {
        Self {
            sequence,
            observed_at_unix_ms,
            draft,
        }
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn observed_at_unix_ms(&self) -> u64 {
        self.observed_at_unix_ms
    }
    pub fn provenance(&self) -> &Provenance {
        &self.draft.provenance
    }
    pub fn kind(&self) -> &OperationalKind {
        &self.draft.kind
    }
    pub fn estimated_bytes(&self) -> usize {
        self.draft.estimated_bytes()
    }
    pub fn retention_class(&self) -> RetentionClass {
        self.kind().retention_class()
    }
}
