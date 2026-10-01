use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri_plugin_stronghold::stronghold::Stronghold;

const SNAPSHOT: &str = "luna-lr3.stronghold";
const LEGACY_UNLOCK: &str = "luna-lr3.unlock";
const CLIENT: &[u8] = b"luna-core";
const SERVICE: &str = "br.com.assistente3d.app";
const ACCOUNT: &str = "stronghold-unlock-v1";

// Debug only: names are fixed code labels, never credential data or paths.
fn measured<T>(_stage: &'static str, operation: impl FnOnce() -> T) -> T {
    #[cfg(debug_assertions)]
    let started = std::time::Instant::now();
    let result = operation();
    #[cfg(debug_assertions)]
    eprintln!(
        "[SecretStore][diag] {_stage}_ms={}",
        started.elapsed().as_millis()
    );
    result
}

#[derive(Clone, Copy)]
pub enum SecretKey {
    Lr3Test,
    GeminiApiKey,
    GroqApiKey,
}
impl SecretKey {
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lr3Test => b"lr3_test_secret",
            Self::GeminiApiKey => b"gemini_api_key",
            Self::GroqApiKey => b"groq_api_key",
        }
    }
}

#[derive(Debug)]
pub enum SecretError {
    Path,
    Io,
    Random,
    CredentialStoreUnavailable,
    MissingUnlockKey,
    InvalidUnlockKey,
    InvalidStorageFile,
    LegacyPermissions,
    LegacyCleanupFailed,
    Snapshot,
    Client,
    Store,
    Persist,
    Lock,
}
impl SecretError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Path => "store_path",
            Self::Io => "store_io",
            Self::Random => "store_random",
            Self::CredentialStoreUnavailable => "unlock_store_unavailable",
            Self::MissingUnlockKey => "unlock_key_missing",
            Self::InvalidUnlockKey => "unlock_key_invalid",
            Self::InvalidStorageFile => "storage_file_invalid",
            Self::LegacyPermissions => "legacy_unlock_permissions",
            Self::LegacyCleanupFailed => "legacy_unlock_cleanup_failed",
            Self::Snapshot => "snapshot_unavailable",
            Self::Client => "client_unavailable",
            Self::Store => "store_operation_failed",
            Self::Persist => "snapshot_persist_failed",
            Self::Lock => "store_lock_failed",
        }
    }
}

pub trait UnlockKeyStore: Send + Sync {
    fn load(&self) -> Result<Option<Vec<u8>>, SecretError>;
    fn store(&self, key: &[u8]) -> Result<(), SecretError>;
    fn delete(&self) -> Result<(), SecretError>;
}

pub struct SystemCredentialStore;
impl SystemCredentialStore {
    fn entry() -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| SecretError::CredentialStoreUnavailable)
    }
}
impl UnlockKeyStore for SystemCredentialStore {
    fn load(&self) -> Result<Option<Vec<u8>>, SecretError> {
        match Self::entry()?.get_secret() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(SecretError::CredentialStoreUnavailable),
        }
    }
    fn store(&self, key: &[u8]) -> Result<(), SecretError> {
        Self::entry()?
            .set_secret(key)
            .map_err(|_| SecretError::CredentialStoreUnavailable)
    }
    fn delete(&self) -> Result<(), SecretError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(SecretError::CredentialStoreUnavailable),
        }
    }
}

pub struct SecretStore {
    directory: PathBuf,
    keys: Arc<dyn UnlockKeyStore>,
    operation: Mutex<()>,
}
impl SecretStore {
    pub fn new(directory: PathBuf) -> Self {
        Self::with_key_store(directory, Arc::new(SystemCredentialStore))
    }
    pub fn with_key_store(directory: PathBuf, keys: Arc<dyn UnlockKeyStore>) -> Self {
        Self {
            directory,
            keys,
            operation: Mutex::new(()),
        }
    }

