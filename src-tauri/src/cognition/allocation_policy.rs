//! B4 persistence DTO and the only routing/allocation snapshot boundary.
use super::policy::{self, CognitiveRole, CognitiveRolePolicy, RoutingMode};
use crate::{cognitive_resources::*, persistence::database::PersistenceError};
use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaidUseMode {
    Deny,
    AllowKnownCostWithinBudget,
}
impl PaidUseMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::AllowKnownCostWithinBudget => "allow_known_cost_within_budget",
        }
    }
}
/// Untrusted config, deliberately separate from the runtime aggregate. Deserializing
/// this DTO never creates validated money, reserve thresholds or a quality floor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CognitiveRoleAllocationPolicy {
    pub role: CognitiveRole,
    pub allocation_profile: AllocationProfile,
    pub variant_selection_mode: VariantSelectionMode,
    pub minimum_cognitive_tier: Option<u16>,
    pub paid_use_policy: PaidUseMode,
    pub max_paid_currency: Option<String>,
    pub max_paid_micros: Option<u64>,
    pub reduced_below_percent: Option<u8>,
    pub reserve_below_percent: Option<u8>,
}
impl CognitiveRoleAllocationPolicy {
    pub fn to_runtime(&self) -> Result<AllocationRuntimePolicy, &'static str> {
        let invalid = || "allocation_policy_invalid";
        let minimum = self
            .minimum_cognitive_tier
            .map(CognitiveTier::new)
            .transpose()
            .map_err(|_| invalid())?;
        let paid_use = match (
            self.paid_use_policy,
            self.max_paid_currency.as_deref(),
            self.max_paid_micros,
        ) {
            (PaidUseMode::Deny, None, None) => PaidUsePolicy::Deny,
            (PaidUseMode::AllowKnownCostWithinBudget, Some(currency), Some(micros)) => {
                PaidUsePolicy::AllowKnownCostWithinBudget {
                    budget: MonetaryAmount::new(currency, micros).map_err(|_| invalid())?,
                }
            }
            _ => return Err(invalid()),
        };
        let reserve = match (self.reduced_below_percent, self.reserve_below_percent) {
            (None, None) => None,
            (Some(reduced), Some(reserve)) => {
                Some(ReservePolicy::new(reduced, reserve).map_err(|_| invalid())?)
            }
            _ => return Err(invalid()),
        };
        Ok(AllocationRuntimePolicy::new(
            AllocationPolicy {
                profile: self.allocation_profile,
                variant_selection_mode: self.variant_selection_mode,
                paid_use,
                reserve,
            },
            minimum,
        ))
    }
}

pub fn load(
    conn: &Connection,
    role: CognitiveRole,
) -> Result<CognitiveRoleAllocationPolicy, PersistenceError> {
    let value = conn.query_row(
        "SELECT allocation_profile,variant_selection_mode,minimum_cognitive_tier,paid_use_policy,max_paid_currency,max_paid_micros,reduced_below_percent,reserve_below_percent FROM cognitive_role_allocation_policies WHERE role=?1",
        [role.as_str()], |r| {
            let profile: String = r.get(0)?;
            let mode: String = r.get(1)?;
            let paid: String = r.get(3)?;
            Ok(CognitiveRoleAllocationPolicy {
                role,
                allocation_profile: match profile.as_str() { "economy" => AllocationProfile::Economy, "balanced" => AllocationProfile::Balanced, "fast" => AllocationProfile::Fast, _ => return Err(rusqlite::Error::InvalidQuery) },
                variant_selection_mode: match mode.as_str() { "explicit" => VariantSelectionMode::Explicit, "auto" => VariantSelectionMode::Auto, _ => return Err(rusqlite::Error::InvalidQuery) },
                minimum_cognitive_tier: r.get(2)?,
                paid_use_policy: match paid.as_str() { "deny" => PaidUseMode::Deny, "allow_known_cost_within_budget" => PaidUseMode::AllowKnownCostWithinBudget, _ => return Err(rusqlite::Error::InvalidQuery) },
                max_paid_currency: r.get(4)?, max_paid_micros: r.get(5)?, reduced_below_percent: r.get(6)?, reserve_below_percent: r.get(7)?,
            })
        }).map_err(|_| PersistenceError::Read)?;
    value.to_runtime().map_err(|_| PersistenceError::Read)?;
    Ok(value)
}

