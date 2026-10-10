use super::{
    registry::ProviderRegistry,
    types::{
        InvocationMode, ProviderCapabilities, ProviderChunk, ProviderError, ProviderRequest,
        ProviderSelection, ProviderTarget, ProviderTaskRequest, RetryPolicy, SchedulerError,
        SchedulerUsage, TaskBudget, TaskResult,
    },
};
use crate::cognitive_resources::{ProviderAutoAllocator, ProviderBridgeError};
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
        score: Option<i64>,
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

/// The shared route representation. Auto scores come solely from B2; explicit
/// selections never enter the allocator. Owned targets freeze model/effort.
#[derive(Clone, Debug)]
pub(crate) struct ProviderRouteEntry {
    pub target: ProviderTarget,
    pub score: Option<i64>,
    variant: Option<crate::cognitive_resources::AllocationVariant>,
}

/// Only Scheduler route resolution constructs this pair. Checkpoints and Worker
/// invocation consume the same immutable selection, never a caller variant.
#[derive(Clone, Debug)]
pub struct PinnedProviderAllocation {
    target: ProviderTarget,
    variant: crate::cognitive_resources::AllocationVariant,
    selection: AllocationSelection,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllocationSelection {
    pub mode: super::policy::RoutingMode,
    pub score: Option<i64>,
}
impl PinnedProviderAllocation {
    pub fn selection(&self) -> &AllocationSelection {
        &self.selection
    }
    pub fn target(&self) -> &ProviderTarget {
        &self.target
    }
    pub fn variant(&self) -> &crate::cognitive_resources::AllocationVariant {
        &self.variant
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProviderChainPurpose {
    Execution,
    Ranking,
    BoundaryRanking,
}

pub struct Scheduler {
    pub(super) admission: super::admission::AdmissionController,
    registry: ProviderRegistry,
    auto_allocator: Result<ProviderAutoAllocator, ProviderBridgeError>,
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
        // An invalid allocation catalog fails Auto closed, while explicit modes
        // retain their pre-B3 construction/execution contract.
        let auto_allocator = ProviderAutoAllocator::production(&registry);
        Ok(Self {
            auto_allocator,
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
    /// Test catalog injection only; allocation policy belongs to the request.
    #[cfg(test)]
    pub(crate) fn with_auto_allocator(mut self, allocator: ProviderAutoAllocator) -> Self {
        self.auto_allocator = Ok(allocator);
        self
    }
    pub fn ranked_provider_ids(
        &self,
        selection: &ProviderSelection,
        targets: &[ProviderTarget],
        required: &ProviderCapabilities,
        mode: &InvocationMode,
        allocation_policy: Option<&crate::cognitive_resources::AllocationRuntimePolicy>,
    ) -> Result<Vec<String>, SchedulerError> {
        Ok(self
            .ranked_provider_targets(selection, targets, required, mode, allocation_policy)?
            .into_iter()
            .map(|target| target.provider_id)
            .collect())
    }
    /// TaskGraph needs the chosen variants as well as IDs. Both read-only views
    /// use the same chain engine and existing operational eligibility check.
    pub(crate) fn ranked_provider_targets(
        &self,
        selection: &ProviderSelection,
        targets: &[ProviderTarget],
        required: &ProviderCapabilities,
        mode: &InvocationMode,
        allocation_policy: Option<&crate::cognitive_resources::AllocationRuntimePolicy>,
    ) -> Result<Vec<ProviderTarget>, SchedulerError> {
        let chain = self.resolve_provider_chain(
            selection,
            targets,
            required,
            mode,
            allocation_policy,
            None,
            0,
            ProviderChainPurpose::Ranking,
        )?;
        let targets: Vec<_> = chain
            .into_iter()
            .filter(|route| self.resilience.eligible(&route.target.provider_id))
            .map(|route| route.target)
            .collect();
        if targets.is_empty() {
            Err(SchedulerError::NoProvider)
        } else {
            Ok(targets)
        }
    }
    /// Fresh read-only decision for a new TaskGraph unit. Auto retains B's exact
    /// variant; explicit routing binds only local runtime identity and invocation,
    /// without consulting economic policy/catalog or changing its order.
    pub(crate) fn ranked_provider_allocations(
        &self,
        selection: &ProviderSelection,
        targets: &[ProviderTarget],
        allocation_policy: Option<&crate::cognitive_resources::AllocationRuntimePolicy>,
    ) -> Result<Vec<PinnedProviderAllocation>, SchedulerError> {
        use crate::cognitive_resources::*;
        let chain = self.resolve_provider_chain(
            selection,
            targets,
            &ProviderCapabilities::text_stream(),
            &InvocationMode::default(),
            allocation_policy,
            None,
            0,
            ProviderChainPurpose::BoundaryRanking,
        )?;
        let mut pins = Vec::new();
        for route in chain
            .into_iter()
            .filter(|r| self.resilience.eligible(&r.target.provider_id))
        {
            let target = route.target;
            let variant = match route.variant {
                Some(variant) => variant,
                None => AllocationVariant {
                    resource_id: ResourceId::new(&target.provider_id)
                        .map_err(|_| SchedulerError::InvalidTargetConfig)?,
                    access_path: AccessPath::new("provider_runtime").expect("local identity"),
                    billing_domain_id: BillingDomainId::new(&target.provider_id)
                        .map_err(|_| SchedulerError::InvalidTargetConfig)?,
                    model_id: ModelId::new(&target.invocation.model)
                        .map_err(|_| SchedulerError::InvalidTargetConfig)?,
                    effort: target
                        .invocation
                        .thinking_level
                        .map(EffortId::from_thinking_level),
                },
            };
            if variant.resource_id.as_str() != target.provider_id
                || variant.model_id.as_str() != target.invocation.model
                || variant.effort.as_ref().map(EffortId::as_str)
                    != target
                        .invocation
                        .thinking_level
                        .map(super::policy::ThinkingLevel::as_str)
            {
                return Err(SchedulerError::InvalidTargetConfig);
            }
            let mode = match selection {
                ProviderSelection::Fixed(_) => super::policy::RoutingMode::Fixed,
                ProviderSelection::Preferred => super::policy::RoutingMode::Preferred,
                ProviderSelection::Auto => super::policy::RoutingMode::Auto,
            };
            pins.push(PinnedProviderAllocation {
                target,
                variant,
                selection: AllocationSelection {
                    mode,
                    score: route.score,
                },
            });
        }
        if pins.is_empty() {
            Err(SchedulerError::NoProvider)
        } else {
            Ok(pins)
        }
    }

    /// Single ordering engine for run, read-only ranking and TaskGraph selection.
    /// Capture LR-8 once per Auto plan. Never refresh live rate state for ranking.
    fn resolve_provider_chain(
        &self,
        selection: &ProviderSelection,
        targets: &[ProviderTarget],
        required: &ProviderCapabilities,
        mode: &InvocationMode,
        allocation_policy: Option<&crate::cognitive_resources::AllocationRuntimePolicy>,
        affinity_key: Option<&str>,
        bytes: usize,
        purpose: ProviderChainPurpose,
    ) -> Result<Vec<ProviderRouteEntry>, SchedulerError> {
        if !mode.valid() || targets.is_empty() || targets.len() > super::policy::MAX_TARGETS {
            return Err(SchedulerError::InvalidTargetConfig);
        }
        let mut ids = HashSet::new();
        for target in targets {
            if !target.invocation.valid() || !ids.insert(&target.provider_id) {
                return Err(SchedulerError::InvalidTargetConfig);
            }
        }
        if matches!(selection, ProviderSelection::Auto) {
            let allocation_policy = allocation_policy.ok_or(SchedulerError::InvalidTargetConfig)?;
            let affinity = affinity_key.and_then(|key| {
                self.affinities
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(key)
                    .map(str::to_owned)
            });
            // Affinity outside the authorized provider universe is not evidence
            // of continuity or switching cost for this request.
            let affinity = affinity.filter(|id| targets.iter().any(|t| &t.provider_id == id));
            let telemetry = self.telemetry.snapshots();
            let rate = self.rate.read_only_snapshots();
            let plan = self
                .auto_allocator
                .as_ref()
                .map_err(|e| e.scheduler_error())?
                .plan_for_boundary(
                    &self.registry,
                    allocation_policy,
                    targets,
                    *required,
                    mode,
                    affinity.as_deref(),
                    bytes,
                    &telemetry,
                    &rate,
                )
                .map_err(|e| e.scheduler_error())?;
            if plan.entries().is_empty() {
                // Only an otherwise eligible paid candidate excluded exclusively
                // by spend authorization proves economic continuation is possible.
                let spend_block = plan.exclusions().iter().any(|e| {
                    self.resilience.eligible(e.variant.resource_id.as_str())
                        && matches!(&e.reason,
                    crate::cognitive_resources::ProviderVariantExclusionReason::Economic { reasons }
                    if !reasons.is_empty() && reasons.iter().all(|r| matches!(r,
                        crate::cognitive_resources::EconomicExclusion::PaidUseDenied |
                        crate::cognitive_resources::EconomicExclusion::PaidBudgetExceeded)))
                });
                return Err(
                    if purpose == ProviderChainPurpose::BoundaryRanking && spend_block {
                        SchedulerError::EconomicAuthorizationRequired
                    } else {
                        SchedulerError::NoProvider
                    },
                );
            }
            return Ok(plan
                .entries()
                .iter()
                .map(|entry| ProviderRouteEntry {
                    target: entry.target.clone(),
                    score: Some(entry.score),
                    variant: Some(entry.variant.clone()),
                })
                .collect());
        }
        // Fixed/Preferred retain their original invocation and policy order,
        // bypassing catalog, expansion, economics, scarcity and paid policy.
        let mut chain = Vec::new();
        for target in targets {
            if let ProviderSelection::Fixed(id) = selection {
                if &target.provider_id != id {
                    continue;
                }
            }
            let entry = self
                .registry
                .get(&target.provider_id)
                .ok_or(SchedulerError::NoProvider)?;
            if !entry.config.enabled {
                if matches!(
                    purpose,
                    ProviderChainPurpose::Ranking | ProviderChainPurpose::BoundaryRanking
                ) {
                    continue;
                }
                return Err(SchedulerError::NoProvider);
            }
            if matches!(
                purpose,
                ProviderChainPurpose::Ranking | ProviderChainPurpose::BoundaryRanking
            ) {
                // Preserve the pre-B3 explicit ranking projection: registered
                // enabled runtimes with the required capabilities, policy order.
                // Exact invocation validation remains at the execution boundary.
                if !entry.config.capabilities.supports(required) {
                    continue;
                }
            } else if !entry.config.capabilities.supports(required)
                || !entry.provider.supports_invocation(&target.invocation, mode)
            {
                if mode.text_stream() {
                    return Err(SchedulerError::NoProvider);
                }
                continue;
            }
            chain.push(ProviderRouteEntry {
                target: target.clone(),
                score: None,
                variant: None,
            });
        }
        Ok(chain)
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
        // Frozen once before the first attempt. Retries and fallback consume
        // these same targets/scores; 429 cannot replan or introduce paid paths.
        let candidates = self.resolve_provider_chain(
            &request.selection,
            &request.targets,
            &request.required_capabilities,
            &request.mode,
            request.allocation_policy.as_ref(),
            request.affinity_key.as_deref(),
            request.estimated_context_bytes,
            ProviderChainPurpose::Execution,
        )?;
        let mut used_any = false;
        for (index, route) in candidates.iter().enumerate() {
            let entry = self
                .registry
                .get(&route.target.provider_id)
                .ok_or(SchedulerError::NoProvider)?;
            let target = &route.target;
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
                        ProviderSelection::Auto => "auto_allocator",
                    },
                    score: route.score,
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
                if candidates.is_empty() {
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
            if candidates.is_empty() && !request.mode.text_stream() {
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
                    &InvocationMode::default(),
                    Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
                        crate::cognitive_resources::provider_allocation_default(),
                        None
                    ))
                    .as_ref(),
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
                    &InvocationMode::default(),
                    Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
                        crate::cognitive_resources::provider_allocation_default(),
                        None
                    ))
                    .as_ref(),
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
                    &InvocationMode::default(),
                    Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
                        crate::cognitive_resources::provider_allocation_default(),
                        None
                    ))
                    .as_ref(),
                )
                .unwrap(),
            vec!["a", "b", "c"]
        );
    }
}
