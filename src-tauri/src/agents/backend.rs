use std::{future::Future, pin::Pin, sync::atomic::AtomicBool};

#[cfg(test)]
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
}

#[cfg(test)]
pub struct MockAgentBackend {
    pub output: String,
    pub emit_event: bool,
}

#[cfg(test)]
impl MockAgentBackend {
    pub fn new(output: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            output: output.into(),
            emit_event: true,
        })
    }
}

#[cfg(test)]
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
