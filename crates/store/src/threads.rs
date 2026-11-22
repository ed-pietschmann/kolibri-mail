//! Konversationen: Nachrichten über References/In-Reply-To zu Gesprächen
//! zusammenfassen – ordnerübergreifend (Posteingang + Gesendet + Archiv …).
//!
//! Jede Nachricht bekommt `thread_root` = Message-ID der ältesten bekannten
//! Nachricht des Gesprächs. Vereinfachter JWZ-Algorithmus: ausreichend für
//! normale Antwortketten, ohne Betreff-Raten (das führt oft zu Fehlgruppen).

use std::collections::HashSet;

use kolibri_core::{AccountId, MessageId, MessageScope, MessageSummary, Result};
use rusqlite::{OptionalExtension, Transaction, params, params_from_iter};

use crate::messages::{SUMMARY_COLUMNS, SUMMARY_LEN, scope_filter, summary_from_row};
use crate::{Store, storage_err};

/// Message-IDs ohne spitze Klammern, in Reihenfolge, ohne Doppelte.
pub(crate) fn split_ids(list: Option<&str>) -> Vec<String> {
    let mut seen = HashSet::new();
    list.unwrap_or_default()
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_owned())
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .collect()
}

pub(crate) fn clean_id(id: Option<&str>) -> Option<String> {
    id.map(|s| s.trim().trim_matches(|c| c == '<' || c == '>').to_owned())
        .filter(|s| !s.is_empty())
}

/// Bestimmt das Gespräch einer neuen Nachricht und vereint ggf. Gespräche,
/// die bisher getrennt waren (Antwort kam vor der Ursprungsnachricht an).
pub(crate) fn assign_root(
    tx: &Transaction<'_>,
    account: AccountId,
    message_id: Option<&str>,
    references: Option<&str>,
    in_reply_to: Option<&str>,
) -> rusqlite::Result<Option<String>> {
    let own = clean_id(message_id);
    let mut chain = split_ids(references);
    for id in split_ids(in_reply_to) {
        if !chain.contains(&id) {
            chain.push(id);
        }
    }

    let mut lookup = tx.prepare_cached(
        "SELECT thread_root FROM messages
         WHERE account_id = ?1 AND message_id = ?2 AND thread_root IS NOT NULL LIMIT 1",
    )?;
    // Bekannt: dieselbe Nachricht in einem anderen Ordner oder ein Vorgänger.
    let known = own
        .iter()
        .chain(chain.iter())
        .find_map(|id| {
            lookup
                .query_row(params![account, id], |r| r.get::<_, String>(0))
                .optional()
                .transpose()
        })
        .transpose()?;
    let root = known
        .or_else(|| chain.first().cloned())
        .or_else(|| own.clone());

    // Gespräche zusammenführen, die bisher an einem Zwischenglied hingen.
    if let Some(root) = &root {
        let mut merge = tx.prepare_cached(
            "UPDATE messages SET thread_root = ?1 WHERE account_id = ?2 AND thread_root = ?3",
        )?;
        for id in own.iter().chain(chain.iter()) {
            if id != root {
                merge.execute(params![root, account, id])?;
            }
        }
    }
    Ok(root)
}

