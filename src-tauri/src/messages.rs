//! Tauri-Commands für Nachrichten und den Hintergrund-Sync.

use kolibri_core::{
    AccountId, Error, ErrorKind, FolderId, FolderRole, MailProvider, MessageDetail, MessageId,
    MessageScope, MessageSummary, Result, SecretStore,
};
use kolibri_mail::ImapSession;
use kolibri_mail::parse::parse_body;
use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::accounts::AppState;
use crate::keychain::account_key;
use crate::sync::{is_itip_update, prefetch_bodies, sync_folder, sync_order};

/// Höchstens so viele Zeilen pro Anfrage (die Liste lädt seitenweise).
const MAX_PAGE: u32 = 500;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub total: u32,
    pub items: Vec<MessageSummary>,
}

/// Ereignis: Inhalt eines Ordners hat sich geändert.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailChanged {
    pub account_id: AccountId,
    pub folder_id: Option<FolderId>,
}

/// Ereignis: Sync eines Kontos startet/endet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub account_id: AccountId,
    pub syncing: bool,
    pub error: Option<Error>,
}

impl AppState {
    pub fn list_messages(
        &self,
        scope: MessageScope,
        offset: u32,
        limit: u32,
        threaded: bool,
    ) -> Result<MessagePage> {
        let limit = limit.min(MAX_PAGE);
        Ok(if threaded {
            MessagePage {
                total: self.store.count_threads(scope)?,
                items: self.store.list_threads(scope, offset, limit)?,
            }
        } else {
            MessagePage {
                total: self.store.count_messages(scope)?,
                items: self.store.list_messages(scope, offset, limit)?,
            }
        })
    }

    /// Inhalt aus dem Cache – oder einmalig vom Server laden und speichern.
    pub async fn get_message(&self, id: MessageId) -> Result<MessageDetail> {
        let loc = self.store.message_location(id)?;
        let body = match self.store.body(id)? {
            Some(body) => body,
            None => {
                let account = self.store.account(loc.summary.account_id)?;
                let password = self.secrets.password(&account_key(account.id))?;
                let raw = self
                    .pool
                    .fetch_raw(&account, &password, &loc.folder_path, loc.uid)
                    .await?;
                let body = parse_body(&raw);
                self.store.save_body(id, &body)?;
                if let Some(invite) = body.invite.as_ref().filter(|i| is_itip_update(i)) {
                    match self.apply_itip(invite).await {
                        Ok(true) => self.calendar_dirty.store(true, Ordering::Relaxed),
                        Ok(false) => {}
                        Err(e) => log::warn!("Termin-Antwort nicht übernommen: {e}"),
                    }
                }
                body
            }
        };
        let sender_trusted = self.store.is_image_sender_trusted(&loc.summary.from_addr)?;
        Ok(MessageDetail {
            summary: loc.summary,
            to: loc.to,
            cc: loc.cc,
            body,
            sender_trusted,
        })
    }

    /// Markiert ein Konto als „synchronisiert gerade“. `false`, wenn schon aktiv.
    fn begin_sync(&self, id: AccountId) -> bool {
        self.syncing.lock().is_ok_and(|mut s| s.insert(id))
    }

    fn end_sync(&self, id: AccountId) {
        if let Ok(mut s) = self.syncing.lock() {
            s.remove(&id);
        }
    }
}

/// Was abgeglichen wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncScope {
    /// Ordnerliste + alle Ordner + Inhalte vorladen.
    All,
    /// Nur Posteingang (nach Push-Meldung) – schnell.
    Inbox,
}

/// Abgleich eines Kontos. Meldet Fortschritt über Ereignisse und zeigt
/// Mitteilungen für neue, ungelesene Mails im Posteingang.
async fn run_account_sync(app: &AppHandle, account_id: AccountId, scope: SyncScope) -> Result<()> {
    let state = app.state::<AppState>();
    let account = state.store.account(account_id)?;
    let password = state.secrets.password(&account_key(account_id))?;
    // Erst ausstehende Aktionen übertragen, damit der Sync sie nicht überholt.
    crate::queue::drain_before_sync(app, account_id).await;

    let mut session = ImapSession::connect(&account.imap, &password).await?;

    if scope == SyncScope::All {
        let remote = session.list_folders().await?;
        state.store.replace_folders(account_id, &remote)?;
        let _ = app.emit("folders-changed", account_id);
    }

    let folders: Vec<_> = sync_order(&state.store.folders(account_id)?)
        .into_iter()
        .filter(|f| scope == SyncScope::All || f.role == Some(FolderRole::Inbox))
        .collect();
    let mut fresh = Vec::new();
    for folder in &folders {
        // Für Mitteilungen: was ist neuer als beim letzten Mal?
        let before = state.store.sync_state(folder.id)?.uid_next;
        match sync_folder(&mut session, &state.store, folder).await {
            Ok(outcome) if outcome.changed() => {
                log::info!("{}: {outcome:?}", folder.path);
                if let (Some(uid), true) = (before, folder.role == Some(FolderRole::Inbox)) {
                    fresh.extend(state.store.unseen_since(folder.id, uid)?);
                }
                let _ = app.emit(
                    "mail-changed",
                    MailChanged {
                        account_id,
                        folder_id: Some(folder.id),
                    },
                );
            }
            Ok(_) => {}
            // Ein einzelner kaputter Ordner soll den Rest nicht aufhalten.
            Err(e) if e.kind == ErrorKind::Protocol => {
                log::warn!("Ordner {} übersprungen: {e}", folder.path);
            }
            Err(e) => return Err(e),
        }
    }
    if !fresh.is_empty() {
        crate::notify::new_mail(app, &fresh);
    }
    crate::notify::update_badge(app);

    let mut itips = Vec::new();
    for folder in &folders {
        let loaded = prefetch_bodies(&mut session, &state.store, folder, &mut itips).await?;
        if loaded > 0 {
            let _ = app.emit(
                "mail-changed",
                MailChanged {
                    account_id,
                    folder_id: Some(folder.id),
                },
            );
        }
    }
    let _ = session.logout().await;
    apply_itips(app, &itips).await;
    if scope == SyncScope::All {
        crate::contacts::refresh(app, account_id).await;
    }
    Ok(())
}

