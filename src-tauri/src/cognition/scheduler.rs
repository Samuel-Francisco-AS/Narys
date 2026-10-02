use super::{
    registry::ProviderRegistry,
    types::{
        ProviderCapabilities, ProviderChunk, ProviderError, ProviderRequest, ProviderSelection,
        ProviderTarget, ProviderTaskRequest, RetryPolicy, SchedulerError, SchedulerUsage,
        TaskBudget, TaskResult,
    },
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum SchedulerEvent {
    Selected {
        provider_id: String,
        model: String,
        attempt: u32,
        routing_reason: &'static str,
        score: Option<u32>,
    },
    Chunk {
        provider_id: String,
        text: String,
    },
    Retry {
        provider_id: String,
        reason_code: &'static str,
    },
    Fallback {
        from: String,
        to: String,
        reason_code: &'static str,
    },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub id: String,
    pub enabled: bool,
    pub priority: u16,
    pub capabilities: super::types::ProviderCapabilities,
    pub cooldown_ms: u64,
}

// Policy position is primary; registry priority is deliberately secondary.
const POLICY_POSITION_WEIGHT: u32 = 100;
const REGISTRY_PRIORITY_CAP: u32 = 32;
const AFFINITY_BASE: u32 = 50;
const AFFINITY_PER_KIB: u32 = 25;
const AFFINITY_CAP: u32 = 500;
const CONTEXT_KIB: usize = 1024;
const MAX_AFFINITIES: usize = 256;
const MAX_AFFINITY_KEY_BYTES: usize = 128;

#[derive(Default)]
struct Affinities(VecDeque<(String, String)>);
impl Affinities {
    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, id)| id.as_str())
    }
    fn remember(&mut self, key: &str, provider: &str) {
        self.0.retain(|(k, _)| k != key);
        if self.0.len() == MAX_AFFINITIES {
            self.0.pop_front();
        }
        self.0.push_back((key.into(), provider.into()));
    }
}

fn auto_score(
    count: usize,
    ordinal: usize,
    priority: u16,
    affinity: bool,
    bytes: usize,
) -> (u32, u32) {
    let policy = (count.saturating_sub(ordinal) as u32).saturating_mul(POLICY_POSITION_WEIGHT);
    let registry = REGISTRY_PRIORITY_CAP - u32::from(priority).min(REGISTRY_PRIORITY_CAP);
    let affinity = if affinity && bytes > 0 {
        let kib = bytes / CONTEXT_KIB + usize::from(bytes % CONTEXT_KIB != 0);
        AFFINITY_BASE
            .saturating_add(
                u32::try_from(kib)
                    .unwrap_or(u32::MAX)
                    .saturating_mul(AFFINITY_PER_KIB),
            )
            .min(AFFINITY_CAP)
    } else {
        0
    };
    (
        policy.saturating_add(registry).saturating_add(affinity),
        affinity,
    )
}

pub struct Scheduler {
    registry: ProviderRegistry,
    cooldowns: Mutex<HashMap<String, Instant>>,
    affinities: Mutex<Affinities>,
}

