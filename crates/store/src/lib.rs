//! Lokaler SQLite-Cache.
//!
//! Eine Verbindung hinter einem Mutex. Die Abfragen sind kurz; sobald die
//! Synchronisierung (M3) viel schreibt, kommt ein eigener Schreib-Thread dazu.

mod calendar;
mod contacts;
mod library;
mod messages;
mod migrations;
mod ops;
mod outbox;
mod search;
mod threads;

pub use calendar::{
    CalendarId, CalendarInfo, DavSource, EventId, EventRecord, LOCAL_URL, StoredEvent,
};
pub use contacts::{Contact, ContactLocation, ContactRecord, Labeled, Suggestion};
pub use library::{CONTENT_PLACEHOLDER, DraftMeta, Signature, Template};
pub use messages::{ComposeSource, MessageLocation, SyncState};
pub use ops::{MessageRef, QueueCounts};
pub use outbox::{OutboxItem, OutboxKind};
pub use search::{Query, SearchPage, parse as parse_query};

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use kolibri_core::{
    Account, AccountId, Error, ErrorKind, Folder, FolderRole, RemoteFolder, Result, Security,
    ServerConfig,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

/// Daten für ein neues Konto.
#[derive(Debug, Clone)]
pub struct NewAccount {
    pub email: String,
    pub display_name: String,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
}

pub struct Store {
    conn: Mutex<Connection>,
}

pub(crate) fn storage_err(err: rusqlite::Error) -> Error {
    if let rusqlite::Error::SqliteFailure(e, _) = &err
        && e.code == rusqlite::ErrorCode::ConstraintViolation
    {
        return Error::new(ErrorKind::Duplicate, err.to_string());
    }
    Error::new(ErrorKind::Storage, err.to_string())
}

impl Store {
    /// Öffnet (oder erstellt) die Datenbank und führt fehlende Migrationen aus.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(storage_err)?;
        Self::init(conn)
    }

    /// Für Tests.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(storage_err)?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -8000;", // 8 MB Seiten-Cache: bewusst klein
        )
        .map_err(storage_err)?;
        migrations::run(&mut conn).map_err(storage_err)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        // Gespräche für Nachrichten aus Versionen vor M6 nachtragen.
        let filled = store.backfill_threads()?;
        if filled > 0 {
            log::info!("Gespräche für {filled} Nachrichten nachgetragen");
        }
        Ok(store)
    }

    pub(crate) fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| Error::new(ErrorKind::Internal, "Datenbank-Sperre vergiftet"))
    }

    // ---- Konten -------------------------------------------------------------

    pub fn insert_account(&self, new: &NewAccount) -> Result<Account> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO accounts (email, display_name,
                imap_host, imap_port, imap_security, imap_username,
                smtp_host, smtp_port, smtp_security, smtp_username, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM accounts))",
            params![
                new.email,
                new.display_name,
                new.imap.host,
                new.imap.port,
                new.imap.security.as_str(),
                new.imap.username,
                new.smtp.host,
                new.smtp.port,
                new.smtp.security.as_str(),
                new.smtp.username,
            ],
        )
        .map_err(storage_err)?;
        Ok(Account {
            id: conn.last_insert_rowid(),
            email: new.email.clone(),
            display_name: new.display_name.clone(),
            imap: new.imap.clone(),
            smtp: new.smtp.clone(),
            signature: String::new(),
        })
    }

    pub fn set_signature(&self, id: AccountId, signature: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE accounts SET signature = ?2 WHERE id = ?1",
            params![id, signature],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(&format!("{ACCOUNT_SELECT} ORDER BY sort_order, id"))
            .map_err(storage_err)?;
        let rows = stmt.query_map([], account_from_row).map_err(storage_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)
    }

    pub fn account(&self, id: AccountId) -> Result<Account> {
        let conn = self.conn()?;
        conn.query_row(
            &format!("{ACCOUNT_SELECT} WHERE id = ?1"),
            [id],
            account_from_row,
        )
        .optional()
        .map_err(storage_err)?
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Konto {id}")))
    }

    pub fn account_exists(&self, email: &str) -> Result<bool> {
        let conn = self.conn()?;
        conn.query_row("SELECT 1 FROM accounts WHERE email = ?1", [email], |_| {
            Ok(())
        })
        .optional()
        .map(|r| r.is_some())
        .map_err(storage_err)
    }

    /// Löscht das Konto samt Ordnern (ON DELETE CASCADE).
    pub fn delete_account(&self, id: AccountId) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM accounts WHERE id = ?1", [id])
            .map_err(storage_err)?;
        Ok(())
    }

    // ---- Ordner -------------------------------------------------------------

    /// Gleicht die Ordnerliste ab: neue anlegen, bestehende aktualisieren
    /// (IDs bleiben stabil), verschwundene löschen.
    pub fn replace_folders(&self, account_id: AccountId, remote: &[RemoteFolder]) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        tx.execute(
            "CREATE TEMP TABLE IF NOT EXISTS seen_paths (path TEXT PRIMARY KEY)",
            [],
        )
        .map_err(storage_err)?;
        tx.execute("DELETE FROM seen_paths", [])
            .map_err(storage_err)?;
        {
            let mut upsert = tx
                .prepare_cached(
                    "INSERT INTO folders (account_id, path, name, delimiter, role, selectable)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (account_id, path) DO UPDATE SET
                        name = excluded.name, delimiter = excluded.delimiter,
                        role = excluded.role, selectable = excluded.selectable",
                )
                .map_err(storage_err)?;
            let mut seen = tx
                .prepare_cached("INSERT OR IGNORE INTO seen_paths (path) VALUES (?1)")
                .map_err(storage_err)?;
            for f in remote {
                upsert
                    .execute(params![
                        account_id,
                        f.path,
                        f.name(),
                        f.delimiter,
                        f.role.map(FolderRole::as_str),
                        f.selectable,
                    ])
                    .map_err(storage_err)?;
                seen.execute(params![f.path]).map_err(storage_err)?;
            }
        }
        tx.execute(
            "DELETE FROM folders WHERE account_id = ?1
               AND path NOT IN (SELECT path FROM seen_paths)",
            [account_id],
        )
        .map_err(storage_err)?;
        tx.commit().map_err(storage_err)
    }

    /// Ordner eines Kontos: zuerst Spezialordner in fester Reihenfolge,
    /// dann alle anderen alphabetisch nach Pfad.
    pub fn folders(&self, account_id: AccountId) -> Result<Vec<Folder>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, account_id, path, name, delimiter, role, selectable
                 FROM folders WHERE account_id = ?1",
            )
            .map_err(storage_err)?;
        let mut folders = stmt
            .query_map([account_id], |row| {
                Ok(Folder {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    path: row.get(2)?,
                    name: row.get(3)?,
                    delimiter: row.get(4)?,
                    role: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|r| FolderRole::parse(&r)),
                    selectable: row.get(6)?,
                })
            })
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)?;
        folders.sort_by(|a, b| {
            let ka = a.role.map_or(u8::MAX, FolderRole::sort_key);
            let kb = b.role.map_or(u8::MAX, FolderRole::sort_key);
            ka.cmp(&kb)
                .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
        });
        Ok(folders)
    }
}

