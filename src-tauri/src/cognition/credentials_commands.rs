use super::summary::SummaryWorker;
use crate::security::{
    audit::{Action, AuditEvent, Outcome},
    secrets::{SecretKey, SecretStore},
};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentialStatus {
    pub configured: bool,
    pub credential_store_available: bool,
}

fn valid_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn valid_account_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

async fn status_for(
    store: Arc<SecretStore>,
    keys: &'static [SecretKey],
) -> Result<ProviderCredentialStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let presence = store.secret_presence(keys).map_err(|error| error.code())?;
        Ok(ProviderCredentialStatus {
            configured: keys
                .iter()
                .all(|key| presence.get(key).copied().unwrap_or(false)),
            credential_store_available: true,
        })
    })
    .await
    .map_err(|_| "credential_store_worker_failed".to_owned())?
}

#[tauri::command]
pub async fn mistral_status(
    store: State<'_, Arc<SecretStore>>,
) -> Result<ProviderCredentialStatus, String> {
    status_for(store.inner().clone(), &[SecretKey::MistralApiKey]).await
}

#[tauri::command]
pub async fn mistral_set_api_key(
    store: State<'_, Arc<SecretStore>>,
    worker: State<'_, Arc<SummaryWorker>>,
    api_key: String,
) -> Result<ProviderCredentialStatus, String> {
    let value = api_key.trim();
    if !valid_value(value) {
        return Err("mistral_key_invalid".into());
    }
    let store = store.inner().clone();
    let write_store = store.clone();
    let value = value.as_bytes().to_vec();
    tauri::async_runtime::spawn_blocking(move || {
        write_store
            .set_secret(SecretKey::MistralApiKey, &value)
            .map_err(|error| error.code())
    })
    .await
    .map_err(|_| "mistral_key_store_failed".to_owned())??;
    AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded)
        .with_detail("mistral_key_configured")
        .emit();
    worker.kick();
    status_for(store, &[SecretKey::MistralApiKey]).await
}

#[tauri::command]
pub async fn mistral_delete_api_key(
    store: State<'_, Arc<SecretStore>>,
    worker: State<'_, Arc<SummaryWorker>>,
) -> Result<ProviderCredentialStatus, String> {
    let store = store.inner().clone();
    let write_store = store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        write_store
            .delete_secret(SecretKey::MistralApiKey)
            .map_err(|error| error.code())
    })
    .await
    .map_err(|_| "mistral_key_delete_failed".to_owned())??;
    AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded)
        .with_detail("mistral_key_deleted")
        .emit();
    worker.kick();
    status_for(store, &[SecretKey::MistralApiKey]).await
}

#[tauri::command]
pub async fn cloudflare_status(
    store: State<'_, Arc<SecretStore>>,
) -> Result<ProviderCredentialStatus, String> {
    status_for(
        store.inner().clone(),
        &[
            SecretKey::CloudflareApiToken,
            SecretKey::CloudflareAccountId,
        ],
    )
    .await
}

#[tauri::command]
pub async fn cloudflare_set_credentials(
    store: State<'_, Arc<SecretStore>>,
    worker: State<'_, Arc<SummaryWorker>>,
    api_token: String,
    account_id: String,
) -> Result<ProviderCredentialStatus, String> {
    let api_token = api_token.trim();
    let account_id = account_id.trim();
    if !valid_value(api_token) {
        return Err("cloudflare_token_invalid".into());
    }
    if !valid_account_id(account_id) {
        return Err("cloudflare_account_id_invalid".into());
    }
    let store = store.inner().clone();
    let write_store = store.clone();
    let token = api_token.as_bytes().to_vec();
    let account = account_id.as_bytes().to_vec();
    tauri::async_runtime::spawn_blocking(move || {
        write_store
            .set_secret(SecretKey::CloudflareApiToken, &token)
            .and_then(|_| write_store.set_secret(SecretKey::CloudflareAccountId, &account))
            .map_err(|error| error.code())
    })
    .await
    .map_err(|_| "cloudflare_credentials_store_failed".to_owned())??;
    AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded)
        .with_detail("cloudflare_credentials_configured")
        .emit();
    worker.kick();
    status_for(
        store,
        &[
            SecretKey::CloudflareApiToken,
            SecretKey::CloudflareAccountId,
        ],
    )
    .await
}

#[tauri::command]
pub async fn cloudflare_delete_credentials(
    store: State<'_, Arc<SecretStore>>,
    worker: State<'_, Arc<SummaryWorker>>,
) -> Result<ProviderCredentialStatus, String> {
    let store = store.inner().clone();
    let write_store = store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        write_store
            .delete_secret(SecretKey::CloudflareApiToken)
            .and_then(|_| write_store.delete_secret(SecretKey::CloudflareAccountId))
            .map_err(|error| error.code())
    })
    .await
    .map_err(|_| "cloudflare_credentials_delete_failed".to_owned())??;
    AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded)
        .with_detail("cloudflare_credentials_deleted")
        .emit();
    worker.kick();
    status_for(
        store,
        &[
            SecretKey::CloudflareApiToken,
            SecretKey::CloudflareAccountId,
        ],
    )
    .await
}
