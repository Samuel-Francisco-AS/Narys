use super::types::{
    InvocationMode, ProviderChunk, ProviderError, ProviderInvocationConfig, ProviderRequest,
    ProviderResponse,
};
use std::{future::Future, pin::Pin, sync::atomic::AtomicBool};

// Boxed future keeps the trait object safe on Rust 1.77.2 without async-trait.
pub type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderError>> + Send + 'a>>;
pub trait Provider: Send + Sync {
    /// Optional proof for this exact target/payload. Includes all billed tokens,
    /// including hidden output. Current production adapters have no such proof.
    fn token_upper_bound(
        &self,
        _request: &ProviderRequest,
    ) -> Option<super::rate::TokenUpperBound> {
        None
    }
    /// Adapter-owned compatibility check, executed by the Scheduler before selection.
    /// The conservative default advertises only the existing text streaming path.
    fn supports_invocation(
        &self,
        invocation: &ProviderInvocationConfig,
        mode: &InvocationMode,
    ) -> bool {
        invocation.valid() && mode.valid() && mode.text_stream()
    }

    /// Conservative default makes no claim about remote invocation or usage.
    /// Adapters opt in explicitly after their credential/payload preflight.
    fn execute_observed<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        _observation: &'a super::telemetry::InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        self.execute(request, cancelled, on_chunk)
    }

    fn execute<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a>;
}
