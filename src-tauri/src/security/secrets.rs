use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, sync::Mutex};

use tauri_plugin_stronghold::stronghold::Stronghold;

const SNAPSHOT: &str = "luna-lr3.stronghold";
const UNLOCK_KEY: &str = "luna-lr3.unlock";
const CLIENT: &[u8] = b"luna-core";

// Public provider settings (enabled, priority, model) can later be stored normally.
// ProviderSecret must be mapped to a fixed backend key here, never accepted as an
// arbitrary key/value pair from an IPC command.
#[derive(Clone, Copy)]
pub enum SecretKey { Lr3Test }

impl SecretKey {
  fn bytes(self) -> &'static [u8] {
    match self { Self::Lr3Test => b"lr3_test_secret" }
  }
}

#[derive(Debug)]
pub enum SecretError {
  Path,
  Io,
  Random,
  MissingUnlockKey,
  InvalidUnlockKey,
  InvalidStorageFile,
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
      Self::MissingUnlockKey => "unlock_key_missing",
      Self::InvalidUnlockKey => "unlock_key_invalid",
      Self::InvalidStorageFile => "storage_file_invalid",
      Self::Snapshot => "snapshot_unavailable",
      Self::Client => "client_unavailable",
      Self::Store => "store_operation_failed",
      Self::Persist => "snapshot_persist_failed",
      Self::Lock => "store_lock_failed",
    }
  }
}

pub struct SecretStore {
  directory: PathBuf,
  operation: Mutex<()>,
}

impl SecretStore {
  pub fn new(directory: PathBuf) -> Self { Self { directory, operation: Mutex::new(()) } }

  fn unlock_key(&self) -> Result<Vec<u8>, SecretError> {
    fs::create_dir_all(&self.directory).map_err(|_| SecretError::Path)?;
    private_directory_permissions(&self.directory).map_err(|_| SecretError::Path)?;
    let key_path = self.directory.join(UNLOCK_KEY);
    let snapshot_path = self.directory.join(SNAPSHOT);
    ensure_regular_file_or_missing(&key_path)?;
    ensure_regular_file_or_missing(&snapshot_path)?;

    if !key_path.exists() {
      // Never silently replace the key of an existing vault.
      if snapshot_path.exists() { return Err(SecretError::MissingUnlockKey); }
      let mut key = vec![0_u8; 32];
      getrandom::fill(&mut key).map_err(|_| SecretError::Random)?;
      match new_private_file(&key_path) {
        Ok(mut file) => {
          file.write_all(&key).and_then(|_| file.sync_all()).map_err(|_| SecretError::Io)?;
          return Ok(key);
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(SecretError::Io),
      }
    }

    let mut key = Vec::new();
    File::open(&key_path).and_then(|mut file| file.read_to_end(&mut key)).map_err(|_| SecretError::Io)?;
    if key.len() != 32 { return Err(SecretError::InvalidUnlockKey); }
    Ok(key)
  }

  fn with_client<T>(&self, write: bool, operation: impl FnOnce(&tauri_plugin_stronghold::stronghold::Stronghold, &iota_stronghold::Client) -> Result<T, SecretError>) -> Result<T, SecretError> {
    let _guard = self.operation.lock().map_err(|_| SecretError::Lock)?;
    let snapshot_path = self.directory.join(SNAPSHOT);
    let key = self.unlock_key()?;
    let stronghold = Stronghold::new(&snapshot_path, key).map_err(|_| SecretError::Snapshot)?;
    let client = if snapshot_path.exists() {
      stronghold.load_client(CLIENT).map_err(|_| SecretError::Client)?
    } else {
      stronghold.create_client(CLIENT).map_err(|_| SecretError::Client)?
    };
    let output = operation(&stronghold, &client)?;
    if write {
      stronghold.write_client(CLIENT).map_err(|_| SecretError::Persist)?;
      stronghold.save().map_err(|_| SecretError::Persist)?;
      private_permissions(&snapshot_path).map_err(|_| SecretError::Persist)?;
    }
    Ok(output)
  }

  pub fn set_secret(&self, key: SecretKey, value: &[u8]) -> Result<(), SecretError> {
    self.with_client(true, |_, client| {
      client.store().insert(key.bytes().to_vec(), value.to_vec(), None).map_err(|_| SecretError::Store)?;
      Ok(())
    })
  }

  pub fn get_secret(&self, key: SecretKey) -> Result<Option<Vec<u8>>, SecretError> {
    self.with_client(false, |_, client| client.store().get(key.bytes()).map_err(|_| SecretError::Store))
  }

  pub fn delete_secret(&self, key: SecretKey) -> Result<(), SecretError> {
    self.with_client(true, |_, client| {
      client.store().delete(key.bytes()).map_err(|_| SecretError::Store)?;
      Ok(())
    })
  }
}

fn ensure_regular_file_or_missing(path: &Path) -> Result<(), SecretError> {
  match fs::symlink_metadata(path) {
    Ok(metadata) if metadata.file_type().is_file() => Ok(()),
    Ok(_) => Err(SecretError::InvalidStorageFile),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
    Err(_) => Err(SecretError::Io),
  }
}

fn new_private_file(path: &Path) -> std::io::Result<File> {
  let mut options = OpenOptions::new();
  options.write(true).create_new(true);
  #[cfg(unix)] {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
  }
  options.open(path)
}

