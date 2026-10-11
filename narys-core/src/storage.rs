//! Controlled takeover of existing SQLite, never of Stronghold or its directory.
//! Source files are left intact; SQLite online backups include committed WAL data.
use crate::{
    persistence::{migrations, ownership::WriterLease},
    server::{mkdir, Config},
};
use rusqlite::{Connection, OpenFlags};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};
const MIGRATION: &str = "server-1a-desktop-takeover";
fn safe_file(path: &Path) -> Result<(), &'static str> {
    let m = fs::symlink_metadata(path).map_err(|_| "migration_source_missing")?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.mode() & 0o077 != 0
    {
        return Err("unsafe_migration_source");
    }
    Ok(())
}
fn read(path: &Path) -> Result<Connection, &'static str> {
    safe_file(path)?;
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "migration_read_failed")?;
    let version: u64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| "migration_schema_invalid")?;
    if !(18..=22).contains(&version) {
        return Err("migration_source_schema_unsupported");
    }
    let check: String = db
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|_| "migration_integrity_failed")?;
    if check != "ok" {
        return Err("migration_integrity_failed");
    }
    Ok(db)
}
fn snapshot(source: &Connection, target: &Path) -> Result<(), &'static str> {
    source
        .backup(rusqlite::DatabaseName::Main, target, None)
        .map_err(|_| "migration_backup_failed")?;
    fs::set_permissions(target, fs::Permissions::from_mode(0o600))
        .map_err(|_| "migration_backup_permissions")?;
    fs::File::open(target)
        .and_then(|f| f.sync_all())
        .map_err(|_| "migration_sync_failed")
}
fn marker(source: &Path) -> Result<(), &'static str> {
    let path = source.with_extension("sqlite3.core-owned");
    let bytes = b"server-1a: authoritative database is ~/.local/state/narys/core/db/luna.sqlite3; use Core IPC\n";
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
    {
        Ok(mut f) => f
            .write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "migration_marker_failed")?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            safe_file(&path)?;
            if fs::read(&path).map_err(|_| "migration_marker_failed")? != bytes {
                return Err("migration_marker_conflict");
            }
        }
        Err(_) => return Err("migration_marker_failed"),
    }
    sync_parent(source)
}
fn sync_parent(path: &Path) -> Result<(), &'static str> {
    fs::File::open(path.parent().ok_or("migration_parent_missing")?)
        .and_then(|f| f.sync_all())
        .map_err(|_| "migration_sync_failed")
}
/// Called before recovery/socket readiness and under the service process lock.
/// Reentrant after crash: published migration row completes the desktop fence.
pub fn initialize(config: &Config) -> Result<(), &'static str> {
    let directory = config.state.join("db");
    mkdir(&directory)?;
    let target = directory.join("luna.sqlite3");
    // Before any Database::open can migrate the authority, preserve a consistent
    // online snapshot under its writer fence. Never roll it back automatically.
    if target.exists() {
        let _lease = WriterLease::acquire(&target.with_extension("sqlite3.writer.lock"))
            .map_err(|e| e.code())?;
        let original = read(&target)?;
        let version: u64 = original
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| "migration_schema_invalid")?;
        if version < 22 {
            let backups = config.state.join("backups");
            mkdir(&backups)?;
            let backup = tempfile::Builder::new()
                .prefix("lr10c-schema022-")
                .tempdir_in(&backups)
                .map_err(|_| "migration_backup_directory_failed")?
                .keep();
            snapshot(
                &original,
                &backup.join("authority-before-schema022.sqlite3"),
            )?;
            sync_parent(&backup.join("authority-before-schema022.sqlite3"))?;
        }
    }
    let desktop = config
        .home
        .join(".local/share/br.com.assistente3d.app/luna.sqlite3");
    // A synthetic/new installation has no legacy source. Never invent personal data.
    if !desktop
        .try_exists()
        .map_err(|_| "migration_source_status_failed")?
    {
        config.db()?;
        return Ok(());
    }
    // Historical desktop binaries do not know the ownership contract. Never
    // import while a known legacy desktop process is alive, even between opens.
    for entry in fs::read_dir("/proc").map_err(|_| "migration_process_inventory_failed")? {
        let entry = entry.map_err(|_| "migration_process_inventory_failed")?;
        if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
            continue;
        }
        if entry
            .metadata()
            .is_ok_and(|m| m.uid() == unsafe { libc::geteuid() })
        {
            if let Ok(exe) = fs::read_link(entry.path().join("exe")) {
                if exe
                    .file_name()
                    .is_some_and(|n| matches!(n.to_str(), Some("assistente-3d" | "assistente_3d")))
                {
                    return Err("legacy_desktop_active_migration_blocked");
                }
            }
        }
    }
    let _target_lease = WriterLease::acquire(&target.with_extension("sqlite3.writer.lock"))
        .map_err(|e| e.code())?;
    let _desktop_lease = WriterLease::acquire(&desktop.with_extension("sqlite3.writer.lock"))
        .map_err(|e| e.code())?;
    let original = if target.exists() {
        Some(read(&target)?)
    } else {
        None
    };
    if let Some(ref db) = original {
        let exists: bool = db
            .query_row(
                "SELECT count(*)>0 FROM sqlite_master WHERE name='server_migrations'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| "migration_read_failed")?;
        if exists
            && db
                .query_row(
                    "SELECT count(*)>0 FROM server_migrations WHERE name=?1",
                    [MIGRATION],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(|_| "migration_read_failed")?
        {
            marker(&desktop)?;
            // Preserve the authoritative pre-1B database, including WAL, before
            // Database::open applies schema020. A failed backup prevents upgrade.
            let version: u64 = db
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .map_err(|_| "migration_schema_invalid")?;
            if version < 20 {
                let backups = config.state.join("backups");
                mkdir(&backups)?;
                let backup = tempfile::Builder::new()
                    .prefix("server-1b-schema020-")
                    .tempdir_in(&backups)
                    .map_err(|_| "migration_backup_directory_failed")?
                    .keep();
                snapshot(db, &backup.join("authority-schema019.sqlite3"))?;
                sync_parent(&backup.join("authority-schema019.sqlite3"))?;
            }
            return Ok(());
        }
        // Two populated domain databases cannot be merged by silently choosing a winner.
        for table in [
            "identity_snapshots",
            "memory_records",
            "conversation_sessions",
            "conversation_messages",
            "task_records",
            "task_subtask_records",
            "cognitive_checkpoints",
            "cognitive_continuations",
        ] {
            let count: u64 = db
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .map_err(|_| "migration_read_failed")?;
            if count != 0 {
                return Err("migration_domain_conflict_preserved");
            }
        }
    }
    let source = read(&desktop)?;
    let backups = config.state.join("backups");
    mkdir(&backups)?;
    let backup = tempfile::Builder::new()
        .prefix("server-1a-")
        .tempdir_in(&backups)
        .map_err(|_| "migration_backup_directory_failed")?
        .keep();
    snapshot(&source, &backup.join("desktop.sqlite3"))?;
    if let Some(ref db) = original {
        snapshot(db, &backup.join("core.sqlite3"))?;
    }
    sync_parent(&backup.join("desktop.sqlite3"))?;
    // Retain candidate/backup on any failure for inspection; originals stay untouched.
    let candidate = backup.join("candidate.sqlite3");
    snapshot(&source, &candidate)?;
    let mut merged = Connection::open(&candidate).map_err(|_| "migration_candidate_failed")?;
    migrations::apply(&merged).map_err(|e| e.code())?;
    let tx = merged
        .transaction()
        .map_err(|_| "migration_transaction_failed")?;
    if let Some(ref db) = original {
        let exists: bool = db
            .query_row(
                "SELECT count(*)>0 FROM sqlite_master WHERE name='headless_tasks'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| "migration_read_failed")?;
        if exists {
            let mut query = db.prepare("SELECT id,directory,objective,expected,state,result,error_code FROM headless_tasks ORDER BY id").map_err(|_| "migration_read_failed")?;
            let rows = query
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<String>>(6)?,
                    ))
                })
                .map_err(|_| "migration_read_failed")?;
            for row in rows {
                let (id, dir, objective, expected, state, result, error) =
                    row.map_err(|_| "migration_read_failed")?;
                tx.execute(
                    "INSERT INTO headless_tasks VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    rusqlite::params![id, dir, objective, expected, state, result, error],
                )
                .map_err(|_| "migration_task_id_conflict_preserved")?;
            }
        }
    }
    tx.execute(
        "INSERT INTO server_migrations(name,source,backup_directory) VALUES(?1,?2,?3)",
        rusqlite::params![MIGRATION, desktop.to_str(), backup.to_str()],
    )
    .map_err(|_| "migration_receipt_failed")?;
    tx.commit().map_err(|_| "migration_commit_failed")?;
    let invalid = merged
        .prepare("PRAGMA foreign_key_check")
        .map_err(|_| "migration_foreign_key_failed")?
        .query([])
        .map_err(|_| "migration_foreign_key_failed")?
        .next()
        .map_err(|_| "migration_foreign_key_failed")?
        .is_some();
    if invalid {
        return Err("migration_foreign_key_failed");
    }
    drop(merged);
    drop(original);
    drop(source);
    fs::File::open(&candidate)
        .and_then(|f| f.sync_all())
        .map_err(|_| "migration_sync_failed")?;
    // Original core WAL must be checkpointed before replacing its main file.
    if target.exists() {
        let old = Connection::open(&target).map_err(|_| "migration_checkpoint_failed")?;
        let (busy, _, _): (i64, i64, i64) = old
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(|_| "migration_checkpoint_failed")?;
        if busy != 0 {
            return Err("migration_writer_active_preserved");
        }
        drop(old);
    }
    fs::rename(&candidate, &target).map_err(|_| "migration_publish_failed")?;
    sync_parent(&target)?;
    marker(&desktop)?;
    Ok(())
}

