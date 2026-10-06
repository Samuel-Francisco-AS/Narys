//! LR-8.5A: descriptions only. No execution, ranking, admission or spend authority.
//! Callers supply public local identities; never credentials or remote account IDs.
mod capabilities;
mod catalog;
mod economics;
mod facts;
mod ids;
mod lr8;
mod types;

pub use capabilities::*;
pub use catalog::*;
pub use economics::*;
pub use facts::*;
pub use ids::*;
pub use lr8::*;
pub use types::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fix1_tests;
