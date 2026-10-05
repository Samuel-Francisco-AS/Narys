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
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Debug)]
pub enum SchedulerEvent {
    Queued {
        provider_id: String,
        traffic_class: super::admission::TrafficClass,
        queue_depth: usize,
    },
    Admitted {
        provider_id: String,
        traffic_class: super::admission::TrafficClass,
        queue_delay_ms: u64,
    },
    Selected {
        provider_id: String,
        model: String,
        attempt: u32,
        routing_reason: &'static str,
        score: Option<u32>,
    },
    OutputObserved {
        provider_id: String,
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

/// Selected/request attempt numbers are provisional until transport denial is
/// ruled out. Consuming this value commits bookkeeping exactly once; dropping
/// it changes no ledger. Other provider preflight outcomes retain the existing
/// conservative call-budget semantics, distinct from LR-8A factual requests.
struct PendingSchedulerAttempt {
    number: u32,
    fallback: bool,
}
impl PendingSchedulerAttempt {
    fn commit(self, attempt: &mut u32, usage: &mut SchedulerUsage, provider_id: &str) {
        *attempt = self.number;
        usage.provider_calls += 1;
        if self.number > 1 {
            usage.retries += 1;
        }
        if self.fallback {
            usage.fallbacks += 1;
        }
        if !usage.providers_used.iter().any(|id| id == provider_id) {
            usage.providers_used.push(provider_id.to_owned());
        }
    }
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
    pub(super) admission: super::admission::AdmissionController,
    registry: ProviderRegistry,
    pub(super) resilience: Arc<super::resilience::ResilienceManager>,
    affinities: Mutex<Affinities>,
    pub(super) telemetry: super::telemetry::TelemetryStore,
    pub rate: Arc<super::rate::RateLimitManager>,
}

impl Scheduler {
    pub fn new(registry: ProviderRegistry) -> Self {
        Self::with_admission_config(registry, super::admission::AdmissionConfig::default())
            .expect("valid local admission defaults")
    }
    pub fn with_admission_config(
        registry: ProviderRegistry,
        config: super::admission::AdmissionConfig,
    ) -> Result<Self, &'static str> {
        Self::with_rate_config(
            registry,
            config,
            Arc::new(super::rate::SystemRateClock::default()),
            None,
        )
        .map_err(|_| "runtime_config_invalid")
    }
    pub fn with_rate_storage(
        registry: ProviderRegistry,
        database: Option<crate::persistence::database::Database>,
    ) -> Result<Self, SchedulerError> {
        Self::with_rate_config(
            registry,
            super::admission::AdmissionConfig::default(),
            Arc::new(super::rate::SystemRateClock::default()),
            database,
        )
    }
    pub fn with_rate_config(
        registry: ProviderRegistry,
        config: super::admission::AdmissionConfig,
        clock: Arc<dyn super::rate::RateClock>,
        database: Option<crate::persistence::database::Database>,
    ) -> Result<Self, SchedulerError> {
        Self::with_resilience_config(
            registry,
            config,
            clock,
            database,
            super::resilience::ResilienceConfig::default(),
            Arc::new(super::resilience::RuntimeJitter::default()),
        )
    }
    pub fn with_resilience_config(
        registry: ProviderRegistry,
        config: super::admission::AdmissionConfig,
        clock: Arc<dyn super::rate::RateClock>,
        database: Option<crate::persistence::database::Database>,
        resilience_config: super::resilience::ResilienceConfig,
        jitter: Arc<dyn super::resilience::JitterSource>,
    ) -> Result<Self, SchedulerError> {
        let resilience = super::resilience::ResilienceManager::new(
            registry.configs().into_iter().map(|c| c.id.clone()),
            resilience_config,
            clock.clone(),
            jitter,
        )
        .map_err(|_| SchedulerError::InvalidTargetConfig)?;
        let admission = super::admission::AdmissionController::new(
            registry.configs().into_iter().map(|c| c.id.clone()),
            config,
        )
        .map_err(|_| SchedulerError::InvalidTargetConfig)?;
        let rate = super::rate::RateLimitManager::new(
            registry.configs().into_iter().map(|c| c.id.clone()),
            clock,
            database,
        )?;
        let telemetry = super::telemetry::TelemetryStore::with_runtime(
            registry.configs().into_iter().map(|c| c.id.clone()),
            rate.clone(),
            resilience.clone(),
        );
        Ok(Self {
            admission,
            telemetry,
            rate,
            registry,
            resilience,
            affinities: Mutex::new(Affinities::default()),
        })
    }
    pub fn admission_snapshot(&self) -> Vec<super::admission::AdmissionSnapshot> {
        self.admission.snapshots()
    }
    pub fn telemetry_snapshot(&self) -> Vec<super::telemetry::ProviderTelemetrySnapshot> {
        self.telemetry.snapshots()
    }
    pub fn rate_snapshot(&self) -> Vec<super::rate::RateSnapshot> {
        self.rate.snapshots()
    }
    pub fn resilience_snapshot(&self) -> Vec<super::resilience::ResilienceSnapshot> {
        self.resilience.snapshots()
    }
    /// Telemetry owns context generation. Invalidation synchronizes rate and
    /// resilience under that authority, preserving every durable local budget.
    pub fn invalidate_rate_context(&self, id: &str) {
        self.telemetry.invalidate_provider_quotas(id);
    }
    pub fn status(&self) -> Vec<ProviderStatus> {
        let health = self.resilience_snapshot();
        self.registry
            .configs()
            .into_iter()
            .map(|config| ProviderStatus {
                id: config.id.clone(),
                enabled: config.enabled,
                priority: config.priority,
                capabilities: config.capabilities,
                cooldown_ms: health
                    .iter()
                    .find(|s| s.provider_id == config.id)
                    .map_or(0, |s| s.cooldown_remaining_ms),
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
            if !entry.config.enabled || !entry.config.capabilities.supports(required) {
                continue;
            }
            let (score, _) = auto_score(targets.len(), ordinal, entry.config.priority, false, 0);
            ranked.push((entry.config.id.clone(), ordinal, score));
        }
        if matches!(selection, ProviderSelection::Auto) {
            ranked.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
        }
        let result: Vec<_> = ranked
            .into_iter()
            .filter(|item| self.resilience.eligible(&item.0))
            .map(|item| item.0)
            .collect();
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
        if !request.mode.valid()
            || request.targets.is_empty()
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
            if !entry.config.enabled {
                return Err(SchedulerError::NoProvider);
            }
            if !entry
                .config
                .capabilities
                .supports(&request.required_capabilities)
                || !entry
                    .provider
                    .supports_invocation(&target.invocation, &request.mode)
            {
                if request.mode.text_stream() {
                    return Err(SchedulerError::NoProvider);
                }
                // An incompatible target consumes neither a call nor retry/fallback.
                continue;
            }
            eligible.push(entry);
            // Operational health is applied after deterministic ranking.
            // No registry-only provider can enter this authorized list.
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
            let mut attempt = 0;
            loop {
                if cancelled.load(Ordering::Acquire) {
                    return Err(SchedulerError::Cancelled);
                }
                // Capture the existing credential era before any rate/admission.
                // The atomic gate owns an optional probe only for this attempt.
                let observation = self.telemetry.attempt(&entry.config.id);
                let Some(health_permit) = self
                    .resilience
                    .authorize(&entry.config.id, observation.context_generation())
                else {
                    break;
                };
                used_any = true;
                observation.attach_resilience(health_permit.handle());
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
                let pending = PendingSchedulerAttempt {
                    // The budget check guarantees room for one more u32 call.
                    number: attempt + 1,
                    fallback: attempt == 0 && last_provider.is_some() && last_error.is_some(),
                };
                if pending.fallback {
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
                    }
                }
                on_event(SchedulerEvent::Selected {
                    provider_id: entry.config.id.clone(),
                    model: target.invocation.model.clone(),
                    attempt: pending.number,
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
                let spent_output = if conservative_output {
                    usage.output_tokens_accounted
                } else {
                    usage.output_tokens
                };
                let remaining_output = output_limit.map(|limit| limit.saturating_sub(spent_output));
                // Include the call about to start: divide the remaining ledger
                // across this attempt and every call still available afterward.
                let attempts_remaining = budget
                    .max_provider_calls
                    .saturating_sub(usage.provider_calls)
                    .max(1);
                let attempt_output_limit = if conservative_output {
                    remaining_output.map(|remaining| {
                        remaining / attempts_remaining
                            + u32::from(remaining % attempts_remaining != 0)
                    })
                } else {
                    remaining_output
                };
                let attempt_request = ProviderRequest {
                    mode: request.mode.clone(),
                    input: request.input.clone(),
                    internal_system_instruction: request.internal_system_instruction.clone(),
                    history: request.history.clone(),
                    context: request.context.clone(),
                    max_output_tokens: attempt_output_limit,
                    target: (*target).clone(),
                    attempt: pending.number,
                };
                let reservation = self.rate.reserve(
                    &entry.config.id,
                    &target.invocation.model,
                    observation.context_generation(),
                    entry.provider.token_upper_bound(&attempt_request),
                    cancelled,
                )?;
                observation.attach_rate(reservation.handle());
                let permit = self
                    .admission
                    .acquire(
                        &entry.config.id,
                        request.traffic_class,
                        cancelled,
                        &mut |queue_depth| {
                            on_event(SchedulerEvent::Queued {
                                provider_id: entry.config.id.clone(),
                                traffic_class: request.traffic_class,
                                queue_depth,
                            })
                            .map_err(|_| {
                                cancelled.store(true, Ordering::Release);
                                SchedulerError::EventSinkClosed
                            })
                        },
                    )
                    .await?;
                on_event(SchedulerEvent::Admitted {
                    provider_id: entry.config.id.clone(),
                    traffic_class: request.traffic_class,
                    queue_delay_ms: permit.queue_delay_ms,
                })
                .map_err(|_| {
                    cancelled.store(true, Ordering::Release);
                    SchedulerError::EventSinkClosed
                })?;
                if cancelled.load(Ordering::Acquire) {
                    return Err(SchedulerError::Cancelled);
                }
                let mut chunks = String::new();
                let mut emitted_chunk = false;
                let mut on_chunk =
                    |chunk: ProviderChunk| -> Result<(), ProviderError> {
                        if cancelled.load(Ordering::Acquire) {
                            return Err(ProviderError::Cancelled);
                        }
                        if request.mode.max_bytes().is_some_and(|limit| {
                            chunks.len().saturating_add(chunk.text.len()) > limit
                        }) {
                            return Err(ProviderError::OutputLimitExceeded);
                        }
                        let first = !emitted_chunk;
                        emitted_chunk = true;
                        chunks.push_str(&chunk.text);
                        if request.mode.max_bytes().is_some() && !first {
                            return Ok(());
                        }
                        on_event(if request.mode.max_bytes().is_some() {
                            SchedulerEvent::OutputObserved {
                                provider_id: entry.config.id.clone(),
                            }
                        } else {
                            SchedulerEvent::Chunk {
                                provider_id: entry.config.id.clone(),
                                text: chunk.text,
                            }
                        })
                        .map_err(|_| {
                            cancelled.store(true, Ordering::Release);
                            ProviderError::EventSinkClosed
                        })
                    };
                // Keep the same structured context across retry/fallback; adapters decide serialization.
                // Final cancellation check at the provider invocation boundary.
                if cancelled.load(Ordering::Acquire) {
                    return Err(SchedulerError::Cancelled);
                }
                match health_permit.handle().revalidate() {
                    // A peer can close the operational gate while this attempt
                    // queues. Release neutrally and advance in the original order.
                    Err(SchedulerError::NoProvider) => break,
                    Err(error) => return Err(error),
                    Ok(()) => {}
                }
                let result = entry
                    .provider
                    .execute_observed(&attempt_request, cancelled, &mut on_chunk, &observation)
                    .await;
                // Release before response processing, retry backoff, fallback or Core consolidation.
                drop(permit);
                observation.finished(result.as_ref().err());
                // RAII covers queue errors, sink failure, cancellation and aborted futures.
                // Reconcile before any response processing/backoff/fallback.
                drop(reservation);
                let boundary_error = observation.rate_error();
                if !observation.was_started() {
                    if let Some(error) = &boundary_error {
                        // No debit ever occurred: queue/HTTP resilience denial
                        // can advance, while context changes remain terminal.
                        if *error == SchedulerError::NoProvider {
                            break;
                        }
                        return Err(error.clone());
                    }
                }
                // A factual call is never rolled back, even if a later local
                // error is reported. Preserve legacy provider-preflight debits.
                pending.commit(&mut attempt, &mut usage, &entry.config.id);
                #[cfg(test)]
                super::lr8e_gate_tests::committed_ack(cancelled, &entry.config.id);
                if let Some(error) = boundary_error {
                    return Err(error);
                }
                if matches!(result, Err(ProviderError::EventSinkClosed)) {
                    return Err(SchedulerError::EventSinkClosed);
                }
                // A local error/cancellation drops the guard neutrally. Provider
                // health uses only the LR-8A HTTP fact, before Core validation.
                if cancelled.load(Ordering::Acquire) {
                    return Err(SchedulerError::Cancelled);
                }
                health_permit.finish(observation.was_started(), result.as_ref().err());
                match result {
                    Ok(response) => {
                        if request
                            .mode
                            .max_bytes()
                            .is_some_and(|limit| response.text.len() > limit)
                        {
                            return Err(SchedulerError::Provider(
                                ProviderError::OutputLimitExceeded,
                            ));
                        }
                        if request.mode.max_bytes().is_some()
                            && !emitted_chunk
                            && !response.text.is_empty()
                        {
                            on_event(SchedulerEvent::OutputObserved {
                                provider_id: entry.config.id.clone(),
                            })
                            .map_err(|_| {
                                cancelled.store(true, Ordering::Release);
                                SchedulerError::EventSinkClosed
                            })?;
                        }
                        if cancelled.load(Ordering::Acquire) && !conservative_output {
                            return Err(SchedulerError::Cancelled);
                        }
                        if conservative_output
                            && attempt_output_limit
                                .is_some_and(|limit| response.usage.output_tokens > limit)
                        {
                            return Err(SchedulerError::BudgetExceeded);
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
                        usage.output_tokens_accounted = usage
                            .output_tokens_accounted
                            .saturating_add(if response.usage.output_tokens_measured {
                                response.usage.output_tokens
                            } else {
                                attempt_request.max_output_tokens.unwrap_or_default()
                            });
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
                            usage.output_tokens_measured = false;
                            usage.output_tokens_accounted = usage
                                .output_tokens_accounted
                                .saturating_add(attempt_output_limit.unwrap_or_default());
                        }
                        if cancelled.load(Ordering::Acquire) {
                            return Err(SchedulerError::Cancelled);
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
                            && usage.provider_calls < budget.max_provider_calls
                            && (!conservative_output
                                || output_limit
                                    .map_or(true, |limit| usage.output_tokens_accounted < limit));
                        #[cfg(debug_assertions)]
                        if retry_policy.enabled && eligible_error && !can_retry {
                            let reason = if usage.provider_calls >= budget.max_provider_calls {
                                "call_budget"
                            } else if conservative_output
                                && output_limit
                                    .is_some_and(|limit| usage.output_tokens_accounted >= limit)
                            {
                                "output_budget"
                            } else {
                                "retry_limit"
                            };
                            eprintln!("[Scheduler][diag] retry_skipped reason={reason}");
                        }
                        if can_retry && self.resilience.eligible(&entry.config.id) {
                            let backoff_ms = self.resilience.backoff_ms(retry_policy, attempt);
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
                            last_provider = Some(entry.config.id.clone());
                            last_error = Some(error);
                            self.resilience.backoff(backoff_ms, cancelled).await?;
                            // The next loop reacquires health, rate and admission.
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
                    for snapshot in self.resilience_snapshot() {
                        let provider = snapshot.provider_id;
                        let cooldown_ms = snapshot.cooldown_remaining_ms;
                        let circuit = snapshot.circuit_state;
                        eprintln!("[Scheduler][diag] no_provider reason=operational_gate provider={provider} circuit={circuit:?} cooldown_ms={cooldown_ms}");
                    }
                }
            }
            if eligible.is_empty() && !request.mode.text_stream() {
                Err(SchedulerError::Provider(ProviderError::UnsupportedMode))
            } else {
                Err(SchedulerError::NoProvider)
            }
        } else if let Some(error) = last_error {
            Err(SchedulerError::Provider(error))
        } else {
            Err(SchedulerError::NoProvider)
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
