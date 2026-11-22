//! Offline-Warteschlange: Nutzeraktionen planen, lokal anwenden, im
//! Hintergrund an den Server übertragen.
//!
//! - Jede Aktion wirkt sofort in der Oberfläche (lokale Datenbank).
//! - Ein Arbeiter pro Konto überträgt die Aktionen der Reihe nach.
//! - Offline / Verbindungsfehler: später erneut (wachsende Pause, max. 5 Min.).
//! - Server lehnt ab: Aktion wird lokal zurückgenommen und gemeldet.

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use kolibri_core::{
    AccountId, Action, Error, ErrorKind, FolderId, FolderRole, MessageId, PendingOp, Result,
    SecretStore, UserAction,
};
use kolibri_mail::ImapSession;
use kolibri_store::Store;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::accounts::AppState;
use crate::keychain::account_key;
use crate::messages::MailChanged;
use crate::sync::is_connection_error;

/// Name für einen neu angelegten Archivordner, falls das Konto keinen hat.
const ARCHIVE_NAME: &str = "Archive";

/// Ereignis: Stand der Warteschlange eines Kontos.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatus {
    pub account_id: AccountId,
    pub pending: u32,
    pub failed: u32,
    /// Letzter Versuch scheiterte an der Verbindung.
    pub offline: bool,
    /// Zuletzt vom Server abgelehnte Aktion.
    pub error: Option<Error>,
}

/// Eine geplante Server-Aktion für einen Ordner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub uids: Vec<u32>,
    pub action: Action,
}

/// Übersetzt eine Nutzeraktion auf Nachrichten in Server-Aktionen je Ordner.
pub fn plan(store: &Store, ids: &[MessageId], user: UserAction) -> Result<Vec<Planned>> {
    // Nach (Konto, Ordner) gruppieren – IMAP arbeitet ordnerweise.
    let mut groups: BTreeMap<(AccountId, FolderId), Vec<u32>> = BTreeMap::new();
    for r in store.message_refs(ids)? {
        groups
            .entry((r.account_id, r.folder_id))
            .or_default()
            .push(r.uid);
    }

    let mut out = Vec::new();
    for ((account_id, folder_id), uids) in groups {
        let source = store.folder(folder_id)?;
        let action = match user {
            UserAction::MarkSeen { value } => Some(Action::SetSeen { value }),
            UserAction::Flag { value } => Some(Action::SetFlagged { value }),
            UserAction::DeleteForever => Some(Action::Delete),
            UserAction::Archive => {
                if source.role == Some(FolderRole::Archive) {
                    None // liegt schon im Archiv
                } else {
                    Some(
                        match store.folder_by_role(account_id, FolderRole::Archive)? {
                            Some(f) => Action::Move {
                                to: f.path,
                                create: false,
                            },
                            None => Action::Move {
                                to: ARCHIVE_NAME.into(),
                                create: true,
                            },
                        },
                    )
                }
            }
            UserAction::Trash => {
                if source.role == Some(FolderRole::Trash) {
                    // Die Oberfläche fragt vorher nach und schickt dann DeleteForever.
                    return Err(Error::new(
                        ErrorKind::Invalid,
                        "Liegt bereits im Papierkorb – endgültig löschen?",
                    ));
                }
                match store.folder_by_role(account_id, FolderRole::Trash)? {
                    Some(f) => Some(Action::Move {
                        to: f.path,
                        create: false,
                    }),
                    None => {
                        return Err(Error::new(
                            ErrorKind::NotFound,
                            "Dieses Konto hat keinen Papierkorb-Ordner",
                        ));
                    }
                }
            }
            UserAction::MoveTo { folder_id: target } => {
                let target = store.folder(target)?;
                if target.account_id != account_id {
                    return Err(Error::new(
                        ErrorKind::Invalid,
                        "Verschieben zwischen Konten ist (noch) nicht möglich",
                    ));
                }
                (target.id != folder_id).then_some(Action::Move {
                    to: target.path,
                    create: false,
                })
            }
        };
        if let Some(action) = action {
            out.push(Planned {
                account_id,
                folder_id,
                uids,
                action,
            });
        }
    }
    Ok(out)
}

