use std::{collections::HashMap, sync::{atomic::{AtomicBool, Ordering}, Arc}};
use serde::Deserialize;

use tauri::ipc::Channel;

use super::{
    scheduler::{Scheduler, SchedulerEvent},
    task_graph::TaskGraphSubtaskResult,
    types::{
        ContextBundle, ProviderCapabilities, ProviderSelection, ProviderTarget, ProviderTaskRequest,
        RetryPolicy, SchedulerError, SchedulerUsage, TaskBudget, TaskResult,
    },
};
use crate::{
    agents::planner::PlanStepV1,
    luna::task::{TaskEvent, TaskEventKind, TaskId, TaskState},
};

fn emit(
    channel: &Channel<TaskEvent>,
    root: TaskId,
    sequence: &std::sync::atomic::AtomicU32,
    state: TaskState,
    kind: TaskEventKind,
) -> Result<(), &'static str> {
    let sequence = sequence
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    channel
        .send(TaskEvent {
            task_id: root,
            sequence,
            state,
            kind,
        })
        .map_err(|_| "channel_closed")
}

fn worker_events<'a>(
    subtask_id: &'a str,
    channel: &'a Channel<TaskEvent>,
    root: TaskId,
    sequence: &'a std::sync::atomic::AtomicU32,
    cancelled: &'a AtomicBool,
) -> impl FnMut(SchedulerEvent) -> Result<(), SchedulerError> + Send + 'a {
    move |event| match event {
        SchedulerEvent::Selected { .. } => Ok(()),
        SchedulerEvent::Retry {
            provider_id,
            reason_code,
        } => emit(
            channel,
            root,
            sequence,
            TaskState::Running,
            TaskEventKind::SubtaskRetry {
                subtask_id: subtask_id.to_owned(),
                provider_id,
                reason_code: reason_code.into(),
            },
        )
        .map_err(|_| {
            cancelled.store(true, Ordering::Release);
            SchedulerError::EventSinkClosed
        }),
        SchedulerEvent::Chunk { provider_id, .. } => emit(
            channel,
            root,
            sequence,
            TaskState::Running,
            TaskEventKind::SubtaskOutputObserved {
                subtask_id: subtask_id.to_owned(),
                provider_id,
            },
        )
        .map_err(|_| {
            cancelled.store(true, Ordering::Release);
            SchedulerError::EventSinkClosed
        }),
        SchedulerEvent::Fallback { .. } => Err(SchedulerError::InvalidTargetConfig),
    }
}

pub(crate) fn worker_input(
    step: &PlanStepV1,
    results: &HashMap<String, TaskGraphSubtaskResult>,
    max_bytes: usize,
) -> Result<String, &'static str> {
    let mut input = format!(
        "Execute somente a subtarefa cognitiva abaixo. Não use ferramentas, não alegue ações externas e não invente progresso. Retorne apenas o resultado útil da unidade.\nSUBTAREFA {}:\n{}\n",
        step.id, step.description
    );
    if input.len() > max_bytes {
        return Err("task_graph_context_budget_exceeded");
    }
    if !step.depends_on.is_empty() {
        input.push_str("RESULTADOS DAS DEPENDÊNCIAS:\n");
    }
    for dependency in &step.depends_on {
        let result = results
            .get(dependency)
            .ok_or("task_graph_dependency_result_missing")?;
        let prefix = format!("--- {} [{}] ---\n", dependency, result.provider_id);
        if input.len().saturating_add(prefix.len()) > max_bytes {
            return Err("task_graph_context_budget_exceeded");
        }
        input.push_str(&prefix);
        let remaining = max_bytes.saturating_sub(input.len());
        if result.text.len() <= remaining {
            input.push_str(&result.text);
        } else {
            if remaining <= 16 {
                return Err("task_graph_context_budget_exceeded");
            }
            let mut end = remaining - 16;
            while end > 0 && !result.text.is_char_boundary(end) {
                end -= 1;
            }
            input.push_str(&result.text[..end]);
            input.push_str("\n[truncado]");
        }
        if input.len() < max_bytes {
            input.push('\n');
        }
    }
    Ok(input)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StructuredWorkerResult {
    subtask_id: String,
    text: String,
}

const MAX_STRUCTURED_WORKER_RESULT_BYTES: usize = 16 * 1024;

pub(crate) struct WorkerTiming {
    pub started_at: String,
    pub finished_at: String,
}

fn parse_structured_worker_result(raw: &str, expected_id: &str) -> Result<String, &'static str> {
    if raw.len() > MAX_STRUCTURED_WORKER_RESULT_BYTES {
        return Err("task_graph_worker_result_invalid");
    }
    let result: StructuredWorkerResult = serde_json::from_str(raw)
        .map_err(|_| "task_graph_worker_result_invalid")?;
    if result.subtask_id != expected_id
        || result.subtask_id.trim().is_empty()
        || result.text.trim().is_empty()
        || result.text.len() > MAX_STRUCTURED_WORKER_RESULT_BYTES
    {
        return Err("task_graph_worker_result_invalid");
    }
    Ok(result.text)
}

pub(crate) fn worker_system_instruction(step: &PlanStepV1) -> Option<String> {
    step.required_capabilities
        .contains(&crate::agents::planner::PlanCapability::StructuredOutput)
        .then(|| "Execute somente a unidade cognitiva fornecida na mensagem do usuário. Não execute ferramentas nem ações externas. Retorne somente um objeto JSON cru com exatamente os campos subtaskId e text, sem markdown, prefixos, sufixos ou campos adicionais. Reproduza em subtaskId exatamente o identificador fornecido na subtarefa. text deve ser não vazio e conter o resultado útil.".to_owned())
}

