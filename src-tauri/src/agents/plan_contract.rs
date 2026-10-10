use super::types::AgentError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
pub const MAX_PLAN_BYTES: usize = 16 * 1024;
pub const MAX_OBJECTIVE_BYTES: usize = 2048;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanV1 {
    pub version: u8,
    pub objective: String,
    pub steps: Vec<PlanStepV1>,
    pub risks: Vec<String>,
    pub needs_user_input: bool,
    pub questions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanStepV1 {
    pub id: String,
    pub description: String,
    pub required_capabilities: Vec<PlanCapability>,
    pub depends_on: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanCapability {
    Planning,
    RepositoryRead,
    FileWrite,
    CommandExecution,
    ToolUse,
    StructuredOutput,
}

impl PlanV1 {
    pub fn parse(raw: &str) -> Result<Self, AgentError> {
        if raw.len() > MAX_PLAN_BYTES {
            return Err(AgentError::Protocol);
        }
        let plan: Self = serde_json::from_str(raw).map_err(|_| AgentError::Protocol)?;
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), AgentError> {
        let invalid = || AgentError::Protocol;
        if self.version != 1
            || !bounded(&self.objective, MAX_OBJECTIVE_BYTES)
            || !(1..=16).contains(&self.steps.len())
            || self.risks.len() > 8
            || self.questions.len() > 8
            || self.risks.iter().any(|s| !bounded(s, 512))
            || self.questions.iter().any(|s| !bounded(s, 512))
            || self.needs_user_input != !self.questions.is_empty()
        {
            return Err(invalid());
        }
        let mut ids = HashSet::new();
        for step in &self.steps {
            if !bounded(&step.id, 64)
                || !bounded(&step.description, 1024)
                || step.required_capabilities.len() > 6
                || step.depends_on.len() > 16
                || !ids.insert(step.id.as_str())
            {
                return Err(invalid());
            }
        }
        let by_id: HashMap<_, _> = self.steps.iter().map(|s| (s.id.as_str(), s)).collect();
        for step in &self.steps {
            if step
                .depends_on
                .iter()
                .any(|id| id == &step.id || !by_id.contains_key(id.as_str()))
            {
                return Err(invalid());
            }
        }
        fn visit<'a>(
            id: &'a str,
            by_id: &HashMap<&'a str, &'a PlanStepV1>,
            marks: &mut HashMap<&'a str, u8>,
        ) -> bool {
            match marks.get(id) {
                Some(1) => return false,
                Some(2) => return true,
                _ => {}
            }
            marks.insert(id, 1);
            if by_id[id]
                .depends_on
                .iter()
                .any(|dep| !visit(dep, by_id, marks))
            {
                return false;
            }
            marks.insert(id, 2);
            true
        }
        let mut marks = HashMap::new();
        if self
            .steps
            .iter()
            .any(|step| !visit(&step.id, &by_id, &mut marks))
        {
            return Err(invalid());
        }
        if serde_json::to_vec(self).map_err(|_| invalid())?.len() > MAX_PLAN_BYTES {
            return Err(invalid());
        }
        Ok(())
    }
}

fn bounded(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max
}
