use std::{future::Future, pin::Pin, sync::atomic::AtomicBool};
use super::types::{ProviderChunk, ProviderError, ProviderRequest, ProviderResponse};

// Boxed future keeps the trait object safe on Rust 1.77.2 without async-trait.
pub type ProviderFuture<'a> = Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderError>> + Send + 'a>>;
pub trait Provider: Send + Sync {
  fn execute<'a>(&'a self, request: &'a ProviderRequest, cancelled: &'a AtomicBool,
    on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send)) -> ProviderFuture<'a>;
}
