use super::database::PersistenceError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneralSettings {
    pub always_on_top: bool,
    pub active_fps: u32,
    pub background_fps: u32,
}

impl GeneralSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=60).contains(&self.active_fps) || !(1..=60).contains(&self.background_fps) {
            return Err("fps_invalid");
        }
        Ok(())
    }
}

pub fn load(conn: &Connection) -> Result<GeneralSettings, PersistenceError> {
    let (top, active, background): (i64, i64, i64) = conn
        .query_row(
            "SELECT always_on_top,active_fps,background_fps FROM general_settings WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| PersistenceError::Read)?;
    let settings = GeneralSettings {
        always_on_top: match top {
            0 => false,
            1 => true,
            _ => return Err(PersistenceError::Read),
        },
        active_fps: u32::try_from(active).map_err(|_| PersistenceError::Read)?,
        background_fps: u32::try_from(background).map_err(|_| PersistenceError::Read)?,
    };
    settings.validate().map_err(|_| PersistenceError::Read)?;
    Ok(settings)
}

pub fn save(conn: &Connection, settings: &GeneralSettings) -> Result<(), PersistenceError> {
    settings.validate().map_err(|_| PersistenceError::Write)?;
    let changed = conn.execute("UPDATE general_settings SET always_on_top=?1,active_fps=?2,background_fps=?3,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=1",
        params![settings.always_on_top, settings.active_fps, settings.background_fps]).map_err(|_| PersistenceError::Write)?;
    if changed != 1 {
        return Err(PersistenceError::Write);
    }
    Ok(())
}
