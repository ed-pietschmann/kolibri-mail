//! Push: je Konto eine IMAP-Verbindung im IDLE-Zustand auf dem Posteingang.
//! Meldet der Server etwas, wird der Posteingang sofort abgeglichen.
//!
//! - Im Warten keine CPU-Last: Der Server meldet sich von selbst.
//! - IDLE wird alle 9 Minuten erneuert (RFC 2177 erlaubt 29; kürzer übersteht
//!   Router, die stille Verbindungen kappen, und das Aufwachen aus dem Ruhezustand).
//! - Server ohne IDLE: alle 2 Minuten den Posteingang abfragen.
//! - Zusätzlich gleicht ein Zeitgeber alle 10 Minuten alle Ordner ab.

use std::time::Duration;

use kolibri_core::{AccountId, FolderRole, MailProvider, Result, SecretStore};
use kolibri_mail::ImapSession;
use tauri::{AppHandle, Manager};

use crate::accounts::AppState;
use crate::keychain::account_key;
use crate::messages::{SyncScope, spawn_sync};

const IDLE_TIMEOUT: Duration = Duration::from_secs(9 * 60);
const POLL_INTERVAL: Duration = Duration::from_secs(2 * 60);
const FULL_SYNC_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Pause nach Fehlern: 15 s, 30 s … höchstens 5 Minuten.
fn backoff(failures: u32) -> Duration {
    Duration::from_secs((15u64 << failures.min(5)).min(300))
}

/// Eine IDLE-Runde: verbinden und warten, bis die Verbindung reißt.
async fn watch_once(app: &AppHandle, account_id: AccountId) -> Result<()> {
    let state = app.state::<AppState>();
    let account = state.store.account(account_id)?;
    let password = state.secrets.password(&account_key(account_id))?;
    let mut session = ImapSession::connect(&account.imap, &password).await?;
    // Der Posteingang heißt laut IMAP-Standard immer „INBOX“ – auch beim
    // allerersten Start, bevor die Ordnerliste bekannt ist.
    let inbox = state
        .store
        .folder_by_role(account_id, FolderRole::Inbox)?
        .map_or_else(|| "INBOX".to_owned(), |f| f.path);

    if !session.supports_idle().await? {
        log::info!("Konto {account_id}: Server kann kein IDLE – frage alle 2 Minuten ab");
        let _ = session.logout().await;
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            spawn_sync(app.clone(), account_id, SyncScope::Inbox);
        }
    }

    loop {
        let (next, changed) = session.idle_wait(&inbox, IDLE_TIMEOUT).await?;
        session = next;
        if changed {
            spawn_sync(app.clone(), account_id, SyncScope::Inbox);
        }
    }
}

/// Startet (oder ersetzt) den Push-Wächter eines Kontos.
pub fn start(app: &AppHandle, account_id: AccountId) {
    let state = app.state::<AppState>();
    stop(&state, account_id);
    let handle_app = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut failures = 0;
        loop {
            match watch_once(&handle_app, account_id).await {
                Ok(()) => failures = 0,
                Err(e) => {
                    log::info!("Push für Konto {account_id} unterbrochen: {e}");
                    failures += 1;
                    tokio::time::sleep(backoff(failures)).await;
                    // Nach einer Unterbrechung (z. B. offline) Verpasstes nachholen.
                    spawn_sync(handle_app.clone(), account_id, SyncScope::Inbox);
                }
            }
        }
    });
    if let Ok(mut w) = state.watchers.lock() {
        w.insert(account_id, handle);
    }
}

pub fn stop(state: &AppState, account_id: AccountId) {
    if let Some(handle) = state
        .watchers
        .lock()
        .ok()
        .and_then(|mut w| w.remove(&account_id))
    {
        handle.abort();
    }
}

/// Beim Start: Push für alle Konten + regelmäßiger Komplettabgleich.
pub fn start_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    for account in state.store.accounts().unwrap_or_default() {
        start(app, account.id);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(FULL_SYNC_INTERVAL).await;
            let state = app.state::<AppState>();
            for account in state.store.accounts().unwrap_or_default() {
                spawn_sync(app.clone(), account.id, SyncScope::All);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_caps() {
        assert_eq!(backoff(1), Duration::from_secs(30));
        assert_eq!(backoff(20), Duration::from_secs(300));
    }
}
