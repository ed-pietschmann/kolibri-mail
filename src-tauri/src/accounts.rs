//! Tauri-Commands rund um Konten und Ordner.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use kolibri_core::{
    Account, AccountId, Error, ErrorKind, Folder, MailProvider, RemoteFolder, Result, SecretStore,
    ServerConfig,
};
use kolibri_mail::{Discovered, ImapSession, smtp};
use kolibri_store::{NewAccount, Store};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::keychain::{Keychain, account_key};
use crate::sync::SessionPool;

pub struct AppState {
    pub store: Arc<Store>,
    pub secrets: Keychain,
    /// Verbindungen für interaktive Abrufe (Mail öffnen).
    pub pool: SessionPool,
    /// Konten, deren Sync gerade läuft.
    pub syncing: Mutex<HashSet<AccountId>>,
    /// Konten, deren Warteschlange gerade abgearbeitet wird.
    pub draining: Mutex<HashSet<AccountId>>,
    /// Nach dem laufenden Sync erneut abgleichen (Push kam währenddessen).
    pub sync_again: Mutex<HashMap<AccountId, crate::messages::SyncScope>>,
    /// Push-Wächter (IDLE) je Konto.
    pub watchers: Mutex<HashMap<AccountId, tauri::async_runtime::JoinHandle<()>>>,
    /// Kalender durch eine geöffnete Termin-Antwort geändert (Ansicht neu laden).
    pub calendar_dirty: std::sync::atomic::AtomicBool,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            store: Arc::new(store),
            secrets: Keychain,
            pool: SessionPool::default(),
            syncing: Mutex::new(HashSet::new()),
            draining: Mutex::new(HashSet::new()),
            sync_again: Mutex::new(HashMap::new()),
            watchers: Mutex::new(HashMap::new()),
            calendar_dirty: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// Konto mit Ordnern, so wie die Seitenleiste es braucht.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    #[serde(flatten)]
    pub account: Account,
    pub folders: Vec<Folder>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddAccountInput {
    pub email: String,
    pub display_name: String,
    pub password: String,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
}

fn validate(input: &AddAccountInput) -> Result<()> {
    let invalid = |msg: &str| Err(Error::new(ErrorKind::Invalid, msg));
    if kolibri_mail::autoconfig::Email::parse(&input.email).is_none() {
        return invalid("Ungültige E-Mail-Adresse");
    }
    if input.password.is_empty() {
        return invalid("Passwort fehlt");
    }
    for server in [&input.imap, &input.smtp] {
        if server.host.trim().is_empty() || server.port == 0 || server.username.trim().is_empty() {
            return invalid("Servereinstellungen unvollständig");
        }
    }
    Ok(())
}

async fn fetch_folders(imap: &ServerConfig, password: &str) -> Result<Vec<RemoteFolder>> {
    let mut session = ImapSession::connect(imap, password).await?;
    let folders = session.list_folders().await;
    // Abmelden ist nur Höflichkeit; ein Fehler dabei ist egal.
    let _ = session.logout().await;
    folders
}

#[tauri::command]
pub async fn discover_settings(email: String) -> Result<Discovered> {
    kolibri_mail::discover(&email).await
}

impl AppState {
    /// Prüft IMAP- und SMTP-Anmeldung, speichert dann Konto, Passwort und Ordner.
    pub async fn add_account(&self, input: AddAccountInput) -> Result<AccountView> {
        validate(&input)?;
        let email = input.email.trim().to_owned();
        if self.store.account_exists(&email)? {
            return Err(Error::new(ErrorKind::Duplicate, email));
        }

        // Beide Server parallel prüfen, bevor irgendetwas gespeichert wird.
        let (remote_folders, smtp_check) = tokio::join!(
            fetch_folders(&input.imap, &input.password),
            smtp::test_login(&input.smtp, &input.password)
        );
        let remote_folders = remote_folders.map_err(|e| tag(e, "imap"))?;
        smtp_check.map_err(|e| tag(e, "smtp"))?;

        let account = self.store.insert_account(&NewAccount {
            email,
            display_name: input.display_name.trim().to_owned(),
            imap: input.imap,
            smtp: input.smtp,
        })?;
        if let Err(e) = self
            .secrets
            .set_password(&account_key(account.id), &input.password)
        {
            // Ohne Passwort ist das Konto unbrauchbar – wieder entfernen.
            let _ = self.store.delete_account(account.id);
            return Err(e);
        }
        self.store.replace_folders(account.id, &remote_folders)?;
        let folders = self.store.folders(account.id)?;
        log::info!(
            "Konto {} hinzugefügt ({} Ordner)",
            account.id,
            folders.len()
        );
        Ok(AccountView { account, folders })
    }
}

/// Markiert, welcher Server den Fehler verursacht hat (für die Fehlermeldung).
fn tag(mut err: Error, server: &str) -> Error {
    err.detail = format!("[{server}] {}", err.detail);
    err
}

impl AppState {
    /// Sofort aus dem lokalen Cache – keine Netzwerkzugriffe.
    pub fn list_accounts(&self) -> Result<Vec<AccountView>> {
        self.store
            .accounts()?
            .into_iter()
            .map(|account| {
                let folders = self.store.folders(account.id)?;
                Ok(AccountView { account, folders })
            })
            .collect()
    }

