use super::ownership::WriterLease;
use rusqlite::{Connection, OpenFlags};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
    lease: Arc<Mutex<Option<Arc<WriterLease>>>>,
}

#[derive(Debug)]
pub enum PersistenceError {
    Unavailable,
    Migration,
    InvalidBootstrap,
    Conflict,
    Read,
    Write,
    Ownership,
    CoreOwned,
}
impl PersistenceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "database_unavailable",
            Self::Migration => "migration_failed",
            Self::InvalidBootstrap => "bootstrap_invalid",
            Self::Conflict => "import_conflict",
            Self::Ownership => "database_writer_busy_or_unsafe",
            Self::CoreOwned => "desktop_migrated_use_core_ipc",
            Self::Read => "read_failed",
            Self::Write => "write_failed",
        }
    }
}

impl Database {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            path: directory.join("luna.sqlite3"),
            lease: Arc::default(),
        }
    }
    pub fn open(&self) -> Result<Connection, PersistenceError> {
        let parent = self.path.parent().ok_or(PersistenceError::Unavailable)?;
        fs::create_dir_all(parent).map_err(|_| PersistenceError::Unavailable)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let m = fs::symlink_metadata(parent).map_err(|_| PersistenceError::Unavailable)?;
            if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } {
                return Err(PersistenceError::Ownership);
            }
            if let Ok(m) = fs::symlink_metadata(&self.path) {
                if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.nlink() != 1 {
                    return Err(PersistenceError::Ownership);
                }
            }
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|_| PersistenceError::Unavailable)?;
        }
        if self.path.with_extension("sqlite3.core-owned").exists() {
            return Err(PersistenceError::CoreOwned);
        }
        let mut lease = self.lease.lock().map_err(|_| PersistenceError::Ownership)?;
        if lease.is_none() {
            *lease = Some(WriterLease::shared(
                &self.path.with_extension("sqlite3.writer.lock"),
            )?);
        }
        drop(lease);
        let conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| PersistenceError::Unavailable)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))
                .map_err(|_| PersistenceError::Unavailable)?;
        }
        conn.busy_timeout(Duration::from_secs(3))
            .map_err(|_| PersistenceError::Unavailable)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|_| PersistenceError::Unavailable)?;
        super::migrations::apply(&conn)?;
        Ok(conn)
    }
    #[cfg(any(test, feature = "desktop-tests"))]
    pub fn for_test(path: PathBuf) -> Self {
        Self {
            path,
            lease: Arc::default(),
        }
    }
}
