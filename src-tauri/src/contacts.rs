//! Kontakte: CardDAV-Abgleich und Adressvorschläge.

use kolibri_core::{Account, AccountId, Error, ErrorKind, Result, SecretStore};
use kolibri_dav::{Dav, candidates, vcard};
use kolibri_store::{Contact, ContactRecord, Labeled, Suggestion};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::accounts::AppState;
use crate::keychain::account_key;

/// Kein CardDAV gefunden → frühestens nach einem Tag erneut suchen.
const RETRY_DISCOVERY_SECS: i64 = 24 * 60 * 60;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn labeled(list: Vec<vcard::Labeled>) -> Vec<Labeled> {
    list.into_iter()
        .map(|l| Labeled {
            value: l.value,
            label: l.label,
        })
        .collect()
}

/// Gleicht alle Adressbücher eines Kontos ab. Liefert, ob sich etwas geändert hat.
pub async fn sync_contacts(state: &AppState, account: &Account) -> Result<bool> {
    let (home, checked) = state.store.carddav_home(account.id)?;
    let password = state.secrets.password(&account_key(account.id))?;
    let local = kolibri_core::is_loopback(&account.imap.host);
    let dav = Dav::new(&account.imap.username, &password, local)?;

    let home = match home {
        Some(h) if !h.is_empty() => h,
        Some(_) if checked.is_some_and(|c| now() - c < RETRY_DISCOVERY_SECS) => return Ok(false),
        _ => {
            let found = dav
                .discover_home(&candidates(&account.email, &account.imap.host))
                .await?;
            state.store.set_carddav_home(account.id, found.as_deref())?;
            match found {
                Some(h) => {
                    log::info!("Konto {}: Adressbücher unter {h}", account.id);
                    h
                }
                None => return Ok(false),
            }
        }
    };

    let mut changed = false;
    for book in dav.address_books(&home).await? {
        let known = state.store.contact_etags(account.id, &book.url)?;
        let changes = dav.sync_book(&book.url, &known).await?;
        if changes.changed.is_empty() && changes.removed.is_empty() {
            continue;
        }
        let records: Vec<ContactRecord> = changes
            .changed
            .into_iter()
            .filter_map(|c| {
                let card = vcard::parse(&c.vcard)?;
                Some(ContactRecord {
                    href: c.href,
                    etag: c.etag,
                    uid: card.uid,
                    name: card.name,
                    org: card.org,
                    emails: labeled(card.emails),
                    phones: labeled(card.phones),
                })
            })
            .collect();
        state
            .store
            .apply_contacts(account.id, &book.url, &records, &changes.removed)?;
        changed = true;
    }
    Ok(changed)
}

/// Im Rahmen des Komplettabgleichs: Kontakte + gesammelte Empfänger.
/// Fehler hier dürfen den Mail-Sync nie stören.
pub async fn refresh(app: &AppHandle, account_id: AccountId) {
    crate::calendar::refresh(app, account_id).await;
    let state = app.state::<AppState>();
    let _ = state.store.collect_from_sent();
    let Ok(account) = state.store.account(account_id) else {
        return;
    };
    match sync_contacts(&state, &account).await {
        Ok(true) => {
            let _ = app.emit("contacts-changed", account_id);
        }
        Ok(false) => {}
        Err(e) if e.kind == ErrorKind::Auth => {
            // Passwort stimmt für IMAP, aber nicht für DAV → nicht ständig neu versuchen.
            log::warn!("CardDAV für Konto {account_id} abgelehnt: {e}");
            let _ = state.store.set_carddav_home(account_id, None);
        }
        Err(e) => log::warn!("Kontakte von Konto {account_id} nicht abgeglichen: {e}"),
    }
}

// ---- Tauri-Commands ----------------------------------------------------------

#[tauri::command]
pub fn list_contacts(state: State<'_, AppState>, filter: String) -> Result<Vec<Contact>> {
    state.store.contacts(&filter)
}

/// Eingabe aus dem Kontakt-Dialog.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactInput {
    /// None = neu.
    pub id: Option<i64>,
    /// Nur bei neuen Kontakten: Konto (CardDAV) oder None = auf diesem Mac.
    pub account_id: Option<AccountId>,
    pub name: String,
    pub org: String,
    pub emails: Vec<Labeled>,
    pub phones: Vec<Labeled>,
}

fn dav_of(state: &AppState, account: &Account) -> Result<Dav> {
    let password = state.secrets.password(&account_key(account.id))?;
    Dav::new(
        &account.imap.username,
        &password,
        kolibri_core::is_loopback(&account.imap.host),
    )
}

