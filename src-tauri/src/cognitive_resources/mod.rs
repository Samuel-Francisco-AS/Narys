//! LR-8.5A descriptions + LR-8.5B pure B1 eligibility and B2 economic decision.
//! No execution, live admission, reservation or spend/debit authority.
//! Callers supply public local identities; never credentials or remote account IDs.
mod allocation;
mod capabilities;
mod catalog;
mod economic_context;
mod economics;
mod facts;
mod ids;
mod lr8;
mod scarcity;
mod scoring;
mod spend;
mod types;

pub use allocation::*;
pub use capabilities::*;
pub use catalog::*;
pub use economic_context::*;
pub use economics::*;
pub use facts::*;
pub use ids::*;
pub use lr8::*;
pub use scarcity::*;
pub use scoring::*;
pub use spend::*;
pub use types::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fix1_tests;

#[cfg(test)]
mod fix2_tests;

#[cfg(test)]
mod fix3_tests;

#[cfg(test)]
mod allocation_tests;

#[cfg(test)]
mod scoring_tests;