/// Überträgt eine Operation an den Server.
async fn execute(session: &mut ImapSession, op: &PendingOp) -> Result<()> {
    let path = op.folder_path.as_str();
    match &op.action {
        Action::SetSeen { value } => session.store_flags(path, &op.uids, "\\Seen", *value).await,
        Action::SetFlagged { value } => {
            session
                .store_flags(path, &op.uids, "\\Flagged", *value)
                .await
        }
        Action::SetAnswered { value } => {
            session
                .store_flags(path, &op.uids, "\\Answered", *value)
                .await
        }
        Action::Move { to, create } => {
            if *create {
                session.create_folder(to).await?;
            }
            session.move_messages(path, &op.uids, to).await
        }
        Action::Delete => session.delete_permanently(path, &op.uids).await,
    }
}

/// Pause vor dem nächsten Versuch: 15 s, 30 s, 60 s … höchstens 5 Minuten.
pub fn backoff(attempts: u32) -> Duration {
    Duration::from_secs((15u64 << attempts.min(5)).min(300))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Drained {
    /// Alles übertragen; `moved`: es wurde verschoben/gelöscht.
    Done { moved: bool },
    /// Verbindung weg – später erneut.
    Offline { attempts: u32 },
}

/// Kern der Abarbeitung – ohne Oberfläche, damit er testbar ist.
/// `on_reject` wird für jede vom Server abgelehnte Aktion aufgerufen.
pub async fn drain_ops(
    store: &Store,
    pool: &crate::sync::SessionPool,
    account: &kolibri_core::Account,
    password: &str,
    mut on_reject: impl FnMut(&PendingOp, Error),
) -> Result<Drained> {
    let mut moved = false;
    while let Some(op) = store.next_op(account.id)? {
        let result = pool
            .run(account, password, |s| {
                let op = op.clone();
                Box::pin(async move { execute(s, &op).await })
            })
            .await;
        match result {
            Ok(()) => {
                moved |= op.action.hides();
                store.complete_op(&op)?;
            }
            Err(e) if is_connection_error(&e) || e.kind == ErrorKind::Auth => {
                store.retry_op(&op, &e.to_string())?;
                return Ok(Drained::Offline {
                    attempts: op.attempts + 1,
                });
            }
            Err(e) => {
                log::warn!("Aktion {:?} vom Server abgelehnt: {e}", op.action);
                store.fail_op(&op, &e.to_string())?;
                on_reject(&op, e);
            }
        }
    }
    Ok(Drained::Done { moved })
}

/// Abarbeitung mit Meldungen an die Oberfläche.
async fn drain(app: &AppHandle, account_id: AccountId) -> Result<Drained> {
    let state = app.state::<AppState>();
    let account = state.store.account(account_id)?;
    let password = state.secrets.password(&account_key(account_id))?;

    // Erst der Postausgang (Senden ist am wichtigsten), dann die Aktionen.
    let outbox = crate::compose::drain_outbox(
        &state.store,
        &state.pool,
        &account,
        &password,
        |subject, e| {
            let detail = format!("„{subject}“ konnte nicht übertragen werden: {}", e.detail);
            emit_status(app, account_id, false, Some(Error::new(e.kind, detail)));
        },
    )
    .await?;
    let sent = match outbox {
        crate::compose::OutboxResult::Offline { attempts } => {
            emit_status(app, account_id, true, None);
            return Ok(Drained::Offline { attempts });
        }
        crate::compose::OutboxResult::Done { changed } => changed,
    };

    let result = drain_ops(&state.store, &state.pool, &account, &password, |op, e| {
        let _ = app.emit(
            "mail-changed",
            MailChanged {
                account_id,
                folder_id: Some(op.folder_id),
            },
        );
        emit_status(app, account_id, false, Some(e));
    })
    .await?;
    if matches!(result, Drained::Offline { .. }) {
        emit_status(app, account_id, true, None);
    }
    Ok(match result {
        Drained::Done { moved } => Drained::Done {
            moved: moved || sent,
        },
        offline => offline,
    })
}

fn emit_status(app: &AppHandle, account_id: AccountId, offline: bool, error: Option<Error>) {
    let counts = app
        .state::<AppState>()
        .store
        .queue_counts(account_id)
        .unwrap_or_default();
    let _ = app.emit(
        "queue-status",
        QueueStatus {
            account_id,
            pending: counts.pending,
            failed: counts.failed,
            offline,
            error,
        },
    );
}

/// Startet den Arbeiter eines Kontos (falls nicht schon aktiv).
pub fn kick(app: AppHandle, account_id: AccountId) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if !state
            .draining
            .lock()
            .is_ok_and(|mut s| s.insert(account_id))
        {
            return;
        }
        let result = drain(&app, account_id).await;
        if let Ok(mut s) = state.draining.lock() {
            s.remove(&account_id);
        }
        match result {
            Ok(Drained::Done { moved }) => {
                emit_status(&app, account_id, false, None);
                // Zielordner (z. B. Archiv) aktualisieren.
                if moved {
                    crate::messages::spawn_account_sync(app.clone(), account_id);
                }
            }
            Ok(Drained::Offline { attempts }) => {
                let wait = backoff(attempts);
                log::info!("Konto {account_id} offline – neuer Versuch in {wait:?}");
                tokio::time::sleep(wait).await;
                kick(app.clone(), account_id);
            }
            Err(e) => {
                log::warn!("Warteschlange von Konto {account_id}: {e}");
                emit_status(&app, account_id, false, Some(e));
            }
        }
    });
}

