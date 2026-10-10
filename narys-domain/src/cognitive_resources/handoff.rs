//! C1: pure boundary eligibility, never execution, allocation or Scheduler fallback.
//! Core supplies observed facts. Confirmation is an in-memory assertion here,
//! not proof of durable commit, freshness or permission to dispatch/spend.
use serde::Serialize;

/// Same exact-integer ceiling as local task identities; zero is never an identity.
pub const MAX_HANDOFF_SEQUENCE: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffContractError {
    InvalidTaskId,
    InvalidUnitSequence,
    InvalidCheckpointSequence,
}
impl std::fmt::Display for HandoffContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for HandoffContractError {}

/// Local numeric identity, not a provider ID or a textual TaskGraph step ID.
/// Sequences advance within a task; gaps allow independent parallel units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionUnitId {
    root_task_id: u64,
    sequence: u64,
}
impl ExecutionUnitId {
    pub fn new(root_task_id: u64, sequence: u64) -> Result<Self, HandoffContractError> {
        if !(1..=MAX_HANDOFF_SEQUENCE).contains(&root_task_id) {
            return Err(HandoffContractError::InvalidTaskId);
        }
        if !(1..=MAX_HANDOFF_SEQUENCE).contains(&sequence) {
            return Err(HandoffContractError::InvalidUnitSequence);
        }
        Ok(Self {
            root_task_id,
            sequence,
        })
    }
    pub fn root_task_id(self) -> u64 {
        self.root_task_id
    }
    pub fn sequence(self) -> u64 {
        self.sequence
    }
    pub fn next(self) -> Result<Self, HandoffContractError> {
        Self::new(self.root_task_id, self.sequence + 1)
    }
}

