use super::database::PersistenceError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeminiTimeouts {
    pub request_timeout_ms: u32,
    pub stream_idle_timeout_ms: u32,
}

impl Default for GeminiTimeouts {
    fn default() -> Self {
        Self {
            request_timeout_ms: 45_000,
            stream_idle_timeout_ms: 15_000,
        }
    }
}

impl GeminiTimeouts {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_timeout_ms == 0 || self.stream_idle_timeout_ms == 0 {
            return Err("gemini_timeout_invalid");
        }
        Ok(())
    }
}

pub fn load(conn: &Connection) -> Result<GeminiTimeouts, PersistenceError> {
    let (request, idle): (i64, i64) = conn.query_row(
        "SELECT request_timeout_ms,stream_idle_timeout_ms FROM gemini_provider_settings WHERE id=1", [],
        |r| Ok((r.get(0)?,r.get(1)?))).map_err(|_| PersistenceError::Read)?;
    let timeouts = GeminiTimeouts {
        request_timeout_ms: u32::try_from(request).map_err(|_| PersistenceError::Read)?,
        stream_idle_timeout_ms: u32::try_from(idle).map_err(|_| PersistenceError::Read)?,
    };
    timeouts.validate().map_err(|_| PersistenceError::Read)?;
    Ok(timeouts)
}

pub fn save(conn: &Connection, timeouts: &GeminiTimeouts) -> Result<(), PersistenceError> {
    timeouts.validate().map_err(|_| PersistenceError::Write)?;
    let changed = conn.execute("UPDATE gemini_provider_settings SET request_timeout_ms=?1,stream_idle_timeout_ms=?2,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=1",
        params![timeouts.request_timeout_ms, timeouts.stream_idle_timeout_ms]).map_err(|_| PersistenceError::Write)?;
    if changed != 1 {
        return Err(PersistenceError::Write);
    }
    Ok(())
}
