use super::*;
use std::collections::{BTreeMap, HashMap};

const STATES: [ExecutionUnitState; 7] = [
    ExecutionUnitState::NotStarted,
    ExecutionUnitState::Running,
    ExecutionUnitState::PartialOutputObserved,
    ExecutionUnitState::Completed,
    ExecutionUnitState::Cancelled,
    ExecutionUnitState::Failed,
    ExecutionUnitState::Unknown,
];
const EFFECTS: [EffectState; 3] = [
    EffectState::NotStarted,
    EffectState::Committed,
    EffectState::UnknownOrInFlight,
];

fn unit(sequence: u64, state: ExecutionUnitState, effects: EffectState) -> ExecutionUnitFacts {
    ExecutionUnitFacts {
        id: ExecutionUnitId::new(17, sequence).unwrap(),
        state,
        effects,
    }
}
fn fresh() -> HandoffRequest {
    let requested_unit = unit(2, ExecutionUnitState::NotStarted, EffectState::NotStarted);
    HandoffRequest {
        previous_unit: None,
        requested_unit,
        boundary: HandoffBoundary::ConfirmedBeforeStart {
            unit_id: requested_unit.id,
        },
        cancellation_observed: false,
    }
}
fn successor(effects: EffectState) -> HandoffRequest {
    let previous_unit = unit(1, ExecutionUnitState::Completed, effects);
    HandoffRequest {
        previous_unit: Some(previous_unit),
        boundary: HandoffBoundary::ConfirmedCompletion {
            checkpoint: CheckpointId::new(previous_unit.id, 1).unwrap(),
        },
        ..fresh()
    }
}
fn blocked(request: &HandoffRequest, reason: HandoffReason) -> HandoffDecision {
    let decision = can_handoff(request);
    assert_eq!(decision.status(), HandoffStatus::Blocked);
    assert_eq!(decision.reason(), reason);
    decision
}

#[test]
fn c1_a_fresh_unit_at_confirmed_boundary_is_eligible() {
    let decision = can_handoff(&fresh());
    assert_eq!(decision.status(), HandoffStatus::Eligible);
    assert_eq!(
        decision.reason(),
        HandoffReason::FreshUnitAtConfirmedBoundary
    );
    assert_eq!(
        decision.requested_unit_replay(),
        ReplayDecision::NotApplicable
    );
    assert_eq!(decision.previous_unit_replay(), None);
}

#[test]
fn c1_b_completed_receipt_permits_successor_without_previous_replay() {
    let decision = can_handoff(&successor(EffectState::NotStarted));
    assert_eq!(decision.status(), HandoffStatus::Eligible);
    assert_eq!(
        decision.reason(),
        HandoffReason::SuccessorAtConfirmedCompletion
    );
    assert_eq!(
        decision.previous_unit_replay(),
        Some(ReplayDecision::Forbidden(ReplayReason::UnitCompleted))
    );
    assert_eq!(
        decision.requested_unit_replay(),
        ReplayDecision::NotApplicable
    );
}

#[test]
fn c1_c_running_unit_cannot_change_allocation() {
    let request = HandoffRequest {
        requested_unit: unit(2, ExecutionUnitState::Running, EffectState::NotStarted),
        ..fresh()
    };
    let decision = blocked(
        &request,
        HandoffReason::RequestedUnitNotFresh {
            state: ExecutionUnitState::Running,
        },
    );
    assert_eq!(
        decision.requested_unit_replay(),
        ReplayDecision::Forbidden(ReplayReason::UnitAlreadyStarted)
    );
}

#[test]
fn c1_d_partial_output_cannot_handoff_or_replay_same_unit() {
    let request = HandoffRequest {
        requested_unit: unit(
            2,
            ExecutionUnitState::PartialOutputObserved,
            EffectState::NotStarted,
        ),
        ..fresh()
    };
    let decision = blocked(
        &request,
        HandoffReason::RequestedUnitNotFresh {
            state: ExecutionUnitState::PartialOutputObserved,
        },
    );
    assert_eq!(
        decision.requested_unit_replay(),
        ReplayDecision::Forbidden(ReplayReason::PartialOutputObserved)
    );
}