#[cfg(test)]
pub fn save(
    conn: &mut Connection,
    value: &CognitiveRoleAllocationPolicy,
) -> Result<CognitiveRoleAllocationPolicy, PersistenceError> {
    value.to_runtime().map_err(|_| PersistenceError::Write)?;
    let tx = conn.transaction().map_err(|_| PersistenceError::Write)?;
    write_in_transaction(&tx, value)?;
    let saved = load(&tx, value.role)?;
    tx.commit().map_err(|_| PersistenceError::Write)?;
    Ok(saved)
}
fn write_in_transaction(
    tx: &Transaction<'_>,
    value: &CognitiveRoleAllocationPolicy,
) -> Result<(), PersistenceError> {
    value.to_runtime().map_err(|_| PersistenceError::Write)?;
    let profile = match value.allocation_profile {
        AllocationProfile::Economy => "economy",
        AllocationProfile::Balanced => "balanced",
        AllocationProfile::Fast => "fast",
    };
    let mode = match value.variant_selection_mode {
        VariantSelectionMode::Explicit => "explicit",
        VariantSelectionMode::Auto => "auto",
    };
    let changed = tx.execute("UPDATE cognitive_role_allocation_policies SET allocation_profile=?2,variant_selection_mode=?3,minimum_cognitive_tier=?4,paid_use_policy=?5,max_paid_currency=?6,max_paid_micros=?7,reduced_below_percent=?8,reserve_below_percent=?9,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE role=?1",
        params![value.role.as_str(),profile,mode,value.minimum_cognitive_tier,value.paid_use_policy.as_str(),value.max_paid_currency,value.max_paid_micros,value.reduced_below_percent,value.reserve_below_percent]).map_err(|_| PersistenceError::Write)?;
    if changed != 1 {
        return Err(PersistenceError::Write);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleRuntimePolicy {
    pub routing: CognitiveRolePolicy,
    pub allocation: Option<AllocationRuntimePolicy>,
}
/// Fixed/Preferred never depend on economics, even if its table/row is corrupt.
/// Multiple roles in a TaskGraph preflight share this one SQLite snapshot.
pub fn load_role_runtime_policies(
    conn: &Connection,
    roles: &[CognitiveRole],
) -> Result<Vec<RoleRuntimePolicy>, PersistenceError> {
    fn read(
        conn: &Connection,
        roles: &[CognitiveRole],
    ) -> Result<Vec<RoleRuntimePolicy>, PersistenceError> {
        roles
            .iter()
            .map(|role| {
                let routing = policy::load(conn, *role)?;
                let allocation = if routing.routing_mode == RoutingMode::Auto {
                    Some(
                        load(conn, *role)?
                            .to_runtime()
                            .map_err(|_| PersistenceError::Read)?,
                    )
                } else {
                    None
                };
                Ok(RoleRuntimePolicy {
                    routing,
                    allocation,
                })
            })
            .collect()
    }
    if !conn.is_autocommit() {
        return read(conn, roles);
    }
    let tx = conn
        .unchecked_transaction()
        .map_err(|_| PersistenceError::Read)?;
    let snapshots = read(&tx, roles)?;
    tx.commit().map_err(|_| PersistenceError::Read)?;
    Ok(snapshots)
}
pub fn load_role_runtime_policy(
    conn: &Connection,
    role: CognitiveRole,
) -> Result<RoleRuntimePolicy, PersistenceError> {
    load_role_runtime_policies(conn, &[role])?
        .pop()
        .ok_or(PersistenceError::Read)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveRoleSettings {
    pub policy: CognitiveRolePolicy,
    pub allocation_policy: CognitiveRoleAllocationPolicy,
}
pub fn save_role_settings(
    conn: &mut Connection,
    routing: &CognitiveRolePolicy,
    allocation: &CognitiveRoleAllocationPolicy,
) -> Result<CognitiveRoleSettings, PersistenceError> {
    validate_role_settings(routing, allocation).map_err(|_| PersistenceError::Write)?;
    let tx = conn.transaction().map_err(|_| PersistenceError::Write)?;
    policy::write_in_transaction(&tx, routing)?;
    write_in_transaction(&tx, allocation)?;
    if routing.role == CognitiveRole::Summary && routing.summary_input_max_bytes == 0 {
        crate::persistence::conversation::disable_pending_summaries(&tx)?;
    }
    let saved = CognitiveRoleSettings {
        policy: policy::load(&tx, routing.role)?,
        allocation_policy: load(&tx, routing.role)?,
    };
    tx.commit().map_err(|_| PersistenceError::Write)?;
    Ok(saved)
}
pub fn validate_role_settings(
    routing: &CognitiveRolePolicy,
    allocation: &CognitiveRoleAllocationPolicy,
) -> Result<(), &'static str> {
    if routing.role != allocation.role {
        return Err("role_mismatch");
    }
    routing.validate()?;
    allocation.to_runtime()?;
    Ok(())
}
#[cfg(test)]
mod tests;

/// Strict Settings API read: unlike execution in explicit modes, the editor
/// needs all four persisted configurations. No catalog/observations are exposed.
pub(crate) fn load_all_role_settings(
    conn: &Connection,
) -> Result<Vec<CognitiveRoleSettings>, PersistenceError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|_| PersistenceError::Read)?;
    let roles = [
        CognitiveRole::Conversation,
        CognitiveRole::Summary,
        CognitiveRole::Orchestrator,
        CognitiveRole::Worker,
    ];
    let settings = roles
        .into_iter()
        .map(|role| {
            Ok(CognitiveRoleSettings {
                policy: policy::load(&tx, role)?,
                allocation_policy: load(&tx, role)?,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    tx.commit().map_err(|_| PersistenceError::Read)?;
    Ok(settings)
}
#[cfg(test)]
mod runtime_tests;
