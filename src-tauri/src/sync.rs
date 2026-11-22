//! Abgleich Server → lokaler Cache.
//!
//! Pro Ordner:
//! 1. EXAMINE (nur lesend) → UIDVALIDITY prüfen; bei Änderung lokalen Ordner verwerfen.
//! 2. UID SEARCH SINCE <Zeitfenster> → Server-UIDs.
//! 3. Neue UIDs: Kopfdaten in Häppchen holen (neueste zuerst).
//! 4. Verschwundene UIDs lokal löschen.
//! 5. Flags der übrigen abgleichen.
//!
//! Danach werden Inhalte kleiner Mails im Hintergrund vorgeladen, damit sie
//! offline lesbar sind. Große Mails werden erst beim Öffnen geladen.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;

use kolibri_core::{
    Account, AccountId, Error, ErrorKind, Folder, FolderRole, MailProvider, Result,
};
use kolibri_mail::ImapSession;
use kolibri_mail::imap::FETCH_BATCH;
use kolibri_mail::parse::parse_body;
use kolibri_store::{Store, SyncState};

/// Zeitfenster für den Offline-Cache: rund 6 Monate.
pub const WINDOW_SECS: i64 = 183 * 24 * 60 * 60;
/// Mails bis zu dieser Größe werden vorgeladen.
pub const PREFETCH_MAX_SIZE: u32 = 256 * 1024;
/// Höchstens so viele Inhalte pro Ordner und Durchlauf vorladen.
pub const PREFETCH_PER_RUN: u32 = 300;
/// Flags in größeren Häppchen abfragen (nur UID + FLAGS, sehr klein).
const FLAG_BATCH: usize = 2000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncOutcome {
    pub added: usize,
    pub removed: usize,
    pub flags_changed: usize,
}

