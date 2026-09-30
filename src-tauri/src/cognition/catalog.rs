use super::{
    policy::{CognitiveRolePolicy, ThinkingLevel},
    scheduler::ProviderStatus,
    types::ProviderCapabilities,
};
use crate::security::secrets::{SecretKey, SecretStore};
use serde::Serialize;

pub struct Integration {
    pub id: &'static str,
    pub display_name: &'static str,
    pub default_model: Option<&'static str>,
    pub secret: SecretKey,
    pub thinking: &'static [ThinkingLevel],
}

const LEVELS: &[ThinkingLevel] = &[
    ThinkingLevel::Low,
    ThinkingLevel::Medium,
    ThinkingLevel::High,
];
pub const INTEGRATIONS: &[Integration] = &[
    Integration {
        id: "gemini",
        display_name: "Gemini",
        default_model: Some(super::gemini::MODEL),
        secret: SecretKey::GeminiApiKey,
        thinking: LEVELS,
    },
    Integration {
        id: "groq",
        display_name: "Groq",
        default_model: Some(super::groq::MODEL),
        secret: SecretKey::GroqApiKey,
        thinking: LEVELS,
    },
];

pub fn integration(id: &str) -> Option<&'static Integration> {
    INTEGRATIONS.iter().find(|item| item.id == id)
}

pub fn configured(store: &SecretStore, id: &str) -> bool {
    integration(id).is_some_and(|item| store.get_secret(item.secret).ok().flatten().is_some())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub configured: bool,
    pub enabled: bool,
    pub capabilities: ProviderCapabilities,
    pub supported_thinking_levels: Vec<&'static str>,
    pub default_model: Option<&'static str>,
}

pub fn infos(statuses: &[ProviderStatus], store: &SecretStore) -> Vec<ProviderInfo> {
    statuses
        .iter()
        .filter_map(|status| {
            integration(&status.id).map(|item| ProviderInfo {
                id: item.id,
                display_name: item.display_name,
                configured: configured(store, item.id),
                enabled: status.enabled,
                capabilities: status.capabilities,
                supported_thinking_levels: item
                    .thinking
                    .iter()
                    .map(|level| level.as_str())
                    .collect(),
                default_model: item.default_model,
            })
        })
        .collect()
}

pub fn validate_target(
    id: &str,
    thinking: Option<ThinkingLevel>,
    statuses: &[ProviderStatus],
    store: &SecretStore,
) -> Result<(), &'static str> {
    validate_registered(id, thinking, statuses)?;
    if !configured(store, id) {
        return Err("provider_not_configured");
    }
    Ok(())
}

pub fn validate_registered(
    id: &str,
    thinking: Option<ThinkingLevel>,
    statuses: &[ProviderStatus],
) -> Result<(), &'static str> {
    let item = integration(id).ok_or("provider_unavailable")?;
    statuses
        .iter()
        .find(|status| {
            status.id == id
                && status.enabled
                && status
                    .capabilities
                    .supports(&ProviderCapabilities::text_stream())
        })
        .ok_or("provider_unavailable")?;
    if thinking.is_some_and(|level| !item.thinking.contains(&level)) {
        return Err("thinking_unavailable");
    }
    Ok(())
}

pub fn validate_policy(
    policy: &CognitiveRolePolicy,
    statuses: &[ProviderStatus],
    store: &SecretStore,
) -> Result<(), &'static str> {
    validate_policy_registered(policy, statuses)?;
    validate_target(&policy.provider_id, policy.thinking_level, statuses, store)?;
    if let Some(id) = policy.fallback_provider_id.as_deref() {
        // Fixed policies may retain a dormant fallback from an earlier route.
        if policy.routing_mode == super::policy::RoutingMode::Preferred {
            validate_target(id, policy.fallback_thinking_level, statuses, store)?;
        }
    }
    Ok(())
}

pub fn validate_policy_registered(
    policy: &CognitiveRolePolicy,
    statuses: &[ProviderStatus],
) -> Result<(), &'static str> {
    policy.validate()?;
    validate_registered(&policy.provider_id, policy.thinking_level, statuses)?;
    if policy.routing_mode == super::policy::RoutingMode::Preferred {
        let id = policy
            .fallback_provider_id
            .as_deref()
            .ok_or("fallback_config_invalid")?;
        validate_registered(id, policy.fallback_thinking_level, statuses)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_requires_registered_enabled_stream_provider_and_exposes_no_secret() {
        let status = ProviderStatus {
            id: "groq".into(),
            enabled: true,
            priority: 2,
            capabilities: ProviderCapabilities::text_stream(),
            cooldown_ms: 0,
        };
        assert_eq!(
            validate_registered("groq", Some(ThinkingLevel::High), &[status.clone()]),
            Ok(())
        );
        assert_eq!(
            validate_registered("unknown", None, &[status.clone()]),
            Err("provider_unavailable")
        );
        assert_eq!(
            validate_registered("gemini", None, &[status.clone()]),
            Err("provider_unavailable")
        );
        let disabled = ProviderStatus {
            enabled: false,
            ..status
        };
        assert_eq!(
            validate_registered("groq", None, &[disabled]),
            Err("provider_unavailable")
        );
        let info = ProviderInfo {
            id: "groq",
            display_name: "Groq",
            configured: true,
            enabled: true,
            capabilities: ProviderCapabilities::text_stream(),
            supported_thinking_levels: vec!["low"],
            default_model: Some(super::super::groq::MODEL),
        };
        let json = serde_json::to_string(&info).unwrap();
        for forbidden in ["apiKey", "token", "bearer", "stronghold", "unlock"] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn catalog_exposes_gemini_integration_default_model() {
        let gemini = integration("gemini").unwrap();
        assert_eq!(gemini.default_model, Some(super::super::gemini::MODEL));
        assert!(!gemini.default_model.unwrap().is_empty());
    }
}
