//! Offline-Warteschlange: Aktionen lokal anwenden, einreihen, abarbeiten.

use std::collections::HashSet;

use kolibri_core::{
    AccountId, Action, Error, ErrorKind, Folder, FolderId, FolderRole, MessageId, PendingOp, Result,
};
use rusqlite::{OptionalExtension, params, params_from_iter};

use crate::{Store, storage_err};

/// Wo eine Nachricht liegt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageRef {
    pub id: MessageId,
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub uid: u32,
}

/// Stand der Warteschlange eines Kontos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueCounts {
    pub pending: u32,
    pub failed: u32,
}

fn apply_local(
    tx: &rusqlite::Transaction<'_>,
    folder: FolderId,
    uids: &[u32],
    action: &Action,
    undo: bool,
) -> rusqlite::Result<()> {
    let (column, value) = match action {
        Action::SetSeen { value } => ("seen", i64::from(*value != undo)),
        Action::SetFlagged { value } => ("flagged", i64::from(*value != undo)),
        Action::SetAnswered { value } => ("answered", i64::from(*value != undo)),
        Action::Move { .. } | Action::Delete => ("pending_move", i64::from(!undo)),
    };
    let mut stmt = tx.prepare_cached(&format!(
        "UPDATE messages SET {column} = ?1 WHERE folder_id = ?2 AND uid = ?3"
    ))?;
    for uid in uids {
        stmt.execute(params![value, folder, uid])?;
    }
    Ok(())
}