impl Scheduler {
    pub fn new(registry: ProviderRegistry) -> Self {
        Self {
            registry,
            cooldowns: Mutex::new(HashMap::new()),
            affinities: Mutex::new(Affinities::default()),
        }
    }
    fn cooling(&self, id: &str) -> bool {
        self.cooldowns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .is_some_and(|until| *until > Instant::now())
    }
    pub fn status(&self) -> Vec<ProviderStatus> {
        let now = Instant::now();
        let cooldowns = self.cooldowns.lock().unwrap_or_else(|p| p.into_inner());
        self.registry
            .configs()
            .into_iter()
            .map(|config| ProviderStatus {
                id: config.id.clone(),
                enabled: config.enabled,
                priority: config.priority,
                capabilities: config.capabilities,
                cooldown_ms: cooldowns
                    .get(&config.id)
                    .map(|until| until.saturating_duration_since(now).as_millis() as u64)
                    .unwrap_or(0),
            })
            .collect()
    }
    pub fn ranked_provider_ids(
        &self,
        selection: &ProviderSelection,
        targets: &[ProviderTarget],
        required: &ProviderCapabilities,
    ) -> Result<Vec<String>, SchedulerError> {
        if targets.is_empty() || targets.len() > super::policy::MAX_TARGETS {
            return Err(SchedulerError::InvalidTargetConfig);
        }
        let mut ids = HashSet::new();
        let mut ranked = Vec::new();
        for (ordinal, target) in targets.iter().enumerate() {
            if !target.invocation.valid() || !ids.insert(&target.provider_id) {
                return Err(SchedulerError::InvalidTargetConfig);
            }
            if let ProviderSelection::Fixed(id) = selection {
                if &target.provider_id != id {
                    continue;
                }
            }
            let entry = self
                .registry
                .get(&target.provider_id)
                .ok_or(SchedulerError::NoProvider)?;
            if !entry.config.enabled
                || !entry.config.capabilities.supports(required)
                || self.cooling(&entry.config.id)
            {
                continue;
            }
            let (score, _) = auto_score(targets.len(), ordinal, entry.config.priority, false, 0);
            ranked.push((entry.config.id.clone(), ordinal, score));
        }
        if matches!(selection, ProviderSelection::Auto) {
            ranked.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
        }
        let result: Vec<_> = ranked.into_iter().map(|item| item.0).collect();
        if result.is_empty() {
            Err(SchedulerError::NoProvider)
        } else {
            Ok(result)
        }
    }

    pub async fn run(
        &self,
        request: ProviderTaskRequest,
        budget: TaskBudget,
        cancelled: &AtomicBool,
        on_event: &mut (dyn FnMut(SchedulerEvent) -> Result<(), SchedulerError> + Send),
    ) -> Result<TaskResult, SchedulerError> {
        self.run_with_retry(
            request,
            budget,
            RetryPolicy {
                enabled: true,
                max_retries: 1,
                initial_backoff_ms: 0,
            },
            cancelled,
            on_event,
        )
        .await
    }
    pub async fn run_with_retry(
        &self,
        request: ProviderTaskRequest,
        budget: TaskBudget,
        retry_policy: RetryPolicy,
        cancelled: &AtomicBool,
        on_event: &mut (dyn FnMut(SchedulerEvent) -> Result<(), SchedulerError> + Send),
    ) -> Result<TaskResult, SchedulerError> {
        self.run_with_retry_mode(request, budget, retry_policy, cancelled, on_event, false)
            .await
    }

    /// Task graph calls use a conservative output ledger because some providers do
    /// not return usage. Each possible attempt receives a share of the remaining
    /// budget; unknown attempts debit their full share.
    pub async fn run_with_retry_conservative_output(
        &self,
        request: ProviderTaskRequest,
        budget: TaskBudget,
        retry_policy: RetryPolicy,
        cancelled: &AtomicBool,
        on_event: &mut (dyn FnMut(SchedulerEvent) -> Result<(), SchedulerError> + Send),
    ) -> Result<TaskResult, SchedulerError> {
        self.run_with_retry_mode(request, budget, retry_policy, cancelled, on_event, true)
            .await
    }

