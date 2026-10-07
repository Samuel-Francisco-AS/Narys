use super::database::PersistenceError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresentationMode { Economy, Presence }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShellLayout {
    pub left_open: bool,
    pub left_width: u32,
    pub right_open: bool,
    pub right_width: u32,
}
impl ShellLayout {
    pub fn clamped(mut self) -> Self {
        self.left_width = self.left_width.clamp(160, 320);
        self.right_width = self.right_width.clamp(220, 360);
        self
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSettings {
    pub presentation_mode: PresentationMode,
    pub layout: ShellLayout,
}
pub fn load(conn: &Connection) -> Result<ShellSettings, PersistenceError> {
    conn.query_row("SELECT presentation_mode,left_open,left_width,right_open,right_width FROM shell_settings WHERE id=1", [], |r| {
        let mode: String = r.get(0)?;
        let presentation_mode = match mode.as_str() {
            "economy" => PresentationMode::Economy,
            "presence" => PresentationMode::Presence,
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        Ok(ShellSettings { presentation_mode, layout: ShellLayout {
            left_open: r.get(1)?, left_width: r.get(2)?, right_open: r.get(3)?, right_width: r.get(4)?,
        }.clamped() })
    }).map_err(|_| PersistenceError::Read)
}
pub fn save_mode(conn: &Connection, mode: PresentationMode) -> Result<(), PersistenceError> {
    let value = match mode { PresentationMode::Economy => "economy", PresentationMode::Presence => "presence" };
    let changed = conn.execute("UPDATE shell_settings SET presentation_mode=?1 WHERE id=1", [value]).map_err(|_| PersistenceError::Write)?;
    if changed != 1 { return Err(PersistenceError::Write); }
    Ok(())
}
pub fn save_layout(conn: &Connection, layout: ShellLayout) -> Result<(), PersistenceError> {
    let l = layout.clamped();
    let changed = conn.execute("UPDATE shell_settings SET left_open=?1,left_width=?2,right_open=?3,right_width=?4 WHERE id=1", params![l.left_open,l.left_width,l.right_open,l.right_width]).map_err(|_| PersistenceError::Write)?;
    if changed != 1 { return Err(PersistenceError::Write); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_defaults_persistence_clamp_and_cognitive_independence() {
        let conn = Connection::open_in_memory().unwrap();
        super::super::migrations::apply(&conn).unwrap();
        let policy = crate::cognition::policy::load(&conn, crate::cognition::policy::CognitiveRole::Conversation).unwrap();
        assert_eq!(load(&conn).unwrap().presentation_mode, PresentationMode::Economy);
        save_mode(&conn, PresentationMode::Presence).unwrap();
        save_layout(&conn, ShellLayout { left_open: false, left_width: 9999, right_open: false, right_width: 0 }).unwrap();
        let settings = load(&conn).unwrap();
        assert_eq!(settings.presentation_mode, PresentationMode::Presence);
        assert_eq!(settings.layout, ShellLayout { left_open: false, left_width: 320, right_open: false, right_width: 220 });
        save_mode(&conn, PresentationMode::Economy).unwrap();
        assert_eq!(load(&conn).unwrap().layout, settings.layout);
        assert_eq!(crate::cognition::policy::load(&conn, crate::cognition::policy::CognitiveRole::Conversation).unwrap(), policy);
        assert!(serde_json::from_str::<PresentationMode>("\"auto\"").is_err());
        assert!(serde_json::from_str::<PresentationMode>("\"headless\"").is_err());
    }
}
