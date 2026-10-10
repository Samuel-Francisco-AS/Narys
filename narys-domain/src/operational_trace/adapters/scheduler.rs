use super::*;
use crate::cognition::{policy::CognitiveRole, scheduler::SchedulerEvent};
use crate::luna::task::TaskId;

#[derive(Clone, Copy)]
pub enum ExposurePolicy {
    MetadataOnly,
    ConversationOutput,
}
/// Native-only callback metadata: never part of a ProviderTaskRequest/Request.
pub struct SchedulerTraceContext {
    pub task_id: Option<TaskId>,
    pub subtask_id: Option<String>,
    pub cognitive_role: CognitiveRole,
    pub exposure_policy: ExposurePolicy,
    pub correlation: Option<TraceId>,
}
impl SchedulerTraceContext {
    pub fn new(task_id: Option<TaskId>, subtask: Option<&str>, role: CognitiveRole) -> Self {
        Self {
            task_id,
            subtask_id: subtask.map(str::to_owned),
            cognitive_role: role,
            exposure_policy: match role {
                CognitiveRole::Conversation => ExposurePolicy::ConversationOutput,
                _ => ExposurePolicy::MetadataOnly,
            },
            correlation: None,
        }
    }
}
pub struct SchedulerTraceAdapter {
    publisher: PassiveTracePublisher,
    context: SchedulerTraceContext,
    correlation: Option<TraceId>,
    selection: u64,
    output_observed: bool,
}
impl SchedulerTraceAdapter {
    pub fn production(context: SchedulerTraceContext) -> Self {
        Self::new(PassiveTracePublisher::production(), context)
    }
    pub fn new(publisher: PassiveTracePublisher, context: SchedulerTraceContext) -> Self {
        static CALLS: AtomicU64 = AtomicU64::new(0);
        let correlation = context
            .correlation
            .clone()
            .or_else(|| next_call(&CALLS, "provider-call"));
        Self {
            publisher,
            context,
            correlation,
            selection: 0,
            output_observed: false,
        }
    }
    fn source(&self, source_type: SourceType, id: &str) -> Result<Provenance, TraceError> {
        let correlation = self
            .correlation
            .clone()
            .ok_or(TraceError::SequenceExhausted)?;
        let mut p = provenance(
            source_type,
            id,
            self.context.task_id,
            self.context.subtask_id.as_deref(),
            Some(correlation.clone()),
        )?;
        p.coalescing_key = Some(TraceId::new(&format!(
            "{}-{}",
            correlation.as_str(),
            self.selection
        ))?);
        Ok(p)
    }
    pub fn observe(&mut self, event: &SchedulerEvent) {
        use SchedulerEvent::*;
        match event {
            Queued {
                provider_id,
                queue_depth,
                ..
            } => {
                if let Ok(id) = TraceId::new(provider_id) {
                    self.publisher.state(
                        &self.source(SourceType::Scheduler, "scheduler"),
                        StateKind::Checkpoint,
                        "provider_queued",
                        &format!("provider={} depth={queue_depth}", id.as_str()),
                    );
                }
            }
            Admitted {
                provider_id,
                queue_delay_ms,
                ..
            } => {
                if let Ok(id) = TraceId::new(provider_id) {
                    self.publisher.state(
                        &self.source(SourceType::Scheduler, "scheduler"),
                        StateKind::Checkpoint,
                        "provider_admitted",
                        &format!("provider={} delay_ms={queue_delay_ms}", id.as_str()),
                    );
                }
            }
            Selected {
                provider_id,
                model,
                attempt,
                routing_reason,
                score,
            } => {
                self.output_observed = false;
                if let Some(selection) = self.selection.checked_add(1) {
                    self.selection = selection;
                } else {
                    self.correlation = None;
                }
                if let Ok(id) = TraceId::new(provider_id) {
                    // Arbitrary display/config strings are never codes or Debug payloads.
                    let model = TraceId::new(model).ok();
                    let reason = match *routing_reason {
                        "fixed" => "fixed",
                        "preferred_order" => "preferred_order",
                        "auto_allocator" => "auto_allocator",
                        _ => "other",
                    };
                    self.publisher.state(
                        &self.source(SourceType::Scheduler, "scheduler"),
                        StateKind::Routing,
                        "provider_selected",
                        &format!(
                            "provider={} model={} attempt={attempt} reason={reason} score={}",
                            id.as_str(),
                            model.as_ref().map_or("unprojected", TraceId::as_str),
                            score.map_or_else(|| "absent".into(), |s| s.to_string())
                        ),
                    );
                }
            }
            Retry { .. } => self.publisher.state(
                &self.source(SourceType::Scheduler, "scheduler"),
                StateKind::Retry,
                "provider_retry",
                "",
            ),
            Fallback { from, to, .. } => {
                if let (Ok(from), Ok(to)) = (TraceId::new(from), TraceId::new(to)) {
                    self.publisher.state(
                        &self.source(SourceType::Scheduler, "scheduler"),
                        StateKind::Fallback,
                        "provider_fallback",
                        &format!("from={} to={}", from.as_str(), to.as_str()),
                    );
                }
            }
            OutputObserved { provider_id } => self.output(provider_id),
            Chunk { provider_id, text } => {
                if self.context.cognitive_role == CognitiveRole::Conversation
                    && matches!(
                        self.context.exposure_policy,
                        ExposurePolicy::ConversationOutput
                    )
                {
                    self.publisher.text(
                        &self.source(SourceType::CognitiveProvider, provider_id),
                        TextChannel::ProviderText,
                        text,
                    );
                } else if !text.is_empty() {
                    self.output(provider_id);
                }
            }
        }
    }
    fn output(&mut self, provider_id: &str) {
        if !self.output_observed {
            self.output_observed = true;
            self.publisher.state(
                &self.source(SourceType::CognitiveProvider, provider_id),
                StateKind::Checkpoint,
                "output_observed",
                "",
            );
        }
    }
}
