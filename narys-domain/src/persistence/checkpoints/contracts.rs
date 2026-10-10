use crate::{
    cognition::{allocation_policy::CognitiveRoleAllocationPolicy, policy::*},
    cognitive_resources::*,
};
use serde::{Deserialize, Serialize};

pub const MAX_CONTEXT_REFERENCES: usize = 32;
pub const MAX_CHECKPOINT_BYTES: usize = 8192;
pub const MAX_POLICY_BYTES: usize = 16384;
pub const MAX_VERIFIED_CHECKPOINTS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    InvalidIdentity,
    UnitMismatch,
    TaskMismatch,
    BoundaryUnconfirmed,
    UnitNotCompleted,
    SequenceNotAdvancing,
    InvalidProvenance,
    InvalidPolicy,
    ContextLimit,
    InvalidContext,
    Conflict,
    InvalidRecord,
    HistoryContradiction,
    ReferenceNotCommitted,
    VerificationLimit,
    DurabilityUnavailable,
    TransactionActive,
    Read,
    Write,
}
impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CheckpointError {}

/// Immutable copy of the existing B4 DTOs, not a second allocation policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskPolicySnapshot {
    routing: CognitiveRolePolicy,
    allocation: Option<CognitiveRoleAllocationPolicy>,
}
impl TaskPolicySnapshot {
    pub fn new(
        routing: CognitiveRolePolicy,
        allocation: Option<CognitiveRoleAllocationPolicy>,
    ) -> Result<Self, CheckpointError> {
        let value = Self {
            routing,
            allocation,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn role(&self) -> CognitiveRole {
        self.routing.role
    }
    pub fn routing(&self) -> &CognitiveRolePolicy {
        &self.routing
    }
    pub fn allocation(&self) -> Option<&CognitiveRoleAllocationPolicy> {
        self.allocation.as_ref()
    }
    pub(super) fn validate(&self) -> Result<(), CheckpointError> {
        self.routing
            .validate()
            .map_err(|_| CheckpointError::InvalidPolicy)?;
        for target in &self.routing.targets {
            RuntimeId::new(&target.provider_id).map_err(|_| CheckpointError::InvalidPolicy)?;
            ModelId::new(&target.model).map_err(|_| CheckpointError::InvalidPolicy)?;
        }
        match (&self.allocation, self.routing.routing_mode) {
            (Some(value), RoutingMode::Auto) if value.role == self.routing.role => {
                value
                    .to_runtime()
                    .map_err(|_| CheckpointError::InvalidPolicy)?;
            }
            (None, RoutingMode::Fixed | RoutingMode::Preferred) => {}
            _ => return Err(CheckpointError::InvalidPolicy),
        }
        if serde_json::to_vec(self)
            .map_err(|_| CheckpointError::InvalidPolicy)?
            .len()
            > MAX_POLICY_BYTES
        {
            return Err(CheckpointError::ContextLimit);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExecutionSource {
    RootTask,
    TaskGraphSubtask(String),
}
impl ExecutionSource {
    pub fn subtask(id: impl Into<String>) -> Result<Self, CheckpointError> {
        let source = Self::TaskGraphSubtask(id.into());
        source.validate()?;
        Ok(source)
    }
    pub(super) fn validate(&self) -> Result<(), CheckpointError> {
        if let Self::TaskGraphSubtask(id) = self {
            if id.is_empty()
                || id.len() > 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            {
                return Err(CheckpointError::InvalidProvenance);
            }
        }
        Ok(())
    }
    pub(super) fn key(&self) -> (&'static str, &str) {
        match self {
            Self::RootTask => ("root_task", ""),
            Self::TaskGraphSubtask(id) => ("task_graph_subtask", id),
        }
    }
}

/// All context consists of durable receipt references. No generic text/payload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandoffContext {
    completed_dependencies: Vec<CheckpointId>,
}
impl HandoffContext {
    pub fn new(mut completed_dependencies: Vec<CheckpointId>) -> Result<Self, CheckpointError> {
        if completed_dependencies.len() > MAX_CONTEXT_REFERENCES {
            return Err(CheckpointError::ContextLimit);
        }
        completed_dependencies.sort_unstable();
        if completed_dependencies
            .windows(2)
            .any(|pair| pair[0] == pair[1])
        {
            return Err(CheckpointError::InvalidContext);
        }
        Ok(Self {
            completed_dependencies,
        })
    }
    pub fn completed_dependencies(&self) -> &[CheckpointId] {
        &self.completed_dependencies
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointProvenance {
    pub source: ExecutionSource,
    pub runtime_id: RuntimeId,
}

/// Prepared Core observations are not durable receipts. Construction validates
/// linkage; only the repository creates CheckpointRecord after SQLite COMMIT.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CognitiveCheckpoint {
    pub(super) id: CheckpointId,
    pub(super) effects: EffectState,
    pub(super) predecessor: Option<CheckpointId>,
    pub(super) policy: TaskPolicySnapshot,
    pub(super) allocation: AllocationVariant,
    pub(super) provenance: CheckpointProvenance,
    pub(super) context: HandoffContext,
}
impl CognitiveCheckpoint {
    pub fn confirmed(
        id: CheckpointId,
        unit: ExecutionUnitFacts,
        boundary: HandoffBoundary,
        predecessor: Option<CheckpointId>,
        policy: TaskPolicySnapshot,
        allocation: AllocationVariant,
        provenance: CheckpointProvenance,
        context: HandoffContext,
    ) -> Result<Self, CheckpointError> {
        if id.unit_id().root_task_id() != unit.id.root_task_id() {
            return Err(CheckpointError::TaskMismatch);
        }
        if id.unit_id() != unit.id {
            return Err(CheckpointError::UnitMismatch);
        }
        if unit.state != ExecutionUnitState::Completed {
            return Err(CheckpointError::UnitNotCompleted);
        }
        if boundary != (HandoffBoundary::ConfirmedCompletion { checkpoint: id }) {
            return Err(CheckpointError::BoundaryUnconfirmed);
        }
        policy.validate()?;
        provenance.source.validate()?;
        let target = policy
            .routing
            .targets
            .iter()
            .find(|t| t.provider_id == provenance.runtime_id.as_str())
            .ok_or(CheckpointError::InvalidProvenance)?;
        if policy.routing.routing_mode != RoutingMode::Auto {
            if target.model != allocation.model_id.as_str()
                || target.thinking_level.map(ThinkingLevel::as_str)
                    != allocation.effort.as_ref().map(EffortId::as_str)
            {
                return Err(CheckpointError::InvalidProvenance);
            }
        }
        for reference in predecessor
            .into_iter()
            .chain(context.completed_dependencies.iter().copied())
        {
            if reference.unit_id().root_task_id() != id.unit_id().root_task_id() {
                return Err(CheckpointError::TaskMismatch);
            }
            if reference.unit_id().sequence() >= id.unit_id().sequence() {
                return Err(CheckpointError::SequenceNotAdvancing);
            }
        }
        let value = Self {
            id,
            effects: unit.effects,
            predecessor,
            policy,
            allocation,
            provenance,
            context,
        };
        if value.encode()?.len() > MAX_CHECKPOINT_BYTES {
            return Err(CheckpointError::ContextLimit);
        }
        Ok(value)
    }
    pub fn id(&self) -> CheckpointId {
        self.id
    }
    pub fn effects(&self) -> EffectState {
        self.effects
    }
    pub fn predecessor(&self) -> Option<CheckpointId> {
        self.predecessor
    }
    pub fn policy(&self) -> &TaskPolicySnapshot {
        &self.policy
    }
    pub fn allocation(&self) -> &AllocationVariant {
        &self.allocation
    }
    pub fn provenance(&self) -> &CheckpointProvenance {
        &self.provenance
    }
    pub fn context(&self) -> &HandoffContext {
        &self.context
    }
    pub(super) fn encode(&self) -> Result<String, CheckpointError> {
        serde_json::to_string(&CheckpointWire::from(self))
            .map_err(|_| CheckpointError::InvalidRecord)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointRecord {
    pub(super) checkpoint: CognitiveCheckpoint,
    pub(super) committed_at: String,
}
impl CheckpointRecord {
    pub fn checkpoint(&self) -> &CognitiveCheckpoint {
        &self.checkpoint
    }
    pub fn committed_at(&self) -> &str {
        &self.committed_at
    }
    pub fn boundary(&self) -> HandoffBoundary {
        if self.checkpoint.effects == EffectState::UnknownOrInFlight {
            HandoffBoundary::Unknown
        } else {
            HandoffBoundary::ConfirmedCompletion {
                checkpoint: self.checkpoint.id,
            }
        }
    }
    pub fn replay(&self) -> ReplayDecision {
        can_handoff(&HandoffRequest {
            previous_unit: None,
            requested_unit: ExecutionUnitFacts {
                id: self.checkpoint.id.unit_id(),
                state: ExecutionUnitState::Completed,
                effects: self.checkpoint.effects,
            },
            boundary: HandoffBoundary::Unconfirmed,
            cancellation_observed: false,
        })
        .requested_unit_replay()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckpointLoadResult {
    Committed(CheckpointRecord),
    Absent,
    HistoryWithoutCheckpoint { state: ExecutionUnitState },
    Invalid(CheckpointError),
}
impl CheckpointLoadResult {
    /// Absence never proves NotStarted or authorizes automatic replay.
    pub fn replay(&self) -> ReplayDecision {
        match self {
            Self::Committed(record) => record.replay(),
            Self::HistoryWithoutCheckpoint { state } => match state {
                ExecutionUnitState::Completed => {
                    ReplayDecision::Forbidden(ReplayReason::UnitCompleted)
                }
                ExecutionUnitState::Cancelled => {
                    ReplayDecision::Forbidden(ReplayReason::UnitCancelled)
                }
                ExecutionUnitState::Failed => ReplayDecision::Forbidden(ReplayReason::UnitFailed),
                _ => ReplayDecision::Forbidden(ReplayReason::UnitStateUnknown),
            },
            _ => ReplayDecision::Forbidden(ReplayReason::UnitStateUnknown),
        }
    }
}

// Only these private, bounded DTOs deserialize database input; C1 is unchanged.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReceiptWire {
    root_task_id: u64,
    unit_sequence: u64,
    sequence: u64,
}
impl From<CheckpointId> for ReceiptWire {
    fn from(id: CheckpointId) -> Self {
        Self {
            root_task_id: id.unit_id().root_task_id(),
            unit_sequence: id.unit_id().sequence(),
            sequence: id.sequence(),
        }
    }
}
impl ReceiptWire {
    fn validated(self) -> Result<CheckpointId, CheckpointError> {
        let unit = ExecutionUnitId::new(self.root_task_id, self.unit_sequence)
            .map_err(|_| CheckpointError::InvalidIdentity)?;
        CheckpointId::new(unit, self.sequence).map_err(|_| CheckpointError::InvalidIdentity)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredEffect {
    NotStarted,
    Committed,
    UnknownOrInFlight,
}
impl StoredEffect {
    pub(super) fn from_effect(effect: EffectState) -> Self {
        match effect {
            EffectState::NotStarted => Self::NotStarted,
            EffectState::Committed => Self::Committed,
            EffectState::UnknownOrInFlight => Self::UnknownOrInFlight,
        }
    }
    pub(super) fn effect(&self) -> EffectState {
        match self {
            Self::NotStarted => EffectState::NotStarted,
            Self::Committed => EffectState::Committed,
            Self::UnknownOrInFlight => EffectState::UnknownOrInFlight,
        }
    }
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Committed => "committed",
            Self::UnknownOrInFlight => "unknown_or_in_flight",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VariantWire {
    resource_id: ResourceId,
    access_path: AccessPath,
    billing_domain_id: BillingDomainId,
    model_id: ModelId,
    effort: Option<EffortId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfirmedLifecycle {
    Completed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfirmedBoundary {
    ConfirmedCompletion,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CheckpointWire {
    id: ReceiptWire,
    lifecycle: ConfirmedLifecycle,
    boundary: ConfirmedBoundary,
    effects: StoredEffect,
    predecessor: Option<ReceiptWire>,
    allocation: VariantWire,
    source: ExecutionSource,
    runtime_id: RuntimeId,
    completed_dependencies: Vec<ReceiptWire>,
}
impl From<&CognitiveCheckpoint> for CheckpointWire {
    fn from(value: &CognitiveCheckpoint) -> Self {
        let a = &value.allocation;
        Self {
            id: value.id.into(),
            lifecycle: ConfirmedLifecycle::Completed,
            boundary: ConfirmedBoundary::ConfirmedCompletion,
            effects: StoredEffect::from_effect(value.effects),
            predecessor: value.predecessor.map(Into::into),
            allocation: VariantWire {
                resource_id: a.resource_id.clone(),
                access_path: a.access_path.clone(),
                billing_domain_id: a.billing_domain_id.clone(),
                model_id: a.model_id.clone(),
                effort: a.effort.clone(),
            },
            source: value.provenance.source.clone(),
            runtime_id: value.provenance.runtime_id.clone(),
            completed_dependencies: value
                .context
                .completed_dependencies
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
        }
    }
}
impl CheckpointWire {
    pub(super) fn validated(
        self,
        policy: TaskPolicySnapshot,
    ) -> Result<CognitiveCheckpoint, CheckpointError> {
        let id = self.id.validated()?;
        let a = self.allocation;
        CognitiveCheckpoint::confirmed(
            id,
            ExecutionUnitFacts {
                id: id.unit_id(),
                state: ExecutionUnitState::Completed,
                effects: self.effects.effect(),
            },
            HandoffBoundary::ConfirmedCompletion { checkpoint: id },
            self.predecessor.map(ReceiptWire::validated).transpose()?,
            policy,
            AllocationVariant {
                resource_id: a.resource_id,
                access_path: a.access_path,
                billing_domain_id: a.billing_domain_id,
                model_id: a.model_id,
                effort: a.effort,
            },
            CheckpointProvenance {
                source: self.source,
                runtime_id: self.runtime_id,
            },
            HandoffContext::new(
                self.completed_dependencies
                    .into_iter()
                    .map(ReceiptWire::validated)
                    .collect::<Result<_, _>>()?,
            )?,
        )
    }
}
