use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, os::unix::fs::MetadataExt, path::Path};
pub const CLI_SHA: &str = "9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99";
pub fn private_directory(path: &Path) -> Result<(), &'static str> {
    let m = fs::symlink_metadata(path).map_err(|_| "directory_unavailable")?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
        return Err("unsafe_private_directory");
    }
    Ok(())
}
pub fn private_file(path: &Path) -> Result<(), &'static str> {
    let m = fs::symlink_metadata(path).map_err(|_| "private_file_unavailable")?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o600
    {
        return Err("unsafe_private_file");
    }
    Ok(())
}
pub fn reviewed_receipt(v: &Value, id: u64) -> Result<(), &'static str> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "clock_unavailable")?
        .as_secs_f64();
    let time = v["human_reviewed_at_unix"]
        .as_f64()
        .ok_or("review_timestamp_unavailable")?;
    if v["max_additional_usd"] != 0
        || v["provider_additional_usage_disabled"] != true
        || v["billing_unit_uncertainty_explicitly_accepted"] != true
        || v["max_sdk_send_calls"] != 1
        || v["authorized_task_id"] != id
        || !time.is_finite()
        || time > now
        || now - time > 1800.0
    {
        return Err("financial_review_scope_or_expiry");
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
    pub objective: String,
    pub model: String,
    pub included_only_approval: bool,
}
pub fn validate_input(input: &TaskInput) -> Result<(), &'static str> {
    if input.objective.trim().is_empty() || input.objective.len() > 2048 || input.model != "auto" {
        return Err("invalid_task_or_model");
    }
    if !input.included_only_approval {
        return Err("human_included_only_approval_required");
    }
    Ok(())
}
/// Provider controls are required independently of human task consent.
/// Legacy request units are reported as such; never inferred as AI credits.
/// An explicit human financial review is also required before first submission.
pub fn financial_preflight(v: &Value) -> Result<(), &'static str> {
    if v["auth"]["authenticated"] != true {
        return Err("authentication_required");
    }
    if !v["catalog"]["models"]
        .as_array()
        .is_some_and(|a| a.iter().any(|m| m["id"] == "auto"))
    {
        return Err("eligible_model_unavailable");
    }
    let s = v["quota"]["snapshots"]
        .as_array()
        .and_then(|a| a.iter().find(|r| r["kind"] == "premium_interactions"));
    let s = s.ok_or("quota_unknown")?;
    if s["state"] != "quota_available" {
        return Err("included_allowance_unknown_or_exhausted");
    }
    if s["snapshot"]["overageAllowedWithExhaustedQuota"] != false
        || s["snapshot"]["usageAllowedWithExhaustedQuota"] != false
    {
        return Err("paid_fallback_not_disabled");
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn authentication_does_not_authorize_spending() {
        assert_eq!(
            financial_preflight(&json!({"auth":{"authenticated":true}})),
            Err("eligible_model_unavailable")
        );
        let t = TaskInput {
            objective: "sum".into(),
            model: "auto".into(),
            included_only_approval: false,
        };
        assert!(validate_input(&t).is_err());
    }
    #[test]
    fn quota_absent_is_not_unlimited() {
        assert!(financial_preflight(&json!({})).is_err());
    }
    #[test]
    fn no_other_model_or_unbounded_task() {
        let mut t = TaskInput {
            objective: "x".into(),
            model: "auto".into(),
            included_only_approval: true,
        };
        assert!(validate_input(&t).is_ok());
        t.model = "paid".into();
        assert!(validate_input(&t).is_err());
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn flags_and_auth_cannot_replace_financial_review() {
        assert!(reviewed_receipt(
            &json!({"authenticated":true,"force":true,"max_additional_usd":0}),
            1
        )
        .is_err());
    }
    #[test]
    fn receipt_requires_scope_expiry_and_zero_paid_budget() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let mut r = json!({"max_additional_usd":0,"provider_additional_usage_disabled":true,"billing_unit_uncertainty_explicitly_accepted":true,"max_sdk_send_calls":1,"authorized_task_id":7,"human_reviewed_at_unix":now});
        assert!(reviewed_receipt(&r, 7).is_ok());
        assert!(reviewed_receipt(&r, 8).is_err());
        r["max_additional_usd"] = json!(1);
        assert!(reviewed_receipt(&r, 7).is_err());
        r["max_additional_usd"] = json!(0);
        r["human_reviewed_at_unix"] = json!(now - 1801.0);
        assert!(reviewed_receipt(&r, 7).is_err());
    }
    #[test]
    fn overage_or_unknown_provider_flags_deny_even_human_consent() {
        let mut v = json!({"auth":{"authenticated":true},"catalog":{"models":[{"id":"auto"}]},"quota":{"snapshots":[{"kind":"premium_interactions","state":"quota_available","snapshot":{"overageAllowedWithExhaustedQuota":false,"usageAllowedWithExhaustedQuota":false}}]}});
        assert!(financial_preflight(&v).is_ok());
        v["quota"]["snapshots"][0]["snapshot"]["usageAllowedWithExhaustedQuota"] = json!(true);
        assert_eq!(financial_preflight(&v), Err("paid_fallback_not_disabled"));
    }
}
