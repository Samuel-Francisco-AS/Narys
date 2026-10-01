use std::{collections::HashMap, sync::{atomic::{AtomicBool, Ordering}, Arc}};

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

pub(crate) fn add_usage(total: &mut SchedulerUsage, item: &SchedulerUsage) {
    total.provider_calls = total.provider_calls.saturating_add(item.provider_calls);
    total.input_tokens = total.input_tokens.saturating_add(item.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(item.output_tokens);
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
    max_output_tokens: Option<u32>,
    max_provider_calls: u32,
    retry_policy: RetryPolicy,
    context: Arc<ContextBundle>,
    cancelled: &AtomicBool,
    channel: &Channel<TaskEvent>,
    sequence: &std::sync::atomic::AtomicU32,
) -> Result<TaskResult, &'static str> {
    if cancelled.load(Ordering::Acquire) {
        return Err("cancelled");
    }
    let provider_id = target.provider_id.clone();
    emit(
        channel,
        root,
        sequence,
        TaskState::Running,
        TaskEventKind::SubtaskStarted {
            subtask_id: subtask_id.clone(),
            provider_id: provider_id.clone(),
        },
    )
    .map_err(|_| {
        cancelled.store(true, Ordering::Release);
        "channel_closed"
    })?;
    let request = ProviderTaskRequest {
        input,
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
    scheduler
        .run_with_retry(
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
        .map_err(|error| error.code())
}
