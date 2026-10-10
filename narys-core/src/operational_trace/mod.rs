#[path = "../../../src-tauri/src/operational_trace/bus.rs"]
pub mod bus;
#[path = "../../../src-tauri/src/operational_trace/contract.rs"]
pub mod contract;
pub use bus::*;
pub use contract::*;
#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn stream(source: &str, value: &str) -> EventDraft {
        EventDraft::new(
            Provenance {
                source: TraceSource {
                    source_type: SourceType::Worker,
                    id: TraceId::new(source).unwrap(),
                    instance: None,
                },
                task_id: None,
                subtask_id: None,
                correlation_id: None,
                coalescing_key: None,
            },
            OperationalKind::TextDelta {
                channel: TextChannel::Stdout,
                text: TraceText::new(value).unwrap(),
            },
        )
        .unwrap()
    }
}
