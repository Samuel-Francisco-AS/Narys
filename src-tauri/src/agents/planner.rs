use std::{collections::{HashMap, HashSet}, sync::{atomic::AtomicBool, Arc}};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{registry::AgentRegistry, types::{AgentCapabilities, AgentError, AgentRequest}};

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
        if raw.len() > MAX_PLAN_BYTES { return Err(AgentError::Protocol); }
        let plan: Self = serde_json::from_str(raw).map_err(|_| AgentError::Protocol)?;
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), AgentError> {
        let invalid = || AgentError::Protocol;
        if self.version != 1 || !bounded(&self.objective, MAX_OBJECTIVE_BYTES)
            || !(1..=16).contains(&self.steps.len()) || self.risks.len() > 8
            || self.questions.len() > 8 || self.risks.iter().any(|s| !bounded(s, 512))
            || self.questions.iter().any(|s| !bounded(s, 512))
            || self.needs_user_input != !self.questions.is_empty() {
            return Err(invalid());
        }
        let mut ids = HashSet::new();
        for step in &self.steps {
            if !bounded(&step.id, 64) || !bounded(&step.description, 1024)
                || step.required_capabilities.len() > 6 || step.depends_on.len() > 16
                || !ids.insert(step.id.as_str()) { return Err(invalid()); }
        }
        let by_id: HashMap<_, _> = self.steps.iter().map(|s| (s.id.as_str(), s)).collect();
        for step in &self.steps {
            if step.depends_on.iter().any(|id| id == &step.id || !by_id.contains_key(id.as_str())) {
                return Err(invalid());
            }
        }
        fn visit<'a>(id: &'a str, by_id: &HashMap<&'a str, &'a PlanStepV1>, marks: &mut HashMap<&'a str, u8>) -> bool {
            match marks.get(id) { Some(1) => return false, Some(2) => return true, _ => {} }
            marks.insert(id, 1);
            if by_id[id].depends_on.iter().any(|dep| !visit(dep, by_id, marks)) { return false; }
            marks.insert(id, 2);
            true
        }
        let mut marks = HashMap::new();
        if self.steps.iter().any(|step| !visit(&step.id, &by_id, &mut marks)) { return Err(invalid()); }
        if serde_json::to_vec(self).map_err(|_| invalid())?.len() > MAX_PLAN_BYTES { return Err(invalid()); }
        Ok(())
    }
}

fn bounded(s: &str, max: usize) -> bool { !s.trim().is_empty() && s.len() <= max }

pub fn output_schema() -> Value {
    // Keep the model-facing schema within the structured-output subset. Rust
    // enforces byte counts, cardinality, dependencies and cross-field rules.
    let text = || json!({"type":"string"});
    json!({
        "type":"object", "additionalProperties":false,
        "required":["version","objective","steps","risks","needsUserInput","questions"],
        "properties":{
            "version":{"type":"integer","enum":[1]},
            "objective":text(),
            "steps":{"type":"array","items":{
                "type":"object","additionalProperties":false,
                "required":["id","description","requiredCapabilities","dependsOn"],
                "properties":{
                    "id":text(),"description":text(),
                    "requiredCapabilities":{"type":"array","items":{"type":"string","enum":["planning","repository_read","file_write","command_execution","tool_use","structured_output"]}},
                    "dependsOn":{"type":"array","items":text()}
                }}},
            "risks":{"type":"array","items":text()},
            "needsUserInput":{"type":"boolean"},
            "questions":{"type":"array","items":text()}
        }
    })
}

pub async fn plan(registry: &Arc<AgentRegistry>, objective: String) -> Result<PlanV1, AgentError> {
    if !bounded(&objective, MAX_OBJECTIVE_BYTES) { return Err(AgentError::InvalidRequest); }
    let entry = registry.get("codex").ok_or(AgentError::Unavailable)?;
    if !entry.config.enabled { return Err(AgentError::Unavailable); }
    let required_capabilities = AgentCapabilities { planning: true, structured_output: true, ..Default::default() };
    if !entry.config.capabilities.supports(&required_capabilities) { return Err(AgentError::UnsupportedCapability); }
    let request = AgentRequest { objective, required_capabilities };
    let cancelled = AtomicBool::new(false);
    let mut sink = |_| Ok(());
    let result = entry.backend.execute(&request, &cancelled, &mut sink).await?;
    PlanV1::parse(&result.output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid() -> PlanV1 { PlanV1 { version: 1, objective: "Objetivo".into(), steps: vec![PlanStepV1 { id:"a".into(), description:"Passo".into(), required_capabilities:vec![PlanCapability::Planning], depends_on:vec![] }], risks:vec![], needs_user_input:false, questions:vec![] } }
    #[test] fn valid_and_invalid_json() { let p=valid(); assert_eq!(PlanV1::parse(&serde_json::to_string(&p).unwrap()),Ok(p)); assert!(PlanV1::parse("{").is_err()); }
    #[test] fn unknown_fields_and_capability() { let mut v=serde_json::to_value(valid()).unwrap(); v["extra"]=json!(1); assert!(PlanV1::parse(&v.to_string()).is_err()); v.as_object_mut().unwrap().remove("extra"); v["steps"][0]["requiredCapabilities"]=json!(["unknown"]); assert!(PlanV1::parse(&v.to_string()).is_err()); }
    #[test] fn version_and_step_limits() { let mut p=valid(); p.version=2; assert!(p.validate().is_err()); p.version=1; p.steps.clear(); assert!(p.validate().is_err()); p.steps=vec![valid().steps[0].clone();17]; assert!(p.validate().is_err()); }
    #[test] fn ids_and_dependencies() { let mut p=valid(); p.steps.push(p.steps[0].clone()); assert!(p.validate().is_err()); p.steps.pop(); p.steps[0].depends_on=vec!["missing".into()]; assert!(p.validate().is_err()); p.steps[0].depends_on=vec!["a".into()]; assert!(p.validate().is_err()); p.steps[0].depends_on=vec!["b".into()]; p.steps.push(PlanStepV1{id:"b".into(),description:"B".into(),required_capabilities:vec![],depends_on:vec!["a".into()]}); assert!(p.validate().is_err()); }
    #[test] fn questions_and_size() { let mut p=valid(); p.needs_user_input=true; assert!(p.validate().is_err()); p.questions=vec!["Pergunta".into()]; assert!(p.validate().is_ok()); p.needs_user_input=false; assert!(p.validate().is_err()); let huge="x".repeat(MAX_PLAN_BYTES); assert!(PlanV1::parse(&huge).is_err()); }
}
