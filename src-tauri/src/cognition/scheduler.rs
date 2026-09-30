use super::{
    registry::ProviderRegistry,
    types::{
        ProviderChunk, ProviderError, ProviderRequest, ProviderSelection, ProviderTaskRequest,
        RetryPolicy, SchedulerError, SchedulerUsage, TaskBudget, TaskResult,
    },
};
use serde::Serialize;
use std::{
    collections::HashMap,
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
        attempt: u32,
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

pub struct Scheduler {
    registry: ProviderRegistry,
    cooldowns: Mutex<HashMap<String, Instant>>,
}
impl Scheduler {
    pub fn new(registry: ProviderRegistry) -> Self {
        Self {
            registry,
            cooldowns: Mutex::new(HashMap::new()),
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
        let output_limit = match (budget.max_output_tokens, request.max_output_tokens) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        let mut usage = SchedulerUsage::default();
        let mut last_error = None;
        let mut eligible = self.registry.eligible(&request.required_capabilities);
        match &request.selection {
            ProviderSelection::Fixed(id) => eligible.retain(|entry| &entry.config.id == id),
            ProviderSelection::Preferred(id) => eligible.sort_by_key(|entry| {
                (
                    &entry.config.id != id,
                    entry.config.priority,
                    entry.config.id.clone(),
                )
            }),
            ProviderSelection::Auto => {}
        }
        if matches!(request.selection, ProviderSelection::Preferred(_)) {
            eligible.retain(|entry| {
                request
                    .targets
                    .iter()
                    .any(|target| target.provider_id == entry.config.id)
            });
        }
        let candidates: Vec<_> = eligible
            .iter()
            .copied()
            .filter(|entry| !self.cooling(&entry.config.id))
            .collect();
        let mut used_any = false;
        for (index, entry) in candidates.iter().enumerate() {
            if cancelled.load(Ordering::Acquire) {
                return Err(SchedulerError::Cancelled);
            }
            let mut matching = request
                .targets
                .iter()
                .filter(|target| target.provider_id == entry.config.id);
            let target = matching
                .next()
                .filter(|target| target.invocation.valid() && matching.next().is_none())
                .ok_or(SchedulerError::InvalidTargetConfig)?;
            used_any = true;
            let mut attempt = 0;
            loop {
                if cancelled.load(Ordering::Acquire) {
                    return Err(SchedulerError::Cancelled);
                }
                if usage.provider_calls >= budget.max_provider_calls
                    || output_limit.is_some_and(|limit| usage.output_tokens >= limit)
                {
                    return Err(SchedulerError::BudgetExceeded);
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
                    attempt,
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
                let attempt_request = ProviderRequest {
                    input: request.input.clone(),
                    history: request.history.clone(),
                    context: request.context.clone(),
                    max_output_tokens: output_limit.map(|limit| limit - usage.output_tokens),
                    target: target.clone(),
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
                        if cancelled.load(Ordering::Acquire) {
                            return Err(SchedulerError::Cancelled);
                        }
                        if output_limit.is_some_and(|limit| {
                            response.usage.output_tokens > limit - usage.output_tokens
                        }) {
                            return Err(SchedulerError::BudgetExceeded);
                        }
                        usage.input_tokens += response.usage.input_tokens;
                        usage.output_tokens += response.usage.output_tokens;
                        usage.total_tokens = response.usage.total_tokens;
                        usage.thought_tokens = response.usage.thought_tokens;
                        let text = if response.text.is_empty() {
                            chunks
                        } else {
                            response.text
                        };
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
                                    Instant::now() + Duration::from_millis(ms),
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
                        if index + 1 < candidates.len() {
                            on_event(SchedulerEvent::Fallback {
                                from: entry.config.id.clone(),
                                to: candidates[index + 1].config.id.clone(),
                                reason_code: error.code(),
                            })
                            .map_err(|_| {
                                cancelled.store(true, Ordering::Release);
                                SchedulerError::EventSinkClosed
                            })?;
                            usage.fallbacks += 1;
                        }
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