#[test]
fn c1_e_committed_effect_forbids_replay_but_can_allow_distinct_successor() {
    let decision = can_handoff(&successor(EffectState::Committed));
    assert_eq!(decision.status(), HandoffStatus::Eligible);
    assert_eq!(
        decision.previous_unit_replay(),
        Some(ReplayDecision::Forbidden(ReplayReason::EffectCommitted))
    );
    for state in STATES {
        let request = HandoffRequest {
            requested_unit: unit(2, state, EffectState::Committed),
            ..fresh()
        };
        let decision = can_handoff(&request);
        assert_eq!(decision.status(), HandoffStatus::Blocked);
        assert_eq!(
            decision.requested_unit_replay(),
            ReplayDecision::Forbidden(ReplayReason::EffectCommitted)
        );
    }
}

#[test]
fn c1_f_unknown_effect_blocks_automatic_replay_and_continuation() {
    let decision = blocked(
        &successor(EffectState::UnknownOrInFlight),
        HandoffReason::PreviousEffectsUncertain,
    );
    assert_eq!(
        decision.previous_unit_replay(),
        Some(ReplayDecision::Forbidden(
            ReplayReason::EffectUnknownOrInFlight
        ))
    );
    for state in STATES {
        let request = HandoffRequest {
            requested_unit: unit(2, state, EffectState::UnknownOrInFlight),
            ..fresh()
        };
        let decision = can_handoff(&request);
        assert_eq!(decision.status(), HandoffStatus::Blocked);
        assert_eq!(
            decision.requested_unit_replay(),
            ReplayDecision::Forbidden(ReplayReason::EffectUnknownOrInFlight)
        );
    }
}

#[test]
fn c1_g_cancellation_before_dispatch_blocks_fresh_and_successor_units() {
    for mut request in [
        fresh(),
        successor(EffectState::NotStarted),
        successor(EffectState::Committed),
    ] {
        assert_eq!(can_handoff(&request).status(), HandoffStatus::Eligible);
        request.cancellation_observed = true;
        blocked(&request, HandoffReason::CancellationObserved);
    }
}

#[test]
fn c1_cancelled_lifecycle_blocks_even_without_cancellation_flag() {
    let request = HandoffRequest {
        requested_unit: unit(2, ExecutionUnitState::Cancelled, EffectState::NotStarted),
        ..fresh()
    };
    let decision = blocked(
        &request,
        HandoffReason::RequestedUnitNotFresh {
            state: ExecutionUnitState::Cancelled,
        },
    );
    assert_eq!(
        decision.requested_unit_replay(),
        ReplayDecision::Forbidden(ReplayReason::UnitCancelled)
    );
    let request = HandoffRequest {
        previous_unit: Some(unit(
            1,
            ExecutionUnitState::Cancelled,
            EffectState::NotStarted,
        )),
        ..successor(EffectState::NotStarted)
    };
    blocked(
        &request,
        HandoffReason::PreviousUnitNotCompleted {
            state: ExecutionUnitState::Cancelled,
        },
    );
}

#[test]
fn c1_h_not_started_with_started_effects_is_contradictory() {
    for effects in [EffectState::Committed, EffectState::UnknownOrInFlight] {
        let request = HandoffRequest {
            requested_unit: unit(2, ExecutionUnitState::NotStarted, effects),
            ..fresh()
        };
        blocked(&request, HandoffReason::ContradictoryUnitEffects);
        let request = HandoffRequest {
            previous_unit: Some(unit(1, ExecutionUnitState::NotStarted, effects)),
            ..successor(EffectState::NotStarted)
        };
        blocked(&request, HandoffReason::ContradictoryUnitEffects);
    }
}

