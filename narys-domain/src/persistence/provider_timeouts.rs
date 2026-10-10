use super::database::PersistenceError;
use crate::cognition::types::ProviderTimeouts;
use rusqlite::{params, Connection};

pub fn load(conn: &Connection, provider_id: &str) -> Result<ProviderTimeouts, PersistenceError> {
    let (request, idle): (i64, i64) = conn.query_row(
        "SELECT request_timeout_ms,stream_idle_timeout_ms FROM provider_timeout_settings WHERE provider_id=?1",
        [provider_id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(|_| PersistenceError::Read)?;
    let timeouts = ProviderTimeouts {
        request_timeout_ms: u32::try_from(request).map_err(|_| PersistenceError::Read)?,
        stream_idle_timeout_ms: u32::try_from(idle).map_err(|_| PersistenceError::Read)?,
    };
    if timeouts.request_timeout_ms == 0 || timeouts.stream_idle_timeout_ms == 0 { return Err(PersistenceError::Read); }
    Ok(timeouts)
}

pub fn save(conn: &Connection, provider_id: &str, timeouts: ProviderTimeouts) -> Result<(), PersistenceError> {
    if timeouts.request_timeout_ms == 0 || timeouts.stream_idle_timeout_ms == 0 { return Err(PersistenceError::Write); }
    let changed = conn.execute(
        "UPDATE provider_timeout_settings SET request_timeout_ms=?2,stream_idle_timeout_ms=?3,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE provider_id=?1",
        params![provider_id, timeouts.request_timeout_ms, timeouts.stream_idle_timeout_ms],
    ).map_err(|_| PersistenceError::Write)?;
    if changed != 1 { return Err(PersistenceError::Write); }
    Ok(())
}
