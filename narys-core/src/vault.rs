//! Existing snapshot only. No creation, migration, chmod, save or secret output.
use iota_stronghold::{KeyProvider, SnapshotPath, Stronghold};
use std::{fs, os::unix::fs::MetadataExt, path::Path};
use zeroize::Zeroizing;

fn secure(path: &Path, directory: bool) -> Result<(), &'static str> {
    let m = fs::symlink_metadata(path).map_err(|_| "existing_vault_missing")?;
    if m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || (directory && !m.is_dir())
        || (!directory && (!m.is_file() || m.nlink() != 1))
    {
        return Err("unsafe_existing_vault");
    }
    Ok(())
}
pub fn existing_status(directory: &Path) -> Result<(), &'static str> {
    secure(directory, true)?;
    if directory
        .join("luna-lr3.unlock")
        .try_exists()
        .map_err(|_| "vault_metadata_failed")?
    {
        return Err("legacy_migration_not_authorized");
    }
    let path = directory.join("luna-lr3.stronghold");
    secure(&path, false)?;
    // Same service/account and key-provider contract as security/secrets.rs.
    // Normal application credential resolution; no value leaves this module.
    let key = keyring::Entry::new("br.com.assistente3d.app", "stronghold-unlock-v1")
        .map_err(|_| "credential_store_unavailable")?
        .get_secret()
        .map_err(|_| "existing_unlock_key_unavailable")?;
    open_existing(&path, key)
}
fn open_existing(path: &Path, key: Vec<u8>) -> Result<(), &'static str> {
    let key = Zeroizing::new(key);
    secure(path, false)?;
    if key.len() != 32 {
        return Err("invalid_existing_key");
    }
    let before = fs::metadata(path).map_err(|_| "vault_metadata_failed")?;
    let stronghold = Stronghold::default();
    let key = KeyProvider::try_from(key).map_err(|_| "invalid_existing_key")?;
    stronghold
        .load_snapshot(&key, &SnapshotPath::from_path(path))
        .map_err(|_| "snapshot_open_failed")?;
    stronghold
        .load_client(b"luna-core")
        .map_err(|_| "existing_client_missing")?;
    let after = fs::symlink_metadata(path).map_err(|_| "vault_metadata_failed")?;
    if (
        before.dev(),
        before.ino(),
        before.len(),
        before.mtime(),
        before.mtime_nsec(),
        before.ctime(),
        before.ctime_nsec(),
    ) != (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec(),
    ) {
        return Err("vault_metadata_changed");
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn missing_snapshot_never_created() {
        let d = tempfile::tempdir().unwrap();
        assert!(existing_status(d.path()).is_err());
        assert!(fs::read_dir(d.path()).unwrap().next().is_none());
    }
    #[test]
    fn real_backend_opens_existing_synthetic_without_write() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("snapshot");
        let sh = Stronghold::default();
        sh.create_client(b"luna-core").unwrap();
        sh.write_client(b"luna-core").unwrap();
        let key = KeyProvider::try_from(Zeroizing::new(vec![7; 32])).unwrap();
        sh.commit_with_keyprovider(&SnapshotPath::from_path(&p), &key)
            .unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        let bytes = fs::read(&p).unwrap();
        open_existing(&p, vec![7; 32]).unwrap();
        assert_eq!(fs::read(&p).unwrap(), bytes);
        assert_eq!(open_existing(&p, vec![8; 32]), Err("snapshot_open_failed"));
    }
    #[test]
    fn symlink_rejected() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("link");
        std::os::unix::fs::symlink("/etc/hosts", &p).unwrap();
        assert!(open_existing(&p, vec![7; 32]).is_err());
    }
}