#[test]
fn c1_started_terminal_or_uncertain_requested_units_are_never_fresh() {
    for state in STATES
        .into_iter()
        .filter(|state| *state != ExecutionUnitState::NotStarted)
    {
        for mut request in [fresh(), successor(EffectState::NotStarted)] {
            request.requested_unit.state = state;
            let decision = blocked(&request, HandoffReason::RequestedUnitNotFresh { state });
            assert!(matches!(
                decision.requested_unit_replay(),
                ReplayDecision::Forbidden(_)
            ));
        }
    }
}

#[test]
fn c1_incomplete_predecessor_cannot_be_laundered_as_new_unit() {
    for state in STATES
        .into_iter()
        .filter(|state| *state != ExecutionUnitState::Completed)
    {
        for effects in [EffectState::NotStarted, EffectState::Committed] {
            if state == ExecutionUnitState::NotStarted && effects == EffectState::Committed {
                continue;
            }
            let previous = unit(1, state, effects);
            for boundary in [fresh().boundary, successor(effects).boundary] {
                let request = HandoffRequest {
                    previous_unit: Some(previous),
                    boundary,
                    ..fresh()
                };
                blocked(&request, HandoffReason::PreviousUnitNotCompleted { state });
            }
        }
    }
}

#[test]
fn c1_same_or_older_identity_never_becomes_a_successor() {
    for sequence in [1, 2] {
        let request = HandoffRequest {
            previous_unit: Some(unit(
                2,
                ExecutionUnitState::Completed,
                EffectState::NotStarted,
            )),
            requested_unit: unit(
                sequence,
                ExecutionUnitState::NotStarted,
                EffectState::NotStarted,
            ),
            ..successor(EffectState::NotStarted)
        };
        blocked(&request, HandoffReason::UnitSequenceNotAdvancing);
    }
    let mut request = successor(EffectState::NotStarted);
    request.requested_unit.id = ExecutionUnitId::new(17, 19).unwrap();
    assert_eq!(
        can_handoff(&request).status(),
        HandoffStatus::Eligible,
        "sequence gaps are valid"
    );
}

#[test]
fn c1_cross_task_predecessor_is_rejected() {
    let mut request = successor(EffectState::NotStarted);
    request.requested_unit.id = ExecutionUnitId::new(18, 2).unwrap();
    blocked(&request, HandoffReason::TaskMismatch);
}

#[test]
fn c1_reused_identity_forbids_replay_in_both_conflicting_views() {
    for state in STATES {
        for source_effects in EFFECTS {
            for target_effects in EFFECTS {
                let request = HandoffRequest {
                    previous_unit: Some(unit(2, state, source_effects)),
                    requested_unit: unit(2, ExecutionUnitState::NotStarted, target_effects),
                    ..fresh()
                };
                let decision = can_handoff(&request);
                assert_eq!(decision.status(), HandoffStatus::Blocked);
                let effects = [source_effects, target_effects];
                let expected = ReplayDecision::Forbidden(
                    if effects.contains(&EffectState::UnknownOrInFlight) {
                        ReplayReason::EffectUnknownOrInFlight
                    } else if effects.contains(&EffectState::Committed) {
                        ReplayReason::EffectCommitted
                    } else {
                        ReplayReason::UnitIdentityReused
                    },
                );
                assert_eq!(decision.requested_unit_replay(), expected);
                assert_eq!(decision.previous_unit_replay(), Some(expected));
            }
        }
    }
}

#[test]
fn c1_before_start_boundary_must_identify_requested_unit_exactly() {
    for id in [
        ExecutionUnitId::new(17, 1).unwrap(),
        ExecutionUnitId::new(18, 2).unwrap(),
    ] {
        let request = HandoffRequest {
            boundary: HandoffBoundary::ConfirmedBeforeStart { unit_id: id },
            ..fresh()
        };
        blocked(&request, HandoffReason::BoundaryUnitMismatch);
    }
}

