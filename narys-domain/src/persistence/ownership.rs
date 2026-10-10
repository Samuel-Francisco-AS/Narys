//! Process-lifetime single-writer lease. SQLite locks serialize transactions;
//! this lock serializes operational authority and recovery across processes.
use super::database::PersistenceError;
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};
pub struct WriterLease(File);
impl WriterLease {
    /// Connections/handles within one process share its operational lease.
    pub fn shared(path: &Path) -> Result<std::sync::Arc<Self>, PersistenceError> {
        use std::{
            collections::HashMap,
            path::PathBuf,
            sync::{Arc, Mutex, OnceLock, Weak},
        };
        static LEASES: OnceLock<Mutex<HashMap<PathBuf, Weak<WriterLease>>>> = OnceLock::new();
        let key = path
            .parent()
            .ok_or(PersistenceError::Ownership)?
            .canonicalize()
            .map_err(|_| PersistenceError::Ownership)?
            .join(path.file_name().ok_or(PersistenceError::Ownership)?);
        let mut leases = LEASES
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| PersistenceError::Ownership)?;
        leases.retain(|_, lease| lease.strong_count() > 0);
        if let Some(lease) = leases.get(&key).and_then(Weak::upgrade) {
            return Ok(lease);
        }
        let lease = Arc::new(Self::acquire(&key)?);
        leases.insert(key, Arc::downgrade(&lease));
        Ok(lease)
    }
    pub fn acquire(path: &Path) -> Result<Self, PersistenceError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| PersistenceError::Ownership)?;
        let m = file.metadata().map_err(|_| PersistenceError::Ownership)?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o077 != 0
            || m.nlink() != 1
        {
            return Err(PersistenceError::Ownership);
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(PersistenceError::Ownership);
        }
        Ok(Self(file))
    }
}
impl Drop for WriterLease {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writer_lease_rejects_duplicates_and_symlinks() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("writer.lock");
        let first = WriterLease::acquire(&p).unwrap();
        assert!(WriterLease::acquire(&p).is_err());
        drop(first);
        assert!(WriterLease::acquire(&p).is_ok());
        let link = d.path().join("link");
        std::os::unix::fs::symlink(&p, &link).unwrap();
        assert!(WriterLease::acquire(&link).is_err());
    }
}