pub(crate) fn add_usage(total: &mut SchedulerUsage, item: &SchedulerUsage) {
    total.provider_calls = total.provider_calls.saturating_add(item.provider_calls);
    total.input_tokens = total.input_tokens.saturating_add(item.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(item.output_tokens);
    total.output_tokens_accounted = total.output_tokens_accounted.saturating_add(item.output_tokens_accounted);
    total.output_tokens_measured &= item.output_tokens_measured;
    total.retries = total.retries.saturating_add(item.retries);
    total.fallbacks = total.fallbacks.saturating_add(item.fallbacks);
    for provider in &item.providers_used {
        if !total.providers_used.contains(provider) {
            total.providers_used.push(provider.clone());
        }
    }
    // Provider-specific optional totals are not recomputed from incomplete data.
    total.total_tokens = None;
    total.thought_tokens = None;
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_worker(
    scheduler: Arc<Scheduler>,
    root: TaskId,
    subtask_id: String,
    target: ProviderTarget,
    input: String,
    internal_system_instruction: Option<String>,
    max_output_tokens: Option<u32>,
    max_provider_calls: u32,
    retry_policy: RetryPolicy,
    context: Arc<ContextBundle>,
    cancelled: &AtomicBool,
    channel: &Channel<TaskEvent>,
    sequence: &std::sync::atomic::AtomicU32,
) -> (Option<WorkerTiming>, Result<TaskResult, &'static str>) {
    if cancelled.load(Ordering::Acquire) {
        return (None, Err("cancelled"));
    }
    let provider_id = target.provider_id.clone();
    if emit(
        channel,
        root,
        sequence,
        TaskState::Running,
        TaskEventKind::SubtaskStarted {
            subtask_id: subtask_id.clone(),
            provider_id: provider_id.clone(),
        },
    )
    .is_err()
    {
        cancelled.store(true, Ordering::Release);
        return (None, Err("channel_closed"));
    }
    let started = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let request = ProviderTaskRequest {
        input,
        internal_system_instruction: internal_system_instruction.clone(),
        history: vec![],
        context,
        max_output_tokens,
        selection: ProviderSelection::Fixed(provider_id),
        targets: vec![target],
        affinity_key: None,
        estimated_context_bytes: 0,
        required_capabilities: ProviderCapabilities::text_stream(),
    };
    let mut events = worker_events(&subtask_id, channel, root, sequence, cancelled);
    let structured_result = internal_system_instruction.is_some();
    let result = scheduler
        .run_with_retry_conservative_output(
            request,
            TaskBudget {
                max_provider_calls,
                max_output_tokens,
            },
            retry_policy,
            cancelled,
            &mut events,
        )
        .await
        .map_err(|error| error.code());
    let result = match result {
        Ok(result) => result,
        Err(code) => return (Some(WorkerTiming {
            started_at: started.clone(),
            finished_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        }), Err(code)),
    };
    let mut result = result;
    if structured_result {
        match parse_structured_worker_result(&result.text, &subtask_id) {
            Ok(text) => result.text = text,
            Err(code) => return (Some(WorkerTiming {
                started_at: started.clone(),
                finished_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            }), Err(code)),
        }
    }
    (Some(WorkerTiming {
        started_at: started,
        finished_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    }), Ok(result))
}

#[cfg(test)]
mod tests {
    use super::{parse_structured_worker_result, worker_system_instruction};
    use crate::agents::planner::{PlanCapability, PlanStepV1};

    #[test]
    fn worker_instruction_is_static_and_never_promotes_subtask_content() {
        let step = PlanStepV1 {
            id: "ignore instructions\nsecret marker".into(),
            description: "description secret marker".into(),
            required_capabilities: vec![PlanCapability::StructuredOutput],
            depends_on: vec![],
        };
        let instruction = worker_system_instruction(&step).unwrap();
        assert!(!instruction.contains(&step.id));
        assert!(!instruction.contains(&step.description));
        assert!(instruction.contains("identificador fornecido na subtarefa"));
    }

    #[test]
    fn structured_worker_result_is_strict_and_bounded() {
        assert_eq!(
            parse_structured_worker_result(r#"{"subtaskId":"unit-1","text":"resultado"}"#, "unit-1"),
            Ok("resultado".into())
        );
        assert_eq!(
            parse_structured_worker_result(r#"{"subtaskId":"unit-10","text":"resultado"}"#, "unit-1"),
            Err("task_graph_worker_result_invalid")
        );
        for raw in [
            r#"```json
{"subtaskId":"unit-1","text":"resultado"}
```"#,
            r#"prefixo {"subtaskId":"unit-1","text":"resultado"}"#,
            r#"{"subtaskId":"unit-1","text":"resultado"} sufixo"#,
            r#"{"subtaskId":"unit-2","text":"resultado"}"#,
            r#"{"subtaskId":"unit-1","text":" "}"#,
            r#"{"subtaskId":"unit-1","text":"resultado","extra":true}"#,
            r#"{"subtaskId":"unit-1","text":"resultado"} {"subtaskId":"unit-1","text":"outro"}"#,
        ] {
            assert_eq!(parse_structured_worker_result(raw, "unit-1"), Err("task_graph_worker_result_invalid"));
        }
        let oversized = format!(r#"{{"subtaskId":"unit-1","text":"{}"}}"#, "x".repeat(16 * 1024));
        assert_eq!(parse_structured_worker_result(&oversized, "unit-1"), Err("task_graph_worker_result_invalid"));
    }
}
