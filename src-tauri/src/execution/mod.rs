//! Process-wide native Execution Plane; no agent adapter, IPC or Presentation
//! dependency. Linux runtime in LR-9B. Contracts and authority are never serde.
mod contract;
pub use contract::*;
#[cfg(target_os = "linux")]
mod broker;
#[cfg(target_os = "linux")]
mod os;
#[cfg(target_os = "linux")]
mod pty;
#[cfg(target_os = "linux")]
pub use broker::*;
#[cfg(target_os = "linux")]
pub use pty::*;
#[cfg(all(feature = "lr9b-probe", target_os = "linux"))]
pub(crate) mod probe;
#[cfg(all(test, target_os = "linux"))]
mod tests;
