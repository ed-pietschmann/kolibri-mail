//! Signaturen, Vorlagen und Zusatzdaten von Entwürfen.

use kolibri_core::{AccountId, Error, ErrorKind, Result};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Store, storage_err};

/// Platzhalter für den geschriebenen Text in einer Vorlage.
pub const CONTENT_PLACEHOLDER: &str = "{{inhalt}}";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    pub id: i64,
    pub account_id: AccountId,
    pub name: String,
    /// Gesäubertes HTML-Fragment.
    pub html: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub html: String,
    /// Mitgeliefert (nicht änderbar, nur kopierbar).
    pub builtin: bool,
}

impl Template {
    /// Enthält einen Platz für eigenen Text (sonst: fertige Mail aus Code).
    pub fn has_content_slot(&self) -> bool {
        self.html.contains(CONTENT_PLACEHOLDER)
    }
}

/// Was beim Verfassen über die Mail hinaus gemerkt wird (nur lokal).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftMeta {
    pub template_id: Option<i64>,
    pub signature_id: Option<i64>,
    pub editor_html: String,
    pub code: Option<String>,
}

fn signature_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Signature> {
    Ok(Signature {
        id: row.get(0)?,
        account_id: row.get(1)?,
        name: row.get(2)?,
        html: row.get(3)?,
        is_default: row.get::<_, i64>(4)? != 0,
    })
}

fn template_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Template> {
    Ok(Template {
        id: row.get(0)?,
        name: row.get(1)?,
        html: row.get(2)?,
        builtin: row.get::<_, i64>(3)? != 0,
    })
}

const SIGNATURE_COLS: &str = "id, account_id, name, html, is_default FROM signatures";
const TEMPLATE_COLS: &str = "id, name, html, builtin FROM templates";

impl Store {
    // ---- Signaturen ---------------------------------------------------------