#[test]
fn c1_completion_receipt_must_identify_previous_unit_exactly() {
    for id in [
        ExecutionUnitId::new(17, 2).unwrap(),
        ExecutionUnitId::new(17, 3).unwrap(),
        ExecutionUnitId::new(18, 1).unwrap(),
    ] {
        let request = HandoffRequest {
            boundary: HandoffBoundary::ConfirmedCompletion {
                checkpoint: CheckpointId::new(id, 1).unwrap(),
            },
            ..successor(EffectState::NotStarted)
        };
        blocked(&request, HandoffReason::BoundaryUnitMismatch);
    }
}

#[test]
fn c1_boundary_kind_and_predecessor_presence_must_agree() {
    let request = HandoffRequest {
        boundary: fresh().boundary,
        ..successor(EffectState::NotStarted)
    };
    blocked(&request, HandoffReason::CompletionBoundaryRequired);
    let request = HandoffRequest {
        boundary: successor(EffectState::NotStarted).boundary,
        ..fresh()
    };
    blocked(&request, HandoffReason::PreviousUnitRequired);
}

#[test]
fn c1_completed_without_confirmed_boundary_does_not_allow_successor() {
    for (boundary, reason) in [
        (
            HandoffBoundary::Unconfirmed,
            HandoffReason::BoundaryUnconfirmed,
        ),
        (HandoffBoundary::Unknown, HandoffReason::BoundaryUnknown),
    ] {
        for mut request in [
            fresh(),
            successor(EffectState::NotStarted),
            successor(EffectState::Committed),
        ] {
            request.boundary = boundary;
            blocked(&request, reason);
        }
    }
}

#[test]
fn c1_unit_identity_and_checkpoint_sequences_are_bounded_monotonic() {
    for invalid in [0, MAX_HANDOFF_SEQUENCE + 1, u64::MAX] {
        assert_eq!(
            ExecutionUnitId::new(invalid, 1),
            Err(HandoffContractError::InvalidTaskId)
        );
        assert_eq!(
            ExecutionUnitId::new(1, invalid),
            Err(HandoffContractError::InvalidUnitSequence)
        );
        assert_eq!(
            CheckpointId::new(fresh().requested_unit.id, invalid),
            Err(HandoffContractError::InvalidCheckpointSequence)
        );
    }
    let first = ExecutionUnitId::new(1, 1).unwrap();
    let next = first.next().unwrap();
    assert_eq!(next.root_task_id(), first.root_task_id());
    assert_eq!(next.sequence(), 2);
    assert!(next > first);
    let receipt = CheckpointId::new(first, 1).unwrap();
    let next_receipt = receipt.next().unwrap();
    assert_eq!(next_receipt.unit_id(), first);
    assert_eq!(next_receipt.sequence(), 2);
    assert!(next_receipt > receipt);
    let maximum = ExecutionUnitId::new(MAX_HANDOFF_SEQUENCE, MAX_HANDOFF_SEQUENCE).unwrap();
    assert_eq!(
        maximum.next(),
        Err(HandoffContractError::InvalidUnitSequence)
    );
    assert_eq!(
        CheckpointId::new(maximum, MAX_HANDOFF_SEQUENCE)
            .unwrap()
            .next(),
        Err(HandoffContractError::InvalidCheckpointSequence)
    );
    let mut request = successor(EffectState::NotStarted);
    request.requested_unit.id = ExecutionUnitId::new(17, MAX_HANDOFF_SEQUENCE).unwrap();
    assert_eq!(can_handoff(&request).status(), HandoffStatus::Eligible);
}