/// State changes call this inside their own SQLite transaction.
pub fn event(
    db: &Connection,
    id: Option<u64>,
    namespace: &str,
    code: &str,
) -> Result<(), &'static str> {
    db.execute(
        "INSERT INTO server_events(namespace,task_id,code) VALUES(?1,?2,?3)",
        rusqlite::params![namespace, id, code],
    )
    .map_err(|_| "event_persist_failed")?;
    db.execute("DELETE FROM server_events WHERE sequence <= (SELECT coalesce(max(sequence),0)-4096 FROM server_events)",[]).map_err(|_|"event_persist_failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::database::Database;
    use std::sync::OnceLock;
    fn config(home: &Path) -> Config {
        let state = home.join(".local/state/narys/core");
        fs::create_dir_all(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        Config {
            home: home.into(),
            state,
            runtime: home.into(),
            root: home.into(),
            cli: home.join("never-launch"),
            binary: home.join("never-launch"),
            database: OnceLock::new(),
        }
    }
    fn source(c: &Config) -> Database {
        Database::new(c.home.join(".local/share/br.com.assistente3d.app"))
    }
    #[test]
    fn transactional_takeover_preserves_colliding_namespaces_history_and_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let c = config(temp.path());
        let desktop = source(&c);
        let mut db = desktop.open().unwrap();
        db.execute("INSERT INTO task_records VALUES(2,'conversation','completed','start','finish','preserved',NULL)",[]).unwrap();
        crate::persistence::conversation::create_diagnostic(&mut db).unwrap();
        drop(db);
        drop(desktop);
        let legacy = Database::new(c.state.join("db"));
        let db = legacy.open().unwrap();
        db.execute("INSERT INTO headless_tasks VALUES(2,'/tmp/narys-task-old','historical','5','completed','5',NULL)",[]).unwrap();
        drop(db);
        drop(legacy);
        let receipts = c.state.join("authorization-sentinel");
        fs::write(&receipts, b"closed-receipt").unwrap();
        initialize(&c).unwrap();
        let db = c.db().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT summary FROM task_records WHERE task_id=2",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "preserved"
        );
        assert_eq!(
            db.query_row("SELECT result FROM headless_tasks WHERE id=2", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "5"
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM conversation_messages", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            2
        );
        assert_eq!(fs::read(receipts).unwrap(), b"closed-receipt");
        assert_eq!(
            source(&c).open().unwrap_err().code(),
            "desktop_migrated_use_core_ipc"
        );
        drop(db);
        // Restart after publication but before fence completion, without recopying.
        let marker = c
            .home
            .join(".local/share/br.com.assistente3d.app/luna.sqlite3.core-owned");
        fs::remove_file(&marker).unwrap();
        drop(c);
        let c = config(temp.path());
        initialize(&c).unwrap();
        assert!(marker.exists());
        assert_eq!(fs::read_dir(c.state.join("backups")).unwrap().count(), 1);
    }
    #[test]
    fn schema020_upgrade_backs_up_authority_once_before_migration() {
        let temp = tempfile::tempdir().unwrap();
        let c = config(temp.path());
        let desktop = source(&c);
        let mut conn = desktop.open().unwrap();
        crate::persistence::conversation::create_diagnostic(&mut conn).unwrap();
        drop(conn);
        drop(desktop);
        initialize(&c).unwrap();
        let conn = c.db().unwrap();
        conn.execute_batch("DROP TABLE conversation_runs; DROP TABLE server_provider_permissions; ALTER TABLE server_events DROP COLUMN details_json; PRAGMA user_version=19;").unwrap();
        drop(conn);
        drop(c);
        let c = config(temp.path());
        initialize(&c).unwrap();
        let backup = fs::read_dir(c.state.join("backups"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("server-1b-schema020-")
            })
            .unwrap();
        let old = Connection::open(backup.join("authority-schema019.sqlite3")).unwrap();
        assert_eq!(
            old.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                .unwrap(),
            19
        );
        assert_eq!(
            old.query_row("SELECT count(*) FROM conversation_messages", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            2
        );
        let upgraded = c.db().unwrap();
        assert_eq!(
            upgraded
                .pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                .unwrap(),
            22
        );
        assert_eq!(
            upgraded
                .query_row("SELECT count(*) FROM conversation_messages", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            2
        );
        drop(upgraded);
        drop(c);
        let c = config(temp.path());
        initialize(&c).unwrap();
        assert_eq!(fs::read_dir(c.state.join("backups")).unwrap().count(), 3);
    }
    #[test]
    fn conflicting_populated_databases_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let c = config(temp.path());
        let desktop = source(&c);
        drop(desktop.open().unwrap());
        drop(desktop);
        let legacy = Database::new(c.state.join("db"));
        let db = legacy.open().unwrap();
        db.execute(
            "INSERT INTO task_records VALUES(9,'task','failed','a','b','keep',NULL)",
            [],
        )
        .unwrap();
        drop(db);
        drop(legacy);
        let p = c.state.join("db/luna.sqlite3");
        let before = fs::read(&p).unwrap();
        assert_eq!(initialize(&c), Err("migration_domain_conflict_preserved"));
        assert_eq!(fs::read(p).unwrap(), before);
        assert!(!source(&c).open().is_err());
    }
}

#[cfg(test)]
mod wal_tests {
    use super::*;
    use crate::persistence::database::Database;
    use std::sync::OnceLock;
    #[test]
    fn desktop_wal_commits_are_included_in_consistent_backup() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let state = home.join("state");
        mkdir(&state).unwrap();
        let desktop_dir = home.join(".local/share/br.com.assistente3d.app");
        let desktop = Database::new(desktop_dir.clone());
        let db = desktop.open().unwrap();
        db.pragma_update(None, "journal_mode", "WAL").unwrap();
        db.execute("INSERT INTO task_records VALUES(33,'conversation','completed','a','b','WAL-preserved',NULL)",[]).unwrap();
        assert!(desktop_dir.join("luna.sqlite3-wal").exists());
        drop(desktop); // Raw connection remains open so SQLite cannot checkpoint on last close.
        let config = Config {
            home: home.into(),
            state,
            runtime: home.into(),
            root: home.into(),
            cli: home.join("never"),
            binary: home.join("never"),
            database: OnceLock::new(),
        };
        initialize(&config).unwrap();
        assert_eq!(
            config
                .db()
                .unwrap()
                .query_row(
                    "SELECT summary FROM task_records WHERE task_id=33",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "WAL-preserved"
        );
        drop(db);
    }
}
