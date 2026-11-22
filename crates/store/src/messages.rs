//! Nachrichten im lokalen Cache.

use kolibri_core::{
    AccountId, Address, Body, Error, ErrorKind, Flags, FolderId, MessageId, MessageScope,
    MessageSummary, RemoteMessage, Result,
};
use rusqlite::{OptionalExtension, Row, params, params_from_iter};

use crate::{Store, storage_err};

/// Synchronisationsstand eines Ordners.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncState {
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub last_sync: Option<i64>,
}

/// Kopf einer Nachricht + Ort auf dem Server (für das Nachladen des Inhalts).
#[derive(Debug, Clone)]
pub struct MessageLocation {
    pub summary: MessageSummary,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub folder_path: String,
    pub uid: u32,
}

pub(crate) const SUMMARY_COLUMNS: &str = "m.id, m.account_id, m.folder_id, m.subject, m.from_name,
    m.from_addr, m.date, m.seen, m.flagged, m.answered, m.has_attachments, m.preview,
    COALESCE(NULLIF(json_extract(m.to_addrs, '$[0].name'), ''), json_extract(m.to_addrs, '$[0].addr'), ''),
    json_array_length(m.to_addrs) + json_array_length(m.cc_addrs),
    EXISTS (SELECT 1 FROM accounts a WHERE a.email = m.from_addr COLLATE NOCASE)";

/// Anzahl Spalten in `SUMMARY_COLUMNS` (weitere Spalten folgen dahinter).
pub(crate) const SUMMARY_LEN: usize = 15;