impl Store {
    /// Ort der Nachrichten (unbekannte IDs werden übersprungen).
    pub fn message_refs(&self, ids: &[MessageId]) -> Result<Vec<MessageRef>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = vec!["?"; ids.len()].join(",");
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id, account_id, folder_id, uid FROM messages WHERE id IN ({placeholders})"
            ))
            .map_err(storage_err)?;
        stmt.query_map(params_from_iter(ids), |r| {
            Ok(MessageRef {
                id: r.get(0)?,
                account_id: r.get(1)?,
                folder_id: r.get(2)?,
                uid: r.get(3)?,
            })
        })
        .map_err(storage_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_err)
    }

    pub fn folder(&self, id: FolderId) -> Result<Folder> {
        let account = {
            let conn = self.conn()?;
            conn.query_row("SELECT account_id FROM folders WHERE id = ?1", [id], |r| {
                r.get::<_, AccountId>(0)
            })
            .optional()
            .map_err(storage_err)?
        };
        let account =
            account.ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Ordner {id}")))?;
        self.folders(account)?
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Ordner {id}")))
    }

    pub fn folder_by_role(&self, account: AccountId, role: FolderRole) -> Result<Option<Folder>> {
        Ok(self
            .folders(account)?
            .into_iter()
            .find(|f| f.role == Some(role) && f.selectable))
    }

    /// Wendet eine Aktion sofort lokal an und reiht sie für den Server ein.
    pub fn enqueue(
        &self,
        account: AccountId,
        folder: FolderId,
        uids: &[u32],
        action: &Action,
    ) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let action_json = serde_json::to_string(action)
            .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        let uids_json = serde_json::to_string(uids)
            .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        apply_local(&tx, folder, uids, action, false).map_err(storage_err)?;
        tx.execute(
            "INSERT INTO pending_ops (account_id, folder_id, uids, action) VALUES (?1, ?2, ?3, ?4)",
            params![account, folder, uids_json, action_json],
        )
        .map_err(storage_err)?;
        tx.commit().map_err(storage_err)
    }

    /// Älteste noch offene Operation eines Kontos.
    pub fn next_op(&self, account: AccountId) -> Result<Option<PendingOp>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT o.id, o.folder_id, f.path, o.uids, o.action, o.attempts
                 FROM pending_ops o JOIN folders f ON f.id = o.folder_id
                 WHERE o.account_id = ?1 AND o.failed = 0 ORDER BY o.id LIMIT 1",
                [account],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, FolderId>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, u32>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_err)?;
        let Some((id, folder_id, folder_path, uids, action, attempts)) = row else {
            return Ok(None);
        };
        let parse_err = |e: serde_json::Error| Error::new(ErrorKind::Storage, e.to_string());
        Ok(Some(PendingOp {
            id,
            account_id: account,
            folder_id,
            folder_path,
            uids: serde_json::from_str(&uids).map_err(parse_err)?,
            action: serde_json::from_str(&action).map_err(parse_err)?,
            attempts,
        }))
    }

    /// Server hat bestätigt: Operation entfernen, verschobene Zeilen löschen.
    pub fn complete_op(&self, op: &PendingOp) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        tx.execute("DELETE FROM pending_ops WHERE id = ?1", [op.id])
            .map_err(storage_err)?;
        if op.action.hides() {
            let mut stmt = tx
                .prepare_cached(
                    "DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2 AND pending_move = 1",
                )
                .map_err(storage_err)?;
            for uid in &op.uids {
                stmt.execute(params![op.folder_id, uid])
                    .map_err(storage_err)?;
            }
        }
        tx.commit().map_err(storage_err)
    }

    /// Vorübergehender Fehler (z. B. offline): später erneut versuchen.
    pub fn retry_op(&self, op: &PendingOp, error: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE pending_ops SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
            params![op.id, error],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    /// Dauerhafter Fehler (Server lehnt ab): aufgeben und lokal zurücknehmen.
    pub fn fail_op(&self, op: &PendingOp, error: &str) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        apply_local(&tx, op.folder_id, &op.uids, &op.action, true).map_err(storage_err)?;
        tx.execute(
            "UPDATE pending_ops SET failed = 1, last_error = ?2 WHERE id = ?1",
            params![op.id, error],
        )
        .map_err(storage_err)?;
        tx.commit().map_err(storage_err)
    }

    pub fn queue_counts(&self, account: AccountId) -> Result<QueueCounts> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT COALESCE(SUM(failed = 0), 0), COALESCE(SUM(failed = 1), 0) FROM (
                 SELECT failed FROM pending_ops WHERE account_id = ?1
                 UNION ALL SELECT failed FROM outbox WHERE account_id = ?1)",
            [account],
            |r| {
                Ok(QueueCounts {
                    pending: r.get(0)?,
                    failed: r.get(1)?,
                })
            },
        )
        .map_err(storage_err)
    }

    /// Fehlgeschlagene Operationen verwerfen (nach Anzeige in der Oberfläche).
    pub fn clear_failed_ops(&self, account: AccountId) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM pending_ops WHERE account_id = ?1 AND failed = 1",
            [account],
        )
        .map_err(storage_err)?;
        conn.execute(
            "DELETE FROM outbox WHERE account_id = ?1 AND failed = 1",
            [account],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    /// UIDs mit offenen Operationen – deren Flags darf der Sync nicht überschreiben.
    pub fn pending_uids(&self, folder: FolderId) -> Result<HashSet<u32>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached("SELECT uids FROM pending_ops WHERE folder_id = ?1 AND failed = 0")
            .map_err(storage_err)?;
        let lists = stmt
            .query_map([folder], |r| r.get::<_, String>(0))
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)?;
        Ok(lists
            .iter()
            .flat_map(|l| serde_json::from_str::<Vec<u32>>(l).unwrap_or_default())
            .collect())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::NewAccount;
    use kolibri_core::{
        Address, Flags, MessageScope, RemoteFolder, RemoteMessage, Security, ServerConfig,
    };

    fn setup() -> (Store, AccountId, FolderId) {
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
        store
            .replace_folders(
                a.id,
                &[RemoteFolder {
                    path: "INBOX".into(),
                    delimiter: Some("/".into()),
                    role: Some(FolderRole::Inbox),
                    selectable: true,
                }],
            )
            .unwrap();
        let inbox = store.folders(a.id).unwrap()[0].id;
        let msgs: Vec<_> = (1..=3)
            .map(|uid| RemoteMessage {
                uid,
                message_id: None,
                in_reply_to: None,
                references: None,
                subject: format!("m{uid}"),
                from: Address {
                    name: String::new(),
                    addr: "x@y.de".into(),
                },
                to: vec![],
                cc: vec![],
                reply_to: vec![],
                date: i64::from(uid),
                size: 1,
                flags: Flags::default(),
                has_attachments: false,
            })
            .collect();
        store.insert_messages(inbox, &msgs).unwrap();
        (store, a.id, inbox)
    }

    fn list(store: &Store, inbox: FolderId) -> Vec<kolibri_core::MessageSummary> {
        store
            .list_messages(MessageScope::Folder { id: inbox }, 0, 10)
            .unwrap()
    }

    #[test]
    fn flags_apply_immediately_and_queue_in_order() {
        let (store, account, inbox) = setup();
        store
            .enqueue(account, inbox, &[1, 2], &Action::SetSeen { value: true })
            .unwrap();
        store
            .enqueue(account, inbox, &[3], &Action::SetFlagged { value: true })
            .unwrap();
        let rows = list(&store, inbox);
        assert!(rows.iter().filter(|m| m.seen).count() == 2);
        assert_eq!(store.queue_counts(account).unwrap().pending, 2);

        let first = store.next_op(account).unwrap().unwrap();
        assert_eq!(first.action, Action::SetSeen { value: true });
        assert_eq!(first.uids, vec![1, 2]);
        assert_eq!(first.folder_path, "INBOX");
        store.complete_op(&first).unwrap();
        let second = store.next_op(account).unwrap().unwrap();
        assert_eq!(second.action, Action::SetFlagged { value: true });
        assert_eq!(store.pending_uids(inbox).unwrap(), HashSet::from([3]));
    }

    #[test]
    fn move_hides_until_confirmed() {
        let (store, account, inbox) = setup();
        let action = Action::Move {
            to: "Archive".into(),
            create: true,
        };
        store.enqueue(account, inbox, &[2], &action).unwrap();
        assert_eq!(
            list(&store, inbox).len(),
            2,
            "verschobene Mail ausgeblendet"
        );
        // Sync darf sie nicht erneut einfügen: UID ist lokal noch bekannt.
        assert_eq!(store.folder_uids(inbox).unwrap(), vec![1, 2, 3]);
        let op = store.next_op(account).unwrap().unwrap();
        store.complete_op(&op).unwrap();
        assert_eq!(store.folder_uids(inbox).unwrap(), vec![1, 3]);
        assert_eq!(store.queue_counts(account).unwrap(), QueueCounts::default());
    }

    #[test]
    fn failed_op_is_rolled_back() {
        let (store, account, inbox) = setup();
        store
            .enqueue(account, inbox, &[1], &Action::Delete)
            .unwrap();
        store
            .enqueue(account, inbox, &[2], &Action::SetSeen { value: true })
            .unwrap();
        let op = store.next_op(account).unwrap().unwrap();
        store.fail_op(&op, "NO Zugriff verweigert").unwrap();
        assert_eq!(list(&store, inbox).len(), 3, "gelöschte Mail ist wieder da");
        let op = store.next_op(account).unwrap().unwrap();
        store.fail_op(&op, "NO").unwrap();
        assert!(
            list(&store, inbox).iter().all(|m| !m.seen),
            "gelesen zurückgenommen"
        );
        assert_eq!(
            store.queue_counts(account).unwrap(),
            QueueCounts {
                pending: 0,
                failed: 2
            }
        );
        store.clear_failed_ops(account).unwrap();
        assert_eq!(store.queue_counts(account).unwrap(), QueueCounts::default());
    }

    #[test]
    fn retry_counts_attempts() {
        let (store, account, inbox) = setup();
        store
            .enqueue(account, inbox, &[1], &Action::Delete)
            .unwrap();
        let op = store.next_op(account).unwrap().unwrap();
        store.retry_op(&op, "offline").unwrap();
        assert_eq!(store.next_op(account).unwrap().unwrap().attempts, 1);
    }

    #[test]
    fn message_refs_resolve_location() {
        let (store, account, inbox) = setup();
        let ids: Vec<_> = list(&store, inbox).iter().map(|m| m.id).collect();
        let refs = store.message_refs(&ids).unwrap();
        assert_eq!(refs.len(), 3);
        assert!(
            refs.iter()
                .all(|r| r.account_id == account && r.folder_id == inbox)
        );
        assert!(
            store
                .folder_by_role(account, FolderRole::Inbox)
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .folder_by_role(account, FolderRole::Archive)
                .unwrap()
                .is_none()
        );
    }
}