fn private_permissions(path: &Path) -> std::io::Result<()> {
  #[cfg(unix)] {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
  }
  #[cfg(not(unix))] { let _ = path; }
  Ok(())
}

fn private_directory_permissions(path: &Path) -> std::io::Result<()> {
  #[cfg(unix)] {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
  }
  #[cfg(not(unix))] { let _ = path; }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::{SystemTime, UNIX_EPOCH};

  #[test]
  fn artificial_secret_roundtrip_persists_and_deletes() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let directory = std::env::temp_dir().join(format!("luna-lr3-{}-{nonce}", std::process::id()));
    let first = SecretStore::new(directory.clone());
    assert_eq!(first.get_secret(SecretKey::Lr3Test).unwrap(), None);
    first.set_secret(SecretKey::Lr3Test, b"lr3_test_secret").unwrap();
    let snapshot = fs::read(directory.join(SNAPSHOT)).unwrap();
    assert!(!snapshot.windows(b"lr3_test_secret".len()).any(|part| part == b"lr3_test_secret"));
    drop(first);
    let reopened = SecretStore::new(directory.clone());
    assert_eq!(reopened.get_secret(SecretKey::Lr3Test).unwrap(), Some(b"lr3_test_secret".to_vec()));
    reopened.delete_secret(SecretKey::Lr3Test).unwrap();
    assert_eq!(reopened.get_secret(SecretKey::Lr3Test).unwrap(), None);
    fs::remove_dir_all(directory).unwrap();
  }

  #[test]
  fn missing_unlock_key_fails_closed() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let directory = std::env::temp_dir().join(format!("luna-lr3-missing-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    File::create(directory.join(SNAPSHOT)).unwrap();
    let store = SecretStore::new(directory.clone());
    assert_eq!(store.get_secret(SecretKey::Lr3Test).unwrap_err().code(), "unlock_key_missing");
    fs::remove_dir_all(directory).unwrap();
  }

  #[test]
  fn corrupt_snapshot_fails_closed() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let directory = std::env::temp_dir().join(format!("luna-lr3-corrupt-{}-{nonce}", std::process::id()));
    let store = SecretStore::new(directory.clone());
    assert_eq!(store.get_secret(SecretKey::Lr3Test).unwrap(), None);
    fs::write(directory.join(SNAPSHOT), b"corrupt snapshot").unwrap();
    assert_eq!(store.get_secret(SecretKey::Lr3Test).unwrap_err().code(), "snapshot_unavailable");
    fs::remove_dir_all(directory).unwrap();
  }
}
