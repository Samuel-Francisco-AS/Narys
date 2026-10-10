use rusqlite::Connection;
use crate::persistence::{conversation, database::PersistenceError, identity, memory::{self, MemoryFilter}};
use super::types::{ContextBundle, ContextMetadata};

pub const MAX_MEMORIES: u32 = 5;
pub const MAX_RECENT_MESSAGES: usize = 6;

#[derive(Clone, Debug)]
pub struct ContextRequest<'a> {
  pub domain: Option<&'a str>, pub kind: Option<&'a str>, pub min_importance: i64,
  pub memory_limit: u32, pub include_recent_conversation: bool,
}
#[derive(Debug, Eq, PartialEq)]
pub enum ContextError { IdentityUnavailable, Storage(&'static str) }
impl ContextError { pub fn code(&self) -> &'static str { match self { Self::IdentityUnavailable => "identity_unavailable", Self::Storage(code) => code } } }
impl From<PersistenceError> for ContextError { fn from(value: PersistenceError) -> Self { Self::Storage(value.code()) } }

pub struct ContextBuilder;
impl ContextBuilder {
  pub fn build(conn: &Connection, request: ContextRequest<'_>) -> Result<ContextBundle, ContextError> {
    let identity = identity::current_identity(conn)?.ok_or(ContextError::IdentityUnavailable)?.input;
    let memories = memory::active_memories(conn, MemoryFilter {
      kind: request.kind, domain: request.domain, min_importance: Some(request.min_importance.clamp(0, 10)),
      limit: request.memory_limit.min(MAX_MEMORIES),
    })?;
    let recent_messages = if request.include_recent_conversation { conversation::recent_messages_limited(conn, MAX_RECENT_MESSAGES)? } else { vec![] };
    let metadata = ContextMetadata { identity_version: identity.version.clone(), memory_count: memories.len(), recent_message_count: recent_messages.len() };
    Ok(ContextBundle { identity, relevant_memories: memories, recent_messages, metadata })
  }
}
