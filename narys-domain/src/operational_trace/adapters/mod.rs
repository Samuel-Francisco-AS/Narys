//! Source-aware, passive projections. No functional result depends on delivery.
mod agent;
mod scheduler;
mod task;

use super::*;
pub use agent::*;
pub use scheduler::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
pub use task::*;

/// Replaceable native publisher; never a functional sink or a subscriber callback.
pub trait TracePublisher: Send + Sync {
    fn publish(&self, draft: EventDraft) -> Result<(), TraceError>;
}
impl TracePublisher for OperationalTraceBus {
    fn publish(&self, draft: EventDraft) -> Result<(), TraceError> {
        OperationalTraceBus::publish(self, draft).map(|_| ())
    }
}
#[derive(Clone)]
pub struct PassiveTracePublisher(Arc<dyn TracePublisher>);
impl PassiveTracePublisher {
    pub fn production() -> Self {
        Self(OperationalTraceBus::process_wide())
    }
    #[cfg(any(test, feature = "desktop-tests"))]
    pub fn new(publisher: Arc<dyn TracePublisher>) -> Self {
        Self(publisher)
    }
    pub fn publish(&self, draft: Result<EventDraft, TraceError>) {
        if let Ok(draft) = draft {
            // Third-party fixtures/observers have no failure authority either.
            let _ =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.0.publish(draft)));
        }
    }
    pub fn state(
        &self,
        provenance: &Result<Provenance, TraceError>,
        kind: StateKind,
        code: &'static str,
        detail: &str,
    ) {
        self.publish((|| {
            EventDraft::new(
                provenance.clone()?,
                OperationalKind::State {
                    kind,
                    code: TraceId::new(code)?,
                    detail: TraceText::new(detail)?,
                },
            )
        })());
    }
    pub fn critical(
        &self,
        provenance: &Result<Provenance, TraceError>,
        kind: CriticalKind,
        code: &'static str,
    ) {
        self.publish((|| {
            EventDraft::new(
                provenance.clone()?,
                OperationalKind::Critical {
                    kind,
                    code: TraceId::new(code)?,
                    message: TraceText::new("")?,
                },
            )
        })());
    }
    /// Call only for content explicitly authorized by the source's exposure policy.
    pub fn text(
        &self,
        provenance: &Result<Provenance, TraceError>,
        channel: TextChannel,
        text: &str,
    ) {
        let Ok(provenance) = provenance else {
            return;
        };
        for fragment in fragments(text) {
            self.publish((|| {
                EventDraft::new(
                    provenance.clone(),
                    OperationalKind::TextDelta {
                        channel,
                        text: TraceText::new(fragment)?,
                    },
                )
            })());
        }
    }
}

/// Lazy, allocation-free slicing; no collection proportional to delta length.
/// No normalization, omitted suffix, or split inside a UTF-8 code point.
pub fn fragments(mut text: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let mut end = text.len().min(MAX_TEXT_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let (fragment, remaining) = text.split_at(end);
        text = remaining;
        Some(fragment)
    })
}
fn provenance(
    source_type: SourceType,
    id: &str,
    task_id: Option<crate::luna::task::TaskId>,
    subtask: Option<&str>,
    correlation: Option<TraceId>,
) -> Result<Provenance, TraceError> {
    Ok(Provenance {
        source: TraceSource {
            source_type,
            id: TraceId::new(id)?,
            instance: None,
        },
        task_id,
        subtask_id: subtask.map(TraceId::new).transpose()?,
        coalescing_key: correlation.clone(),
        correlation_id: correlation,
    })
}
/// Local identity only; never a bus sequence, PID, remote ID, or cognitive input.
fn next_call(counter: &AtomicU64, prefix: &str) -> Option<TraceId> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .ok()
        .and_then(|n| TraceId::new(&format!("{prefix}-{}", n + 1)).ok())
}

#[cfg(any(test, feature = "desktop-tests"))]
pub mod tests;