/// (id, Konto, Message-ID, References, In-Reply-To)
type BackfillRow = (
    MessageId,
    AccountId,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Ausdruck für die Gesprächs-Gruppierung (Nachrichten ohne Message-ID sind allein).
const THREAD_KEY: &str = "COALESCE(m.thread_root, 'id:' || m.id)";

impl Store {
    /// Füllt `thread_root` für Nachrichten aus älteren Datenbankständen.
    pub fn backfill_threads(&self) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        let rows: Vec<BackfillRow> = {
            let mut stmt = tx
                .prepare(
                    "SELECT id, account_id, message_id, refs, in_reply_to FROM messages
                     WHERE thread_root IS NULL AND message_id IS NOT NULL ORDER BY date, id",
                )
                .map_err(storage_err)?;
            stmt.query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map_err(storage_err)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage_err)?
        };
        for (id, account, mid, refs, irt) in &rows {
            let root = assign_root(
                &tx,
                *account,
                mid.as_deref(),
                refs.as_deref(),
                irt.as_deref(),
            )
            .map_err(storage_err)?;
            tx.execute(
                "UPDATE messages SET thread_root = ?2 WHERE id = ?1",
                params![id, root],
            )
            .map_err(storage_err)?;
        }
        tx.commit().map_err(storage_err)?;
        Ok(rows.len())
    }

    /// Anzahl Gespräche im Bereich.
    pub fn count_threads(&self, scope: MessageScope) -> Result<u32> {
        let (filter, args) = scope_filter(scope);
        let conn = self.conn()?;
        conn.query_row(
            &format!("SELECT COUNT(DISTINCT m.account_id || ':' || {THREAD_KEY}) FROM messages m WHERE {filter}"),
            params_from_iter(args),
            |r| r.get(0),
        )
        .map_err(storage_err)
    }

    /// Eine Zeile pro Gespräch (neueste Nachricht im Bereich), neueste zuerst.
    /// `seen` = alle gelesen, `flagged` = eine markiert, `thread_count` = Gesprächslänge
    /// über alle Ordner (ohne Papierkorb/Spam).
    pub fn list_threads(
        &self,
        scope: MessageScope,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<MessageSummary>> {
        let (filter, mut args) = scope_filter(scope);
        args.push(i64::from(limit));
        args.push(i64::from(offset));
        let sql = format!(
            "WITH s AS (
                SELECT m.*,
                    ROW_NUMBER() OVER w AS rn,
                    MIN(m.seen) OVER p AS all_seen,
                    MAX(m.flagged) OVER p AS any_flagged,
                    MAX(m.has_attachments) OVER p AS any_attachment
                FROM messages m WHERE {filter}
                WINDOW p AS (PARTITION BY m.account_id, {THREAD_KEY}),
                       w AS (PARTITION BY m.account_id, {THREAD_KEY} ORDER BY m.date DESC, m.id DESC)
             )
             SELECT {SUMMARY_COLUMNS}, m.all_seen, m.any_flagged, m.any_attachment,
                CASE WHEN m.thread_root IS NULL THEN 1 ELSE MAX(1, (
                    SELECT COUNT(DISTINCT COALESCE(t.message_id, t.id)) FROM messages t
                    WHERE t.account_id = m.account_id AND t.thread_root = m.thread_root
                      AND t.pending_move = 0
                      AND t.folder_id NOT IN (SELECT id FROM folders WHERE role IN ('trash', 'junk'))
                )) END
             FROM s m WHERE m.rn = 1
             ORDER BY m.date DESC, m.id DESC LIMIT ? OFFSET ?"
        );
        let conn = self.conn()?;
        let mut stmt = conn.prepare_cached(&sql).map_err(storage_err)?;
        stmt.query_map(params_from_iter(args), |r| {
            let mut m = summary_from_row(r)?;
            m.seen = r.get(SUMMARY_LEN)?;
            m.flagged = r.get(SUMMARY_LEN + 1)?;
            m.has_attachments = r.get(SUMMARY_LEN + 2)?;
            m.thread_count = r.get(SUMMARY_LEN + 3)?;
            Ok(m)
        })
        .map_err(storage_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_err)
    }

    /// Alle Nachrichten des Gesprächs, älteste zuerst. Dieselbe Nachricht in
    /// mehreren Ordnern (z. B. Gmail „Alle Nachrichten“) erscheint nur einmal –
    /// bevorzugt die angeklickte. Papierkorb/Spam nur, wenn man dort ist.
    pub fn thread_members(&self, id: MessageId) -> Result<Vec<MessageSummary>> {
        let conn = self.conn()?;
        let anchor: Option<(AccountId, Option<String>, bool)> = conn
            .query_row(
                "SELECT m.account_id, m.thread_root,
                    m.folder_id IN (SELECT id FROM folders WHERE role IN ('trash', 'junk'))
                 FROM messages m WHERE m.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(storage_err)?;
        let Some((account, Some(root), in_bin)) = anchor else {
            drop(conn);
            return Ok(self
                .message_location(id)
                .map(|l| vec![l.summary])
                .unwrap_or_default());
        };
        let bin_filter = if in_bin {
            ""
        } else {
            "AND m.folder_id NOT IN (SELECT id FROM folders WHERE role IN ('trash', 'junk'))"
        };
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {SUMMARY_COLUMNS}, m.message_id FROM messages m
                 WHERE m.account_id = ?1 AND m.thread_root = ?2 AND m.pending_move = 0 {bin_filter}
                 ORDER BY m.date, m.id"
            ))
            .map_err(storage_err)?;
        let rows = stmt
            .query_map(params![account, root], |r| {
                Ok((
                    summary_from_row(r)?,
                    r.get::<_, Option<String>>(SUMMARY_LEN)?,
                ))
            })
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)?;

        // Doppelte (gleiche Message-ID) zusammenfassen.
        let mut out: Vec<(MessageSummary, Option<String>)> = Vec::with_capacity(rows.len());
        for (m, mid) in rows {
            if let Some(existing) = mid
                .as_ref()
                .and_then(|mid| out.iter_mut().find(|(_, o)| o.as_ref() == Some(mid)))
            {
                if m.id == id {
                    existing.0 = m;
                }
            } else {
                out.push((m, mid));
            }
        }
        let count = u32::try_from(out.len()).unwrap_or(u32::MAX);
        Ok(out
            .into_iter()
            .map(|(mut m, _)| {
                m.thread_count = count;
                m
            })
            .collect())
    }

    /// IDs der Gesprächs-Nachrichten, die im Bereich liegen (für Aktionen auf
    /// ein ganzes Gespräch, z. B. Archivieren aus dem Posteingang).
    pub fn thread_ids_in_scope(
        &self,
        id: MessageId,
        scope: MessageScope,
    ) -> Result<Vec<MessageId>> {
        let (filter, mut args) = scope_filter(scope);
        args.push(id);
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT m.id FROM messages m, messages a
                 WHERE {filter} AND a.id = ?
                   AND m.account_id = a.account_id
                   AND (m.id = a.id OR (a.thread_root IS NOT NULL AND m.thread_root = a.thread_root))
                 ORDER BY m.date"
            ))
            .map_err(storage_err)?;
        stmt.query_map(params_from_iter(args), |r| r.get(0))
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::NewAccount;
    use kolibri_core::{
        Address, Flags, FolderId, FolderRole, RemoteFolder, RemoteMessage, Security, ServerConfig,
    };

    struct T {
        store: Store,
        inbox: FolderId,
        sent: FolderId,
        trash: FolderId,
    }

    fn setup() -> T {
        let store = Store::open_in_memory().unwrap();
        let server = ServerConfig {
            host: "h".into(),
            port: 1,
            security: Security::Tls,
            username: "u".into(),
        };
        let a = store
            .insert_account(&NewAccount {
                email: "ich@x.de".into(),
                display_name: "Ich".into(),
                imap: server.clone(),
                smtp: server,
            })
            .unwrap();
        let f = |path: &str, role| RemoteFolder {
            path: path.into(),
            delimiter: Some("/".into()),
            role: Some(role),
            selectable: true,
        };
        store
            .replace_folders(
                a.id,
                &[
                    f("INBOX", FolderRole::Inbox),
                    f("Sent", FolderRole::Sent),
                    f("Trash", FolderRole::Trash),
                ],
            )
            .unwrap();
        let id = |r| store.folder_by_role(a.id, r).unwrap().unwrap().id;
        T {
            inbox: id(FolderRole::Inbox),
            sent: id(FolderRole::Sent),
            trash: id(FolderRole::Trash),
            store,
        }
    }

    fn msg(
        uid: u32,
        mid: &str,
        refs: Option<&str>,
        irt: Option<&str>,
        date: i64,
        from: &str,
    ) -> RemoteMessage {
        RemoteMessage {
            uid,
            message_id: Some(mid.into()),
            in_reply_to: irt.map(Into::into),
            references: refs.map(Into::into),
            subject: format!("Betreff {uid}"),
            from: Address {
                name: String::new(),
                addr: from.into(),
            },
            to: vec![Address {
                name: "Anna".into(),
                addr: "anna@x.de".into(),
            }],
            cc: vec![],
            reply_to: vec![],
            date,
            size: 1,
            flags: Flags {
                seen: true,
                ..Flags::default()
            },
            has_attachments: false,
        }
    }

    #[test]
    fn groups_across_folders_and_counts() {
        let t = setup();
        t.store
            .insert_messages(t.inbox, &[msg(1, "a@x", None, None, 10, "anna@x.de")])
            .unwrap();
        t.store
            .insert_messages(
                t.sent,
                &[msg(1, "b@x", Some("<a@x>"), Some("<a@x>"), 20, "ich@x.de")],
            )
            .unwrap();
        t.store
            .insert_messages(
                t.inbox,
                &[msg(
                    2,
                    "c@x",
                    Some("<a@x> <b@x>"),
                    Some("<b@x>"),
                    30,
                    "anna@x.de",
                )],
            )
            .unwrap();
        t.store
            .insert_messages(t.inbox, &[msg(3, "z@x", None, None, 5, "max@x.de")])
            .unwrap();

        let inbox = MessageScope::Folder { id: t.inbox };
        assert_eq!(t.store.count_threads(inbox).unwrap(), 2);
        let rows = t.store.list_threads(inbox, 0, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].date, 30, "neueste Nachricht vertritt das Gespräch");
        assert_eq!(
            rows[0].thread_count, 3,
            "inkl. eigener Antwort aus Gesendet"
        );
        assert_eq!(rows[1].thread_count, 1);

        let members = t.store.thread_members(rows[0].id).unwrap();
        let dates: Vec<_> = members.iter().map(|m| m.date).collect();
        assert_eq!(dates, [10, 20, 30]);
        assert!(members[1].outgoing);
        assert_eq!(members[1].to_display, "Anna");
        assert_eq!(members[1].to_count, 1);

        let ids = t.store.thread_ids_in_scope(rows[0].id, inbox).unwrap();
        assert_eq!(ids.len(), 2, "nur die Gesprächs-Nachrichten im Posteingang");
    }

    #[test]
    fn child_before_parent_is_merged() {
        let t = setup();
        // Antwort C (kennt nur ihren direkten Vorgänger B) kommt zuerst …
        t.store
            .insert_messages(
                t.inbox,
                &[msg(1, "c@x", None, Some("<b@x>"), 30, "anna@x.de")],
            )
            .unwrap();
        // … dann B (Antwort auf A), dann A.
        t.store
            .insert_messages(
                t.sent,
                &[msg(1, "b@x", Some("<a@x>"), Some("<a@x>"), 20, "ich@x.de")],
            )
            .unwrap();
        t.store
            .insert_messages(t.inbox, &[msg(2, "a@x", None, None, 10, "anna@x.de")])
            .unwrap();
        let rows = t
            .store
            .list_threads(MessageScope::Folder { id: t.inbox }, 0, 10)
            .unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].thread_count, 3);
    }

    #[test]
    fn trash_is_excluded_and_duplicates_collapse() {
        let t = setup();
        t.store
            .insert_messages(t.inbox, &[msg(1, "a@x", None, None, 10, "anna@x.de")])
            .unwrap();
        t.store
            .insert_messages(
                t.trash,
                &[msg(1, "b@x", Some("<a@x>"), None, 20, "anna@x.de")],
            )
            .unwrap();
        // Dieselbe Nachricht noch einmal in Gesendet (z. B. Gmail-Label).
        t.store
            .insert_messages(t.sent, &[msg(2, "a@x", None, None, 10, "anna@x.de")])
            .unwrap();
        let id = t
            .store
            .list_threads(MessageScope::Folder { id: t.inbox }, 0, 10)
            .unwrap()[0]
            .id;
        let members = t.store.thread_members(id).unwrap();
        assert_eq!(members.len(), 1, "{members:?}");
        assert_eq!(members[0].id, id, "angeklickte Kopie bevorzugt");
    }

    #[test]
    fn unread_and_flags_aggregate() {
        let t = setup();
        let mut a = msg(1, "a@x", None, None, 10, "anna@x.de");
        a.flags = Flags {
            seen: false,
            flagged: true,
            ..Flags::default()
        };
        t.store
            .insert_messages(
                t.inbox,
                &[a, msg(2, "b@x", Some("<a@x>"), None, 20, "anna@x.de")],
            )
            .unwrap();
        let row = &t
            .store
            .list_threads(MessageScope::Folder { id: t.inbox }, 0, 10)
            .unwrap()[0];
        assert_eq!(row.date, 20);
        assert!(!row.seen, "ungelesen, wenn eine ungelesen ist");
        assert!(row.flagged);
    }

    #[test]
    fn backfill_assigns_roots() {
        let t = setup();
        t.store
            .insert_messages(
                t.inbox,
                &[
                    msg(1, "a@x", None, None, 10, "x@x"),
                    msg(2, "b@x", Some("<a@x>"), None, 20, "x@x"),
                ],
            )
            .unwrap();
        t.store
            .conn()
            .unwrap()
            .execute("UPDATE messages SET thread_root = NULL", [])
            .unwrap();
        assert_eq!(t.store.backfill_threads().unwrap(), 2);
        let rows = t
            .store
            .list_threads(MessageScope::Folder { id: t.inbox }, 0, 10)
            .unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn id_helpers() {
        assert_eq!(split_ids(Some("<a@x> <b@x>\r\n <a@x>")), ["a@x", "b@x"]);
        assert_eq!(clean_id(Some(" <a@x> ")), Some("a@x".into()));
        assert_eq!(clean_id(Some("<>")), None);
    }
}