    fn unlock_key(&self) -> Result<Vec<u8>, SecretError> {
        fs::create_dir_all(&self.directory).map_err(|_| SecretError::Path)?;
        private_directory_permissions(&self.directory).map_err(|_| SecretError::Path)?;
        let legacy = self.directory.join(LEGACY_UNLOCK);
        let snapshot = self.directory.join(SNAPSHOT);
        ensure_regular_file_or_missing(&legacy)?;
        ensure_regular_file_or_missing(&snapshot)?;
        if snapshot.exists() {
            private_permissions(&snapshot).map_err(|_| SecretError::Io)?;
        }
        let stored = measured("key_store_load", || self.keys.load())?;
        if let Some(key) = stored {
            validate_key(&key)?;
            if snapshot.exists() {
                validate_snapshot(&snapshot, &key)?;
            }
            if legacy.exists() {
                // The system store is authoritative. Remove an old file only after the
                // recovered credential has opened the existing snapshot.
                validate_legacy_permissions(&legacy)?;
                let mut old = Vec::new();
                File::open(&legacy)
                    .and_then(|mut f| f.read_to_end(&mut old))
                    .map_err(|_| SecretError::Io)?;
                if old != key {
                    return Err(SecretError::InvalidUnlockKey);
                }
                remove_legacy(&legacy)?;
            }
            return Ok(key);
        }
        if legacy.exists() {
            validate_legacy_permissions(&legacy)?;
            let mut key = Vec::new();
            File::open(&legacy)
                .and_then(|mut f| f.read_to_end(&mut key))
                .map_err(|_| SecretError::Io)?;
            validate_key(&key)?;
            if !snapshot.exists() {
                return Err(SecretError::MissingUnlockKey);
            }
            validate_snapshot(&snapshot, &key)?;
            self.keys.store(&key)?;
            let recovered = measured("key_store_load", || self.keys.load())?
                .ok_or(SecretError::MissingUnlockKey)?;
            if recovered != key {
                return Err(SecretError::InvalidUnlockKey);
            }
            validate_snapshot(&snapshot, &recovered)?;
            remove_legacy(&legacy)?;
            crate::security::audit::AuditEvent::new(
                crate::security::audit::Action::UnlockKeyMigrated,
                crate::security::audit::Outcome::Succeeded,
            )
            .emit();
            return Ok(recovered);
        }
        if snapshot.exists() {
            return Err(SecretError::MissingUnlockKey);
        }
        let mut key = vec![0_u8; 32];
        getrandom::fill(&mut key).map_err(|_| SecretError::Random)?;
        self.keys.store(&key)?;
        let recovered = measured("key_store_load", || self.keys.load())?
            .ok_or(SecretError::MissingUnlockKey)?;
        if recovered != key {
            return Err(SecretError::InvalidUnlockKey);
        }
        Ok(recovered)
    }

    fn with_client<T>(
        &self,
        write: bool,
        operation: impl FnOnce(&Stronghold, &iota_stronghold::Client) -> Result<T, SecretError>,
    ) -> Result<T, SecretError> {
        let _guard =
            measured("operation_wait", || self.operation.lock()).map_err(|_| SecretError::Lock)?;
        let snapshot = self.directory.join(SNAPSHOT);
        let key = measured("secret_unlock", || self.unlock_key())?;
        let (stronghold, client) = measured("secret_open_client", || {
            let stronghold = Stronghold::new(&snapshot, key).map_err(|_| SecretError::Snapshot)?;
            let client = if snapshot.exists() {
                stronghold
                    .load_client(CLIENT)
                    .map_err(|_| SecretError::Client)?
            } else {
                stronghold
                    .create_client(CLIENT)
                    .map_err(|_| SecretError::Client)?
            };
            Ok::<_, SecretError>((stronghold, client))
        })?;
        let output = measured("secret_lookup", || operation(&stronghold, &client))?;
        if write {
            stronghold
                .write_client(CLIENT)
                .map_err(|_| SecretError::Persist)?;
            stronghold.save().map_err(|_| SecretError::Persist)?;
            private_permissions(&snapshot).map_err(|_| SecretError::Persist)?;
        }
        Ok(output)
    }
    pub fn set_secret(&self, key: SecretKey, value: &[u8]) -> Result<(), SecretError> {
        self.with_client(true, |_, client| {
            client
                .store()
                .insert(key.bytes().to_vec(), value.to_vec(), None)
                .map_err(|_| SecretError::Store)?;
            Ok(())
        })
    }
    pub fn get_secret(&self, key: SecretKey) -> Result<Option<Vec<u8>>, SecretError> {
        self.with_client(false, |_, client| {
            client
                .store()
                .get(key.bytes())
                .map_err(|_| SecretError::Store)
        })
    }
    pub fn delete_secret(&self, key: SecretKey) -> Result<(), SecretError> {
        self.with_client(true, |_, client| {
            client
                .store()
                .delete(key.bytes())
                .map_err(|_| SecretError::Store)?;
            Ok(())
        })
    }
    pub fn legacy_key_present(&self) -> bool {
        self.directory.join(LEGACY_UNLOCK).exists()
    }
}
fn validate_key(key: &[u8]) -> Result<(), SecretError> {
    if key.len() == 32 {
        Ok(())
    } else {
        Err(SecretError::InvalidUnlockKey)
    }
}
fn validate_snapshot(snapshot: &Path, key: &[u8]) -> Result<(), SecretError> {
    measured("snapshot_validation", || {
        let stronghold =
            Stronghold::new(snapshot, key.to_vec()).map_err(|_| SecretError::Snapshot)?;
        stronghold
            .load_client(CLIENT)
            .map_err(|_| SecretError::Snapshot)?;
        Ok(())
    })
}
fn remove_legacy(path: &Path) -> Result<(), SecretError> {
    fs::remove_file(path).map_err(|_| SecretError::LegacyCleanupFailed)?;
    if path.exists() {
        return Err(SecretError::LegacyCleanupFailed);
    }
    Ok(())
}
fn ensure_regular_file_or_missing(path: &Path) -> Result<(), SecretError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(_) => Err(SecretError::InvalidStorageFile),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(SecretError::Io),
    }
}
fn validate_legacy_permissions(path: &Path) -> Result<(), SecretError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)
            .map_err(|_| SecretError::Io)?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(SecretError::LegacyPermissions);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
