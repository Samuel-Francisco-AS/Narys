use std::{future::Future, pin::Pin, sync::atomic::AtomicBool};

use std::sync::Arc;

use super::types::{AgentError, AgentEvent, AgentRequest, AgentResult};

pub type AgentFuture<'a> =
    Pin<Box<dyn Future<Output = Result<AgentResult, AgentError>> + Send + 'a>>;

/// Backends report facts, not synthetic progress or unvalidated partial output.
/// Event callbacks must return promptly. A callback error means the consumer
/// is unavailable; active work must stop safely and resources must be reclaimed.
/// Cancellation and completion races are resolved by each backend's observed
/// terminal state. Dropping the future must also reclaim owned resources.
pub trait AgentBackend: Send + Sync {
    fn execute<'a>(
        &'a self,
        request: &'a AgentRequest,
        cancelled: &'a AtomicBool,
        on_event: &'a mut (dyn FnMut(AgentEvent) -> Result<(), AgentError> + Send),
    ) -> AgentFuture<'a>;

    fn execute_observed<'a>(
        &'a self,
        request: &'a AgentRequest,
        cancelled: &'a AtomicBool,
        on_event: &'a mut (dyn FnMut(AgentEvent) -> Result<(), AgentError> + Send),
        trace: Arc<dyn super::trace::AgentTraceSink>,
    ) -> AgentFuture<'a> {
        Box::pin(async move {
            let mut tap = |event: AgentEvent| {
                super::trace::observe_passively(trace.as_ref(), super::trace::AgentTraceObservation::Lifecycle(&event));
                on_event(event)
            };
            self.execute(request, cancelled, &mut tap).await
        })
    }

}

#[cfg(any(test, feature = "desktop-tests"))]
pub struct MockAgentBackend {
    pub output: String,
    pub emit_event: bool,
}

#[cfg(any(test, feature = "desktop-tests"))]
impl MockAgentBackend {
    pub fn new(output: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            output: output.into(),
            emit_event: true,
        })
    }
}

#[cfg(any(test, feature = "desktop-tests"))]
impl AgentBackend for MockAgentBackend {
    fn execute<'a>(
        &'a self,
        request: &'a AgentRequest,
        cancelled: &'a AtomicBool,
        on_event: &'a mut (dyn FnMut(AgentEvent) -> Result<(), AgentError> + Send),
    ) -> AgentFuture<'a> {
        Box::pin(async move {
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return Err(AgentError::Cancelled);
            }
            if request.objective.trim().is_empty() {
                return Err(AgentError::InvalidRequest);
            }
            if self.emit_event {
                on_event(AgentEvent::Output {
                    text: self.output.clone(),
                })?;
            }
            Ok(AgentResult {
                output: self.output.clone(),
            })
        })
    }
}