    /// Alle Signaturen (aller Konten), je Konto in Reihenfolge.
    pub fn signatures(&self) -> Result<Vec<Signature>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {SIGNATURE_COLS} ORDER BY account_id, sort_order, id"
            ))
            .map_err(storage_err)?;
        let rows = stmt
            .query_map([], signature_row)
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)?;
        Ok(rows)
    }

    pub fn signature(&self, id: i64) -> Result<Option<Signature>> {
        let conn = self.conn()?;
        conn.query_row(
            &format!("SELECT {SIGNATURE_COLS} WHERE id = ?1"),
            [id],
            signature_row,
        )
        .optional()
        .map_err(storage_err)
    }

    /// Standard-Signatur eines Kontos (falls vorhanden).
    pub fn default_signature(&self, account: AccountId) -> Result<Option<Signature>> {
        let conn = self.conn()?;
        conn.query_row(
            &format!("SELECT {SIGNATURE_COLS} WHERE account_id = ?1 AND is_default = 1 LIMIT 1"),
            [account],
            signature_row,
        )
        .optional()
        .map_err(storage_err)
    }

    /// Neu anlegen (`id == 0`) oder ändern. `html` muss bereits gesäubert sein.
    /// Die erste Signatur eines Kontos wird automatisch Standard.
    pub fn save_signature(&self, sig: &Signature) -> Result<Signature> {
        let name = sig.name.trim();
        if name.is_empty() {
            return Err(Error::new(ErrorKind::Invalid, "Bitte einen Namen angeben."));
        }
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        let id = if sig.id == 0 {
            let first: bool = tx
                .query_row(
                    "SELECT NOT EXISTS (SELECT 1 FROM signatures WHERE account_id = ?1)",
                    [sig.account_id],
                    |r| r.get(0),
                )
                .map_err(storage_err)?;
            tx.execute(
                "INSERT INTO signatures (account_id, name, html, is_default, sort_order)
                 VALUES (?1, ?2, ?3, ?4,
                   (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM signatures WHERE account_id = ?1))",
                params![sig.account_id, name, sig.html, i64::from(first || sig.is_default)],
            )
            .map_err(storage_err)?;
            tx.last_insert_rowid()
        } else {
            let changed = tx
                .execute(
                    "UPDATE signatures SET name = ?2, html = ?3 WHERE id = ?1",
                    params![sig.id, name, sig.html],
                )
                .map_err(storage_err)?;
            if changed == 0 {
                return Err(Error::new(ErrorKind::NotFound, "Signatur nicht gefunden"));
            }
            sig.id
        };
        if sig.is_default {
            tx.execute(
                "UPDATE signatures SET is_default = (id = ?1) WHERE account_id = ?2",
                params![id, sig.account_id],
            )
            .map_err(storage_err)?;
        }
        let saved = tx
            .query_row(
                &format!("SELECT {SIGNATURE_COLS} WHERE id = ?1"),
                [id],
                signature_row,
            )
            .map_err(storage_err)?;
        tx.commit().map_err(storage_err)?;
        Ok(saved)
    }

    /// Als Standard setzen (`None` = beim Verfassen keine Signatur).
    pub fn set_default_signature(&self, account: AccountId, id: Option<i64>) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE signatures SET is_default = (id IS ?2) WHERE account_id = ?1",
            params![account, id],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    pub fn delete_signature(&self, id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM signatures WHERE id = ?1", [id])
            .map_err(storage_err)?;
        Ok(())
    }

    // ---- Vorlagen -----------------------------------------------------------

    pub fn templates(&self) -> Result<Vec<Template>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {TEMPLATE_COLS} ORDER BY builtin DESC, sort_order, id"
            ))
            .map_err(storage_err)?;
        let rows = stmt
            .query_map([], template_row)
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)?;
        Ok(rows)
    }

    pub fn template(&self, id: i64) -> Result<Option<Template>> {
        let conn = self.conn()?;
        conn.query_row(
            &format!("SELECT {TEMPLATE_COLS} WHERE id = ?1"),
            [id],
            template_row,
        )
        .optional()
        .map_err(storage_err)
    }

    /// Eigene Vorlage anlegen (`id == 0`) oder ändern. Mitgelieferte sind fest.
    pub fn save_template(&self, t: &Template) -> Result<Template> {
        let name = t.name.trim();
        if name.is_empty() {
            return Err(Error::new(ErrorKind::Invalid, "Bitte einen Namen angeben."));
        }
        if t.html.trim().is_empty() {
            return Err(Error::new(ErrorKind::Invalid, "Die Vorlage ist leer."));
        }
        let conn = self.conn()?;
        let id = if t.id == 0 {
            conn.execute(
                "INSERT INTO templates (name, html, sort_order)
                 VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM templates))",
                params![name, t.html],
            )
            .map_err(storage_err)?;
            conn.last_insert_rowid()
        } else {
            let changed = conn
                .execute(
                    "UPDATE templates SET name = ?2, html = ?3 WHERE id = ?1 AND builtin = 0",
                    params![t.id, name, t.html],
                )
                .map_err(storage_err)?;
            if changed == 0 {
                return Err(Error::new(
                    ErrorKind::Invalid,
                    "Mitgelieferte Vorlagen können nicht geändert werden – bitte kopieren.",
                ));
            }
            t.id
        };
        conn.query_row(
            &format!("SELECT {TEMPLATE_COLS} WHERE id = ?1"),
            [id],
            template_row,
        )
        .map_err(storage_err)
    }

    pub fn delete_template(&self, id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM templates WHERE id = ?1 AND builtin = 0", [id])
            .map_err(storage_err)?;
        Ok(())
    }

    // ---- Entwürfe -----------------------------------------------------------

    pub fn set_draft_meta(&self, draft_id: &str, meta: &DraftMeta) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO draft_meta (draft_id, template_id, signature_id, editor_html, code)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(draft_id) DO UPDATE SET template_id = excluded.template_id,
               signature_id = excluded.signature_id, editor_html = excluded.editor_html,
               code = excluded.code, updated_at = unixepoch()",
            params![
                draft_id,
                meta.template_id,
                meta.signature_id,
                meta.editor_html,
                meta.code
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    pub fn draft_meta(&self, draft_id: &str) -> Result<Option<DraftMeta>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT template_id, signature_id, editor_html, code FROM draft_meta WHERE draft_id = ?1",
            [draft_id],
            |r| {
                Ok(DraftMeta {
                    template_id: r.get(0)?,
                    signature_id: r.get(1)?,
                    editor_html: r.get(2)?,
                    code: r.get(3)?,
                })
            },
        )
        .optional()
        .map_err(storage_err)
    }

    pub fn delete_draft_meta(&self, draft_id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM draft_meta WHERE draft_id = ?1", [draft_id])
            .map_err(storage_err)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::NewAccount;
    use kolibri_core::{Security, ServerConfig};

    fn store_with_account() -> (Store, AccountId) {
        let store = Store::open_in_memory().unwrap();
        let s = ServerConfig {
            host: "h".into(),
            port: 1,
            security: Security::Tls,
            username: "u".into(),
        };
        let a = store
            .insert_account(&NewAccount {
                email: "a@b.de".into(),
                display_name: "A".into(),
                imap: s.clone(),
                smtp: s,
            })
            .unwrap();
        (store, a.id)
    }

    fn sig(account_id: AccountId, name: &str) -> Signature {
        Signature {
            id: 0,
            account_id,
            name: name.into(),
            html: "<p>x</p>".into(),
            is_default: false,
        }
    }

    #[test]
    fn first_signature_becomes_default_and_default_is_unique() {
        let (store, acc) = store_with_account();
        let a = store.save_signature(&sig(acc, "Privat")).unwrap();
        assert!(a.is_default);
        let mut b = sig(acc, "Arbeit");
        b.is_default = true;
        let b = store.save_signature(&b).unwrap();
        assert!(b.is_default);
        assert_eq!(store.default_signature(acc).unwrap().unwrap().id, b.id);
        assert_eq!(store.signatures().unwrap().len(), 2);
        store.set_default_signature(acc, None).unwrap();
        assert!(store.default_signature(acc).unwrap().is_none());
        store.delete_signature(a.id).unwrap();
        assert_eq!(store.signatures().unwrap().len(), 1);
    }

    #[test]
    fn builtin_templates_exist_and_are_read_only() {
        let (store, _) = store_with_account();
        let list = store.templates().unwrap();
        assert!(list.len() >= 4);
        assert!(list.iter().all(|t| t.builtin && t.has_content_slot()));
        let mut t = list[0].clone();
        t.html = "x".into();
        assert!(store.save_template(&t).is_err());
        store.delete_template(t.id).unwrap();
        assert_eq!(store.templates().unwrap().len(), list.len());

        let own = store
            .save_template(&Template {
                id: 0,
                name: "Mein".into(),
                html: "<p>fertig</p>".into(),
                builtin: false,
            })
            .unwrap();
        assert!(!own.has_content_slot());
        store.delete_template(own.id).unwrap();
    }

    #[test]
    fn draft_meta_roundtrip() {
        let (store, _) = store_with_account();
        let meta = DraftMeta {
            template_id: Some(2),
            signature_id: None,
            editor_html: "<p>Hallo</p>".into(),
            code: None,
        };
        store.set_draft_meta("x@y", &meta).unwrap();
        assert_eq!(store.draft_meta("x@y").unwrap().unwrap(), meta);
        store.delete_draft_meta("x@y").unwrap();
        assert!(store.draft_meta("x@y").unwrap().is_none());
    }

    #[test]
    fn local_contacts_calendars_and_sources() {
        use crate::{ContactRecord, Labeled};
        let (store, acc) = store_with_account();
        store
            .record_recipients(&[
                kolibri_core::Address {
                    name: "Anna".into(),
                    addr: "anna@x.de".into(),
                },
                kolibri_core::Address {
                    name: "Max".into(),
                    addr: "max@y.de".into(),
                },
            ])
            .unwrap();
        assert_eq!(store.collected("", 10).unwrap().len(), 2);

        // Lokaler Kontakt für Anna → sie ist kein „früherer Empfänger“ mehr.
        let rec = ContactRecord {
            href: "local:1".into(),
            etag: String::new(),
            uid: "u1".into(),
            name: "Anna Schäfer".into(),
            org: String::new(),
            emails: vec![Labeled {
                value: "Anna@x.de".into(),
                label: String::new(),
            }],
            phones: vec![],
        };
        let id = store.upsert_contact(None, "local", &rec).unwrap();
        assert_eq!(store.upsert_contact(None, "local", &rec).unwrap(), id);
        assert_eq!(store.contacts("").unwrap()[0].account_id, None);
        let left = store.collected("", 10).unwrap();
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0].address, "max@y.de");
        store.forget_collected("max@y.de").unwrap();
        assert!(store.collected("", 10).unwrap().is_empty());
        store.delete_contact(id).unwrap();
        assert!(store.contacts("").unwrap().is_empty());

        // Eigener lokaler Kalender, der Standard-Kalender ist nicht löschbar.
        let cal = store.create_local_calendar("Sport", "#ff3b30").unwrap();
        store.update_calendar(cal, "Training", "#34c759").unwrap();
        let c = store.calendar(cal).unwrap();
        assert_eq!((c.name.as_str(), c.color.as_str()), ("Training", "#34c759"));
        let local = store.calendars().unwrap()[0].id;
        store.delete_calendar(local).unwrap();
        store.delete_calendar(cal).unwrap();
        assert_eq!(store.calendars().unwrap().len(), 1);

        // Verbundenes Konto mit zwei Kalendern, dann einer weg.
        let src = store
            .insert_dav_source("iCloud", "https://caldav.icloud.com", "me", "h")
            .unwrap();
        let two = [
            ("https://c/a".to_owned(), "A".to_owned(), "#111".to_owned()),
            ("https://c/b".to_owned(), "B".to_owned(), String::new()),
        ];
        store.replace_source_calendars(src, &two).unwrap();
        store.replace_source_calendars(src, &two[..1]).unwrap();
        let from_src: Vec<_> = store
            .calendars()
            .unwrap()
            .into_iter()
            .filter(|c| c.source_id == Some(src))
            .collect();
        assert_eq!(from_src.len(), 1);
        store.delete_dav_source(src).unwrap();
        assert_eq!(
            store.calendars().unwrap().len(),
            1,
            "Kalender mit Verbindung gelöscht"
        );
        let _ = acc;
    }
}
