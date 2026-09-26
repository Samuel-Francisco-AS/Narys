use std::{fs, path::PathBuf, time::Duration};
use rusqlite::{Connection, OpenFlags};

#[derive(Clone)]
pub struct Database { path: PathBuf }

#[derive(Debug)]
pub enum PersistenceError { Unavailable, Migration, InvalidBootstrap, Conflict, Read, Write }
impl PersistenceError {
  pub fn code(&self) -> &'static str { match self {
    Self::Unavailable => "database_unavailable", Self::Migration => "migration_failed",
    Self::InvalidBootstrap => "bootstrap_invalid", Self::Conflict => "import_conflict",
    Self::Read => "read_failed", Self::Write => "write_failed",
  }}
}

impl Database {
  pub fn new(directory: PathBuf) -> Self { Self { path: directory.join("luna.sqlite3") } }
  pub fn open(&self) -> Result<Connection, PersistenceError> {
    let parent = self.path.parent().ok_or(PersistenceError::Unavailable)?;
    fs::create_dir_all(parent).map_err(|_| PersistenceError::Unavailable)?;
    #[cfg(unix)] {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|_| PersistenceError::Unavailable)?;
    }
    let conn = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
      .map_err(|_| PersistenceError::Unavailable)?;
    #[cfg(unix)] {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600)).map_err(|_| PersistenceError::Unavailable)?;
    }
    conn.busy_timeout(Duration::from_secs(3)).map_err(|_| PersistenceError::Unavailable)?;
    conn.pragma_update(None, "foreign_keys", "ON").map_err(|_| PersistenceError::Unavailable)?;
    super::migrations::apply(&conn)?;
    Ok(conn)
  }
  #[cfg(test)]
  pub fn for_test(path: PathBuf) -> Self { Self { path } }
}
