use super::{
    policy::{CognitiveRolePolicy, ThinkingLevel},
    scheduler::ProviderStatus,
    types::ProviderCapabilities,
};
use crate::security::secrets::{SecretError, SecretKey, SecretStore};
use serde::Serialize;
use std::collections::HashMap;

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

/// Runtime snapshot of presence only; callers must not reuse it as task authorization.
pub fn configured_many(
    store: &SecretStore,
    ids: &[&str],
) -> Result<HashMap<String, bool>, SecretError> {
    let keys: Vec<_> = ids
        .iter()
        .filter_map(|id| integration(id).map(|item| item.secret))
        .collect();
    let presence = store.secret_presence(&keys).inspect_err(|error| {
        #[cfg(debug_assertions)]
        eprintln!(
            "[Catalog][diag] credential_presence_failed code={}",
            error.code()
        );
        #[cfg(not(debug_assertions))]
        let _ = error;
    })?;
    Ok(ids
        .iter()
        .map(|id| {
            (
                (*id).to_owned(),
                integration(id).is_some_and(|item| presence.get(&item.secret) == Some(&true)),
            )
        })
        .collect())
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

pub struct CatalogInfos {
    pub providers: Vec<ProviderInfo>,
    pub credential_store_available: bool,
}

pub fn infos(statuses: &[ProviderStatus], store: &SecretStore) -> CatalogInfos {
    // Settings availability and provider states share one vault snapshot.
    let ids: Vec<_> = INTEGRATIONS.iter().map(|item| item.id).collect();
    let presence = configured_many(store, &ids);
    let credential_store_available = presence.is_ok();
    let configured = presence.unwrap_or_default();
    let providers = statuses
        .iter()
        .filter_map(|status| {
            integration(&status.id).map(|item| ProviderInfo {
                id: item.id,
                display_name: item.display_name,
                configured: configured.get(item.id) == Some(&true),
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
        .collect();
    CatalogInfos {
        providers,
        credential_store_available,
    }
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
    let ids: Vec<_> = policy
        .targets
        .iter()
        .map(|target| target.provider_id.as_str())
        .collect();
    let configured = configured_many(store, &ids).map_err(|_| "provider_not_configured")?;
    if ids.iter().any(|id| configured.get(*id) != Some(&true)) {
        return Err("provider_not_configured");
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
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    };
    #[derive(Default)]
    struct Keys {
        value: Mutex<Option<Vec<u8>>>,
        loads: AtomicUsize,
        unavailable: AtomicBool,
    }
    impl UnlockKeyStore for Keys {
        fn load(&self) -> Result<Option<Vec<u8>>, SecretError> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            if self.unavailable.load(Ordering::SeqCst) {
                return Err(SecretError::CredentialStoreUnavailable);
            }
            Ok(self.value.lock().unwrap().clone())
        }
        fn store(&self, key: &[u8]) -> Result<(), SecretError> {
            *self.value.lock().unwrap() = Some(key.to_vec());
            Ok(())
        }
        fn delete(&self) -> Result<(), SecretError> {
            *self.value.lock().unwrap() = None;
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
        let keys = std::sync::Arc::new(Keys::default());
        let store = SecretStore::with_key_store(dir.clone(), keys.clone());
        store
            .set_secret(SecretKey::GeminiApiKey, b"synthetic")
            .unwrap();
        keys.loads.store(0, Ordering::SeqCst);
        assert_eq!(
            validate_policy(&policy, &statuses, &store),
            Err("provider_not_configured")
        );
        assert_eq!(keys.loads.load(Ordering::SeqCst), 1);
        store
            .set_secret(SecretKey::GroqApiKey, b"synthetic")
            .unwrap();
        for mode in [
            RoutingMode::Fixed,
            RoutingMode::Preferred,
            RoutingMode::Auto,
        ] {
            let mut route = policy.clone();
            route.routing_mode = mode;
            if mode == RoutingMode::Fixed {
                route.targets.truncate(1);
            }
            keys.loads.store(0, Ordering::SeqCst);
            assert_eq!(validate_policy(&route, &statuses, &store), Ok(()));
            assert_eq!(keys.loads.load(Ordering::SeqCst), 1);
        }
        keys.loads.store(0, Ordering::SeqCst);
        let settings = infos(&statuses, &store);
        assert!(settings.credential_store_available);
        assert!(settings
            .providers
            .iter()
            .all(|provider| provider.configured));
        assert_eq!(keys.loads.load(Ordering::SeqCst), 1);
        let frontend = serde_json::to_string(&settings.providers).unwrap();
        for forbidden in ["synthetic", "apiKey", "unlock", "bearer"] {
            assert!(!frontend.contains(forbidden));
        }
        keys.unavailable.store(true, Ordering::SeqCst);
        assert_eq!(
            validate_policy(&policy, &statuses, &store),
            Err("provider_not_configured")
        );
        let settings = infos(&statuses, &store);
        assert!(!settings.credential_store_available);
        assert!(settings
            .providers
            .iter()
            .all(|provider| !provider.configured));
        keys.unavailable.store(false, Ordering::SeqCst);
        crate::cognition::policy::save(&mut conn, &policy).unwrap();
        keys.loads.store(0, Ordering::SeqCst);
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
        assert_eq!(
            validate_policy(&policy, &statuses, &store),
            Err("provider_unavailable")
        );
        assert_eq!(keys.loads.load(Ordering::SeqCst), 0);
        policy.targets[0].model.clear();
        assert_eq!(
            validate_policy(&policy, &statuses, &store),
            Err("model_invalid")
        );
        assert!(
            serde_json::from_value::<CognitiveTargetPolicy>(serde_json::json!({
                "providerId":"gemini", "model":"valid", "thinkingLevel":"invalid"
            }))
            .is_err()
        );
        assert_eq!(keys.loads.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