/// Für den Sync: vorher alles Ausstehende übertragen (Fehler sind dort egal).
pub async fn drain_before_sync(app: &AppHandle, account_id: AccountId) {
    let state = app.state::<AppState>();
    if !state
        .draining
        .lock()
        .is_ok_and(|mut s| s.insert(account_id))
    {
        return;
    }
    let result = drain(app, account_id).await;
    if let Ok(mut s) = state.draining.lock() {
        s.remove(&account_id);
    }
    if !matches!(result, Ok(Drained::Offline { .. })) {
        emit_status(app, account_id, false, None);
    }
}

// ---- Tauri-Commands --------------------------------------------------------

/// Führt eine Aktion auf Nachrichten aus (sofort lokal, dann im Hintergrund).
#[tauri::command]
pub fn apply_action(app: AppHandle, ids: Vec<MessageId>, action: UserAction) -> Result<()> {
    let state = app.state::<AppState>();
    let planned = plan(&state.store, &ids, action)?;
    let mut accounts = HashSet::new();
    for p in &planned {
        state
            .store
            .enqueue(p.account_id, p.folder_id, &p.uids, &p.action)?;
        accounts.insert(p.account_id);
        let _ = app.emit(
            "mail-changed",
            MailChanged {
                account_id: p.account_id,
                folder_id: Some(p.folder_id),
            },
        );
    }
    crate::notify::update_badge(&app);
    for account in accounts {
        emit_status(&app, account, false, None);
        kick(app.clone(), account);
    }
    Ok(())
}

/// Fehlgeschlagene Aktionen eines Kontos verwerfen (Meldung gelesen).
#[tauri::command]
pub fn dismiss_failed(app: AppHandle, account_id: AccountId) -> Result<()> {
    app.state::<AppState>().store.clear_failed_ops(account_id)?;
    emit_status(&app, account_id, false, None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(0), Duration::from_secs(15));
        assert_eq!(backoff(1), Duration::from_secs(30));
        assert_eq!(backoff(3), Duration::from_secs(120));
        assert_eq!(backoff(10), Duration::from_secs(300));
    }
}