#[test]
fn c1_i_equal_facts_give_equal_decisions_without_mutation() {
    for request in [
        fresh(),
        successor(EffectState::Committed),
        successor(EffectState::UnknownOrInFlight),
    ] {
        let before = request;
        let expected = can_handoff(&request);
        for _ in 0..100 {
            assert_eq!(can_handoff(&request), expected);
        }
        assert_eq!(request, before);
    }
}

#[test]
fn c1_i_auxiliary_hashmap_insertion_iteration_order_cannot_change_decisions() {
    let requests: Vec<_> = (2..40)
        .map(|sequence| {
            let mut request = successor(EffectState::Committed);
            request.requested_unit.id = ExecutionUnitId::new(17, sequence).unwrap();
            if sequence % 2 == 0 {
                request.requested_unit.state = ExecutionUnitState::Running;
            }
            request
        })
        .collect();
    let evaluate = |requests: &HashMap<ExecutionUnitId, HandoffRequest>| -> BTreeMap<_, _> {
        requests
            .iter()
            .map(|(id, request)| (*id, can_handoff(request)))
            .collect()
    };
    let forward: HashMap<_, _> = requests
        .iter()
        .map(|request| (request.requested_unit.id, *request))
        .collect();
    let reverse: HashMap<_, _> = requests
        .iter()
        .rev()
        .map(|request| (request.requested_unit.id, *request))
        .collect();
    assert_eq!(evaluate(&forward), evaluate(&reverse));
}

