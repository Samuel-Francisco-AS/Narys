use crate::agents::planner::{PlanCapability, PlanV1, PlanStepV1};
use serde::Serialize;

use super::types::SchedulerUsage;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskGraphSubtaskResult {
    pub subtask_id: String,
    pub provider_id: String,
    pub text: String,
    pub usage: SchedulerUsage,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskGraphResult {
    pub planner_provider_id: String,
    pub planner_usage: SchedulerUsage,
    pub plan: PlanV1,
    pub subtasks: Vec<TaskGraphSubtaskResult>,
    pub consolidated_text: String,
    pub worker_usage: SchedulerUsage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtaskState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Blocked,
}

#[derive(Clone, Debug)]
pub struct GraphSubtask {
    pub step: PlanStepV1,
    pub state: SubtaskState,
}

#[derive(Clone, Debug)]
pub struct TaskGraph {
    pub objective: String,
    subtasks: Vec<GraphSubtask>,
}

impl TaskGraph {
    pub fn compile(plan: &PlanV1) -> Result<Self, &'static str> {
        plan.validate().map_err(|_| "task_graph_plan_invalid")?;
        if plan.needs_user_input {
            return Err("task_graph_user_input_required");
        }
        if plan.steps.iter().any(|step| {
            step.required_capabilities.iter().any(|capability| {
                !matches!(capability, PlanCapability::Planning | PlanCapability::StructuredOutput)
            })
        }) {
            return Err("task_graph_capability_unsupported");
        }
        Ok(Self {
            objective: plan.objective.clone(),
            subtasks: plan.steps.iter().cloned().map(|step| GraphSubtask {
                step,
                state: SubtaskState::Pending,
            }).collect(),
        })
    }

    pub fn len(&self) -> usize { self.subtasks.len() }
    pub fn subtasks(&self) -> &[GraphSubtask] { &self.subtasks }

    pub fn state(&self, id: &str) -> Option<SubtaskState> {
        self.subtasks.iter().find(|item| item.step.id == id).map(|item| item.state)
    }

    pub fn ready_ids(&self) -> Vec<String> {
        self.subtasks.iter()
            .filter(|item| item.state == SubtaskState::Pending)
            .filter(|item| item.step.depends_on.iter().all(|dependency| self.state(dependency) == Some(SubtaskState::Completed)))
            .map(|item| item.step.id.clone())
            .collect()
    }

    pub fn waiting_dependencies(&self, id: &str) -> Option<Vec<String>> {
        let item = self.subtasks.iter().find(|item| item.step.id == id)?;
        (item.state == SubtaskState::Pending).then(|| {
            item.step.depends_on.iter()
                .filter(|dependency| self.state(dependency) != Some(SubtaskState::Completed))
                .cloned().collect()
        })
    }

    pub fn mark_running(&mut self, id: &str) -> Result<(), &'static str> {
        let item = self.subtasks.iter_mut().find(|item| item.step.id == id).ok_or("subtask_unknown")?;
        if item.state != SubtaskState::Pending { return Err("subtask_state_invalid"); }
        item.state = SubtaskState::Running;
        Ok(())
    }

    pub fn mark_completed(&mut self, id: &str) -> Result<(), &'static str> {
        self.transition_running(id, SubtaskState::Completed)
    }

    pub fn mark_failed(&mut self, id: &str) -> Result<(), &'static str> {
        self.transition_running(id, SubtaskState::Failed)?;
        self.block_failed_dependents();
        Ok(())
    }

    fn transition_running(&mut self, id: &str, target: SubtaskState) -> Result<(), &'static str> {
        let item = self.subtasks.iter_mut().find(|item| item.step.id == id).ok_or("subtask_unknown")?;
        if item.state != SubtaskState::Running { return Err("subtask_state_invalid"); }
        item.state = target;
        Ok(())
    }

    pub fn block_failed_dependents(&mut self) {
        loop {
            let terminal: std::collections::HashSet<String> = self.subtasks.iter()
                .filter(|item| matches!(item.state, SubtaskState::Failed | SubtaskState::Blocked | SubtaskState::Cancelled))
                .map(|item| item.step.id.clone()).collect();
            let mut changed = false;
            for item in &mut self.subtasks {
                if item.state == SubtaskState::Pending && item.step.depends_on.iter().any(|dependency| terminal.contains(dependency)) {
                    item.state = SubtaskState::Blocked;
                    changed = true;
                }
            }
            if !changed { break; }
        }
    }

    pub fn cancel_unfinished(&mut self) {
        for item in &mut self.subtasks {
            if matches!(item.state, SubtaskState::Pending | SubtaskState::Running) {
                item.state = SubtaskState::Cancelled;
            }
        }
        self.block_failed_dependents();
    }

    pub fn block_unfinished(&mut self) {
        for item in &mut self.subtasks {
            if matches!(item.state, SubtaskState::Pending | SubtaskState::Running) {
                item.state = SubtaskState::Blocked;
            }
        }
    }

    pub fn all_completed(&self) -> bool {
        self.subtasks.iter().all(|item| item.state == SubtaskState::Completed)
    }

    pub fn has_failure(&self) -> bool {
        self.subtasks.iter().any(|item| matches!(item.state, SubtaskState::Failed | SubtaskState::Blocked))
    }

    pub fn step(&self, id: &str) -> Option<&PlanStepV1> {
        self.subtasks.iter().find(|item| item.step.id == id).map(|item| &item.step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn step(id: &str, depends_on: &[&str]) -> PlanStepV1 {
        PlanStepV1 {
            id: id.into(),
            description: format!("Subtarefa {id}"),
            required_capabilities: vec![PlanCapability::Planning],
            depends_on: depends_on.iter().map(|item| (*item).into()).collect(),
        }
    }
    fn plan(steps: Vec<PlanStepV1>) -> PlanV1 {
        PlanV1 { version: 1, objective: "Objetivo sintético".into(), steps, risks: vec![], needs_user_input: false, questions: vec![] }
    }

    #[test]
    fn independent_steps_share_first_ready_wave_and_dependency_waits() {
        let mut graph = TaskGraph::compile(&plan(vec![step("a",&[]), step("b",&[]), step("c",&["a","b"])])).unwrap();
        assert_eq!(graph.ready_ids(), vec!["a","b"]);
        assert_eq!(graph.waiting_dependencies("c").unwrap(), vec!["a","b"]);
        graph.mark_running("a").unwrap();
        graph.mark_running("b").unwrap();
        graph.mark_completed("a").unwrap();
        assert!(graph.ready_ids().is_empty());
        graph.mark_completed("b").unwrap();
        assert_eq!(graph.ready_ids(), vec!["c"]);
    }

    #[test]
    fn unsupported_operational_capability_fails_closed() {
        let mut operational = step("write",&[]);
        operational.required_capabilities = vec![PlanCapability::FileWrite];
        assert_eq!(TaskGraph::compile(&plan(vec![operational])).unwrap_err(), "task_graph_capability_unsupported");
    }

    #[test]
    fn failure_blocks_transitive_dependents_but_not_independent_siblings() {
        let mut graph = TaskGraph::compile(&plan(vec![step("a",&[]),step("b",&[]),step("c",&["a"]),step("d",&["c"])] )).unwrap();
        graph.mark_running("a").unwrap();
        graph.mark_failed("a").unwrap();
        assert_eq!(graph.state("c"), Some(SubtaskState::Blocked));
        assert_eq!(graph.state("d"), Some(SubtaskState::Blocked));
        assert_eq!(graph.state("b"), Some(SubtaskState::Pending));
        assert_eq!(graph.ready_ids(), vec!["b"]);
    }

    #[test]
    fn cancellation_marks_pending_and_running_without_fake_completion() {
        let mut graph = TaskGraph::compile(&plan(vec![step("a",&[]),step("b",&["a"])] )).unwrap();
        graph.mark_running("a").unwrap();
        graph.cancel_unfinished();
        assert_eq!(graph.state("a"), Some(SubtaskState::Cancelled));
        assert_eq!(graph.state("b"), Some(SubtaskState::Cancelled));
        assert!(!graph.all_completed());
    }
}
