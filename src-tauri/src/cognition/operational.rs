//! Memory-only operational view. Each authority supplies an internally coherent
//! snapshot (admission per provider); aggregation is deliberately NOT globally
//! atomic. Locks are acquired independently, never held across authorities.
use super::{
    admission::AdmissionSnapshot, rate::RateSnapshot, resilience::ResilienceSnapshot,
    scheduler::Scheduler, telemetry::ProviderTelemetrySnapshot,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOperationalSnapshot {
    pub captured_at_unix_ms: Option<u64>,
    pub telemetry: Vec<ProviderTelemetrySnapshot>,
    pub admission: Vec<AdmissionSnapshot>,
    pub rate: Vec<RateSnapshot>,
    pub resilience: Vec<ResilienceSnapshot>,
}
impl Scheduler {
    /// Does not invoke adapters, catalog/credential presence or durable storage.
    pub fn operational_snapshot(&self) -> ProviderOperationalSnapshot {
        let telemetry = self.telemetry_snapshot();
        ProviderOperationalSnapshot {
            captured_at_unix_ms: telemetry.first().and_then(|s| s.captured_at_unix_ms),
            telemetry,
            admission: self.admission_snapshot(),
            rate: self.rate.read_only_snapshots(),
            resilience: self.resilience_snapshot(),
        }
    }
}
