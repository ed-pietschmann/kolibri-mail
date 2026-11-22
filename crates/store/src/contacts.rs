//! Kontakte (aus CardDAV), gesammelte Empfänger und Adressvorschläge.

use std::collections::HashMap;

use kolibri_core::{AccountId, Address, Result};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Store, storage_err};

/// Wert mit Bezeichnung (E-Mail, Telefon).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Labeled {
    pub value: String,
    pub label: String,
}

/// Kontakt für Anzeige und Speicherung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: i64,
    /// None = nur auf diesem Mac.
    pub account_id: Option<AccountId>,
    pub name: String,
    pub org: String,
    pub emails: Vec<Labeled>,
    pub phones: Vec<Labeled>,
}

/// Kontakt, wie er vom Server kommt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactRecord {
    pub href: String,
    pub etag: String,
    pub uid: String,
    pub name: String,
    pub org: String,
    pub emails: Vec<Labeled>,
    pub phones: Vec<Labeled>,
}

/// Ablage eines Kontakts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactLocation {
    pub account_id: Option<AccountId>,
    pub book_url: String,
    pub href: String,
    pub etag: String,
    pub uid: String,
}

/// Vorschlag beim Tippen einer Adresse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub name: String,
    pub address: String,
    /// Aus den Kontakten (sonst: früherer Empfänger).
    pub contact: bool,
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Store {
    // ---- CardDAV-Stand ----------------------------------------------------------

    /// (Adressbuch-Verzeichnis, Zeitpunkt der letzten Suche danach).
    pub fn carddav_home(&self, account: AccountId) -> Result<(Option<String>, Option<i64>)> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT carddav_home, carddav_checked FROM accounts WHERE id = ?1",
            [account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(storage_err)
    }

    /// `home` = None: kein CardDAV gefunden (wird später erneut gesucht).
    pub fn set_carddav_home(&self, account: AccountId, home: Option<&str>) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE accounts SET carddav_home = ?2, carddav_checked = ?3 WHERE id = ?1",
            params![account, home.unwrap_or(""), now()],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    /// href → ETag der bekannten Kontakte eines Adressbuchs.
    pub fn contact_etags(&self, account: AccountId, book: &str) -> Result<HashMap<String, String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT href, etag FROM contacts WHERE account_id = ?1 AND book_url = ?2",
            )
            .map_err(storage_err)?;
        stmt.query_map(params![account, book], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(storage_err)?
            .collect::<rusqlite::Result<HashMap<_, _>>>()
            .map_err(storage_err)
    }

    /// Änderungen eines Adressbuchs übernehmen.
    pub fn apply_contacts(
        &self,
        account: AccountId,
        book: &str,
        changed: &[ContactRecord],
        removed: &[String],
    ) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        {
            let mut upsert = tx
                .prepare_cached(
                    "INSERT INTO contacts (account_id, book_url, href, etag, uid, name, org, emails, phones)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT (account_id, href) DO UPDATE SET
                        book_url = excluded.book_url, etag = excluded.etag, uid = excluded.uid,
                        name = excluded.name, org = excluded.org, emails = excluded.emails,
                        phones = excluded.phones",
                )
                .map_err(storage_err)?;
            for c in changed {
                upsert
                    .execute(params![
                        account,
                        book,
                        c.href,
                        c.etag,
                        c.uid,
                        c.name,
                        c.org,
                        json(&c.emails),
                        json(&c.phones)
                    ])
                    .map_err(storage_err)?;
            }
            let mut delete = tx
                .prepare_cached("DELETE FROM contacts WHERE account_id = ?1 AND href = ?2")
                .map_err(storage_err)?;
            for href in removed {
                delete
                    .execute(params![account, href])
                    .map_err(storage_err)?;
            }
        }
        tx.commit().map_err(storage_err)
    }

    /// Kontakte (optional gefiltert nach Name, Firma oder Adresse), alphabetisch.
    pub fn contacts(&self, filter: &str) -> Result<Vec<Contact>> {
        let pattern = format!("%{}%", filter.trim().replace(['%', '_'], ""));
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, account_id, name, org, emails, phones FROM contacts
                 WHERE ?1 = '%%' OR name LIKE ?1 OR org LIKE ?1 OR emails LIKE ?1
                 ORDER BY name COLLATE NOCASE LIMIT 2000",
            )
            .map_err(storage_err)?;
        stmt.query_map([pattern], |r| {
            let emails: String = r.get(4)?;
            let phones: String = r.get(5)?;
            Ok(Contact {
                id: r.get(0)?,
                account_id: r.get(1)?,
                name: r.get(2)?,
                org: r.get(3)?,
                emails: serde_json::from_str(&emails).unwrap_or_default(),
                phones: serde_json::from_str(&phones).unwrap_or_default(),
            })
        })
        .map_err(storage_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_err)
    }

    /// Wo ein Kontakt liegt (für Ändern/Löschen auf dem Server).
    pub fn contact_location(&self, id: i64) -> Result<ContactLocation> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT account_id, book_url, href, etag, uid FROM contacts WHERE id = ?1",
            [id],
            |r| {
                Ok(ContactLocation {
                    account_id: r.get(0)?,
                    book_url: r.get(1)?,
                    href: r.get(2)?,
                    etag: r.get(3)?,
                    uid: r.get(4)?,
                })
            },
        )
        .map_err(storage_err)
    }

    /// Kontakt speichern (neu oder geändert). Für Server-Kontakte nach dem PUT
    /// aufrufen; ETag leer = beim nächsten Abgleich frisch holen.
    pub fn upsert_contact(
        &self,
        account: Option<AccountId>,
        book: &str,
        rec: &ContactRecord,
    ) -> Result<i64> {
        let conn = self.conn()?;
        // UNIQUE (account_id, href) greift bei NULL nicht – lokal per href suchen.
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM contacts WHERE account_id IS ?1 AND href = ?2",
                params![account, rec.href],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_err)?;
        match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE contacts SET book_url = ?2, etag = ?3, uid = ?4, name = ?5, org = ?6,
                       emails = ?7, phones = ?8 WHERE id = ?1",
                    params![
                        id,
                        book,
                        rec.etag,
                        rec.uid,
                        rec.name,
                        rec.org,
                        json(&rec.emails),
                        json(&rec.phones)
                    ],
                )
                .map_err(storage_err)?;
                Ok(id)
            }
            None => {
                conn.execute(
                    "INSERT INTO contacts (account_id, book_url, href, etag, uid, name, org, emails, phones)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        account,
                        book,
                        rec.href,
                        rec.etag,
                        rec.uid,
                        rec.name,
                        rec.org,
                        json(&rec.emails),
                        json(&rec.phones)
                    ],
                )
                .map_err(storage_err)?;
                Ok(conn.last_insert_rowid())
            }
        }
    }

    pub fn delete_contact(&self, id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM contacts WHERE id = ?1", [id])
            .map_err(storage_err)?;
        Ok(())
    }

    /// Frühere Empfänger, die (noch) kein Kontakt sind – häufigste zuerst.
    pub fn collected(&self, filter: &str, limit: usize) -> Result<Vec<Suggestion>> {
        let pattern = format!("%{}%", filter.trim().replace(['%', '_'], ""));
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT c.address, c.name FROM collected_addresses c
                 WHERE (?1 = '%%' OR c.address LIKE ?1 OR c.name LIKE ?1)
                   AND NOT EXISTS (SELECT 1 FROM contacts k, json_each(k.emails) e
                                   WHERE lower(json_extract(e.value, '$.value')) = lower(c.address))
                 ORDER BY c.use_count DESC, c.last_used DESC LIMIT ?2",
            )
            .map_err(storage_err)?;
        stmt.query_map(
            params![pattern, i64::try_from(limit).unwrap_or(i64::MAX)],
            |r| {
                Ok(Suggestion {
                    address: r.get(0)?,
                    name: r.get(1)?,
                    contact: false,
                })
            },
        )
        .map_err(storage_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_err)
    }

    /// Früheren Empfänger vergessen.
    pub fn forget_collected(&self, address: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM collected_addresses WHERE address = ?1",
            [address],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    // ---- Gesammelte Adressen --------------------------------------------------------

    /// Empfänger einer gesendeten Nachricht merken.
    pub fn record_recipients(&self, recipients: &[Address]) -> Result<()> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "INSERT INTO collected_addresses (address, name, use_count, last_used) VALUES (?1, ?2, 1, ?3)
                 ON CONFLICT (address) DO UPDATE SET
                    use_count = use_count + 1, last_used = MAX(last_used, excluded.last_used),
                    name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE name END",
            )
            .map_err(storage_err)?;
        let t = now();
        for a in recipients.iter().filter(|a| a.addr.contains('@')) {
            stmt.execute(params![a.addr.trim(), a.name.trim(), t])
                .map_err(storage_err)?;
        }
        Ok(())
    }

    /// Empfänger aus allen Gesendet-Ordnern übernehmen (beim Sync). Zählt jede
    /// Nachricht nur einmal (über `collected_from` merken wir den Stand).
    pub fn collect_from_sent(&self) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        let since: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(last_used), 0) FROM collected_addresses",
                [],
                |r| r.get(0),
            )
            .map_err(storage_err)?;
        let changed = tx
            .execute(
                "INSERT INTO collected_addresses (address, name, use_count, last_used)
                 SELECT json_extract(j.value, '$.addr'), MAX(json_extract(j.value, '$.name')),
                        COUNT(*), MAX(m.date)
                 FROM messages m, json_each(m.to_addrs || '') j
                 WHERE m.folder_id IN (SELECT id FROM folders WHERE role = 'sent')
                   AND m.date > ?1 AND json_extract(j.value, '$.addr') LIKE '%@%'
                 GROUP BY json_extract(j.value, '$.addr') COLLATE NOCASE
                 ON CONFLICT (address) DO UPDATE SET
                    use_count = use_count + excluded.use_count,
                    last_used = MAX(last_used, excluded.last_used),
                    name = CASE WHEN name = '' THEN excluded.name ELSE name END",
                [since],
            )
            .map_err(storage_err)?;
        tx.commit().map_err(storage_err)?;
        Ok(changed)
    }

    // ---- Vorschläge -----------------------------------------------------------------

    /// Bis zu `limit` Vorschläge: Wortanfang im Namen oder Anfang der Adresse.
    /// Kontakte vor früheren Empfängern; diese nach Häufigkeit und Aktualität.
    pub fn suggest_addresses(&self, prefix: &str, limit: usize) -> Result<Vec<Suggestion>> {
        let needle = prefix.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(vec![]);
        }
        let like = format!("{}%", needle.replace(['%', '_'], ""));
        let word = format!("% {}%", needle.replace(['%', '_'], ""));
        let conn = self.conn()?;
        let mut out: Vec<Suggestion> = Vec::new();
        let mut push = |s: Suggestion| {
            if !out
                .iter()
                .any(|o| o.address.eq_ignore_ascii_case(&s.address))
            {
                out.push(s);
            }
        };
        {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT c.name, json_extract(e.value, '$.value') AS addr
                     FROM contacts c, json_each(c.emails) e
                     WHERE lower(c.name) LIKE ?1 OR lower(c.name) LIKE ?2 OR lower(addr) LIKE ?1
                     ORDER BY c.name COLLATE NOCASE LIMIT 50",
                )
                .map_err(storage_err)?;
            let rows = stmt
                .query_map(params![like, word], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(storage_err)?;
            for row in rows {
                let (name, address) = row.map_err(storage_err)?;
                push(Suggestion {
                    name,
                    address,
                    contact: true,
                });
            }
        }
        {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT name, address FROM collected_addresses
                     WHERE lower(address) LIKE ?1 OR lower(name) LIKE ?1 OR lower(name) LIKE ?2
                     ORDER BY use_count DESC, last_used DESC LIMIT 50",
                )
                .map_err(storage_err)?;
            let rows = stmt
                .query_map(params![like, word], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(storage_err)?;
            for row in rows {
                let (name, address) = row.map_err(storage_err)?;
                push(Suggestion {
                    name,
                    address,
                    contact: false,
                });
            }
        }
        out.truncate(limit);
        Ok(out)
    }

    /// Kontaktname zu einer Adresse (für die Anzeige), falls bekannt.
    pub fn contact_name(&self, address: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT c.name FROM contacts c, json_each(c.emails) e
             WHERE json_extract(e.value, '$.value') = ?1 COLLATE NOCASE LIMIT 1",
            [address],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_err)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::NewAccount;
    use kolibri_core::{Flags, FolderRole, RemoteFolder, RemoteMessage, Security, ServerConfig};

    fn setup() -> (Store, AccountId) {
        let store = Store::open_in_memory().unwrap();
        let s = ServerConfig {
            host: "h".into(),
            port: 1,
            security: Security::Tls,
            username: "u".into(),
        };
        let a = store
            .insert_account(&NewAccount {
                email: "ich@x.de".into(),
                display_name: "Ich".into(),
                imap: s.clone(),
                smtp: s,
            })
            .unwrap();
        (store, a.id)
    }

    fn rec(href: &str, name: &str, mails: &[&str]) -> ContactRecord {
        ContactRecord {
            href: href.into(),
            etag: format!("e-{href}"),
            uid: href.into(),
            name: name.into(),
            org: String::new(),
            emails: mails
                .iter()
                .map(|m| Labeled {
                    value: (*m).into(),
                    label: String::new(),
                })
                .collect(),
            phones: vec![],
        }
    }

    #[test]
    fn contacts_roundtrip_and_removal() {
        let (store, a) = setup();
        assert_eq!(store.carddav_home(a).unwrap(), (None, None));
        store.set_carddav_home(a, Some("https://x/card/")).unwrap();
        assert_eq!(
            store.carddav_home(a).unwrap().0.as_deref(),
            Some("https://x/card/")
        );

        store
            .apply_contacts(
                a,
                "b",
                &[
                    rec("/1", "Anna Schäfer", &["anna@x.de"]),
                    rec("/2", "Max", &["max@x.de"]),
                ],
                &[],
            )
            .unwrap();
        assert_eq!(store.contacts("").unwrap().len(), 2);
        assert_eq!(store.contacts("schä").unwrap()[0].name, "Anna Schäfer");
        assert_eq!(store.contacts("max@").unwrap().len(), 1);
        assert_eq!(store.contact_etags(a, "b").unwrap().len(), 2);
        assert_eq!(
            store.contact_name("ANNA@x.de").unwrap().as_deref(),
            Some("Anna Schäfer")
        );

        store
            .apply_contacts(
                a,
                "b",
                &[rec("/2", "Max Braun", &["max@x.de"])],
                &["/1".into()],
            )
            .unwrap();
        let all = store.contacts("").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Max Braun");
    }

    #[test]
    fn suggestions_rank_contacts_then_frequent() {
        let (store, a) = setup();
        store
            .apply_contacts(
                a,
                "b",
                &[rec("/1", "Anna Schäfer", &["anna@x.de", "a.s@firma.de"])],
                &[],
            )
            .unwrap();
        let addr = |n: &str, a: &str| Address {
            name: n.into(),
            addr: a.into(),
        };
        store.record_recipients(&[addr("", "anton@y.de")]).unwrap();
        store
            .record_recipients(&[addr("Andrea", "andrea@y.de")])
            .unwrap();
        store.record_recipients(&[addr("", "andrea@y.de")]).unwrap();
        store.record_recipients(&[addr("", "anna@x.de")]).unwrap(); // auch Kontakt → nur einmal

        let s = store.suggest_addresses("an", 10).unwrap();
        let addrs: Vec<_> = s.iter().map(|s| s.address.as_str()).collect();
        assert_eq!(
            addrs,
            ["anna@x.de", "a.s@firma.de", "andrea@y.de", "anton@y.de"]
        );
        assert!(s[0].contact && !s[2].contact);
        assert_eq!(
            s[2].name, "Andrea",
            "Name bleibt beim erneuten Senden ohne Namen erhalten"
        );
        // Wortanfang im Nachnamen.
        assert_eq!(store.suggest_addresses("schä", 10).unwrap().len(), 2);
        assert!(store.suggest_addresses("", 10).unwrap().is_empty());
        assert_eq!(store.suggest_addresses("an", 2).unwrap().len(), 2);
    }

    #[test]
    fn collects_from_sent_once() {
        let (store, a) = setup();
        store
            .replace_folders(
                a,
                &[RemoteFolder {
                    path: "Sent".into(),
                    delimiter: Some("/".into()),
                    role: Some(FolderRole::Sent),
                    selectable: true,
                }],
            )
            .unwrap();
        let sent = store
            .folder_by_role(a, FolderRole::Sent)
            .unwrap()
            .unwrap()
            .id;
        let m = |uid: u32, date: i64| RemoteMessage {
            uid,
            message_id: Some(format!("s{uid}@x")),
            in_reply_to: None,
            references: None,
            subject: "s".into(),
            from: Address {
                name: String::new(),
                addr: "ich@x.de".into(),
            },
            to: vec![Address {
                name: "Lena Wolf".into(),
                addr: "lena@x.de".into(),
            }],
            cc: vec![],
            reply_to: vec![],
            date,
            size: 1,
            flags: Flags::default(),
            has_attachments: false,
        };
        store.insert_messages(sent, &[m(1, 10), m(2, 20)]).unwrap();
        assert_eq!(store.collect_from_sent().unwrap(), 1);
        assert_eq!(
            store.collect_from_sent().unwrap(),
            0,
            "kein doppeltes Zählen"
        );
        let s = store.suggest_addresses("lena", 5).unwrap();
        assert_eq!(s[0].name, "Lena Wolf");
        store.insert_messages(sent, &[m(3, 30)]).unwrap();
        assert_eq!(store.collect_from_sent().unwrap(), 1);
    }
}
