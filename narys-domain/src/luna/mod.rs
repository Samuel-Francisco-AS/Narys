pub mod runtime;
pub mod events;
pub mod task;
pub mod task_id;

use crate::{persistence::database::Database, cognition::{policy::{self,CognitiveRole,RoutingMode}, scheduler::ProviderStatus}, security::{validation,secrets::SecretStore}};
use runtime::TaskRegistry;
use task::TaskId;
use serde::Serialize;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationProviderState {
    provider_id: String,
    display_name: String,
    configured: bool,
    cooldown_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRoutingStatus {
    routing_mode: RoutingMode,
    targets: Vec<ConversationProviderState>,
}

pub fn routing_status_from_backend(
    db: &Database,
    statuses: &[ProviderStatus],
    store: &SecretStore,
) -> Result<ConversationRoutingStatus, String> {
    let conn = db.open().map_err(|e| e.code().to_owned())?;
    let policy =
        policy::load(&conn, CognitiveRole::Conversation).map_err(|e| e.code().to_owned())?;
    crate::cognition::catalog::validate_policy_registered(&policy, statuses)
        .map_err(str::to_owned)?;
    let ids: Vec<_> = policy
        .targets
        .iter()
        .map(|target| target.provider_id.as_str())
        .collect();
    // Informative UX only. The later task always revalidates its own backend snapshot.
    let configured = crate::cognition::catalog::configured_many(store, &ids).unwrap_or_default();
    Ok(ConversationRoutingStatus {
        routing_mode: policy.routing_mode,
        targets: policy
            .targets
            .iter()
            .map(|target| ConversationProviderState {
                provider_id: target.provider_id.clone(),
                display_name: crate::cognition::catalog::integration(&target.provider_id)
                    .map(|item| item.display_name)
                    .unwrap_or(&target.provider_id)
                    .to_owned(),
                configured: configured.get(&target.provider_id) == Some(&true),
                cooldown_ms: statuses
                    .iter()
                    .find(|status| status.id == target.provider_id)
                    .map(|status| status.cooldown_ms)
                    .unwrap_or(0),
            })
            .collect(),
    })
}

pub async fn cancel_task_core(
    registry: &TaskRegistry,
    db: &Database,
    root: TaskId,
) -> Result<bool, &'static str> {
    validation::task_id(root.0)?;
    let active = registry.cancel(root);
    let durable = crate::persistence::continuations::with_connection(db, move |conn| {
        crate::persistence::continuations::ContinuationRepository::cancel(conn, root.0)
    })
    .await?;
    Ok(active || durable)
}

