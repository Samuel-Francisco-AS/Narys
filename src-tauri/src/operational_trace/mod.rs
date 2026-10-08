//! Passive, ephemeral observation of explicitly published, already authorized
//! and sanitized content. No inference, execution, persistence or Presentation.
//! The sole production instance is registered at the composition root; adapters
//! are deliberately deferred to LR-9D.
mod bus;
mod coalesce;
mod contract;

pub use bus::*;
pub use coalesce::*;
pub use contract::*;

#[cfg(test)]
mod tests;
