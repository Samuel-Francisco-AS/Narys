use super::CatalogError;
use crate::cognition::telemetry::{Fact, Provenance, MAX_FACT_VALUE};
use serde::Serialize;

/// Only the two catalog origins missing from LR-8 are added here. Operational
/// provenance retains the original LR-8 enum; no arbitrary source strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "source", rename_all = "snake_case")]
pub enum CatalogProvenance {
    IntegrationCatalog,
    RuntimeContract,
    Operational(Provenance),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CatalogFact<T> {
    Unknown,
    Known {
        value: T,
        provenance: CatalogProvenance,
        observed_at_unix_ms: Option<u64>,
    },
}
impl<T> Default for CatalogFact<T> {
    fn default() -> Self {
        Self::Unknown
    }
}
impl<T> CatalogFact<T> {
    pub fn known(
        value: T,
        provenance: CatalogProvenance,
        observed_at_unix_ms: Option<u64>,
    ) -> Result<Self, CatalogError> {
        if observed_at_unix_ms.is_some_and(|n| n > MAX_FACT_VALUE) {
            return Err(CatalogError::InvalidFact);
        }
        Ok(Self::Known {
            value,
            provenance,
            observed_at_unix_ms,
        })
    }
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Unknown => None,
            Self::Known { value, .. } => Some(value),
        }
    }
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        if matches!(self, Self::Known { observed_at_unix_ms: Some(n), .. } if *n > MAX_FACT_VALUE) {
            Err(CatalogError::InvalidFact)
        } else {
            Ok(())
        }
    }
}
impl<T: Clone> From<&Fact<T>> for CatalogFact<T> {
    fn from(fact: &Fact<T>) -> Self {
        match fact {
            Fact::Unknown => Self::Unknown,
            Fact::Known {
                value,
                provenance,
                observed_at_unix_ms,
            } => Self::Known {
                value: value.clone(),
                provenance: CatalogProvenance::Operational(*provenance),
                observed_at_unix_ms: *observed_at_unix_ms,
            },
        }
    }
}
