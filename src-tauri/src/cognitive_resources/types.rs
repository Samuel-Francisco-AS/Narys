use super::*;
use crate::{
    agents::types::AgentConfig,
    cognition::{policy::ThinkingLevel, telemetry::MAX_FACT_VALUE, types::ProviderConfig},
};
use serde::Serialize;
use std::collections::BTreeSet;

pub const MAX_RESOURCES: usize = 256;
pub const MAX_MODELS: usize = 128;
pub const MAX_EFFORTS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceClass {
    CognitiveProvider,
    SpecialistAgent,
    LocalSupport,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingKind {
    IncludedAllowance,
    FreeTier,
    PrepaidCredits,
    MeteredBilling,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingDomain {
    /// Public local label, independent of family and remote billing/account IDs.
    pub id: BillingDomainId,
    pub kind: CatalogFact<BillingKind>,
    pub balance: CatalogFact<MonetaryAmount>,
}
/// Amount in millionths of an explicitly supplied three-letter currency code.
/// No exchange rate, billing schedule or price inference exists in the Core.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonetaryAmount {
    currency: String,
    micros: u64,
}
impl MonetaryAmount {
    pub fn new(currency: &str, micros: u64) -> Result<Self, CatalogError> {
        if currency.len() != 3
            || !currency.bytes().all(|b| b.is_ascii_uppercase())
            || micros > MAX_FACT_VALUE
        {
            return Err(CatalogError::InvalidFact);
        }
        Ok(Self {
            currency: currency.into(),
            micros,
        })
    }
    pub fn currency(&self) -> &str {
        &self.currency
    }
    pub fn micros(&self) -> u64 {
        self.micros
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFacts {
    /// A supplied label, with no built-in quality ordering.
    pub quality: CatalogFact<QualityLabel>,
    pub context_tokens: CatalogFact<u64>,
    pub latency_ms: CatalogFact<u64>,
    /// Cost per invocation as explicitly described by the source.
    pub monetary_cost: CatalogFact<MonetaryAmount>,
    /// Consumption per invocation in the integration's declared allowance units.
    pub quota_cost: CatalogFact<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffortProfile {
    pub id: EffortId,
    pub availability: CatalogFact<Availability>,
}
impl EffortId {
    /// Exact vocabulary bridge only; this makes no claim of model/backend support.
    pub fn from_thinking_level(level: ThinkingLevel) -> Self {
        Self::new(level.as_str()).expect("bounded legacy vocabulary")
    }
    pub fn try_thinking_level(&self) -> Result<ThinkingLevel, CatalogError> {
        match self.as_str() {
            "low" => Ok(ThinkingLevel::Low),
            "medium" => Ok(ThinkingLevel::Medium),
            "high" => Ok(ThinkingLevel::High),
            _ => Err(CatalogError::LegacyEffortNotRepresentable),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfile {
    pub id: ModelId,
    pub availability: CatalogFact<Availability>,
    pub capabilities: CapabilitySet,
    /// Unknown differs from a known empty list (explicitly no selectable efforts).
    pub supported_efforts: CatalogFact<Vec<EffortProfile>>,
    pub facts: ModelFacts,
}
impl ModelProfile {
    /// A locally configured model name alone proves none of its capabilities,
    /// availability or effort support. Integration authors fill factual metadata.
    pub fn unknown(id: ModelId) -> Self {
        Self {
            id,
            availability: CatalogFact::Unknown,
            capabilities: CapabilitySet::default(),
            supported_efforts: CatalogFact::Unknown,
            facts: ModelFacts::default(),
        }
    }
    pub fn effort(&self, id: &EffortId) -> Result<&EffortProfile, CatalogError> {
        self.supported_efforts
            .value()
            .ok_or(CatalogError::EffortSupportUnknown)?
            .iter()
            .find(|effort| &effort.id == id)
            .ok_or(CatalogError::EffortNotSupported)
    }
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        self.availability.validate()?;
        self.capabilities.validate()?;
        self.supported_efforts.validate()?;
        if let Some(efforts) = self.supported_efforts.value() {
            if efforts.len() > MAX_EFFORTS {
                return Err(CatalogError::CapacityExceeded);
            }
            let mut ids = BTreeSet::new();
            for effort in efforts {
                if !ids.insert(&effort.id) {
                    return Err(CatalogError::DuplicateEffort);
                }
                effort.availability.validate()?;
            }
        }
        self.facts.quality.validate()?;
        self.facts.monetary_cost.validate()?;
        for fact in [
            &self.facts.context_tokens,
            &self.facts.latency_ms,
            &self.facts.quota_cost,
        ] {
            fact.validate()?;
            if fact.value().is_some_and(|n| *n > MAX_FACT_VALUE) {
                return Err(CatalogError::InvalidFact);
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "runtimeId", rename_all = "snake_case")]
pub enum ResourceOrigin {
    Provider(RuntimeId),
    Agent(RuntimeId),
    Local,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceIdentity {
    pub id: ResourceId,
    pub class: ResourceClass,
    pub family: ProviderFamily,
    pub access_path: AccessPath,
    pub billing_domain: BillingDomain,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveResource {
    pub identity: ResourceIdentity,
    pub origin: ResourceOrigin,
    pub enabled: CatalogFact<bool>,
    /// Descriptive availability; never an LR-8 operational authorization.
    pub availability: CatalogFact<Availability>,
    /// Broad runtime contract; no automatic inheritance into model capabilities.
    pub capabilities: CapabilitySet,
    pub models: CatalogFact<Vec<ModelProfile>>,
}
impl CognitiveResource {
    pub fn from_provider_config(
        identity: ResourceIdentity,
        config: &ProviderConfig,
    ) -> Result<Self, CatalogError> {
        let resource = Self::from_config(
            identity,
            ResourceOrigin::Provider(RuntimeId::new(&config.id)?),
            config.enabled,
            CapabilitySet::from_provider(config.capabilities),
        );
        resource.validate()?;
        Ok(resource)
    }
    pub fn from_agent_config(
        identity: ResourceIdentity,
        config: &AgentConfig,
    ) -> Result<Self, CatalogError> {
        let resource = Self::from_config(
            identity,
            ResourceOrigin::Agent(RuntimeId::new(&config.id)?),
            config.enabled,
            CapabilitySet::from_agent(config.capabilities),
        );
        resource.validate()?;
        Ok(resource)
    }
    fn from_config(
        identity: ResourceIdentity,
        origin: ResourceOrigin,
        enabled: bool,
        capabilities: CapabilitySet,
    ) -> Self {
        // Registry configuration can come from Core defaults or user settings.
        // Reading it proves the registered contract, not who configured it.
        let provenance = CatalogProvenance::RuntimeContract;
        Self {
            identity,
            origin,
            enabled: CatalogFact::Known {
                value: enabled,
                provenance,
                observed_at_unix_ms: None,
            },
            availability: if enabled {
                CatalogFact::Unknown
            } else {
                CatalogFact::Known {
                    value: Availability::Unavailable,
                    provenance,
                    observed_at_unix_ms: None,
                }
            },
            capabilities,
            models: CatalogFact::Unknown,
        }
    }
    pub fn model(&self, id: &ModelId) -> Result<&ModelProfile, CatalogError> {
        self.models
            .value()
            .ok_or(CatalogError::ModelCatalogUnknown)?
            .iter()
            .find(|model| &model.id == id)
            .ok_or(CatalogError::ModelNotFound)
    }
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        if !matches!(
            (&self.origin, self.identity.class),
            (
                ResourceOrigin::Provider(_),
                ResourceClass::CognitiveProvider
            ) | (ResourceOrigin::Agent(_), ResourceClass::SpecialistAgent)
                | (ResourceOrigin::Local, ResourceClass::LocalSupport)
        ) {
            return Err(CatalogError::InvalidOrigin);
        }
        self.identity.billing_domain.kind.validate()?;
        self.identity.billing_domain.balance.validate()?;
        self.enabled.validate()?;
        self.availability.validate()?;
        self.capabilities.validate()?;
        self.models.validate()?;
        if let Some(models) = self.models.value() {
            if models.len() > MAX_MODELS {
                return Err(CatalogError::CapacityExceeded);
            }
            let mut ids = BTreeSet::new();
            for model in models {
                if !ids.insert(&model.id) {
                    return Err(CatalogError::DuplicateModel);
                }
                model.validate()?;
            }
        }
        Ok(())
    }
}
/// An explicitly requested description, including unavailable/unknown facts.
/// It is neither a ranked candidate nor permission to invoke a backend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionVariant {
    pub resource_id: ResourceId,
    pub access_path: AccessPath,
    pub billing_domain_id: BillingDomainId,
    pub model_id: ModelId,
    pub effort: Option<EffortId>,
    pub resource_availability: CatalogFact<Availability>,
    pub model_availability: CatalogFact<Availability>,
    pub effort_availability: CatalogFact<Availability>,
}
