use super::*;
use std::sync::Arc;

pub const MAX_COALESCED_TEXT_BYTES: usize = 32 * 1024;
pub const MAX_COALESCED_FRAGMENTS: usize = 32;

/// A derived batch view; original history and live identities remain intact.
/// Contains an exact sequence range and both observation timestamps.
#[derive(Debug)]
pub struct CoalescedText {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub first_observed_at_unix_ms: u64,
    pub last_observed_at_unix_ms: u64,
    pub provenance: Provenance,
    pub channel: TextChannel,
    pub fragments: usize,
    text: String,
}
impl CoalescedText {
    pub fn text(&self) -> &str {
        &self.text
    }
}
#[derive(Debug)]
pub enum CoalescedItem {
    Event(Arc<OperationalEvent>),
    Text(CoalescedText),
}

/// Reads at most one capped batch, no persistent accumulator. Only adjacent,
/// contiguous TextDelta fragments with a present key and identical provenance
/// and channel can concatenate. No reordering, normalization or interpretation.
pub fn coalesce_batch(events: &[Arc<OperationalEvent>]) -> Result<Vec<CoalescedItem>, TraceError> {
    if events.len() > MAX_BATCH_EVENTS
        || events
            .iter()
            .map(|event| event.estimated_bytes())
            .sum::<usize>()
            > MAX_BATCH_BYTES
    {
        return Err(TraceError::InvalidBatchLimits);
    }
    let mut result = Vec::new();
    for event in events {
        let OperationalKind::TextDelta { channel, text } = event.kind() else {
            result.push(CoalescedItem::Event(event.clone()));
            continue;
        };
        if event.provenance().coalescing_key.is_none() {
            result.push(CoalescedItem::Event(event.clone()));
            continue;
        }
        if let Some(CoalescedItem::Text(previous)) = result.last_mut() {
            if previous.last_sequence.checked_add(1) == Some(event.sequence())
                && previous.provenance == *event.provenance()
                && previous.channel == *channel
                && previous.fragments < MAX_COALESCED_FRAGMENTS
                && previous.text.len() + text.as_str().len() <= MAX_COALESCED_TEXT_BYTES
            {
                previous.text.push_str(text.as_str());
                previous.last_sequence = event.sequence();
                previous.last_observed_at_unix_ms = event.observed_at_unix_ms();
                previous.fragments += 1;
                continue;
            }
        }
        result.push(CoalescedItem::Text(CoalescedText {
            first_sequence: event.sequence(),
            last_sequence: event.sequence(),
            first_observed_at_unix_ms: event.observed_at_unix_ms(),
            last_observed_at_unix_ms: event.observed_at_unix_ms(),
            provenance: event.provenance().clone(),
            channel: *channel,
            fragments: 1,
            text: text.as_str().into(),
        }));
    }
    Ok(result)
}