/// Termin-Antworten/-Absagen aus neuen Mails in den Kalender übernehmen.
async fn apply_itips(app: &AppHandle, itips: &[kolibri_core::Invite]) {
    let state = app.state::<AppState>();
    let mut changed = state.calendar_dirty.swap(false, Ordering::Relaxed);
    for invite in itips {
        match state.apply_itip(invite).await {
            Ok(c) => changed |= c,
            Err(e) => log::warn!("Termin-Antwort nicht übernommen: {e}"),
        }
    }
    if changed {
        let _ = app.emit("calendar-changed", 0);
    }
}

/// Kompletter Abgleich im Hintergrund (kehrt sofort zurück).
pub fn spawn_account_sync(app: AppHandle, account_id: AccountId) {
    spawn_sync(app, account_id, SyncScope::All);
}

/// Abgleich im Hintergrund. Läuft schon einer, wird danach erneut abgeglichen
/// (sonst ginge eine Push-Meldung während eines langen Syncs verloren).
pub fn spawn_sync(app: AppHandle, account_id: AccountId, scope: SyncScope) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if !state.begin_sync(account_id) {
            if let Ok(mut again) = state.sync_again.lock() {
                let entry = again.entry(account_id).or_insert(scope);
                if scope == SyncScope::All {
                    *entry = SyncScope::All;
                }
            }
            return;
        }
        let mut scope = scope;
        loop {
            let _ = app.emit(
                "sync-status",
                SyncStatus {
                    account_id,
                    syncing: true,
                    error: None,
                },
            );
            let result = run_account_sync(&app, account_id, scope).await;
            if let Err(e) = &result {
                log::warn!("Sync von Konto {account_id} fehlgeschlagen: {e}");
            }
            let _ = app.emit(
                "sync-status",
                SyncStatus {
                    account_id,
                    syncing: false,
                    error: result.err(),
                },
            );
            match state
                .sync_again
                .lock()
                .ok()
                .and_then(|mut a| a.remove(&account_id))
            {
                Some(next) => scope = next,
                None => break,
            }
        }
        state.end_sync(account_id);
    });
}

// ---- Tauri-Commands --------------------------------------------------------

#[tauri::command]
pub fn list_messages(
    state: State<'_, AppState>,
    scope: MessageScope,
    offset: u32,
    limit: u32,
    threaded: bool,
) -> Result<MessagePage> {
    state.list_messages(scope, offset, limit, threaded)
}

/// Gespräch: alle Nachrichten (älteste zuerst) + welche davon im Bereich liegen.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub members: Vec<MessageSummary>,
    pub in_scope: Vec<MessageId>,
}

#[tauri::command]
pub fn get_thread(
    state: State<'_, AppState>,
    id: MessageId,
    scope: MessageScope,
) -> Result<Thread> {
    Ok(Thread {
        members: state.store.thread_members(id)?,
        in_scope: state.store.thread_ids_in_scope(id, scope)?,
    })
}

/// Volltextsuche (in allen Ordnern oder einem), neueste zuerst.
#[tauri::command]
pub fn search_messages(
    state: State<'_, AppState>,
    query: String,
    folder_id: Option<FolderId>,
    offset: u32,
    limit: u32,
) -> Result<MessagePage> {
    let page = state
        .store
        .search(&query, folder_id, offset, limit.min(MAX_PAGE))?;
    Ok(MessagePage {
        total: page.total,
        items: page.items,
    })
}

/// Ungelesene je Ordner-ID.
#[tauri::command]
pub fn unread_counts(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<FolderId, u32>> {
    Ok(state.store.unread_counts()?.into_iter().collect())
}

#[tauri::command]
pub async fn get_message(app: AppHandle, id: MessageId) -> Result<MessageDetail> {
    let state = app.state::<AppState>();
    let detail = state.get_message(id).await?;
    if state.calendar_dirty.swap(false, Ordering::Relaxed) {
        let _ = app.emit("calendar-changed", 0);
    }
    Ok(detail)
}

#[tauri::command]
pub fn trust_image_sender(state: State<'_, AppState>, address: String) -> Result<()> {
    if !address.contains('@') {
        return Err(Error::new(ErrorKind::Invalid, "Ungültige Adresse"));
    }
    state.store.trust_image_sender(address.trim())
}

#[tauri::command]
pub fn sync_all(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    for account in state.store.accounts()? {
        spawn_account_sync(app.clone(), account.id);
    }
    Ok(())
}

#[tauri::command]
pub fn sync_account(app: AppHandle, account_id: AccountId) {
    spawn_account_sync(app, account_id);
}
