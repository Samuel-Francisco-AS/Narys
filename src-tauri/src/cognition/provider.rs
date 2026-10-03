use super::types::{
    InvocationMode, ProviderChunk, ProviderError, ProviderInvocationConfig, ProviderRequest,
    ProviderResponse,
};
use std::{future::Future, pin::Pin, sync::atomic::AtomicBool};

// Boxed future keeps the trait object safe on Rust 1.77.2 without async-trait.
pub type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderError>> + Send + 'a>>;
pub trait Provider: Send + Sync {
    /// Adapter-owned compatibility check, executed by the Scheduler before selection.
    /// The conservative default advertises only the existing text streaming path.
    fn supports_invocation(
        &self,
        invocation: &ProviderInvocationConfig,
        mode: &InvocationMode,
    ) -> bool {
        invocation.valid() && mode.valid() && mode.text_stream()
    }

    /// Local providers may use the default boundary. Remote adapters override this
    /// to mark the HTTP send only after their credential/payload preflight.
    fn execute_observed<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        observation: &'a super::telemetry::InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return Err(ProviderError::Cancelled);
            }
            observation.started();
            let result = self.execute(request, cancelled, on_chunk).await;
            if let Ok(response) = &result {
                observation.usage(response.usage);
            }
            result
        })
    }

    fn execute<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a>;
}