/// Gegen den lokalen Testserver mit frischen Testdaten:
/// `scripts/test-server.sh reset && python3 -I scripts/seed-mail.py`
/// `KOLIBRI_TEST_SERVER=1 cargo test -p kolibri --lib -- --ignored queue`
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod server_tests {
    use super::*;
    use crate::sync::{sync_folder, sync_order};
    use kolibri_core::{MailProvider, MessageScope, Security, ServerConfig};
    use kolibri_store::NewAccount;

    async fn fresh_sync(store: &Store, account: AccountId, session: &mut ImapSession) {
        store
            .replace_folders(account, &session.list_folders().await.unwrap())
            .unwrap();
        for f in sync_order(&store.folders(account).unwrap()) {
            sync_folder(session, store, &f).await.unwrap();
        }
    }

    async fn run_queue(store: &Store, account: AccountId, session: &mut ImapSession) {
        while let Some(op) = store.next_op(account).unwrap() {
            execute(session, &op).await.unwrap();
            store.complete_op(&op).unwrap();
        }
    }

    fn find(
        store: &Store,
        scope: MessageScope,
        subject: &str,
    ) -> Option<kolibri_core::MessageSummary> {
        store
            .list_messages(scope, 0, 10_000)
            .unwrap()
            .into_iter()
            .find(|m| m.subject.contains(subject))
    }

    #[tokio::test]
    #[ignore = "verändert Daten auf dem Testserver"]
    async fn queue_actions_against_server() {
        if std::env::var("KOLIBRI_TEST_SERVER").is_err() {
            return;
        }
        let cfg = |port| ServerConfig {
            host: "localhost".into(),
            port,
            security: Security::None,
            username: "test@kolibri.test".into(),
        };
        let store = Store::open_in_memory().unwrap();
        let account = store
            .insert_account(&NewAccount {
                email: "test@kolibri.test".into(),
                display_name: "Test".into(),
                imap: cfg(1143),
                smtp: cfg(1587),
            })
            .unwrap()
            .id;
        let mut session = ImapSession::connect(&cfg(1143), "kolibri-test")
            .await
            .unwrap();
        fresh_sync(&store, account, &mut session).await;
        let inbox_id = store
            .folder_by_role(account, FolderRole::Inbox)
            .unwrap()
            .unwrap()
            .id;
        let inbox = MessageScope::Folder { id: inbox_id };

        let a = find(&store, inbox, "Ümläute").unwrap();
        let b = find(&store, inbox, "Rechnung Oktober").unwrap();
        let c = find(&store, inbox, "Herbst-Angebote").unwrap();
        let d = find(&store, inbox, "Eingebettetes Bild").unwrap();
        assert!(!a.seen);

        for (ids, action) in [
            (vec![a.id], UserAction::MarkSeen { value: true }),
            (vec![a.id], UserAction::Flag { value: true }),
            (vec![b.id], UserAction::Archive),
            (vec![c.id, d.id], UserAction::Trash),
        ] {
            for p in plan(&store, &ids, action).unwrap() {
                store
                    .enqueue(p.account_id, p.folder_id, &p.uids, &p.action)
                    .unwrap();
            }
        }
        // Sofort lokal sichtbar: b, c, d ausgeblendet.
        assert!(find(&store, inbox, "Rechnung Oktober").is_none());
        assert!(find(&store, inbox, "Herbst-Angebote").is_none());
        run_queue(&store, account, &mut session).await;
        assert_eq!(store.queue_counts(account).unwrap().pending, 0);

        // Mit frischem Cache prüfen, was wirklich auf dem Server steht.
        let check = Store::open_in_memory().unwrap();
        let account2 = check
            .insert_account(&NewAccount {
                email: "test@kolibri.test".into(),
                display_name: "Test".into(),
                imap: cfg(1143),
                smtp: cfg(1587),
            })
            .unwrap()
            .id;
        fresh_sync(&check, account2, &mut session).await;
        let role = |r| MessageScope::Folder {
            id: check.folder_by_role(account2, r).unwrap().unwrap().id,
        };
        let a2 = find(&check, role(FolderRole::Inbox), "Ümläute").unwrap();
        assert!(a2.seen && a2.flagged, "Flags nicht auf dem Server: {a2:?}");
        let archive = check
            .folders(account2)
            .unwrap()
            .into_iter()
            .find(|f| f.path == "Archive")
            .expect("Archivordner wurde nicht angelegt");
        assert!(
            find(
                &check,
                MessageScope::Folder { id: archive.id },
                "Rechnung Oktober"
            )
            .is_some()
        );
        assert!(find(&check, role(FolderRole::Trash), "Herbst-Angebote").is_some());
        assert!(find(&check, role(FolderRole::Inbox), "Herbst-Angebote").is_none());

        // Endgültig löschen aus dem Papierkorb.
        let trashed = find(&check, role(FolderRole::Trash), "Herbst-Angebote").unwrap();
        let err = plan(&check, &[trashed.id], UserAction::Trash).unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::Invalid,
            "Papierkorb→Papierkorb muss nachfragen"
        );
        for p in plan(&check, &[trashed.id], UserAction::DeleteForever).unwrap() {
            check
                .enqueue(p.account_id, p.folder_id, &p.uids, &p.action)
                .unwrap();
        }
        run_queue(&check, account2, &mut session).await;
        let trash = check
            .folder_by_role(account2, FolderRole::Trash)
            .unwrap()
            .unwrap();
        sync_folder(&mut session, &check, &trash).await.unwrap();
        assert!(find(&check, role(FolderRole::Trash), "Herbst-Angebote").is_none());

        session.logout().await.unwrap();
    }

    /// Server weg → Aktionen bleiben in der Warteschlange → Server zurück → übertragen.
    /// Stoppt und startet den Testserver selbst.
    #[tokio::test]
    #[ignore = "stoppt und startet den Testserver"]
    async fn queue_survives_offline() {
        if std::env::var("KOLIBRI_TEST_SERVER").is_err() {
            return;
        }
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/test-server.sh");
        let server = |cmd: &str| {
            let ok = std::process::Command::new("bash")
                .args([script, cmd])
                .status()
                .unwrap()
                .success();
            assert!(ok, "test-server.sh {cmd} fehlgeschlagen");
        };
        let cfg = |port| ServerConfig {
            host: "localhost".into(),
            port,
            security: Security::None,
            username: "test@kolibri.test".into(),
        };
        let store = Store::open_in_memory().unwrap();
        let account = store
            .insert_account(&NewAccount {
                email: "test@kolibri.test".into(),
                display_name: "Test".into(),
                imap: cfg(1143),
                smtp: cfg(1587),
            })
            .unwrap();
        let mut session = ImapSession::connect(&cfg(1143), "kolibri-test")
            .await
            .unwrap();
        fresh_sync(&store, account.id, &mut session).await;
        let inbox_id = store
            .folder_by_role(account.id, FolderRole::Inbox)
            .unwrap()
            .unwrap()
            .id;
        let target = find(&store, MessageScope::Folder { id: inbox_id }, "Kurze Frage").unwrap();
        let pool = crate::sync::SessionPool::default();

        server("stop");
        for p in plan(&store, &[target.id], UserAction::Flag { value: true }).unwrap() {
            store
                .enqueue(p.account_id, p.folder_id, &p.uids, &p.action)
                .unwrap();
        }
        let offline = drain_ops(&store, &pool, &account, "kolibri-test", |_, e| {
            panic!("darf nicht abgelehnt werden: {e}")
        })
        .await
        .unwrap();
        assert_eq!(offline, Drained::Offline { attempts: 1 });
        assert_eq!(store.queue_counts(account.id).unwrap().pending, 1);
        // Lokal schon markiert, obwohl offline.
        assert!(
            store
                .list_messages(MessageScope::Folder { id: inbox_id }, 0, 1000)
                .unwrap()
                .iter()
                .any(|m| m.id == target.id && m.flagged)
        );

        server("start");
        let done = drain_ops(&store, &pool, &account, "kolibri-test", |_, e| {
            panic!("darf nicht abgelehnt werden: {e}")
        })
        .await
        .unwrap();
        assert_eq!(done, Drained::Done { moved: false });
        assert_eq!(store.queue_counts(account.id).unwrap().pending, 0);
    }
}
