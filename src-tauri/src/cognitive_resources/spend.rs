//! Local invocation ceiling only; no ledger, purchase, refill, FX or debit.
use super::*;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EconomicExclusion {
    AllowanceExhausted,
    Lr8ConstraintSaturated,
    PaidUseDenied,
    PaidCostUnknown,
    CurrencyMismatch,
    PaidBudgetExceeded,
    PrepaidBalanceUnknown,
    PrepaidBalanceCurrencyMismatch,
    PrepaidBalanceInsufficient,
    EconomicEvidenceConflict,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "reasons", rename_all = "snake_case")]
pub enum EconomicEligibility {
    Eligible,
    Excluded(Vec<EconomicExclusion>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpendOutcome {
    NoPositivePaidEvidence,
    AllowedWithinBudget,
    Excluded,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendAssessment {
    pub billing_kind: CatalogFact<BillingKind>,
    pub monetary_cost: ResolvedFact<MonetaryAmount>,
    pub monetary_balance: CatalogFact<MonetaryAmount>,
    pub budget: Option<PaidBudget>,
    pub paid_path_evidence: bool,
    pub positive_cost_evidence: bool,
    pub outcome: SpendOutcome,
    pub exclusions: Vec<EconomicExclusion>,
}
pub(super) fn assess_spend(
    economics: &EconomicFacts,
    cost: &ResolvedFact<MonetaryAmount>,
    policy: &PaidUsePolicy,
) -> SpendAssessment {
    let paid_path = matches!(
        economics.billing_kind.value(),
        Some(BillingKind::MeteredBilling | BillingKind::PrepaidCredits)
    );
    let positive_cost = cost.fact.value().is_some_and(|c| c.micros() > 0);
    let mut result = SpendAssessment {
        billing_kind: economics.billing_kind.clone(),
        monetary_cost: cost.clone(),
        monetary_balance: economics.monetary_balance.clone(),
        budget: match policy {
            PaidUsePolicy::Deny => None,
            PaidUsePolicy::AllowKnownCostWithinBudget { budget } => Some(budget.clone()),
        },
        paid_path_evidence: paid_path,
        positive_cost_evidence: positive_cost,
        outcome: SpendOutcome::NoPositivePaidEvidence,
        exclusions: vec![],
    };
    if !paid_path && !positive_cost {
        return result;
    }
    match policy {
        PaidUsePolicy::Deny => result.exclusions.push(EconomicExclusion::PaidUseDenied),
        PaidUsePolicy::AllowKnownCostWithinBudget { budget } => {
            if let Some(cost) = cost.fact.value() {
                if cost.currency() != budget.currency() {
                    result.exclusions.push(EconomicExclusion::CurrencyMismatch);
                } else if cost.micros() > budget.micros() {
                    result
                        .exclusions
                        .push(EconomicExclusion::PaidBudgetExceeded);
                }
                if economics.billing_kind.value() == Some(&BillingKind::PrepaidCredits) {
                    match economics.monetary_balance.value() {
                        None => result
                            .exclusions
                            .push(EconomicExclusion::PrepaidBalanceUnknown),
                        Some(balance) if balance.currency() != cost.currency() => result
                            .exclusions
                            .push(EconomicExclusion::PrepaidBalanceCurrencyMismatch),
                        Some(balance) if balance.micros() < cost.micros() => result
                            .exclusions
                            .push(EconomicExclusion::PrepaidBalanceInsufficient),
                        Some(_) => {}
                    }
                }
            } else {
                result.exclusions.push(EconomicExclusion::PaidCostUnknown);
            }
        }
    }
    result.outcome = if result.exclusions.is_empty() {
        SpendOutcome::AllowedWithinBudget
    } else {
        SpendOutcome::Excluded
    };
    result
}