fn private_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
fn private_directory_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    #[derive(Default)]
    struct FakeKeys {
        value: Mutex<Option<Vec<u8>>>,
        unavailable: AtomicBool,
        corrupt_read: AtomicBool,
    }
    impl UnlockKeyStore for FakeKeys {
        fn load(&self) -> Result<Option<Vec<u8>>, SecretError> {
            if self.unavailable.load(Ordering::SeqCst) {
                return Err(SecretError::CredentialStoreUnavailable);
            }
            let mut value = self.value.lock().unwrap().clone();
            if self.corrupt_read.load(Ordering::SeqCst) {
                if let Some(ref mut key) = value {
                    key[0] ^= 1;
                }
            }
            Ok(value)
        }
        fn store(&self, key: &[u8]) -> Result<(), SecretError> {
            if self.unavailable.load(Ordering::SeqCst) {
                Err(SecretError::CredentialStoreUnavailable)
            } else {
                *self.value.lock().unwrap() = Some(key.to_vec());
                Ok(())
            }
        }
        fn delete(&self) -> Result<(), SecretError> {
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }
    fn fixture() -> (PathBuf, Arc<FakeKeys>) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        (
            std::env::temp_dir().join(format!("luna-secret-{}-{nonce}", std::process::id())),
            Arc::new(FakeKeys::default()),
        )
    }
    #[test]
    fn first_run_reopen_and_fail_closed() {
        let (dir, keys) = fixture();
        let store = SecretStore::with_key_store(dir.clone(), keys.clone());
        assert_eq!(store.get_secret(SecretKey::Lr3Test).unwrap(), None);
        assert_eq!(keys.load().unwrap().unwrap().len(), 32);
        assert!(!dir.join(LEGACY_UNLOCK).exists());
        store.set_secret(SecretKey::Lr3Test, b"artificial").unwrap();
        let snapshot = fs::read(dir.join(SNAPSHOT)).unwrap();
        assert!(!snapshot.windows(10).any(|part| part == b"artificial"));
        drop(store);
        let reopened = SecretStore::with_key_store(dir.clone(), keys.clone());
        assert_eq!(
            reopened.get_secret(SecretKey::Lr3Test).unwrap(),
            Some(b"artificial".to_vec())
        );
        keys.delete().unwrap();
        assert_eq!(
            reopened.get_secret(SecretKey::Lr3Test).unwrap_err().code(),
            "unlock_key_missing"
        );
        keys.unavailable.store(true, Ordering::SeqCst);
        assert_eq!(
            reopened.get_secret(SecretKey::Lr3Test).unwrap_err().code(),
            "unlock_store_unavailable"
        );
        keys.unavailable.store(false, Ordering::SeqCst);
        *keys.value.lock().unwrap() = Some(vec![1; 10]);
        assert_eq!(
            reopened.get_secret(SecretKey::Lr3Test).unwrap_err().code(),
            "unlock_key_invalid"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn legacy_migration_preserves_secret() {
        let (dir, keys) = fixture();
        fs::create_dir_all(&dir).unwrap();
        let mut old = vec![0; 32];
        getrandom::fill(&mut old).unwrap();
        let snapshot = dir.join(SNAPSHOT);
        let stronghold = Stronghold::new(&snapshot, old.clone()).unwrap();
        let client = stronghold.create_client(CLIENT).unwrap();
        client
            .store()
            .insert(
                SecretKey::Lr3Test.bytes().to_vec(),
                b"artificial".to_vec(),
                None,
            )
            .unwrap();
        stronghold.write_client(CLIENT).unwrap();
        stronghold.save().unwrap();
        let legacy = dir.join(LEGACY_UNLOCK);
        fs::write(&legacy, &old).unwrap();
        private_permissions(&legacy).unwrap();
        let store = SecretStore::with_key_store(dir.clone(), keys.clone());
        keys.unavailable.store(true, Ordering::SeqCst);
        assert_eq!(
            store.get_secret(SecretKey::Lr3Test).unwrap_err().code(),
            "unlock_store_unavailable"
        );
        assert!(legacy.exists());
        keys.unavailable.store(false, Ordering::SeqCst);
        assert_eq!(
            store.get_secret(SecretKey::Lr3Test).unwrap(),
            Some(b"artificial".to_vec())
        );
        assert!(!legacy.exists());
        assert!(snapshot.exists());
        assert_eq!(keys.load().unwrap().unwrap(), old);
        drop(store);
        let reopened = SecretStore::with_key_store(dir.clone(), keys);
        assert_eq!(
            reopened.get_secret(SecretKey::Lr3Test).unwrap(),
            Some(b"artificial".to_vec())
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn corrupt_snapshot_and_invalid_legacy_fail_closed() {
        let (dir, keys) = fixture();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(SNAPSHOT), b"corrupt").unwrap();
        fs::write(dir.join(LEGACY_UNLOCK), vec![0; 32]).unwrap();
        private_permissions(&dir.join(LEGACY_UNLOCK)).unwrap();
        let store = SecretStore::with_key_store(dir.clone(), keys.clone());
        assert_eq!(
            store.get_secret(SecretKey::Lr3Test).unwrap_err().code(),
            "snapshot_unavailable"
        );
        assert!(keys.load().unwrap().is_none());
        assert!(dir.join(LEGACY_UNLOCK).exists());
        fs::write(dir.join(LEGACY_UNLOCK), b"short").unwrap();
        assert_eq!(
            store.get_secret(SecretKey::Lr3Test).unwrap_err().code(),
            "unlock_key_invalid"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn migration_verification_failure_keeps_legacy_for_retry() {
        let (dir, keys) = fixture();
        fs::create_dir_all(&dir).unwrap();
        let mut old = vec![0; 32];
        getrandom::fill(&mut old).unwrap();
        let snapshot = dir.join(SNAPSHOT);
        let stronghold = Stronghold::new(&snapshot, old.clone()).unwrap();
        stronghold.create_client(CLIENT).unwrap();
        stronghold.write_client(CLIENT).unwrap();
        stronghold.save().unwrap();
        let legacy = dir.join(LEGACY_UNLOCK);
        fs::write(&legacy, &old).unwrap();
        private_permissions(&legacy).unwrap();
        let store = SecretStore::with_key_store(dir.clone(), keys.clone());
        keys.corrupt_read.store(true, Ordering::SeqCst);
        assert_eq!(
            store.get_secret(SecretKey::Lr3Test).unwrap_err().code(),
            "unlock_key_invalid"
        );
        assert!(legacy.exists());
        assert!(snapshot.exists());
        keys.corrupt_read.store(false, Ordering::SeqCst);
        assert_eq!(store.get_secret(SecretKey::Lr3Test).unwrap(), None);
        assert!(!legacy.exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn permissions_are_revalidated_on_reopen() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, keys) = fixture();
        let store = SecretStore::with_key_store(dir.clone(), keys);
        store.set_secret(SecretKey::Lr3Test, b"artificial").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(dir.join(SNAPSHOT), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            store.get_secret(SecretKey::Lr3Test).unwrap(),
            Some(b"artificial".to_vec())
        );
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(dir.join(SNAPSHOT))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