const ACCOUNT_SELECT: &str = "SELECT id, email, display_name,
    imap_host, imap_port, imap_security, imap_username,
    smtp_host, smtp_port, smtp_security, smtp_username, signature FROM accounts";

fn account_from_row(row: &Row<'_>) -> rusqlite::Result<Account> {
    let security = |idx: usize| -> rusqlite::Result<Security> {
        let s: String = row.get(idx)?;
        Security::parse(&s).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                idx,
                rusqlite::types::Type::Text,
                format!("unbekannte Verschlüsselung: {s}").into(),
            )
        })
    };
    Ok(Account {
        id: row.get(0)?,
        email: row.get(1)?,
        display_name: row.get(2)?,
        imap: ServerConfig {
            host: row.get(3)?,
            port: row.get(4)?,
            security: security(5)?,
            username: row.get(6)?,
        },
        smtp: ServerConfig {
            host: row.get(7)?,
            port: row.get(8)?,
            security: security(9)?,
            username: row.get(10)?,
        },
        signature: row.get(11)?,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn server(host: &str, port: u16) -> ServerConfig {
        ServerConfig {
            host: host.into(),
            port,
            security: Security::Tls,
            username: "eddie@example.com".into(),
        }
    }

    fn new_account(email: &str) -> NewAccount {
        NewAccount {
            email: email.into(),
            display_name: "Eddie".into(),
            imap: server("imap.example.com", 993),
            smtp: server("smtp.example.com", 465),
        }
    }

    fn folder(path: &str, role: Option<FolderRole>) -> RemoteFolder {
        RemoteFolder {
            path: path.into(),
            delimiter: Some("/".into()),
            role,
            selectable: true,
        }
    }

    #[test]
    fn account_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let a = store
            .insert_account(&new_account("eddie@example.com"))
            .unwrap();
        assert_eq!(store.account(a.id).unwrap(), a);
        assert_eq!(store.accounts().unwrap(), vec![a]);
        assert!(store.account_exists("EDDIE@example.com").unwrap());
    }

    #[test]
    fn duplicate_email_is_rejected() {
        let store = Store::open_in_memory().unwrap();
        store.insert_account(&new_account("a@example.com")).unwrap();
        let err = store
            .insert_account(&new_account("A@example.com"))
            .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Duplicate);
    }

    #[test]
    fn folders_keep_ids_and_sort_by_role() {
        let store = Store::open_in_memory().unwrap();
        let a = store.insert_account(&new_account("a@example.com")).unwrap();
        store
            .replace_folders(
                a.id,
                &[
                    folder("Rechnungen", None),
                    folder("Sent", Some(FolderRole::Sent)),
                    folder("INBOX", Some(FolderRole::Inbox)),
                    folder("Alt", None),
                ],
            )
            .unwrap();
        let first = store.folders(a.id).unwrap();
        let names: Vec<_> = first.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(names, ["INBOX", "Sent", "Alt", "Rechnungen"]);

        // Zweiter Abgleich: "Alt" verschwindet, "Neu" kommt dazu, IDs bleiben.
        store
            .replace_folders(
                a.id,
                &[
                    folder("INBOX", Some(FolderRole::Inbox)),
                    folder("Sent", Some(FolderRole::Sent)),
                    folder("Rechnungen", None),
                    folder("Neu", None),
                ],
            )
            .unwrap();
        let second = store.folders(a.id).unwrap();
        let id_of = |list: &[Folder], p: &str| list.iter().find(|f| f.path == p).map(|f| f.id);
        assert_eq!(id_of(&first, "INBOX"), id_of(&second, "INBOX"));
        assert_eq!(id_of(&first, "Rechnungen"), id_of(&second, "Rechnungen"));
        assert!(id_of(&second, "Alt").is_none());
        assert!(id_of(&second, "Neu").is_some());
    }

    #[test]
    fn deleting_account_removes_folders() {
        let store = Store::open_in_memory().unwrap();
        let a = store.insert_account(&new_account("a@example.com")).unwrap();
        store
            .replace_folders(a.id, &[folder("INBOX", Some(FolderRole::Inbox))])
            .unwrap();
        store.delete_account(a.id).unwrap();
        assert!(store.folders(a.id).unwrap().is_empty());
        assert!(store.accounts().unwrap().is_empty());
    }

    #[test]
    fn reopening_file_keeps_data_and_skips_migrations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sqlite3");
        {
            let store = Store::open(&path).unwrap();
            store.insert_account(&new_account("a@example.com")).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.accounts().unwrap().len(), 1);
    }

    #[test]
    fn fts5_is_available() {
        // Voraussetzung für die Volltextsuche in M7.
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(body)")
            .unwrap();
    }
}