    /// Lädt die Ordnerliste neu vom Server.
    pub async fn refresh_folders(&self, account_id: AccountId) -> Result<Vec<Folder>> {
        let account = self.store.account(account_id)?;
        let password = self.secrets.password(&account_key(account_id))?;
        let remote = fetch_folders(&account.imap, &password).await?;
        self.store.replace_folders(account_id, &remote)?;
        self.store.folders(account_id)
    }

    /// Entfernt Konto, Ordner-Cache und Passwort. Auf dem Server bleibt alles erhalten.
    pub async fn remove_account(&self, account_id: AccountId) -> Result<()> {
        crate::idle::stop(self, account_id);
        self.pool.close(account_id).await;
        self.secrets.delete_password(&account_key(account_id))?;
        self.store.delete_account(account_id)?;
        log::info!("Konto {account_id} entfernt");
        Ok(())
    }
}

// ---- Tauri-Commands: dünne Hüllen um AppState ------------------------------

#[tauri::command]
pub async fn add_account(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: AddAccountInput,
) -> Result<AccountView> {
    let view = state.add_account(input).await?;
    crate::idle::start(&app, view.account.id);
    Ok(view)
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<AccountView>> {
    state.list_accounts()
}

#[tauri::command]
pub async fn refresh_folders(
    state: State<'_, AppState>,
    account_id: AccountId,
) -> Result<Vec<Folder>> {
    state.refresh_folders(account_id).await
}

#[tauri::command]
pub async fn remove_account(state: State<'_, AppState>, account_id: AccountId) -> Result<()> {
    state.remove_account(account_id).await
}

/// Ende-zu-Ende gegen den lokalen Testserver (scripts/test-server.sh):
/// `KOLIBRI_TEST_SERVER=1 cargo test -p kolibri --lib -- --ignored accounts`
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use kolibri_core::{FolderRole, Security};

    fn local(port: u16) -> ServerConfig {
        ServerConfig {
            host: "localhost".into(),
            port,
            security: Security::None,
            username: "test@kolibri.test".into(),
        }
    }

    fn input(password: &str) -> AddAccountInput {
        AddAccountInput {
            email: "test@kolibri.test".into(),
            display_name: "Test".into(),
            password: password.into(),
            imap: local(1143),
            smtp: local(1587),
        }
    }

    #[tokio::test]
    #[ignore = "braucht scripts/test-server.sh und schreibt in den Schlüsselbund"]
    async fn accounts_end_to_end() {
        if std::env::var("KOLIBRI_TEST_SERVER").is_err() {
            return;
        }
        let state = AppState::new(Store::open_in_memory().unwrap());

        // Falsches Passwort: nichts darf gespeichert werden.
        let err = state.add_account(input("falsch")).await.unwrap_err();
        assert_eq!(err.kind, ErrorKind::Auth);
        assert!(
            err.detail.starts_with('['),
            "Server-Markierung fehlt: {err}"
        );
        assert!(state.list_accounts().unwrap().is_empty());

        // Richtiges Passwort: Konto, Schlüsselbund-Eintrag und Ordner.
        let view = state.add_account(input("kolibri-test")).await.unwrap();
        assert!(
            view.folders
                .iter()
                .any(|f| f.role == Some(FolderRole::Inbox))
        );
        assert_eq!(
            state
                .secrets
                .password(&account_key(view.account.id))
                .unwrap(),
            "kolibri-test"
        );

        // Doppelt hinzufügen wird abgelehnt.
        let dup = state.add_account(input("kolibri-test")).await.unwrap_err();
        assert_eq!(dup.kind, ErrorKind::Duplicate);

        // Neu laden nutzt das Passwort aus dem Schlüsselbund; IDs bleiben stabil.
        let refreshed = state.refresh_folders(view.account.id).await.unwrap();
        assert_eq!(
            refreshed.iter().map(|f| f.id).collect::<Vec<_>>(),
            view.folders.iter().map(|f| f.id).collect::<Vec<_>>()
        );
        println!(
            "Ordner: {:?}",
            refreshed
                .iter()
                .map(|f| (&f.path, f.role))
                .collect::<Vec<_>>()
        );

        // Entfernen räumt Datenbank und Schlüsselbund auf.
        state.remove_account(view.account.id).await.unwrap();
        assert!(state.list_accounts().unwrap().is_empty());
        assert_eq!(
            state
                .secrets
                .password(&account_key(view.account.id))
                .unwrap_err()
                .kind,
            ErrorKind::NotFound
        );
    }
}
