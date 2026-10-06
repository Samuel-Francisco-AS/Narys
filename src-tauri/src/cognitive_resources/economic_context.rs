//! Identity-bearing, read-only B2 capture. Detached Lr8Facts cannot be injected:
//! their original provider identity was removed by the A projection. Capture from
//! identity-bearing DTOs, then compare the complete descriptor against B1.
use super::*;
use crate::cognition::{
    rate::{ConstraintSource, RateSnapshot},
    telemetry::{
        Fact, ProviderTelemetrySnapshot, QuotaScope, QuotaSnapshot, Timing, MAX_FACT_VALUE,
    },
};
use std::collections::BTreeSet;

/// Five dimensions, three sources, provider + bounded described model scopes.
/// Memory bound only, never a commercial rate or quota.
pub const MAX_B2_RATE_CONSTRAINTS: usize = (MAX_MODELS + 1) * 5 * 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoringError {
    TooManyCandidates,
    DuplicateCandidate,
    CandidateNotInB1Universe,
    B1Unresolved,
    B1Ineligible,
    InvalidSignals,
    TooManyContexts,
    DuplicateEconomicContext,
    UnexpectedEconomicContext,
    ConflictingResourceSnapshot,
    ConflictingBillingDomainFacts,
    Lr8ResourceMismatch,
    InvalidLr8Evidence,
    Lr8ContextMismatch,
}
impl std::fmt::Display for ScoringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ScoringError {}

#[derive(Clone, Debug)]
pub struct EconomicContext {
    descriptor: CognitiveResource,
    lr8: Option<Lr8Facts>,
}
impl EconomicContext {
    /// No manager read, clock, refresh or mutation. Both DTOs must identify the
    /// exact Provider runtime of this descriptor. None remains unreported.
    pub fn capture(
        descriptor: &CognitiveResource,
        telemetry: Option<&ProviderTelemetrySnapshot>,
        rate: Option<&RateSnapshot>,
    ) -> Result<Self, ScoringError> {
        descriptor
            .validate()
            .map_err(|_| ScoringError::ConflictingResourceSnapshot)?;
        if telemetry.is_some() || rate.is_some() {
            let ResourceOrigin::Provider(id) = &descriptor.origin else {
                return Err(ScoringError::Lr8ResourceMismatch);
            };
            if telemetry.is_some_and(|t| t.provider_id != id.as_str())
                || rate.is_some_and(|r| r.provider_id != id.as_str())
            {
                return Err(ScoringError::Lr8ResourceMismatch);
            }
        }
        if let Some(t) = telemetry {
            validate_telemetry(t)?;
        }
        if let Some(r) = rate {
            validate_rate(r)?;
        }
        let lr8 = super::lr8::project_lr8(
            descriptor,
            &telemetry.cloned().into_iter().collect::<Vec<_>>(),
            &rate.cloned().into_iter().collect::<Vec<_>>(),
        )
        .map_err(|_| ScoringError::Lr8ContextMismatch)?;
        Ok(Self {
            descriptor: descriptor.clone(),
            lr8,
        })
    }
    pub fn descriptor(&self) -> &CognitiveResource {
        &self.descriptor
    }
    pub fn lr8(&self) -> Option<&Lr8Facts> {
        self.lr8.as_ref()
    }
}
fn numeric(values: impl IntoIterator<Item = u64>) -> Result<(), ScoringError> {
    if values.into_iter().any(|v| v > MAX_FACT_VALUE) {
        Err(ScoringError::InvalidLr8Evidence)
    } else {
        Ok(())
    }
}
fn validate_fact<T>(fact: &Fact<T>) -> Result<(), ScoringError> {
    if let Fact::Known {
        observed_at_unix_ms,
        ..
    } = fact
    {
        numeric(*observed_at_unix_ms)?;
    }
    Ok(())
}
fn validate_quota(q: &QuotaSnapshot) -> Result<(), ScoringError> {
    for f in [&q.limit, &q.remaining] {
        validate_fact(f)?;
        if let Fact::Known { value, .. } = f {
            numeric([*value])?;
        }
    }
    validate_fact(&q.reset)?;
    if let Fact::Known {
        value: Timing::DelayMs(v) | Timing::UnixMs(v),
        ..
    } = &q.reset
    {
        numeric([*v])?;
    }
    if matches!((&q.limit, &q.remaining), (Fact::Known {value:l,..}, Fact::Known {value:r,..}) if r > l)
    {
        return Err(ScoringError::InvalidLr8Evidence);
    }
    Ok(())
}
fn scope_key(scope: &QuotaScope) -> Result<Option<ModelId>, ScoringError> {
    match scope {
        QuotaScope::Provider => Ok(None),
        QuotaScope::Model { model } => ModelId::new(model)
            .map(Some)
            .map_err(|_| ScoringError::InvalidLr8Evidence),
    }
}
fn validate_telemetry(t: &ProviderTelemetrySnapshot) -> Result<(), ScoringError> {
    numeric([t.context_generation])?;
    numeric(t.captured_at_unix_ms)?;
    if t.quotas.len() > MAX_MODELS + 1 {
        return Err(ScoringError::InvalidLr8Evidence);
    }
    let mut scopes = BTreeSet::new();
    for scoped in &t.quotas {
        if !scopes.insert(scope_key(&scoped.scope)?) {
            return Err(ScoringError::InvalidLr8Evidence);
        }
        for q in scoped.dimensions.values() {
            validate_quota(q)?;
        }
    }
    // Retry hints and usage are not decision inputs. They cannot authorize spend.
    Ok(())
}
fn validate_rate(r: &RateSnapshot) -> Result<(), ScoringError> {
    numeric([r.context_generation, r.local_blocks])?;
    numeric(r.captured_at_unix_ms)?;
    if r.constraints.len() > MAX_B2_RATE_CONSTRAINTS {
        return Err(ScoringError::InvalidLr8Evidence);
    }
    let mut keys = BTreeSet::new();
    for c in &r.constraints {
        let source = match c.source {
            ConstraintSource::ExternalFact => 0,
            ConstraintSource::LocalPolicy => 1,
            ConstraintSource::DailyBudget => 2,
        };
        if !keys.insert((scope_key(&c.scope)?, c.dimension, source)) {
            return Err(ScoringError::InvalidLr8Evidence);
        }
        numeric([c.consumed, c.reserved, c.unaccounted_token_calls])?;
        for n in [
            c.capacity,
            c.effective_remaining,
            c.reset_unix_ms,
            c.reset_in_ms,
        ] {
            numeric(n)?;
        }
        if c.effective_remaining
            .zip(c.capacity)
            .is_some_and(|(remaining, cap)| remaining > cap)
        {
            return Err(ScoringError::InvalidLr8Evidence);
        }
        if let Some(q) = &c.external {
            validate_quota(q)?;
        }
    }
    Ok(())
}
