//! Observational runtime authority. Never consulted by routing/admission.
use super::types::{ProviderError, ProviderUsage};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// Numeric metadata bound, not a commercial quota; exactly representable in JSON/JS.
pub const MAX_FACT_VALUE: u64 = (1_u64 << 53) - 1;
fn unix_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|ms| *ms <= MAX_FACT_VALUE)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    ProviderHeader,
    ProviderResponse,
    UserConfiguration,
    LocalRuntime,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Fact<T> {
    Unknown,
    Known {
        value: T,
        provenance: Provenance,
        observed_at_unix_ms: Option<u64>,
    },
}
impl<T> Fact<T> {
    fn known(value: T, provenance: Provenance) -> Self {
        Self::Known {
            value,
            provenance,
            observed_at_unix_ms: unix_ms(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageDimension {
    Requests,
    InputTokens,
    OutputTokens,
    TotalTokens,
    ThoughtTokens,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaDimension {
    RequestsPerMinute,
    TokensPerMinute,
    RequestsPerDay,
    TokensPerDay,
    Concurrency,
}
impl QuotaDimension {
    const ALL: [Self; 5] = [
        Self::RequestsPerMinute,
        Self::TokensPerMinute,
        Self::RequestsPerDay,
        Self::TokensPerDay,
        Self::Concurrency,
    ];
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Timing {
    DelayMs(u64),
    UnixMs(u64),
}
impl Timing {
    fn valid(self) -> bool {
        match self {
            Self::DelayMs(n) | Self::UnixMs(n) => n <= MAX_FACT_VALUE,
        }
    }
}
/// Fields are independent; remaining is never inferred from observed usage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    pub limit: Fact<u64>,
    pub remaining: Fact<u64>,
    pub reset: Fact<Timing>,
}
impl Default for QuotaSnapshot {
    fn default() -> Self {
        Self {
            limit: Fact::Unknown,
            remaining: Fact::Unknown,
            reset: Fact::Unknown,
        }
    }
}
/// Sum of the reported subset only. reportingRequests identifies completeness.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCounter {
    pub observed: Fact<u64>,
    pub reporting_requests: u64,
    pub saturated: bool,
}
impl Default for UsageCounter {
    fn default() -> Self {
        Self {
            observed: Fact::Unknown,
            reporting_requests: 0,
            saturated: false,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed { code: &'static str },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTelemetrySnapshot {
    pub provider_id: String,
    pub captured_at_unix_ms: Option<u64>,
    pub updated_age_ms: Option<u64>,
    pub usage: BTreeMap<UsageDimension, UsageCounter>,
    pub quotas: BTreeMap<QuotaDimension, QuotaSnapshot>,
    /// Last observed hint (historical, not permission to execute or wait).
    pub retry_hint: Fact<Timing>,
    pub last_outcome: Fact<Outcome>,
}
struct State {
    snapshot: ProviderTelemetrySnapshot,
    updated: Option<Instant>,
}
impl State {
    fn new(id: String) -> Self {
        let mut usage: BTreeMap<_, _> = [
            UsageDimension::Requests,
            UsageDimension::InputTokens,
            UsageDimension::OutputTokens,
            UsageDimension::TotalTokens,
            UsageDimension::ThoughtTokens,
        ]
        .into_iter()
        .map(|d| (d, UsageCounter::default()))
        .collect();
        usage.get_mut(&UsageDimension::Requests).unwrap().observed =
            Fact::known(0, Provenance::LocalRuntime);
        Self {
            snapshot: ProviderTelemetrySnapshot {
                provider_id: id,
                captured_at_unix_ms: None,
                updated_age_ms: None,
                usage,
                quotas: QuotaDimension::ALL
                    .into_iter()
                    .map(|d| (d, QuotaSnapshot::default()))
                    .collect(),
                retry_hint: Fact::Unknown,
                last_outcome: Fact::Unknown,
            },
            updated: None,
        }
    }
    fn touch(&mut self) {
        self.updated = Some(Instant::now());
    }
    fn add(&mut self, dim: UsageDimension, delta: u64, first: bool, source: Provenance) {
        let counter = self.snapshot.usage.get_mut(&dim).unwrap();
        let previous = match counter.observed {
            Fact::Known { value, .. } => value,
            Fact::Unknown => 0,
        };
        counter.saturated |= previous
            .checked_add(delta)
            .is_none_or(|n| n > MAX_FACT_VALUE);
        counter.observed = Fact::known(previous.saturating_add(delta).min(MAX_FACT_VALUE), source);
        if first {
            counter.reporting_requests = counter
                .reporting_requests
                .saturating_add(1)
                .min(MAX_FACT_VALUE);
        }
        self.touch();
    }
}
/// One store owned by the shared Scheduler in ProviderRuntime. No HTTP data is accepted.
pub struct TelemetryStore {
    states: Mutex<BTreeMap<String, State>>,
}
impl TelemetryStore {
    pub fn new(ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            states: Mutex::new(
                ids.into_iter()
                    .filter(|id| valid_id(id))
                    .map(|id| (id.clone(), State::new(id)))
                    .collect(),
            ),
        }
    }
    fn update(&self, id: &str, f: impl FnOnce(&mut State)) {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(state) = states.get_mut(id) {
            f(state);
        }
    }
    pub fn snapshots(&self) -> Vec<ProviderTelemetrySnapshot> {
        let states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let captured = unix_ms();
        states
            .values()
            .map(|state| {
                let mut snapshot = state.snapshot.clone();
                snapshot.captured_at_unix_ms = captured;
                snapshot.updated_age_ms = state.updated.map(|at| {
                    now.saturating_duration_since(at)
                        .as_millis()
                        .min(u64::MAX as u128) as u64
                });
                snapshot
            })
            .collect()
    }
    /// Trusted, normalized update boundary for adapters/configuration. No guessed defaults.
    /// Invalid metadata is ignored as a whole, preserving the last factual observation.
    pub fn observe_quota(
        &self,
        id: &str,
        dim: QuotaDimension,
        limit: Option<u64>,
        remaining: Option<u64>,
        reset: Option<Timing>,
        source: Provenance,
    ) {
        if limit
            .into_iter()
            .chain(remaining)
            .any(|n| n > MAX_FACT_VALUE)
            || limit.zip(remaining).is_some_and(|(l, r)| r > l)
            || reset.is_some_and(|r| !r.valid())
        {
            return;
        }
        self.update(id, |state| {
            state.snapshot.quotas.insert(
                dim,
                QuotaSnapshot {
                    limit: limit.map_or(Fact::Unknown, |n| Fact::known(n, source)),
                    remaining: remaining.map_or(Fact::Unknown, |n| Fact::known(n, source)),
                    reset: reset.map_or(Fact::Unknown, |n| Fact::known(n, source)),
                },
            );
            state.touch();
        });
    }
    pub fn attempt<'a>(&'a self, id: &'a str) -> InvocationObservation<'a> {
        InvocationObservation {
            store: Some(self),
            id,
            attempt: Mutex::new(Attempt::default()),
        }
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
#[derive(Default)]
struct Attempt {
    started: bool,
    usage: BTreeMap<UsageDimension, u64>,
}
/// Ephemeral per-attempt deduplication, never a second provider authority.
/// Adapters must mark started inside the future that actually begins the send,
/// after building/validating the request; no lock is held across await.
pub struct InvocationObservation<'a> {
    store: Option<&'a TelemetryStore>,
    id: &'a str,
    attempt: Mutex<Attempt>,
}
impl InvocationObservation<'_> {
    pub fn disabled() -> Self {
        Self {
            store: None,
            id: "",
            attempt: Mutex::new(Attempt::default()),
        }
    }
    pub fn started(&self) {
        let mut attempt = self.attempt.lock().unwrap_or_else(|p| p.into_inner());
        if attempt.started {
            return;
        }
        attempt.started = true;
        if let Some(store) = self.store {
            store.update(self.id, |s| {
                s.add(UsageDimension::Requests, 1, true, Provenance::LocalRuntime)
            });
        }
    }
    /// Current adapters require all input/output/total counters when usage exists.
    /// calls is deliberately ignored: requests come exclusively from started().
    pub fn usage(&self, usage: ProviderUsage) {
        if !usage.output_tokens_measured
            || usage.total_tokens.is_some_and(|total| {
                u64::from(total) < u64::from(usage.input_tokens) + u64::from(usage.output_tokens)
            })
        {
            return;
        }
        self.observed_usage([
            (
                UsageDimension::InputTokens,
                Some(u64::from(usage.input_tokens)),
            ),
            (
                UsageDimension::OutputTokens,
                Some(u64::from(usage.output_tokens)),
            ),
            (
                UsageDimension::TotalTokens,
                usage.total_tokens.map(u64::from),
            ),
            (
                UsageDimension::ThoughtTokens,
                usage.thought_tokens.map(u64::from),
            ),
        ]);
    }
    pub fn observed_usage(&self, values: impl IntoIterator<Item = (UsageDimension, Option<u64>)>) {
        let mut attempt = self.attempt.lock().unwrap_or_else(|p| p.into_inner());
        if !attempt.started {
            return;
        }
        if let Some(store) = self.store {
            store.update(self.id, |state| {
                for (dim, value) in values {
                    if dim == UsageDimension::Requests {
                        continue;
                    }
                    let Some(value) = value.filter(|n| *n <= MAX_FACT_VALUE) else {
                        continue;
                    };
                    let previous = attempt.usage.get(&dim).copied();
                    // Provider usage is cumulative per invocation, never per chunk.
                    if previous.is_some_and(|p| value <= p) {
                        continue;
                    }
                    attempt.usage.insert(dim, value);
                    state.add(
                        dim,
                        value.saturating_sub(previous.unwrap_or(0)),
                        previous.is_none(),
                        Provenance::ProviderResponse,
                    );
                }
            });
        }
    }
    pub fn retry_hint(&self, ms: Option<u64>) {
        let attempt = self.attempt.lock().unwrap_or_else(|p| p.into_inner());
        if !attempt.started {
            return;
        }
        if let Some(store) = self.store {
            store.update(self.id, |state| {
                state.snapshot.retry_hint = ms
                    .filter(|n| *n <= MAX_FACT_VALUE)
                    .map_or(Fact::Unknown, |n| {
                        Fact::known(Timing::DelayMs(n), Provenance::ProviderHeader)
                    });
                state.touch();
            });
        }
    }
    pub fn finished(&self, error: Option<&ProviderError>) {
        let attempt = self.attempt.lock().unwrap_or_else(|p| p.into_inner());
        if !attempt.started {
            return;
        }
        if let Some(store) = self.store {
            store.update(self.id, |state| {
                state.snapshot.last_outcome = Fact::known(
                    error.map_or(Outcome::Succeeded, |e| Outcome::Failed { code: e.code() }),
                    Provenance::LocalRuntime,
                );
                state.touch();
            });
        }
    }
}
