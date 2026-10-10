//! Scheduling independent of presentation. Use the host Tokio runtime when
//! entered; synchronous clients/tests share a lazy runtime, with no provider boot.
use std::future::Future;
use tokio::{
    runtime::{Handle, Runtime},
    task::JoinHandle,
};
fn shared() -> &'static Runtime {
    static RUNTIME: std::sync::OnceLock<Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("domain runtime"))
}
fn handle() -> Handle {
    Handle::try_current().unwrap_or_else(|_| shared().handle().clone())
}
pub fn spawn<F: Future + Send + 'static>(future: F) -> JoinHandle<F::Output>
where
    F::Output: Send + 'static,
{
    handle().spawn(future)
}
pub fn spawn_blocking<F, R>(function: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    handle().spawn_blocking(function)
}
pub fn block_on<F: Future>(future: F) -> F::Output {
    shared().block_on(future)
}
