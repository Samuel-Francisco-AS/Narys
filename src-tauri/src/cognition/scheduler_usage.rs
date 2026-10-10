use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchedulerUsage {
    pub provider_calls: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Sum of provider-reported output tokens; incomplete when any attempt is unmeasured.
    pub output_tokens_measured: bool,
    /// Output budget conservatively debited, including attempts without usage.
    pub output_tokens_accounted: u32,
    pub total_tokens: Option<u32>,
    pub thought_tokens: Option<u32>,
    pub providers_used: Vec<String>,
    pub retries: u32,
    pub fallbacks: u32,
}
