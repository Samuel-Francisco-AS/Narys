//! Generic descriptive economics. No admission, depletion classification, refill,
//! currency conversion or interpretation of model/effort names.
use super::*;
use crate::cognition::telemetry::{Timing, MAX_FACT_VALUE};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Local normalized ordinal scale, 0..=255. Larger means greater cognitive
/// capacity according to the explicit source's assessment. No model/effort is
/// assigned a tier by default, and this is not a quality-floor algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct CognitiveTier(u8);
impl CognitiveTier {
    pub fn new(value: u16) -> Result<Self, CatalogError> {
        u8::try_from(value)
            .map(Self)
            .map_err(|_| CatalogError::InvalidFact)
    }
    pub fn value(self) -> u8 {
        self.0
    }
}
impl TryFrom<u16> for CognitiveTier {
    type Error = CatalogError;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<CognitiveTier> for u16 {
    fn from(value: CognitiveTier) -> Self {
        value.0.into()
    }
}

/// Local normalized relative consumption scale, 0..=255, increasing in expense.
/// Known(0) is a relative index, never a statement of zero monetary/allowance cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct RelativeCostTier(u8);
impl RelativeCostTier {
    pub fn new(value: u16) -> Result<Self, CatalogError> {
        u8::try_from(value)
            .map(Self)
            .map_err(|_| CatalogError::InvalidFact)
    }
    pub fn value(self) -> u8 {
        self.0
    }
}
impl TryFrom<u16> for RelativeCostTier {
    type Error = CatalogError;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<RelativeCostTier> for u16 {
    fn from(value: RelativeCostTier) -> Self {
        value.0.into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum AllowanceUnit {
    Requests,
    Tokens,
    Credits,
    /// Explicit integer percentage points, 0..=100. Never derived from a ratio.
    Percent,
    Custom(AllowanceUnitId),
}
impl AllowanceUnit {
    fn validate_amount(&self, amount: u64) -> Result<(), CatalogError> {
        if amount > MAX_FACT_VALUE || (*self == Self::Percent && amount > 100) {
            Err(CatalogError::InvalidFact)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowanceConsumption {
    pub unit: AllowanceUnit,
    pub amount: u64,
}
impl AllowanceConsumption {
    fn validate(&self) -> Result<(), CatalogError> {
        self.unit.validate_amount(self.amount)
    }
}
/// Per-invocation facts for either a model or an effort. These are separate
/// assessments: no automatic inheritance, addition or override between profiles.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionFacts {
    pub cognitive_tier: CatalogFact<CognitiveTier>,
    pub relative_cost: CatalogFact<RelativeCostTier>,
    pub latency_ms: CatalogFact<u64>,
    pub monetary_cost: CatalogFact<MonetaryAmount>,
    pub allowance_cost: CatalogFact<AllowanceConsumption>,
}
impl ExecutionFacts {
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        self.cognitive_tier.validate()?;
        self.relative_cost.validate()?;
        validate_number(&self.latency_ms)?;
        self.monetary_cost.validate()?;
        self.allowance_cost.validate()?;
        if let Some(value) = self.allowance_cost.value() {
            value.validate()?;
        }
        Ok(())
    }
}

/// Economic observation usable by any resource class, independent of LR-8 scope.
/// Unknown fields are independent. Timing is historical evidence, not a refill
/// instruction or permission to invoke. Credits carry no monetary interpretation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowanceState {
    pub unit: AllowanceUnit,
    pub limit: CatalogFact<u64>,
    pub remaining: CatalogFact<u64>,
    pub reset: CatalogFact<Timing>,
}
impl AllowanceState {
    pub fn unknown(unit: AllowanceUnit) -> Self {
        Self {
            unit,
            limit: CatalogFact::Unknown,
            remaining: CatalogFact::Unknown,
            reset: CatalogFact::Unknown,
        }
    }
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        for fact in [&self.limit, &self.remaining] {
            validate_number(fact)?;
            if let Some(amount) = fact.value() {
                self.unit.validate_amount(*amount)?;
            }
        }
        if self
            .limit
            .value()
            .zip(self.remaining.value())
            .is_some_and(|(limit, remaining)| remaining > limit)
        {
            return Err(CatalogError::InvalidFact);
        }
        self.reset.validate()?;
        if self.reset.value().is_some_and(|timing| match timing {
            Timing::DelayMs(n) | Timing::UnixMs(n) => *n > MAX_FACT_VALUE,
        }) {
            return Err(CatalogError::InvalidFact);
        }
        Ok(())
    }
}
pub const MAX_ALLOWANCES: usize = 16;
/// Resource-associated observations about a stable billing domain. Collection
/// absence means unreported dimensions, not zero/free/unlimited allowance.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EconomicFacts {
    pub billing_kind: CatalogFact<BillingKind>,
    pub monetary_balance: CatalogFact<MonetaryAmount>,
    /// One observation per explicit unit. Multiple units are not convertible.
    pub allowances: Vec<AllowanceState>,
}
impl EconomicFacts {
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        self.billing_kind.validate()?;
        self.monetary_balance.validate()?;
        if self.allowances.len() > MAX_ALLOWANCES {
            return Err(CatalogError::CapacityExceeded);
        }
        let mut units = BTreeSet::new();
        for allowance in &self.allowances {
            if !units.insert(&allowance.unit) {
                return Err(CatalogError::DuplicateAllowanceUnit);
            }
            allowance.validate()?;
        }
        Ok(())
    }
}
pub(super) fn validate_number(fact: &CatalogFact<u64>) -> Result<(), CatalogError> {
    fact.validate()?;
    if fact.value().is_some_and(|n| *n > MAX_FACT_VALUE) {
        Err(CatalogError::InvalidFact)
    } else {
        Ok(())
    }
}