impl AppState {
    /// Erstes Adressbuch eines Kontos (für neue Kontakte).
    async fn first_book(&self, account: &Account) -> Result<String> {
        let (home, _) = self.store.carddav_home(account.id)?;
        let home = home.filter(|h| !h.is_empty()).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Für dieses Konto wurde (noch) kein Adressbuch gefunden.",
            )
        })?;
        dav_of(self, account)?
            .address_books(&home)
            .await?
            .into_iter()
            .next()
            .map(|b| b.url)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Kein Adressbuch auf dem Server."))
    }

    pub async fn save_contact(&self, input: ContactInput) -> Result<i64> {
        let name = input.name.trim().to_owned();
        let emails: Vec<Labeled> = input
            .emails
            .into_iter()
            .filter(|e| !e.value.trim().is_empty())
            .collect();
        if name.is_empty() && emails.is_empty() {
            return Err(Error::new(
                ErrorKind::Invalid,
                "Bitte einen Namen oder eine E-Mail-Adresse angeben.",
            ));
        }
        if let Some(bad) = emails.iter().find(|e| !e.value.contains('@')) {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!("Ungültige Adresse: {}", bad.value.trim()),
            ));
        }
        let phones: Vec<Labeled> = input
            .phones
            .into_iter()
            .filter(|p| !p.value.trim().is_empty())
            .collect();
        let existing = input
            .id
            .map(|id| self.store.contact_location(id))
            .transpose()?;
        let account_id = existing.as_ref().map_or(input.account_id, |l| l.account_id);
        let uid = existing
            .as_ref()
            .map(|l| l.uid.clone())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| {
                kolibri_mail::compose::new_message_id("kolibri@kolibri.local")
                    .replace(".kolibri@kolibri.local", "-kolibri")
            });
        let display = if name.is_empty() {
            emails
                .first()
                .map(|e| e.value.trim().to_owned())
                .unwrap_or_default()
        } else {
            name
        };
        let card = vcard::Card {
            uid: uid.clone(),
            name: display.clone(),
            org: input.org.trim().to_owned(),
            emails: emails
                .iter()
                .map(|e| vcard::Labeled {
                    value: e.value.trim().to_owned(),
                    label: e.label.clone(),
                })
                .collect(),
            phones: phones
                .iter()
                .map(|p| vcard::Labeled {
                    value: p.value.trim().to_owned(),
                    label: p.label.clone(),
                })
                .collect(),
        };
        let (book, href) = match (account_id, &existing) {
            (None, Some(loc)) => (loc.book_url.clone(), loc.href.clone()),
            (None, None) => ("local".to_owned(), format!("local:{uid}")),
            (Some(acc), loc) => {
                let account = self.store.account(acc)?;
                let dav = dav_of(self, &account)?;
                let text = vcard::write(&card);
                match loc {
                    Some(l) => {
                        dav.replace_card(&l.book_url, &l.href, &text, &l.etag)
                            .await?;
                        (l.book_url.clone(), l.href.clone())
                    }
                    None => {
                        let book = self.first_book(&account).await?;
                        let href = dav.put_card(&book, &uid, &text).await?;
                        (book, href)
                    }
                }
            }
        };
        self.store.upsert_contact(
            account_id,
            &book,
            &ContactRecord {
                href,
                // Leer: der nächste Abgleich holt die Fassung des Servers.
                etag: String::new(),
                uid,
                name: display,
                org: card.org,
                emails,
                phones,
            },
        )
    }

    pub async fn delete_contact(&self, id: i64) -> Result<()> {
        let loc = self.store.contact_location(id)?;
        if let Some(acc) = loc.account_id {
            let account = self.store.account(acc)?;
            dav_of(self, &account)?
                .delete_event(&loc.book_url, &loc.href, &loc.etag)
                .await?;
        }
        self.store.delete_contact(id)
    }
}

#[tauri::command]
pub async fn save_contact(app: AppHandle, input: ContactInput) -> Result<i64> {
    let id = app.state::<AppState>().save_contact(input).await?;
    let _ = app.emit("contacts-changed", 0);
    Ok(id)
}

#[tauri::command]
pub async fn delete_contact(app: AppHandle, id: i64) -> Result<()> {
    app.state::<AppState>().delete_contact(id).await?;
    let _ = app.emit("contacts-changed", 0);
    Ok(())
}

/// Konten, in deren Adressbuch neue Kontakte gespeichert werden können.
#[tauri::command]
pub fn contact_accounts(state: State<'_, AppState>) -> Result<Vec<AccountId>> {
    let mut out = Vec::new();
    for a in state.store.accounts()? {
        if state
            .store
            .carddav_home(a.id)?
            .0
            .is_some_and(|h| !h.is_empty())
        {
            out.push(a.id);
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn list_collected(state: State<'_, AppState>, filter: String) -> Result<Vec<Suggestion>> {
    state.store.collected(&filter, 200)
}

#[tauri::command]
pub fn forget_collected(state: State<'_, AppState>, address: String) -> Result<()> {
    state.store.forget_collected(&address)
}

#[tauri::command]
pub fn suggest_addresses(state: State<'_, AppState>, prefix: String) -> Result<Vec<Suggestion>> {
    state.store.suggest_addresses(&prefix, 8)
}

/// Gegen den lokalen Testserver: `scripts/test-all.sh`.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod server_tests {
    use super::*;
    use kolibri_core::{Security, ServerConfig};
    use kolibri_store::{NewAccount, Store};

    #[tokio::test]
    #[ignore = "braucht den Testserver und schreibt in den Schlüsselbund"]
    async fn syncs_contacts_from_server() {
        if std::env::var("KOLIBRI_TEST_SERVER").is_err() {
            return;
        }
        let cfg = |port| ServerConfig {
            host: "localhost".into(),
            port,
            security: Security::None,
            username: "test@kolibri.test".into(),
        };
        let state = AppState::new(Store::open_in_memory().unwrap());
        let account = state
            .store
            .insert_account(&NewAccount {
                email: "test@kolibri.test".into(),
                display_name: "Test".into(),
                imap: cfg(1143),
                smtp: cfg(1587),
            })
            .unwrap();
        state
            .secrets
            .set_password(&account_key(account.id), "kolibri-test")
            .unwrap();

        assert!(
            sync_contacts(&state, &account).await.unwrap(),
            "erster Abgleich ohne Änderungen?"
        );
        let contacts = state.store.contacts("").unwrap();
        assert!(contacts.len() >= 4, "{contacts:?}");
        assert!(
            !sync_contacts(&state, &account).await.unwrap(),
            "zweiter Abgleich sollte nichts ändern"
        );
        let s = state.store.suggest_addresses("schä", 5).unwrap();
        assert_eq!(s.first().map(|s| s.name.as_str()), Some("Anna Schäfer"));

        state
            .secrets
            .delete_password(&account_key(account.id))
            .unwrap();
    }
}
