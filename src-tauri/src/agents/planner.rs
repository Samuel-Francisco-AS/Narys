use std::sync::{atomic::AtomicBool, Arc};
use serde_json::{json, Value};
use super::{registry::AgentRegistry, types::{AgentCapabilities, AgentError, AgentRequest}};
pub use super::plan_contract::*;

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
    let result = entry.backend.execute_observed(&request, &cancelled, &mut sink,
        Arc::new(crate::operational_trace::adapters::AgentTraceAdapter::production(&entry.config.id))).await?;
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

fn bounded(s: &str, max: usize) -> bool { !s.trim().is_empty() && s.len() <= max }