pub(crate) fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<MessageSummary> {
    Ok(MessageSummary {
        id: row.get(0)?,
        account_id: row.get(1)?,
        folder_id: row.get(2)?,
        subject: row.get(3)?,
        from_name: row.get(4)?,
        from_addr: row.get(5)?,
        date: row.get(6)?,
        seen: row.get(7)?,
        flagged: row.get(8)?,
        answered: row.get(9)?,
        has_attachments: row.get(10)?,
        preview: row.get(11)?,
        to_display: row.get(12)?,
        to_count: row.get(13)?,
        outgoing: row.get(14)?,
        thread_count: 1,
    })
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

fn from_json<T: serde::de::DeserializeOwned + Default>(json: &str) -> T {
    serde_json::from_str(json).unwrap_or_default()
}

pub(crate) fn scope_filter(scope: MessageScope) -> (String, Vec<i64>) {
    match scope {
        MessageScope::Folder { id } => ("m.pending_move = 0 AND m.folder_id = ?".into(), vec![id]),
        MessageScope::Role { role } => (
            format!(
                "m.pending_move = 0 AND m.folder_id IN (SELECT id FROM folders WHERE role = '{}')",
                role.as_str()
            ),
            vec![],
        ),
        MessageScope::Flagged => (
            "m.pending_move = 0 AND m.flagged = 1 AND m.folder_id NOT IN (SELECT id FROM folders WHERE role IN ('trash','junk'))"
                .into(),
            vec![],
        ),
    }
}

/// Was zum Antworten/Weiterleiten über eine Nachricht bekannt ist.
#[derive(Debug, Clone)]
pub struct ComposeSource {
    pub summary: MessageSummary,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub message_id: Option<String>,
    pub references: Option<String>,
    pub in_reply_to: Option<String>,
}

impl Store {
    pub fn compose_source(&self, id: MessageId) -> Result<ComposeSource> {
        let conn = self.conn()?;
        conn.query_row(
            &format!(
                "SELECT {SUMMARY_COLUMNS}, m.to_addrs, m.cc_addrs, m.reply_to, m.message_id,
                    m.refs, m.in_reply_to
                 FROM messages m WHERE m.id = ?1"
            ),
            [id],
            |r| {
                Ok(ComposeSource {
                    summary: summary_from_row(r)?,
                    to: from_json(&r.get::<_, String>(SUMMARY_LEN)?),
                    cc: from_json(&r.get::<_, String>(SUMMARY_LEN + 1)?),
                    reply_to: from_json(&r.get::<_, String>(SUMMARY_LEN + 2)?),
                    message_id: r.get(SUMMARY_LEN + 3)?,
                    references: r.get(SUMMARY_LEN + 4)?,
                    in_reply_to: r.get(SUMMARY_LEN + 5)?,
                })
            },
        )
        .optional()
        .map_err(storage_err)?
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Nachricht {id}")))
    }

    // ---- Sync-Stand -----------------------------------------------------------

    pub fn sync_state(&self, folder: FolderId) -> Result<SyncState> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT uid_validity, uid_next, last_sync FROM folders WHERE id = ?1",
            [folder],
            |r| {
                Ok(SyncState {
                    uid_validity: r.get(0)?,
                    uid_next: r.get(1)?,
                    last_sync: r.get(2)?,
                })
            },
        )
        .map_err(storage_err)
    }

    pub fn set_sync_state(&self, folder: FolderId, state: SyncState) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE folders SET uid_validity = ?2, uid_next = ?3, last_sync = ?4 WHERE id = ?1",
            params![folder, state.uid_validity, state.uid_next, state.last_sync],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    /// UIDVALIDITY hat sich geändert: alle lokalen Nachrichten des Ordners verwerfen.
    pub fn clear_folder(&self, folder: FolderId) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM messages WHERE folder_id = ?1", [folder])
            .map_err(storage_err)?;
        Ok(())
    }

    /// Alle bekannten UIDs eines Ordners, aufsteigend.
    pub fn folder_uids(&self, folder: FolderId) -> Result<Vec<u32>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached("SELECT uid FROM messages WHERE folder_id = ?1 ORDER BY uid")
            .map_err(storage_err)?;
        stmt.query_map([folder], |r| r.get(0))
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<u32>>>()
            .map_err(storage_err)
    }

    // ---- Schreiben ------------------------------------------------------------

    pub fn insert_messages(&self, folder: FolderId, messages: &[RemoteMessage]) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        let mut inserted = 0;
        let account: AccountId = tx
            .query_row(
                "SELECT account_id FROM folders WHERE id = ?1",
                [folder],
                |r| r.get(0),
            )
            .map_err(storage_err)?;
        {
            let mut set_root = tx
                .prepare_cached("UPDATE messages SET thread_root = ?2 WHERE id = ?1")
                .map_err(storage_err)?;
            let mut stmt = tx
                .prepare_cached(
                    "INSERT OR IGNORE INTO messages (account_id, folder_id, uid, message_id,
                        in_reply_to, refs, subject, from_name, from_addr, to_addrs, cc_addrs,
                        date, size, seen, flagged, answered, draft, has_attachments, reply_to)
                     SELECT account_id, id, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                        ?13, ?14, ?15, ?16, ?17, ?18
                     FROM folders WHERE id = ?1",
                )
                .map_err(storage_err)?;
            for m in messages {
                let added = stmt
                    .execute(params![
                        folder,
                        m.uid,
                        m.message_id,
                        m.in_reply_to,
                        m.references,
                        m.subject,
                        m.from.name,
                        m.from.addr,
                        to_json(&m.to),
                        to_json(&m.cc),
                        m.date,
                        m.size,
                        m.flags.seen,
                        m.flags.flagged,
                        m.flags.answered,
                        m.flags.draft,
                        m.has_attachments,
                        to_json(&m.reply_to),
                    ])
                    .map_err(storage_err)?;
                if added > 0 {
                    let id = tx.last_insert_rowid();
                    let root = crate::threads::assign_root(
                        &tx,
                        account,
                        m.message_id.as_deref(),
                        m.references.as_deref(),
                        m.in_reply_to.as_deref(),
                    )
                    .map_err(storage_err)?;
                    set_root.execute(params![id, root]).map_err(storage_err)?;
                }
                inserted += added;
            }
        }
        tx.commit().map_err(storage_err)?;
        Ok(inserted)
    }

    /// Setzt Flags; liefert die Zahl tatsächlich geänderter Nachrichten.
    pub fn update_flags(&self, folder: FolderId, flags: &[(u32, Flags)]) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        let mut changed = 0;
        {
            let mut stmt = tx
                .prepare_cached(
                    "UPDATE messages SET seen = ?3, flagged = ?4, answered = ?5, draft = ?6
                     WHERE folder_id = ?1 AND uid = ?2
                       AND (seen, flagged, answered, draft) IS NOT (?3, ?4, ?5, ?6)",
                )
                .map_err(storage_err)?;
            for (uid, f) in flags {
                changed += stmt
                    .execute(params![folder, uid, f.seen, f.flagged, f.answered, f.draft])
                    .map_err(storage_err)?;
            }
        }
        tx.commit().map_err(storage_err)?;
        Ok(changed)
    }

    pub fn delete_uids(&self, folder: FolderId, uids: &[u32]) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        let mut deleted = 0;
        {
            let mut stmt = tx
                .prepare_cached("DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2")
                .map_err(storage_err)?;
            for uid in uids {
                deleted += stmt.execute(params![folder, uid]).map_err(storage_err)?;
            }
        }
        tx.commit().map_err(storage_err)?;
        Ok(deleted)
    }

    pub fn save_body(&self, message: MessageId, body: &Body) -> Result<()> {
        let attachments = to_json(&body.attachments);
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        tx.execute(
            "INSERT OR REPLACE INTO message_bodies (message_id, html, text, remote_images, attachments, invite)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                message,
                body.html,
                body.text,
                body.remote_images,
                attachments,
                body.invite.as_ref().map(to_json)
            ],
        )
        .map_err(storage_err)?;
        tx.execute(
            "UPDATE messages SET preview = ?2, has_attachments = ?3 WHERE id = ?1",
            params![message, body.preview, !body.attachments.is_empty()],
        )
        .map_err(storage_err)?;
        tx.commit().map_err(storage_err)
    }

    // ---- Lesen ----------------------------------------------------------------

    pub fn count_messages(&self, scope: MessageScope) -> Result<u32> {
        let (filter, args) = scope_filter(scope);
        let conn = self.conn()?;
        conn.query_row(
            &format!("SELECT COUNT(*) FROM messages m WHERE {filter}"),
            params_from_iter(args),
            |r| r.get(0),
        )
        .map_err(storage_err)
    }

    /// Ausschnitt einer Liste, neueste zuerst.
    pub fn list_messages(
        &self,
        scope: MessageScope,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<MessageSummary>> {
        let (filter, mut args) = scope_filter(scope);
        args.push(i64::from(limit));
        args.push(i64::from(offset));
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM messages m WHERE {filter}
                 ORDER BY m.date DESC, m.id DESC LIMIT ? OFFSET ?"
            ))
            .map_err(storage_err)?;
        stmt.query_map(params_from_iter(args), summary_from_row)
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)
    }

    pub fn message_location(&self, id: MessageId) -> Result<MessageLocation> {
        let conn = self.conn()?;
        conn.query_row(
            &format!(
                "SELECT {SUMMARY_COLUMNS}, m.to_addrs, m.cc_addrs, f.path, m.uid
                 FROM messages m JOIN folders f ON f.id = m.folder_id WHERE m.id = ?1"
            ),
            [id],
            |r| {
                Ok(MessageLocation {
                    summary: summary_from_row(r)?,
                    to: from_json(&r.get::<_, String>(SUMMARY_LEN)?),
                    cc: from_json(&r.get::<_, String>(SUMMARY_LEN + 1)?),
                    folder_path: r.get(SUMMARY_LEN + 2)?,
                    uid: r.get(SUMMARY_LEN + 3)?,
                })
            },
        )
        .optional()
        .map_err(storage_err)?
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Nachricht {id}")))
    }

    pub fn body(&self, id: MessageId) -> Result<Option<Body>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT b.html, b.text, b.remote_images, b.attachments, m.preview, b.invite
             FROM message_bodies b JOIN messages m ON m.id = b.message_id
             WHERE b.message_id = ?1",
            [id],
            |r| {
                let attachments: String = r.get(3)?;
                Ok(Body {
                    html: r.get(0)?,
                    text: r.get(1)?,
                    remote_images: r.get(2)?,
                    attachments: from_json(&attachments),
                    preview: r.get(4)?,
                    invite: r
                        .get::<_, Option<String>>(5)?
                        .and_then(|j| serde_json::from_str(&j).ok()),
                })
            },
        )
        .optional()
        .map_err(storage_err)
    }

    /// Nachrichten ohne geladenen Inhalt, neueste zuerst (für das Vorladen).
    pub fn messages_without_body(
        &self,
        folder: FolderId,
        max_size: u32,
        limit: u32,
    ) -> Result<Vec<(MessageId, u32)>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT m.id, m.uid FROM messages m
                 LEFT JOIN message_bodies b ON b.message_id = m.id
                 WHERE m.folder_id = ?1 AND b.message_id IS NULL AND m.size <= ?2
                 ORDER BY m.date DESC LIMIT ?3",
            )
            .map_err(storage_err)?;
        stmt.query_map(params![folder, max_size, limit], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(storage_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_err)
    }

    /// Ungelesene je Ordner (für die Seitenleiste und das Dock-Symbol).
    pub fn unread_counts(&self) -> Result<Vec<(FolderId, u32)>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT folder_id, COUNT(*) FROM messages
                 WHERE seen = 0 AND pending_move = 0 GROUP BY folder_id",
            )
            .map_err(storage_err)?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)
    }

    /// Neue, ungelesene Nachrichten ab einer UID (für Mitteilungen), neueste zuerst.
    pub fn unseen_since(&self, folder: FolderId, uid: u32) -> Result<Vec<MessageSummary>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM messages m
                 WHERE m.folder_id = ?1 AND m.uid >= ?2 AND m.seen = 0 AND m.pending_move = 0
                 ORDER BY m.date DESC LIMIT 50"
            ))
            .map_err(storage_err)?;
        stmt.query_map(params![folder, uid], summary_from_row)
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)
    }

    // ---- Bilder-Whitelist -----------------------------------------------------

    pub fn is_image_sender_trusted(&self, address: &str) -> Result<bool> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT 1 FROM image_whitelist WHERE address = ?1",
            [address],
            |_| Ok(()),
        )
        .optional()
        .map(|r| r.is_some())
        .map_err(storage_err)
    }

    pub fn trust_image_sender(&self, address: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO image_whitelist (address) VALUES (?1)",
            [address],
        )
        .map_err(storage_err)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::NewAccount;
    use kolibri_core::{Attachment, FolderRole, RemoteFolder, Security, ServerConfig};

    fn setup() -> (Store, FolderId, FolderId) {
        let store = Store::open_in_memory().unwrap();
        let server = ServerConfig {
            host: "h".into(),
            port: 1,
            security: Security::Tls,
            username: "u".into(),
        };
        let a = store
            .insert_account(&NewAccount {
                email: "a@b.de".into(),
                display_name: "A".into(),
                imap: server.clone(),
                smtp: server,
            })
            .unwrap();
        let f = |path: &str, role| RemoteFolder {
            path: path.into(),
            delimiter: Some("/".into()),
            role,
            selectable: true,
        };
        store
            .replace_folders(a.id, &[f("INBOX", Some(FolderRole::Inbox)), f("Alt", None)])
            .unwrap();
        let folders = store.folders(a.id).unwrap();
        (store, folders[0].id, folders[1].id)
    }

    fn remote(uid: u32, date: i64, subject: &str) -> RemoteMessage {
        RemoteMessage {
            uid,
            message_id: Some(format!("m{uid}@x")),
            in_reply_to: None,
            references: None,
            subject: subject.into(),
            from: Address {
                name: "Jürgen \"JJ\" Müller".into(),
                addr: "j@x.de".into(),
            },
            to: vec![Address {
                name: "Zeile\nUmbruch".into(),
                addr: "t@x.de".into(),
            }],
            cc: vec![],
            reply_to: vec![],
            date,
            size: 100,
            flags: Flags::default(),
            has_attachments: false,
        }
    }

    #[test]
    fn insert_list_count_and_paging() {
        let (store, inbox, other) = setup();
        let msgs: Vec<_> = (1..=25)
            .map(|i| remote(i, i64::from(i) * 10, "s"))
            .collect();
        assert_eq!(store.insert_messages(inbox, &msgs).unwrap(), 25);
        // Doppelt einfügen ändert nichts.
        assert_eq!(store.insert_messages(inbox, &msgs[..5]).unwrap(), 0);
        store
            .insert_messages(other, &[remote(1, 999, "x")])
            .unwrap();

        let scope = MessageScope::Folder { id: inbox };
        assert_eq!(store.count_messages(scope).unwrap(), 25);
        let page = store.list_messages(scope, 10, 10).unwrap();
        assert_eq!(page.len(), 10);
        assert_eq!(page[0].date, 150); // neueste zuerst: 250, 240, … → Index 10 = 150
        let unified = MessageScope::Role {
            role: FolderRole::Inbox,
        };
        assert_eq!(store.count_messages(unified).unwrap(), 25);
    }

    #[test]
    fn flags_update_only_counts_changes() {
        let (store, inbox, _) = setup();
        store
            .insert_messages(inbox, &[remote(1, 1, "a"), remote(2, 2, "b")])
            .unwrap();
        let seen = Flags {
            seen: true,
            ..Flags::default()
        };
        assert_eq!(
            store
                .update_flags(inbox, &[(1, seen), (2, Flags::default())])
                .unwrap(),
            1
        );
        assert_eq!(store.update_flags(inbox, &[(1, seen)]).unwrap(), 0);
        let flagged = Flags {
            flagged: true,
            ..Flags::default()
        };
        store.update_flags(inbox, &[(2, flagged)]).unwrap();
        assert_eq!(store.count_messages(MessageScope::Flagged).unwrap(), 1);
    }

    #[test]
    fn delete_and_clear() {
        let (store, inbox, _) = setup();
        store
            .insert_messages(
                inbox,
                &[remote(1, 1, "a"), remote(2, 2, "b"), remote(3, 3, "c")],
            )
            .unwrap();
        assert_eq!(store.delete_uids(inbox, &[2, 99]).unwrap(), 1);
        assert_eq!(store.folder_uids(inbox).unwrap(), vec![1, 3]);
        store.clear_folder(inbox).unwrap();
        assert!(store.folder_uids(inbox).unwrap().is_empty());
    }

    #[test]
    fn location_roundtrips_addresses_with_special_chars() {
        let (store, inbox, _) = setup();
        store
            .insert_messages(inbox, &[remote(7, 1, "Betreff")])
            .unwrap();
        let id = store
            .list_messages(MessageScope::Folder { id: inbox }, 0, 1)
            .unwrap()[0]
            .id;
        let loc = store.message_location(id).unwrap();
        assert_eq!(loc.uid, 7);
        assert_eq!(loc.folder_path, "INBOX");
        assert_eq!(loc.summary.from_name, "Jürgen \"JJ\" Müller");
        assert_eq!(loc.to[0].name, "Zeile\nUmbruch");
        assert_eq!(loc.to[0].addr, "t@x.de");
    }

    #[test]
    fn body_roundtrip_updates_preview() {
        let (store, inbox, _) = setup();
        store.insert_messages(inbox, &[remote(1, 1, "a")]).unwrap();
        let id = store
            .list_messages(MessageScope::Folder { id: inbox }, 0, 1)
            .unwrap()[0]
            .id;
        assert_eq!(
            store.messages_without_body(inbox, 1000, 10).unwrap().len(),
            1
        );
        let body = Body {
            html: Some("<p>Hi</p>".into()),
            text: Some("Hi".into()),
            remote_images: true,
            attachments: vec![Attachment {
                name: "Rech\"nung.pdf".into(),
                mime: "application/pdf".into(),
                size: 1234,
            }],
            preview: "Hi".into(),
            invite: Some(kolibri_core::Invite {
                method: "REQUEST".into(),
                uid: "u".into(),
                summary: "Termin".into(),
                location: String::new(),
                start: 1,
                end: 2,
                all_day: false,
                organizer_name: String::new(),
                organizer_email: "o@x.de".into(),
                ics: "BEGIN:VCALENDAR".into(),
            }),
        };
        store.save_body(id, &body).unwrap();
        assert_eq!(store.body(id).unwrap().unwrap(), body);
        assert!(
            store
                .messages_without_body(inbox, 1000, 10)
                .unwrap()
                .is_empty()
        );
        let row = &store
            .list_messages(MessageScope::Folder { id: inbox }, 0, 1)
            .unwrap()[0];
        assert_eq!(row.preview, "Hi");
        assert!(row.has_attachments);
    }

    #[test]
    fn image_whitelist_is_case_insensitive() {
        let (store, _, _) = setup();
        assert!(!store.is_image_sender_trusted("News@Shop.example").unwrap());
        store.trust_image_sender("news@shop.example").unwrap();
        assert!(store.is_image_sender_trusted("News@Shop.example").unwrap());
    }
}