/// A minimal receipt identity bound to exactly one unit. Its sequence is local
/// to that unit. No state/context/persistence or historical ledger is implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointId {
    unit_id: ExecutionUnitId,
    sequence: u64,
}
impl CheckpointId {
    pub fn new(unit_id: ExecutionUnitId, sequence: u64) -> Result<Self, HandoffContractError> {
        if !(1..=MAX_HANDOFF_SEQUENCE).contains(&sequence) {
            return Err(HandoffContractError::InvalidCheckpointSequence);
        }
        Ok(Self { unit_id, sequence })
    }
    pub fn unit_id(self) -> ExecutionUnitId {
        self.unit_id
    }
    pub fn sequence(self) -> u64 {
        self.sequence
    }
    pub fn next(self) -> Result<Self, HandoffContractError> {
        Self::new(self.unit_id, self.sequence + 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionUnitState {
    NotStarted,
    Running,
    /// Sticky observation until terminal completion; request failure/EOF does
    /// not turn this into NotStarted or authorize replay.
    PartialOutputObserved,
    Completed,
    Cancelled,
    Failed,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectState {
    NotStarted,
    Committed,
    UnknownOrInFlight,
}

/// Aggregate fence for this unit's effects: any uncertain/in-flight effect
/// requires UnknownOrInFlight; otherwise any committed effect requires Committed.
/// NotStarted is valid only when no effect has started. No effect payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionUnitFacts {
    pub id: ExecutionUnitId,
    pub state: ExecutionUnitState,
    pub effects: EffectState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HandoffBoundary {
    /// Only for a fresh unit with no predecessor in this transition.
    ConfirmedBeforeStart {
        unit_id: ExecutionUnitId,
    },
    /// Core has confirmed completion and the receipt for the predecessor.
    ConfirmedCompletion {
        checkpoint: CheckpointId,
    },
    Unconfirmed,
    Unknown,
}

/// The requested unit is always the proposed new allocation recipient. Giving
/// it an already-started identity/state cannot authorize reallocation or replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffRequest {
    pub previous_unit: Option<ExecutionUnitFacts>,
    pub requested_unit: ExecutionUnitFacts,
    pub boundary: HandoffBoundary,
    pub cancellation_observed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffStatus {
    /// Boundary eligibility only. All allocation/authorization/admission gates
    /// and a fresh cancellation check still belong to the runtime authorities.
    Eligible,
    Blocked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandoffReason {
    FreshUnitAtConfirmedBoundary,
    SuccessorAtConfirmedCompletion,
    CancellationObserved,
    ContradictoryUnitEffects,
    TaskMismatch,
    UnitSequenceNotAdvancing,
    RequestedUnitNotFresh { state: ExecutionUnitState },
    PreviousEffectsUncertain,
    PreviousUnitNotCompleted { state: ExecutionUnitState },
    BoundaryUnconfirmed,
    BoundaryUnknown,
    CompletionBoundaryRequired,
    PreviousUnitRequired,
    BoundaryUnitMismatch,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayReason {
    EffectCommitted,
    EffectUnknownOrInFlight,
    UnitIdentityReused,
    UnitAlreadyStarted,
    PartialOutputObserved,
    UnitCompleted,
    UnitCancelled,
    UnitFailed,
    UnitStateUnknown,
}
/// This layer NEVER grants replay. NotApplicable denotes a unit that has never
/// started, not replay permission. Scheduler retry/fallback remains separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum ReplayDecision {
    NotApplicable,
    Forbidden(ReplayReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffDecision {
    requested_unit_id: ExecutionUnitId,
    previous_unit_id: Option<ExecutionUnitId>,
    status: HandoffStatus,
    reason: HandoffReason,
    requested_unit_replay: ReplayDecision,
    previous_unit_replay: Option<ReplayDecision>,
}
impl HandoffDecision {
    pub fn status(self) -> HandoffStatus {
        self.status
    }
    pub fn reason(self) -> HandoffReason {
        self.reason
    }
    pub fn requested_unit_replay(self) -> ReplayDecision {
        self.requested_unit_replay
    }
    pub fn previous_unit_replay(self) -> Option<ReplayDecision> {
        self.previous_unit_replay
    }
}

/// Pure, synchronous, clock-free and bounded. Precedence is cancellation,
/// contradictions, identity, requested lifecycle, predecessor effect/lifecycle,
/// then boundary linkage. No collection ordering or implicit external facts.
pub fn can_handoff(request: &HandoffRequest) -> HandoffDecision {
    let result = evaluate_boundary(request);
    let (status, reason) = match result {
        Ok(()) => (
            HandoffStatus::Eligible,
            if request.previous_unit.is_some() {
                HandoffReason::SuccessorAtConfirmedCompletion
            } else {
                HandoffReason::FreshUnitAtConfirmedBoundary
            },
        ),
        Err(reason) => (HandoffStatus::Blocked, reason),
    };
    let mut requested_unit_replay = replay_decision(request.requested_unit);
    let mut previous_unit_replay = request.previous_unit.map(replay_decision);
    if let Some(previous) = request.previous_unit {
        if previous.id == request.requested_unit.id {
            // Conflicting views cannot make the same identity fresh. Preserve
            // the strongest effect fence for both views of this single unit.
            let effects = [previous.effects, request.requested_unit.effects];
            let reason = if effects.contains(&EffectState::UnknownOrInFlight) {
                ReplayReason::EffectUnknownOrInFlight
            } else if effects.contains(&EffectState::Committed) {
                ReplayReason::EffectCommitted
            } else {
                ReplayReason::UnitIdentityReused
            };
            requested_unit_replay = ReplayDecision::Forbidden(reason);
            previous_unit_replay = Some(requested_unit_replay);
        }
    }
    HandoffDecision {
        requested_unit_id: request.requested_unit.id,
        previous_unit_id: request.previous_unit.map(|unit| unit.id),
        status,
        reason,
        requested_unit_replay,
        previous_unit_replay,
    }
}

fn evaluate_boundary(request: &HandoffRequest) -> Result<(), HandoffReason> {
    let target = request.requested_unit;
    let previous = request.previous_unit;
    if request.cancellation_observed {
        return Err(HandoffReason::CancellationObserved);
    }
    let contradictory = |unit: ExecutionUnitFacts| {
        unit.state == ExecutionUnitState::NotStarted && unit.effects != EffectState::NotStarted
    };
    if contradictory(target) || previous.is_some_and(contradictory) {
        return Err(HandoffReason::ContradictoryUnitEffects);
    }
    if let Some(previous) = previous {
        if previous.id.root_task_id != target.id.root_task_id {
            return Err(HandoffReason::TaskMismatch);
        }
        if target.id.sequence <= previous.id.sequence {
            return Err(HandoffReason::UnitSequenceNotAdvancing);
        }
    }
    if target.state != ExecutionUnitState::NotStarted {
        return Err(HandoffReason::RequestedUnitNotFresh {
            state: target.state,
        });
    }
    if let Some(previous) = previous {
        if previous.effects == EffectState::UnknownOrInFlight {
            return Err(HandoffReason::PreviousEffectsUncertain);
        }
        if previous.state != ExecutionUnitState::Completed {
            return Err(HandoffReason::PreviousUnitNotCompleted {
                state: previous.state,
            });
        }
    }
    match request.boundary {
        HandoffBoundary::Unconfirmed => Err(HandoffReason::BoundaryUnconfirmed),
        HandoffBoundary::Unknown => Err(HandoffReason::BoundaryUnknown),
        HandoffBoundary::ConfirmedBeforeStart { unit_id } => {
            if previous.is_some() {
                return Err(HandoffReason::CompletionBoundaryRequired);
            }
            if unit_id != target.id {
                return Err(HandoffReason::BoundaryUnitMismatch);
            }
            Ok(())
        }
        HandoffBoundary::ConfirmedCompletion { checkpoint } => {
            let previous = previous.ok_or(HandoffReason::PreviousUnitRequired)?;
            if checkpoint.unit_id != previous.id {
                return Err(HandoffReason::BoundaryUnitMismatch);
            }
            Ok(())
        }
    }
}

fn replay_decision(unit: ExecutionUnitFacts) -> ReplayDecision {
    // Effect fences must survive all lifecycle states, including contradictory
    // facts claiming NotStarted alongside an already-started/committed effect.
    match unit.effects {
        EffectState::Committed => return ReplayDecision::Forbidden(ReplayReason::EffectCommitted),
        EffectState::UnknownOrInFlight => {
            return ReplayDecision::Forbidden(ReplayReason::EffectUnknownOrInFlight)
        }
        EffectState::NotStarted => {}
    }
    match unit.state {
        ExecutionUnitState::NotStarted => ReplayDecision::NotApplicable,
        ExecutionUnitState::Running => ReplayDecision::Forbidden(ReplayReason::UnitAlreadyStarted),
        ExecutionUnitState::PartialOutputObserved => {
            ReplayDecision::Forbidden(ReplayReason::PartialOutputObserved)
        }
        ExecutionUnitState::Completed => ReplayDecision::Forbidden(ReplayReason::UnitCompleted),
        ExecutionUnitState::Cancelled => ReplayDecision::Forbidden(ReplayReason::UnitCancelled),
        ExecutionUnitState::Failed => ReplayDecision::Forbidden(ReplayReason::UnitFailed),
        ExecutionUnitState::Unknown => ReplayDecision::Forbidden(ReplayReason::UnitStateUnknown),
    }
}
