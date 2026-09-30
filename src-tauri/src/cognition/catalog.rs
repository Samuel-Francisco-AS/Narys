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
    for target in &policy.targets {
        validate_target(&target.provider_id, target.thinking_level, statuses, store)?;
    }
    Ok(())
}

pub fn validate_policy_registered(
    policy: &CognitiveRolePolicy,
    statuses: &[ProviderStatus],
) -> Result<(), &'static str> {
    policy.validate()?;
    for target in &policy.targets {
        validate_registered(&target.provider_id, target.thinking_level, statuses)?;
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

#[cfg(test)]
mod route_tests {
    use super::*;
    use crate::cognition::policy::{CognitiveRole, CognitiveTargetPolicy, RoutingMode};
    use crate::persistence::database::Database;
    use crate::security::secrets::{SecretError, UnlockKeyStore};
    use std::sync::Mutex;
    #[derive(Default)]
    struct Keys(Mutex<Option<Vec<u8>>>);
    impl UnlockKeyStore for Keys {
        fn load(&self) -> Result<Option<Vec<u8>>, SecretError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn store(&self, key: &[u8]) -> Result<(), SecretError> {
            *self.0.lock().unwrap() = Some(key.to_vec());
            Ok(())
        }
        fn delete(&self) -> Result<(), SecretError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }
    #[test]
    fn validates_every_target_credentials_and_ignores_cooldown_for_saving() {
        let dir = std::env::temp_dir().join(format!(
            "d2-catalog-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let db = Database::for_test(dir.join("policy.sqlite3"));
        let mut conn = db.open().unwrap();
        let mut policy =
            crate::cognition::policy::load(&conn, CognitiveRole::Conversation).unwrap();
        policy.routing_mode = RoutingMode::Auto;
        policy.targets.push(CognitiveTargetPolicy {
            provider_id: "groq".into(),
            model: "groq-own".into(),
            thinking_level: None,
        });
        let mut statuses: Vec<_> = ["gemini", "groq"]
            .into_iter()
            .map(|id| ProviderStatus {
                id: id.into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::text_stream(),
                cooldown_ms: 60000,
            })
            .collect();
        let store = SecretStore::with_key_store(dir.clone(), std::sync::Arc::new(Keys::default()));
        store
            .set_secret(SecretKey::GeminiApiKey, b"synthetic")
            .unwrap();
        assert_eq!(
            validate_policy(&policy, &statuses, &store),
            Err("provider_not_configured")
        );
        store
            .set_secret(SecretKey::GroqApiKey, b"synthetic")
            .unwrap();
        assert_eq!(validate_policy(&policy, &statuses, &store), Ok(()));
        crate::cognition::policy::save(&mut conn, &policy).unwrap();
        statuses[1].enabled = false;
        assert_eq!(
            validate_policy_registered(&policy, &statuses),
            Err("provider_unavailable")
        );
        statuses[1].enabled = true;
        statuses[1].capabilities = ProviderCapabilities::default();
        assert_eq!(
            validate_policy_registered(&policy, &statuses),
            Err("provider_unavailable")
        );
        policy.targets[1].provider_id = "unknown".into();
        assert_eq!(
            validate_policy_registered(&policy, &statuses),
            Err("provider_unavailable")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
