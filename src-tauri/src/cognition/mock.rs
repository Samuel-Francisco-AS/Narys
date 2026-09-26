use std::{sync::atomic::{AtomicBool, AtomicU32, Ordering}, time::Duration};
use super::{provider::{Provider, ProviderFuture}, types::{ProviderChunk, ProviderError, ProviderRequest, ProviderResponse, ProviderUsage}};

#[derive(Clone, Copy, Debug)]
pub enum MockScenario { Normal, Streaming, TransientThenSuccess, RateLimited, Timeout, QuotaExceeded, Fatal }
pub struct MockProvider { scenario: MockScenario, calls: AtomicU32 }
impl MockProvider {
  pub fn new(scenario: MockScenario) -> Self { Self { scenario, calls: AtomicU32::new(0) } }
  #[cfg(test)] pub fn calls(&self) -> u32 { self.calls.load(Ordering::SeqCst) }
}
async fn delay(cancelled: &AtomicBool, millis: u64) -> Result<(), ProviderError> {
  let deadline = tokio::time::Instant::now() + Duration::from_millis(millis);
  while tokio::time::Instant::now() < deadline {
    if cancelled.load(Ordering::Acquire) { return Err(ProviderError::Cancelled); }
    tokio::time::sleep(Duration::from_millis(15)).await;
  }
  if cancelled.load(Ordering::Acquire) { Err(ProviderError::Cancelled) } else { Ok(()) }
}
impl Provider for MockProvider {
  fn execute<'a>(&'a self, request: &'a ProviderRequest, cancelled: &'a AtomicBool,
    on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send)) -> ProviderFuture<'a> {
    Box::pin(async move {
      if cancelled.load(Ordering::Acquire) { return Err(ProviderError::Cancelled); }
      let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
      // Check structure only. The mock never echoes identity, memories or conversation.
      if request.context.identity.canonical_name.is_empty() { return Err(ProviderError::Fatal); }
      match self.scenario {
        MockScenario::RateLimited => return Err(ProviderError::RateLimited { retry_after_ms: Some(3_000) }),
        MockScenario::QuotaExceeded => return Err(ProviderError::QuotaExceeded),
        MockScenario::Fatal => return Err(ProviderError::Fatal),
        MockScenario::Timeout if call == 1 => { delay(cancelled, 100).await?; return Err(ProviderError::Timeout); },
        MockScenario::TransientThenSuccess if call == 1 => return Err(ProviderError::Timeout),
        _ => {}
      }
      let pieces: Vec<String> = if matches!(self.scenario, MockScenario::Streaming) {
        vec!["Analisando ".into(), "contexto ".into(), "local...".into()]
      } else {
        vec![format!("MockProvider concluiu a tarefa usando identidade e {} memórias relevantes.", request.context.relevant_memories.len())]
      };
      let mut text = String::new();
      let mut output_tokens = 0;
      for piece in pieces {
        delay(cancelled, if matches!(self.scenario, MockScenario::Streaming) { 120 } else { 40 }).await?;
        let words = piece.split_whitespace().count() as u32;
        let remaining = request.max_output_tokens.saturating_sub(output_tokens);
        if remaining == 0 { break; }
        let chunk = if words > remaining { piece.split_whitespace().take(remaining as usize).collect::<Vec<_>>().join(" ") } else { piece };
        output_tokens += chunk.split_whitespace().count() as u32;
        text.push_str(&chunk);
        on_chunk(ProviderChunk { text: chunk })?;
      }
      Ok(ProviderResponse { text, usage: ProviderUsage { calls: 1,
        input_tokens: 8 + request.context.relevant_memories.len() as u32 * 2 + request.context.recent_messages.len() as u32 * 2,
        output_tokens } })
    })
  }
}