    async fn run_with_retry_mode(
        &self,
        request: ProviderTaskRequest,
        budget: TaskBudget,
        retry_policy: RetryPolicy,
        cancelled: &AtomicBool,
        on_event: &mut (dyn FnMut(SchedulerEvent) -> Result<(), SchedulerError> + Send),
        conservative_output: bool,
    ) -> Result<TaskResult, SchedulerError> {
        if cancelled.load(Ordering::Acquire) && !conservative_output {
            return Err(SchedulerError::Cancelled);
        }
        let output_limit = match (budget.max_output_tokens, request.max_output_tokens) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        let mut usage = SchedulerUsage {
            output_tokens_measured: true,
            ..SchedulerUsage::default()
        };
        let mut last_error: Option<ProviderError> = None;
        let mut last_provider: Option<String> = None;
        if request.targets.is_empty()
            || request.targets.len() > super::policy::MAX_TARGETS
            || request
                .affinity_key
                .as_ref()
                .is_some_and(|key| key.is_empty() || key.len() > MAX_AFFINITY_KEY_BYTES)
        {
            return Err(SchedulerError::InvalidTargetConfig);
        }
        let mut ids = HashSet::new();
        // Validate every authorized invocation before starting any provider call.
        for target in &request.targets {
            if !target.invocation.valid() || !ids.insert(&target.provider_id) {
                return Err(SchedulerError::InvalidTargetConfig);
            }
        }
        let affinity = request.affinity_key.as_deref().and_then(|key| {
            self.affinities
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(key)
                .map(str::to_owned)
        });
        let mut eligible = Vec::new();
        let mut candidates = Vec::new();
        for (ordinal, target) in request.targets.iter().enumerate() {
            if let ProviderSelection::Fixed(id) = &request.selection {
                if &target.provider_id != id {
                    continue;
                }
            }
            let entry = self
                .registry
                .get(&target.provider_id)
                .ok_or(SchedulerError::NoProvider)?;
            if !entry.config.enabled
                || !entry
                    .config
                    .capabilities
                    .supports(&request.required_capabilities)
            {
                return Err(SchedulerError::NoProvider);
            }
            eligible.push(entry);
            if self.cooling(&entry.config.id) {
                continue;
            }
            // Hard gates precede score; no registry-only provider can enter this list.
            let (score, affinity_score) = auto_score(
                request.targets.len(),
                ordinal,
                entry.config.priority,
                affinity.as_deref() == Some(entry.config.id.as_str()),
                request.estimated_context_bytes,
            );
            // Retain ordinal independently from incidental registry order.
            candidates.push((entry, target, ordinal, score, affinity_score));
        }
        if matches!(request.selection, ProviderSelection::Auto) {
            candidates.sort_by(|a, b| {
                b.3.cmp(&a.3)
                    .then(a.2.cmp(&b.2))
                    .then(a.0.config.id.cmp(&b.0.config.id))
            });
        }
        // Attribute Auto affinity only when its bonus changes the winning selection.
        let affinity_winner = if matches!(request.selection, ProviderSelection::Auto) {
            let without = candidates.iter().max_by(|a, b| {
                (a.3 - a.4)
                    .cmp(&(b.3 - b.4))
                    .then(b.2.cmp(&a.2))
                    .then(b.0.config.id.cmp(&a.0.config.id))
            });
            candidates
                .first()
                .zip(without)
                .is_some_and(|(winner, base)| winner.4 > 0 && winner.2 != base.2)
        } else {
            false
        };
        let mut used_any = false;
        for (index, (entry, target, _, score, _)) in candidates.iter().enumerate() {
            if cancelled.load(Ordering::Acquire) {
                return Err(SchedulerError::Cancelled);
            }
            // Shared cooldown may have changed since initial candidate resolution.
            if self.cooling(&entry.config.id) {
                continue;
            }
            used_any = true;
            let mut attempt = 0;
            loop {
                if cancelled.load(Ordering::Acquire) {
                    return Err(SchedulerError::Cancelled);
                }
                if usage.provider_calls >= budget.max_provider_calls
                    || output_limit.is_some_and(|limit| {
                        let spent = if conservative_output {
                            usage.output_tokens_accounted
                        } else {
                            usage.output_tokens
                        };
                        spent >= limit
                    })
                {
                    return Err(SchedulerError::BudgetExceeded);
                }
                if attempt == 0 {
                    if let (Some(from), Some(error)) = (&last_provider, &last_error) {
                        on_event(SchedulerEvent::Fallback {
                            from: from.clone(),
                            to: entry.config.id.clone(),
                            reason_code: error.code(),
                        })
                        .map_err(|_| {
                            cancelled.store(true, Ordering::Release);
                            SchedulerError::EventSinkClosed
                        })?;
                        usage.fallbacks += 1;
                    }
                }
                attempt += 1;
                usage.provider_calls += 1;
                if attempt > 1 {
                    usage.retries += 1;
                }
                if !usage.providers_used.contains(&entry.config.id) {
                    usage.providers_used.push(entry.config.id.clone());
                }
                on_event(SchedulerEvent::Selected {
                    provider_id: entry.config.id.clone(),
                    model: target.invocation.model.clone(),
                    attempt,
                    routing_reason: match request.selection {
                        ProviderSelection::Fixed(_) => "fixed",
                        ProviderSelection::Preferred => "preferred_order",
                        ProviderSelection::Auto if index == 0 && affinity_winner => "auto_affinity",
                        ProviderSelection::Auto => "auto_score",
                    },
                    score: matches!(request.selection, ProviderSelection::Auto).then_some(*score),
                })
                .map_err(|_| {
                    cancelled.store(true, Ordering::Release);
                    SchedulerError::EventSinkClosed
                })?;
                let mut chunks = String::new();
                let mut emitted_chunk = false;
                let mut on_chunk = |chunk: ProviderChunk| -> Result<(), ProviderError> {
                    if cancelled.load(Ordering::Acquire) {
                        return Err(ProviderError::Cancelled);
                    }
                    emitted_chunk = true;
                    chunks.push_str(&chunk.text);
                    on_event(SchedulerEvent::Chunk {
                        provider_id: entry.config.id.clone(),
                        text: chunk.text,
                    })
                    .map_err(|_| {
                        cancelled.store(true, Ordering::Release);
                        ProviderError::EventSinkClosed
                    })
                };
                let spent_output = if conservative_output {
                    usage.output_tokens_accounted
                } else {
                    usage.output_tokens
                };
                let remaining_output = output_limit.map(|limit| limit.saturating_sub(spent_output));
                let remaining_calls = budget.max_provider_calls.saturating_sub(usage.provider_calls);
                let attempt_output_limit = if conservative_output {
                    remaining_output.map(|remaining| {
                        remaining
                            .saturating_add(remaining_calls.saturating_sub(1))
                            .checked_div(remaining_calls.max(1))
                            .unwrap_or(0)
                    })
                } else {
                    remaining_output
                };
                let attempt_request = ProviderRequest {
                    input: request.input.clone(),
                    internal_system_instruction: request.internal_system_instruction.clone(),
                    history: request.history.clone(),
                    context: request.context.clone(),
                    max_output_tokens: attempt_output_limit,
                    target: (*target).clone(),
                    attempt,
                };
                // Keep the same structured context across retry/fallback; adapters decide serialization.
                let result = entry
                    .provider
                    .execute(&attempt_request, cancelled, &mut on_chunk)
                    .await;
                if matches!(result, Err(ProviderError::EventSinkClosed)) {
                    return Err(SchedulerError::EventSinkClosed);
                }
                match result {
                    Ok(response) => {
                        if cancelled.load(Ordering::Acquire) && !conservative_output {
                            return Err(SchedulerError::Cancelled);
                        }
                        if output_limit.is_some_and(|limit| {
                            let spent = if conservative_output {
                                usage.output_tokens_accounted
                            } else {
                                usage.output_tokens
                            };
                            response.usage.output_tokens > limit - spent
                        }) {
                            return Err(SchedulerError::BudgetExceeded);
                        }
                        usage.input_tokens += response.usage.input_tokens;
                        usage.output_tokens += response.usage.output_tokens;
                        usage.output_tokens_measured &= response.usage.output_tokens_measured;
                        usage.output_tokens_accounted = usage.output_tokens_accounted.saturating_add(
                            if response.usage.output_tokens_measured {
                                response.usage.output_tokens
                            } else {
                                attempt_request.max_output_tokens.unwrap_or_default()
                            },
                        );
                        usage.total_tokens = response.usage.total_tokens;
                        usage.thought_tokens = response.usage.thought_tokens;
                        let text = if response.text.is_empty() {
                            chunks
                        } else {
                            response.text
                        };
                        if let Some(key) = request.affinity_key.as_deref() {
                            self.affinities
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .remember(key, &entry.config.id);
                        }
                        return Ok(TaskResult {
                            text,
                            provider_id: entry.config.id.clone(),
                            usage,
                            context_metadata: request.context.metadata.clone(),
                        });
                    }
                    Err(ProviderError::Cancelled) => return Err(SchedulerError::Cancelled),
                    Err(ProviderError::EventSinkClosed) => {
                        return Err(SchedulerError::EventSinkClosed)
                    }
                    Err(error) => {
                        if conservative_output {
                            usage.output_tokens_accounted = usage
                                .output_tokens_accounted
                                .saturating_add(attempt_output_limit.unwrap_or_default());
                        }
                        if cancelled.load(Ordering::Acquire) {
                            return Err(SchedulerError::Cancelled);
                        }
                        let cooldown_ms = match error {
                            ProviderError::RateLimited { retry_after_ms } => {
                                Some(retry_after_ms.unwrap_or(3_000).max(1))
                            }
                            ProviderError::Unavailable {
                                retry_after_ms: Some(ms),
                            } => Some(ms.max(1)),
                            _ => None,
                        };
                        if let Some(ms) = cooldown_ms {
                            self.cooldowns
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .insert(
                                    entry.config.id.clone(),
                                    Instant::now()
                                        .checked_add(Duration::from_millis(ms))
                                        .unwrap_or_else(|| {
                                            Instant::now() + Duration::from_secs(86_400)
                                        }),
                                );
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "[Scheduler][diag] cooldown provider={} reason={} cooldown_ms={ms}",
                                entry.config.id,
                                error.code()
                            );
                        }
                        // Once text has reached the UI, another attempt would concatenate
                        // incompatible partial answers and could double provider cost.
                        if emitted_chunk {
                            return Err(SchedulerError::Provider(error));
                        }
                        let eligible_error = matches!(
                            error,
                            ProviderError::Timeout
                                | ProviderError::Unavailable {
                                    retry_after_ms: None
                                }
                        );
                        let retries_used = attempt - 1;
                        let can_retry = retry_policy.enabled
                            && eligible_error
                            && retries_used < retry_policy.max_retries
                            && usage.provider_calls < budget.max_provider_calls;
                        #[cfg(debug_assertions)]
                        if retry_policy.enabled && eligible_error && !can_retry {
                            let reason = if usage.provider_calls >= budget.max_provider_calls {
                                "call_budget"
                            } else {
                                "retry_limit"
                            };
                            eprintln!("[Scheduler][diag] retry_skipped reason={reason}");
                        }
                        if can_retry {
                            let backoff_ms = retry_policy.backoff_ms(attempt);
                            #[cfg(debug_assertions)]
                            {
                                let provider = &entry.config.id;
                                eprintln!("[Scheduler][diag] retry provider={provider} attempt={} reason={} backoff_ms={backoff_ms}", attempt + 1, error.code());
                            }
                            on_event(SchedulerEvent::Retry {
                                provider_id: entry.config.id.clone(),
                                reason_code: error.code(),
                            })
                            .map_err(|_| {
                                cancelled.store(true, Ordering::Release);
                                SchedulerError::EventSinkClosed
                            })?;
                            // Cancellable asynchronous backoff.
                            let now = tokio::time::Instant::now();
                            let until = now
                                .checked_add(Duration::from_millis(backoff_ms))
                                .unwrap_or_else(|| now + Duration::from_secs(86_400));
                            while tokio::time::Instant::now() < until {
                                if cancelled.load(Ordering::Acquire) {
                                    return Err(SchedulerError::Cancelled);
                                }
                                tokio::time::sleep(
                                    (until - tokio::time::Instant::now())
                                        .min(Duration::from_millis(25)),
                                )
                                .await;
                            }
                            continue;
                        }
                        let can_fallback =
                            !matches!(request.selection, ProviderSelection::Fixed(_))
                                && matches!(
                                    error,
                                    ProviderError::RateLimited { .. }
                                        | ProviderError::Unavailable { .. }
                                        | ProviderError::Timeout
                                );
                        if !can_fallback {
                            return Err(SchedulerError::Provider(error));
                        }
                        if index + 1 < candidates.len()
                            && usage.provider_calls >= budget.max_provider_calls
                        {
                            return Err(SchedulerError::Provider(error));
                        }
                        last_provider = Some(entry.config.id.clone());
                        last_error = Some(error);
                        break;
                    }
                }
            }
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(SchedulerError::Cancelled);
        }
        if !used_any {
            #[cfg(debug_assertions)]
            {
                if eligible.is_empty() {
                    eprintln!("[Scheduler][diag] no_provider reason=no_eligible_provider");
                } else {
                    let now = Instant::now();
                    let cooldowns = self.cooldowns.lock().unwrap_or_else(|p| p.into_inner());
                    for entry in eligible {
                        let cooldown_ms = cooldowns
                            .get(&entry.config.id)
                            .map(|until| until.saturating_duration_since(now).as_millis() as u64)
                            .unwrap_or(0);
                        let provider = &entry.config.id;
                        eprintln!("[Scheduler][diag] no_provider reason=cooldown provider={provider} cooldown_ms={cooldown_ms}");
                    }
                }
            }
            Err(SchedulerError::NoProvider)
        } else {
            Err(SchedulerError::Provider(last_error.unwrap_or(
                ProviderError::Unavailable {
                    retry_after_ms: None,
                },
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn affinity_storage_evicts_oldest_success_and_refreshes_bounded_entries() {
        let mut entries = Affinities::default();
        for n in 0..MAX_AFFINITIES {
            entries.remember(&format!("s{n}"), "a");
        }
        entries.remember("s0", "b");
        entries.remember("new", "c");
        assert_eq!(entries.0.len(), MAX_AFFINITIES);
        assert_eq!(entries.get("s0"), Some("b"));
        assert_eq!(entries.get("s1"), None);
        assert_eq!(entries.get("new"), Some("c"));
    }
    #[test]
    fn affinity_score_is_bounded_even_for_maximum_context_size() {
        assert_eq!(auto_score(8, 7, u16::MAX, true, usize::MAX), (600, 500));
        assert_eq!(auto_score(2, 1, 32, true, 0), (100, 0));
    }

    #[test]
    fn task_graph_ranking_respects_authorized_order_fixed_and_auto() {
        use crate::cognition::{
            mock::{MockProvider, MockScenario},
            types::{ProviderConfig, ProviderInvocationConfig, ProviderTimeouts},
        };
        let mut registry = ProviderRegistry::default();
        for (id, priority) in [("a", 10), ("b", 1), ("c", 20)] {
            registry
                .register(
                    ProviderConfig {
                        id: id.into(),
                        enabled: true,
                        priority,
                        capabilities: ProviderCapabilities::text_stream(),
                    },
                    std::sync::Arc::new(MockProvider::new(MockScenario::Normal)),
                )
                .unwrap();
        }
        let scheduler = Scheduler::new(registry);
        let target = |id: &str| ProviderTarget {
            provider_id: id.into(),
            invocation: ProviderInvocationConfig {
                model: format!("{id}-model"),
                thinking_level: None,
                timeouts: Some(ProviderTimeouts {
                    request_timeout_ms: 1000,
                    stream_idle_timeout_ms: 1000,
                }),
            },
        };
        let targets = vec![target("a"), target("b"), target("c")];
        assert_eq!(
            scheduler
                .ranked_provider_ids(
                    &ProviderSelection::Preferred,
                    &targets,
                    &ProviderCapabilities::text_stream(),
                )
                .unwrap(),
            vec!["a", "b", "c"]
        );
        assert_eq!(
            scheduler
                .ranked_provider_ids(
                    &ProviderSelection::Fixed("b".into()),
                    &targets,
                    &ProviderCapabilities::text_stream(),
                )
                .unwrap(),
            vec!["b"]
        );
        // Policy position is intentionally stronger than registry priority.
        assert_eq!(
            scheduler
                .ranked_provider_ids(
                    &ProviderSelection::Auto,
                    &targets,
                    &ProviderCapabilities::text_stream(),
                )
                .unwrap(),
            vec!["a", "b", "c"]
        );
    }
}
