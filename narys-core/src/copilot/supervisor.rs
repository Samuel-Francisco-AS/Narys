//! One Core-owned runtime. No worker or polling exists until an admitted demand.
use crate::agents::lifecycle::{AgentLifecycleOperation, AgentRuntimeState};
use serde::Serialize;
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::Notify;

pub type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, &'static str>> + Send + 'a>>;
/// Contain a backend unwind without abandoning the lease/cleanup owner.
struct CatchPoll<F: Future> {
    future: Pin<Box<F>>,
}
impl<F: Future> Future for CatchPoll<F> {
    type Output = Result<F::Output, ()>;
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.future.as_mut().poll(cx)
        })) {
            Ok(std::task::Poll::Ready(v)) => std::task::Poll::Ready(Ok(v)),
            Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
            Err(_) => std::task::Poll::Ready(Err(())),
        }
    }
}
async fn contain<F: Future>(future: F) -> Result<F::Output, ()> {
    CatchPoll {
        future: Box::pin(future),
    }
    .await
}
#[derive(Default)]
pub struct Cancellation {
    requested: AtomicBool,
    changed: Notify,
}
impl Cancellation {
    pub fn cancel(&self) {
        self.requested.store(true, Ordering::Release);
        self.changed.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
    pub async fn wait(&self) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}
#[derive(Clone)]
pub struct SessionInvocation {
    pub operation: AgentLifecycleOperation,
    pub directory: PathBuf,
    pub provider_session_id: Option<String>,
    pub expected_history_anchor: Option<String>,
}
#[derive(Debug)]
pub struct SessionReceipt {
    pub provider_session_id: String,
    pub observation_gaps: u64,
    pub history_anchor: Option<String>,
}
pub trait ManagedRuntime: Send + Sync {
    fn session<'a>(
        &'a self,
        invocation: &'a SessionInvocation,
        cancel: &'a Cancellation,
        progress: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> RuntimeFuture<'a, SessionReceipt>;
    fn stop(&self) -> RuntimeFuture<'_, ()>;
    fn process_id(&self) -> Option<u32>;
    fn healthy(&self) -> bool;
    fn force_stop(&self) {}
    fn ownership_ref(&self) -> Option<String> {
        None
    }
}
/// Positive startup evidence is independent of the sanitized functional error.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupSafety {
    NoProcessLaunched,
    CleanupVerified,
    CleanupUnverified,
    PersistenceUncertain,
}
#[derive(Clone, Debug, Serialize)]
pub struct StartupFailure {
    pub code: &'static str,
    pub safety: StartupSafety,
    pub safety_error: Option<&'static str>,
    pub runtime_ref: Option<String>,
}
impl StartupFailure {
    pub fn no_process(code: &'static str) -> Self {
        Self {
            code,
            safety: StartupSafety::NoProcessLaunched,
            safety_error: None,
            runtime_ref: None,
        }
    }
    pub fn uncertain(code: &'static str) -> Self {
        Self {
            code,
            safety: StartupSafety::CleanupUnverified,
            safety_error: None,
            runtime_ref: None,
        }
    }
    pub fn persistence(code: &'static str) -> Self {
        Self {
            code,
            safety: StartupSafety::PersistenceUncertain,
            safety_error: Some(code),
            runtime_ref: None,
        }
    }
    pub fn cleanup_verified(&self) -> bool {
        matches!(
            self.safety,
            StartupSafety::NoProcessLaunched | StartupSafety::CleanupVerified
        )
    }
}
pub type StartupFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Arc<dyn ManagedRuntime>, StartupFailure>> + Send + 'a>>;
pub trait RuntimeFactory: Send + Sync {
    fn start(&self) -> StartupFuture<'_>;
}
#[derive(Clone, Debug, Serialize)]
pub struct SupervisorSnapshot {
    pub state: AgentRuntimeState,
    pub leases: usize,
    pub generation: u64,
    pub process_id: Option<u32>,
    pub cleanup_verified: bool,
    pub last_error: Option<&'static str>,
    pub admission_closed: bool,
    pub runtime_ref: Option<String>,
    pub startup_failure: Option<StartupFailure>,
}
struct Inner {
    runtime: Option<Arc<dyn ManagedRuntime>>,
    leases: usize,
}
pub struct AgentRuntimeSupervisor {
    factory: Arc<dyn RuntimeFactory>,
    inner: tokio::sync::Mutex<Inner>,
    view: Mutex<SupervisorSnapshot>,
    closed: AtomicBool,
    drained: Notify,
}
pub struct RuntimeOutcome {
    pub result: Result<SessionReceipt, &'static str>,
    pub cleanup_verified: bool,
    pub runtime_ref: Option<String>,
    pub startup_safety: Option<StartupSafety>,
}
impl AgentRuntimeSupervisor {
    pub fn new(factory: Arc<dyn RuntimeFactory>) -> Arc<Self> {
        Arc::new(Self {
            factory,
            inner: tokio::sync::Mutex::new(Inner {
                runtime: None,
                leases: 0,
            }),
            view: Mutex::new(SupervisorSnapshot {
                state: AgentRuntimeState::Dormant,
                leases: 0,
                generation: 0,
                process_id: None,
                cleanup_verified: true,
                last_error: None,
                admission_closed: false,
                runtime_ref: None,
                startup_failure: None,
            }),
            closed: AtomicBool::new(false),
            drained: Notify::new(),
        })
    }
    pub fn snapshot(&self) -> SupervisorSnapshot {
        self.view.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn state(
        &self,
        state: AgentRuntimeState,
        leases: usize,
        pid: Option<u32>,
        cleanup: bool,
        error: Option<&'static str>,
    ) {
        let mut v = self.view.lock().unwrap_or_else(|p| p.into_inner());
        v.state = state;
        v.leases = leases;
        v.process_id = pid;
        v.cleanup_verified = cleanup;
        v.last_error = error;
    }
    /// The inner owner task cannot be aborted by dropping a client future.
    pub async fn run(
        self: &Arc<Self>,
        invocation: SessionInvocation,
        cancel: Arc<Cancellation>,
        progress: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> RuntimeOutcome {
        let this = self.clone();
        let control = cancel.clone();
        let gaps = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let observed = gaps.clone();
        let progress: Arc<dyn Fn(&'static str) + Send + Sync> = Arc::new(move |code| {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| progress(code))).is_err() {
                observed.fetch_add(1, Ordering::Relaxed);
            }
        });
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let mut outcome = this.run_owned(invocation, control, progress).await;
            if let Ok(receipt) = &mut outcome.result {
                receipt.observation_gaps = receipt
                    .observation_gaps
                    .saturating_add(gaps.load(Ordering::Relaxed));
            }
            let _ = tx.send(outcome);
        });
        struct CancelOnDrop(Option<Arc<Cancellation>>);
        impl Drop for CancelOnDrop {
            fn drop(&mut self) {
                if let Some(c) = &self.0 {
                    c.cancel();
                }
            }
        }
        let mut guard = CancelOnDrop(Some(cancel));
        let outcome = rx.await.unwrap_or(RuntimeOutcome {
            result: Err("runtime_owner_lost"),
            cleanup_verified: false,
            runtime_ref: None,
            startup_safety: None,
        });
        guard.0 = None;
        outcome
    }
    async fn run_owned(
        self: &Arc<Self>,
        invocation: SessionInvocation,
        cancel: Arc<Cancellation>,
        progress: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> RuntimeOutcome {
        let rejected = |code| RuntimeOutcome {
            result: Err(code),
            cleanup_verified: true,
            runtime_ref: None,
            startup_safety: None,
        };
        let mut inner = tokio::select! { biased; _=cancel.wait()=>return rejected("cancelled"), lock=self.inner.lock()=>lock };
        if cancel.is_cancelled() {
            return rejected("cancelled");
        }
        if self.closed.load(Ordering::Acquire) {
            return rejected("supervisor_stopping");
        }
        if !self.snapshot().cleanup_verified && inner.runtime.is_none() {
            return RuntimeOutcome {
                result: Err("cleanup_not_verified"),
                cleanup_verified: false,
                runtime_ref: None,
                startup_safety: None,
            };
        }
        if inner.leases >= 2 {
            return rejected("agent_concurrency_limit");
        }
        if inner.runtime.is_none() {
            {
                let mut v = self.view.lock().unwrap_or_else(|p| p.into_inner());
                v.generation = v.generation.saturating_add(1);
                v.startup_failure = None;
            }
            self.view
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .runtime_ref = None;
            self.state(AgentRuntimeState::Starting, 0, None, false, None);
            progress("runtime_starting");
            // Start must complete its own bounded cleanup on failure. Do not drop
            // Client::start at a select boundary and mistake a kill for a reap.
            match contain(self.factory.start())
                .await
                .unwrap_or(Err(StartupFailure::uncertain("runtime_owner_panicked")))
            {
                Ok(runtime) => inner.runtime = Some(runtime),
                Err(failure) => {
                    let verified = failure.cleanup_verified();
                    self.state(
                        AgentRuntimeState::Faulted,
                        0,
                        None,
                        verified,
                        Some(failure.code),
                    );
                    let mut view = self.view.lock().unwrap_or_else(|p| p.into_inner());
                    view.runtime_ref = failure.runtime_ref.clone();
                    view.startup_failure = Some(failure.clone());
                    return RuntimeOutcome {
                        result: Err(failure.code),
                        cleanup_verified: verified,
                        runtime_ref: failure.runtime_ref,
                        startup_safety: Some(failure.safety),
                    };
                }
            }
            self.view
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .runtime_ref = inner.runtime.as_ref().and_then(|r| r.ownership_ref());
            self.state(
                AgentRuntimeState::Ready,
                0,
                inner.runtime.as_ref().and_then(|r| r.process_id()),
                false,
                None,
            );
            progress("runtime_ready");
        }
        let runtime = inner.runtime.as_ref().unwrap().clone();
        inner.leases += 1;
        self.state(
            AgentRuntimeState::Busy,
            inner.leases,
            runtime.process_id(),
            false,
            None,
        );
        drop(inner);
        let result = if cancel.is_cancelled() || self.closed.load(Ordering::Acquire) {
            Err("cancelled")
        } else if !runtime.healthy() {
            Err("runtime_died")
        } else {
            contain(runtime.session(&invocation, &cancel, progress.clone()))
                .await
                .unwrap_or(Err("runtime_operation_panicked"))
        };
        // Cleanup is serialized with acquire; a new demand never races stop.
        let mut inner = self.inner.lock().await;
        inner.leases -= 1;
        let mut cleanup_verified = false;
        let mut result = result;
        if inner.leases == 0 {
            self.state(
                AgentRuntimeState::Stopping,
                0,
                runtime.process_id(),
                false,
                None,
            );
            progress("runtime_stopping");
            let stopped = match contain(runtime.stop()).await {
                Ok(result) => result,
                Err(_) => {
                    runtime.force_stop();
                    contain(runtime.stop())
                        .await
                        .unwrap_or(Err("runtime_cleanup_incomplete"))
                }
            };
            match stopped {
                Ok(()) => {
                    cleanup_verified = true;
                    self.state(AgentRuntimeState::Dormant, 0, None, true, None);
                    progress("runtime_stopped");
                }
                Err(code) => {
                    cleanup_verified = code == "sdk_shutdown_recovered";
                    self.state(
                        AgentRuntimeState::Faulted,
                        0,
                        None,
                        cleanup_verified,
                        Some(code),
                    );
                    result = Err(code);
                    progress("runtime_faulted");
                }
            }
            inner.runtime = None;
        } else {
            // Other leases deliberately keep the runtime alive. This operation
            // has detached its session; runtime cleanup is pending, not asserted.
            self.state(
                AgentRuntimeState::Busy,
                inner.leases,
                runtime.process_id(),
                false,
                None,
            );
        }
        self.drained.notify_waiters();
        RuntimeOutcome {
            result,
            cleanup_verified,
            runtime_ref: runtime.ownership_ref(),
            startup_safety: None,
        }
    }
    pub fn fault_cleanup(&self, code: &'static str) {
        self.state(AgentRuntimeState::Faulted, 0, None, false, Some(code));
    }
    pub fn acknowledge_recovered_cleanup(&self) -> Result<(), &'static str> {
        let mut view = self.view.lock().unwrap_or_else(|p| p.into_inner());
        if view.leases != 0
            || matches!(
                view.state,
                AgentRuntimeState::Starting | AgentRuntimeState::Stopping | AgentRuntimeState::Busy
            )
        {
            return Err("agent_runtime_busy");
        }
        view.cleanup_verified = true;
        view.state = AgentRuntimeState::Dormant;
        view.last_error = None;
        view.startup_failure = None;
        Ok(())
    }
    pub fn close_admission(&self) {
        self.closed.store(true, Ordering::Release);
        self.view
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .admission_closed = true;
    }
    pub async fn shutdown(&self) -> Result<(), &'static str> {
        self.close_admission();
        // Service cancels each admitted control before calling this barrier.
        loop {
            let notified = self.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let inner = self.inner.lock().await;
            if inner.leases == 0 && inner.runtime.is_none() {
                return if self.snapshot().cleanup_verified {
                    Ok(())
                } else {
                    Err("cleanup_not_verified")
                };
            }
            drop(inner);
            notified.await;
        }
    }
}