#[test]
fn c1_matrix_fails_closed_for_all_lifecycle_effect_and_boundary_combinations() {
    let predecessors: Vec<_> = std::iter::once(None)
        .chain(STATES.into_iter().flat_map(|state| {
            EFFECTS
                .into_iter()
                .map(move |effects| Some(unit(1, state, effects)))
        }))
        .collect();
    for previous in predecessors {
        for state in STATES {
            for effects in EFFECTS {
                for boundary in [
                    fresh().boundary,
                    successor(EffectState::NotStarted).boundary,
                    HandoffBoundary::Unconfirmed,
                    HandoffBoundary::Unknown,
                ] {
                    for cancellation in [false, true] {
                        let request = HandoffRequest {
                            previous_unit: previous,
                            requested_unit: unit(2, state, effects),
                            boundary,
                            cancellation_observed: cancellation,
                        };
                        let decision = can_handoff(&request);
                        let predecessor_safe = match previous {
                            None => {
                                matches!(boundary, HandoffBoundary::ConfirmedBeforeStart { .. })
                            }
                            Some(unit) => {
                                unit.state == ExecutionUnitState::Completed
                                    && unit.effects != EffectState::UnknownOrInFlight
                                    && matches!(
                                        boundary,
                                        HandoffBoundary::ConfirmedCompletion { .. }
                                    )
                            }
                        };
                        let eligible = !cancellation
                            && state == ExecutionUnitState::NotStarted
                            && effects == EffectState::NotStarted
                            && predecessor_safe;
                        assert_eq!(
                            decision.status() == HandoffStatus::Eligible,
                            eligible,
                            "{request:?}"
                        );
                        if state != ExecutionUnitState::NotStarted
                            || effects != EffectState::NotStarted
                        {
                            assert!(matches!(
                                decision.requested_unit_replay(),
                                ReplayDecision::Forbidden(_)
                            ));
                        }
                        if let Some(previous) = previous {
                            if previous.state != ExecutionUnitState::NotStarted
                                || previous.effects != EffectState::NotStarted
                            {
                                assert!(matches!(
                                    decision.previous_unit_replay(),
                                    Some(ReplayDecision::Forbidden(_))
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn c1_reason_precedence_is_explicit_under_multiple_blockers() {
    let mut request = successor(EffectState::UnknownOrInFlight);
    request.requested_unit.effects = EffectState::Committed;
    request.requested_unit.id = ExecutionUnitId::new(18, 1).unwrap();
    request.boundary = HandoffBoundary::Unknown;
    request.cancellation_observed = true;
    blocked(&request, HandoffReason::CancellationObserved);
    request.cancellation_observed = false;
    blocked(&request, HandoffReason::ContradictoryUnitEffects);
    request.requested_unit.effects = EffectState::NotStarted;
    blocked(&request, HandoffReason::TaskMismatch);
    request.requested_unit.id = ExecutionUnitId::new(17, 1).unwrap();
    blocked(&request, HandoffReason::UnitSequenceNotAdvancing);
    request.requested_unit.id = ExecutionUnitId::new(17, 2).unwrap();
    request.requested_unit.state = ExecutionUnitState::Running;
    blocked(
        &request,
        HandoffReason::RequestedUnitNotFresh {
            state: ExecutionUnitState::Running,
        },
    );
    request.requested_unit.state = ExecutionUnitState::NotStarted;
    blocked(&request, HandoffReason::PreviousEffectsUncertain);
    request.previous_unit = Some(unit(
        1,
        ExecutionUnitState::PartialOutputObserved,
        EffectState::NotStarted,
    ));
    blocked(
        &request,
        HandoffReason::PreviousUnitNotCompleted {
            state: ExecutionUnitState::PartialOutputObserved,
        },
    );
    request.previous_unit = Some(unit(
        1,
        ExecutionUnitState::Completed,
        EffectState::NotStarted,
    ));
    blocked(&request, HandoffReason::BoundaryUnknown);
}

#[test]
fn c1_j_decision_serializes_only_numeric_ids_and_enum_reasons() {
    assert_eq!(
        serde_json::to_value(can_handoff(&successor(EffectState::Committed))).unwrap(),
        serde_json::json!({
            "requestedUnitId": {"rootTaskId": 17, "sequence": 2},
            "previousUnitId": {"rootTaskId": 17, "sequence": 1},
            "status": "eligible",
            "reason": {"kind": "successor_at_confirmed_completion"},
            "requestedUnitReplay": {"kind": "not_applicable"},
            "previousUnitReplay": {"kind": "forbidden", "reason": "effect_committed"}
        })
    );
    assert_eq!(
        serde_json::to_value(can_handoff(&HandoffRequest {
            requested_unit: unit(
                2,
                ExecutionUnitState::PartialOutputObserved,
                EffectState::NotStarted
            ),
            ..fresh()
        }))
        .unwrap()["reason"],
        serde_json::json!({"kind": "requested_unit_not_fresh", "state": "partial_output_observed"})
    );
    let source = include_str!("handoff.rs");
    // The complete contract has no caller-controlled text or generic payload slot,
    // even in Debug/error diagnostics. Aggregates cannot bypass validation via serde.
    for forbidden in [
        "String",
        "&str",
        "serde_json",
        "Deserialize",
        "Vec<",
        "HashMap",
        "Box<",
        "dyn ",
    ] {
        assert!(
            !source.contains(forbidden),
            "unexpected payload surface: {forbidden}"
        );
    }
    for error in [
        HandoffContractError::InvalidTaskId,
        HandoffContractError::InvalidUnitSequence,
        HandoffContractError::InvalidCheckpointSequence,
    ] {
        assert_eq!(error.to_string(), format!("{error:?}"));
        assert!(!error.to_string().contains(&u64::MAX.to_string()));
    }
}

#[test]
fn c1_k_contract_has_no_commercial_or_runtime_authority_dependency() {
    let source = include_str!("handoff.rs").to_ascii_lowercase();
    for forbidden in [
        "groq",
        "cloudflare",
        "gemini",
        "codex",
        "copilot",
        "reqwest",
        "rusqlite",
        "tokio",
        "crate::",
        "super::",
        "resourceallocator",
        "paidusepolicy",
        "allocationvariant",
        "resourcecatalog",
        "std::time",
        "std::fs",
        "std::net",
    ] {
        assert!(
            !source.contains(forbidden),
            "unexpected authority/dependency: {forbidden}"
        );
    }
}