impl SyncOutcome {
    pub fn changed(&self) -> bool {
        self.added + self.removed + self.flags_changed > 0
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Gleicht einen Ordner ab.
pub async fn sync_folder(
    session: &mut ImapSession,
    store: &Store,
    folder: &Folder,
) -> Result<SyncOutcome> {
    let status = session.examine(&folder.path).await?;
    let state = store.sync_state(folder.id)?;
    if state.uid_validity.is_some_and(|v| v != status.uid_validity) {
        log::info!(
            "UIDVALIDITY von {} geändert – Ordner wird neu geladen",
            folder.path
        );
        store.clear_folder(folder.id)?;
    }

    let server = if status.exists == 0 {
        Vec::new()
    } else {
        session
            .uids_since(&folder.path, now() - WINDOW_SECS)
            .await?
    };
    let local = store.folder_uids(folder.id)?;
    let server_set: HashSet<u32> = server.iter().copied().collect();
    let local_set: HashSet<u32> = local.iter().copied().collect();

    let mut outcome = SyncOutcome::default();

    // Neue Nachrichten, neueste (höchste UID) zuerst – so ist die Liste schnell gefüllt.
    let mut new: Vec<u32> = server
        .iter()
        .copied()
        .filter(|u| !local_set.contains(u))
        .collect();
    new.sort_unstable_by(|a, b| b.cmp(a));
    for chunk in new.chunks(FETCH_BATCH) {
        let headers = session.fetch_headers(&folder.path, chunk).await?;
        outcome.added += store.insert_messages(folder.id, &headers)?;
    }

    // Auf dem Server gelöscht (oder aus dem Zeitfenster gefallen).
    let gone: Vec<u32> = local
        .iter()
        .copied()
        .filter(|u| !server_set.contains(u))
        .collect();
    outcome.removed = store.delete_uids(folder.id, &gone)?;

    // Flags der bereits bekannten abgleichen.
    let known: Vec<u32> = server
        .iter()
        .copied()
        .filter(|u| local_set.contains(u))
        .collect();
    for chunk in known.chunks(FLAG_BATCH) {
        let flags = session.fetch_flags(&folder.path, chunk).await?;
        outcome.flags_changed += store.update_flags(folder.id, &flags)?;
    }

    store.set_sync_state(
        folder.id,
        SyncState {
            uid_validity: Some(status.uid_validity),
            uid_next: Some(status.uid_next),
            last_sync: Some(now()),
        },
    )?;
    Ok(outcome)
}

/// Antwort auf eine eigene Einladung oder Absage eines Termins?
pub fn is_itip_update(invite: &kolibri_core::Invite) -> bool {
    matches!(invite.method.to_uppercase().as_str(), "REPLY" | "CANCEL")
}

/// Lädt Inhalte kleiner Mails vor (neueste zuerst). Liefert die Anzahl.
/// Termin-Antworten und -Absagen darin landen in `itips` (zum Übernehmen).
pub async fn prefetch_bodies(
    session: &mut ImapSession,
    store: &Store,
    folder: &Folder,
    itips: &mut Vec<kolibri_core::Invite>,
) -> Result<usize> {
    let pending = store.messages_without_body(folder.id, PREFETCH_MAX_SIZE, PREFETCH_PER_RUN)?;
    let mut done = 0;
    for (id, uid) in pending {
        match session.fetch_raw(&folder.path, uid).await {
            Ok(raw) => {
                let body = parse_body(&raw);
                if let Some(invite) = body.invite.as_ref().filter(|i| is_itip_update(i)) {
                    itips.push(invite.clone());
                }
                store.save_body(id, &body)?;
                done += 1;
            }
            // Inzwischen gelöscht: beim nächsten Sync verschwindet sie auch lokal.
            Err(e) if e.kind == ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

/// Reihenfolge: Posteingang, dann Spezialordner, dann der Rest.
pub fn sync_order(folders: &[Folder]) -> Vec<Folder> {
    let mut list: Vec<Folder> = folders.iter().filter(|f| f.selectable).cloned().collect();
    list.sort_by_key(|f| match f.role {
        Some(FolderRole::Inbox) => 0,
        Some(FolderRole::Junk | FolderRole::Trash) => 3,
        Some(_) => 1,
        None => 2,
    });
    list
}

/// Wiederverwendbare IMAP-Verbindungen für interaktive Abrufe (Mail öffnen),
/// getrennt vom Sync, damit das Öffnen nie hinter einem langen Sync wartet.
#[derive(Default)]
pub struct SessionPool {
    slots: Mutex<HashMap<AccountId, Arc<tokio::sync::Mutex<Option<ImapSession>>>>>,
}

/// Fehler, nach denen sich ein neuer Verbindungsversuch lohnt (nicht: Server
/// lehnt einen Befehl ab – der würde sonst doppelt ausgeführt).
pub fn is_connection_error(e: &Error) -> bool {
    matches!(
        e.kind,
        ErrorKind::Connect | ErrorKind::Timeout | ErrorKind::Tls
    )
}

impl SessionPool {
    fn slot(&self, id: AccountId) -> Result<Arc<tokio::sync::Mutex<Option<ImapSession>>>> {
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| Error::new(ErrorKind::Internal, "Verbindungs-Pool gesperrt"))?;
        Ok(Arc::clone(slots.entry(id).or_default()))
    }

    /// Führt `f` mit der Verbindung des Kontos aus; verbindet bei Bedarf und
    /// versucht es nach einem Verbindungsabbruch genau einmal neu.
    pub async fn run<T, F>(&self, account: &Account, password: &str, mut f: F) -> Result<T>
    where
        F: for<'a> FnMut(&'a mut ImapSession) -> BoxFuture<'a, Result<T>>,
    {
        let slot = self.slot(account.id)?;
        let mut guard = slot.lock().await;
        for attempt in 0..2 {
            if guard.is_none() {
                *guard = Some(ImapSession::connect(&account.imap, password).await?);
            }
            let Some(session) = guard.as_mut() else {
                continue;
            };
            match f(session).await {
                Ok(value) => return Ok(value),
                Err(e) if is_connection_error(&e) => {
                    *guard = None;
                    if attempt == 0 {
                        log::info!("IMAP-Verbindung für Konto {} erneuert: {e}", account.id);
                    } else {
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Err(Error::new(
            ErrorKind::Connect,
            "Keine Verbindung zum Server",
        ))
    }

    /// Holt eine komplette Nachricht.
    pub async fn fetch_raw(
        &self,
        account: &Account,
        password: &str,
        path: &str,
        uid: u32,
    ) -> Result<Vec<u8>> {
        let path = path.to_owned();
        self.run(account, password, |s| {
            let path = path.clone();
            Box::pin(async move { s.fetch_raw(&path, uid).await })
        })
        .await
    }

    /// Verbindung eines Kontos schließen (z. B. beim Entfernen).
    pub async fn close(&self, id: AccountId) {
        let slot = self.slots.lock().ok().and_then(|mut s| s.remove(&id));
        if let Some(slot) = slot
            && let Some(session) = slot.lock().await.take()
        {
            let _ = tokio::time::timeout(Duration::from_secs(3), session.logout()).await;
        }
    }
}

/// Gegen den lokalen Testserver mit eingespielten Mails (scripts/seed-mail.py):
/// `KOLIBRI_TEST_SERVER=1 cargo test -p kolibri --lib -- --ignored sync`
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use kolibri_core::{MessageScope, Security, ServerConfig};
    use kolibri_store::NewAccount;

    #[tokio::test]
    #[ignore = "braucht scripts/test-server.sh + scripts/seed-mail.py"]
    async fn sync_against_seeded_server() {
        if std::env::var("KOLIBRI_TEST_SERVER").is_err() {
            return;
        }
        let cfg = |port| ServerConfig {
            host: "localhost".into(),
            port,
            security: Security::None,
            username: "test@kolibri.test".into(),
        };
        // Als Datei, damit die Größe (inkl. Suchindex) messbar ist.
        let db =
            std::env::temp_dir().join(format!("kolibri-sync-test-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let store = Store::open(&db).unwrap();
        let account = store
            .insert_account(&NewAccount {
                email: "test@kolibri.test".into(),
                display_name: "Test".into(),
                imap: cfg(1143),
                smtp: cfg(1587),
            })
            .unwrap();
        let mut session = ImapSession::connect(&account.imap, "kolibri-test")
            .await
            .unwrap();
        store
            .replace_folders(account.id, &session.list_folders().await.unwrap())
            .unwrap();
        let folders = sync_order(&store.folders(account.id).unwrap());
        assert_eq!(folders[0].role, Some(FolderRole::Inbox));

        let started = std::time::Instant::now();
        let mut first = SyncOutcome::default();
        for f in &folders {
            let o = sync_folder(&mut session, &store, f).await.unwrap();
            first.added += o.added;
        }
        println!("Erster Sync: {first:?} in {:?}", started.elapsed());
        assert!(first.added > 30, "{first:?}");

        let inbox = MessageScope::Folder { id: folders[0].id };
        let all = store.list_messages(inbox, 0, 10_000).unwrap();
        assert!(
            !all.iter().any(|m| m.subject.starts_with("Sehr alte Mail")),
            "Mails außerhalb des Zeitfensters dürfen nicht geladen werden"
        );
        assert!(all.iter().any(|m| m.subject.contains("Ümläute")));
        assert!(
            all.windows(2).all(|w| w[0].date >= w[1].date),
            "nicht nach Datum sortiert"
        );

        // Zweiter Durchlauf: nichts Neues.
        for f in &folders {
            let o = sync_folder(&mut session, &store, f).await.unwrap();
            assert!(!o.changed(), "{}: {o:?}", f.path);
        }

        // Vorladen der Inhalte.
        let t = std::time::Instant::now();
        let loaded = prefetch_bodies(&mut session, &store, &folders[0], &mut Vec::new())
            .await
            .unwrap();
        println!("Vorladen: {loaded} Inhalte in {:?}", t.elapsed());
        assert!(loaded > 30, "nur {loaded} geladen");
        let find = |needle: &str| {
            store
                .list_messages(inbox, 0, 10_000)
                .unwrap()
                .into_iter()
                .find(|m| m.subject.contains(needle))
                .unwrap()
        };
        let news = store.body(find("Herbst-Angebote").id).unwrap().unwrap();
        let html = news.html.unwrap();
        assert!(news.remote_images);
        assert!(
            !html.contains("<script") && !html.contains("onclick"),
            "{html}"
        );
        let cid = store.body(find("Eingebettetes Bild").id).unwrap().unwrap();
        assert!(cid.html.unwrap().contains("data:image/png;base64,"));
        // Ältere Mail ohne vorgeladenen Inhalt: wird beim Öffnen geholt.
        let invoice = find("Rechnung Oktober");
        if store.body(invoice.id).unwrap().is_none() {
            let loc = store.message_location(invoice.id).unwrap();
            let raw = session.fetch_raw(&loc.folder_path, loc.uid).await.unwrap();
            store.save_body(invoice.id, &parse_body(&raw)).unwrap();
        }
        let invoice = find("Rechnung Oktober");
        assert!(invoice.has_attachments);
        assert!(!invoice.preview.is_empty());

        // Suche über den lokalen Index (Kopfzeilen + geladene Inhalte).
        let t = std::time::Instant::now();
        let hits = store.search("rechnung", None, 0, 50).unwrap();
        let by_sender = store
            .search("von:stadtwerke hat:anhang", None, 0, 50)
            .unwrap();
        let umlaut = store.search("koln", None, 0, 50).unwrap();
        let body = store.search("agenda", None, 0, 50).unwrap();
        let wide = store.search("lasttest", None, 0, 100).unwrap();
        println!(
            "Suche: 5 Abfragen in {:?} (rechnung {}, von:stadtwerke {}, koln {}, agenda {}, lasttest {})",
            t.elapsed(),
            hits.total,
            by_sender.total,
            umlaut.total,
            body.total,
            wide.total
        );
        assert!(hits.items.iter().any(|m| m.subject == "Rechnung Oktober"));
        assert_eq!(
            by_sender.items.first().map(|m| m.subject.as_str()),
            Some("Rechnung Oktober")
        );
        assert!(umlaut.total >= 1, "„koln“ muss „Köln“ finden");
        assert!(body.total >= 1, "Inhalt („Agenda folgt“) nicht durchsucht");

        session.logout().await.unwrap();
        drop(store);
        let size = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0)
            + std::fs::metadata(db.with_extension("sqlite3-wal"))
                .map(|m| m.len())
                .unwrap_or(0);
        println!("Datenbank: {:.1} MB", size as f64 / 1_048_576.0);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(db.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(db.with_extension("sqlite3-shm"));
    }
}
