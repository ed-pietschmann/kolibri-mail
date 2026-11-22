//! Postausgang: gesendete Nachrichten und Entwürfe, bis der Server sie hat.

use kolibri_core::{AccountId, Error, ErrorKind, MessageId, Result};
use rusqlite::{OptionalExtension, params};

use crate::{Store, storage_err};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxKind {
    /// Per SMTP senden.
    Send,
    /// Gesendet – Kopie in „Gesendet“ ablegen, Entwurf löschen.
    Sent,
    /// Entwurf (gleiche Message-ID) auf dem Server ersetzen.
    Draft,
    /// Entwurf mit dieser Message-ID löschen.
    Discard,
}

impl OutboxKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Sent => "sent",
            Self::Draft => "draft",
            Self::Discard => "discard",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "send" => Self::Send,
            "sent" => Self::Sent,
            "draft" => Self::Draft,
            "discard" => Self::Discard,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxItem {
    pub id: i64,
    pub account_id: AccountId,
    pub kind: OutboxKind,
    pub message_id: String,
    pub raw: Vec<u8>,
    pub envelope_from: String,
    pub envelope_to: Vec<String>,
    pub answered_id: Option<MessageId>,
    pub subject: String,
    pub attempts: u32,
}

impl Store {
    /// Neuer Eintrag. Ältere, noch nicht übertragene Entwurfsfassungen derselben
    /// Nachricht werden dabei verworfen – nur die neueste zählt.
    #[allow(clippy::too_many_arguments)]
    pub fn outbox_push(
        &self,
        account: AccountId,
        kind: OutboxKind,
        message_id: &str,
        raw: &[u8],
        envelope_from: &str,
        envelope_to: &[String],
        answered_id: Option<MessageId>,
        subject: &str,
    ) -> Result<i64> {
        let to = serde_json::to_string(envelope_to)
            .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        tx.execute(
            "DELETE FROM outbox WHERE account_id = ?1 AND message_id = ?2
               AND kind = 'draft' AND failed = 0",
            params![account, message_id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "INSERT INTO outbox (account_id, kind, message_id, raw, envelope_from, envelope_to,
                answered_id, subject) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                account,
                kind.as_str(),
                message_id,
                raw,
                envelope_from,
                to,
                answered_id,
                subject
            ],
        )
        .map_err(storage_err)?;
        let id = tx.last_insert_rowid();
        tx.commit().map_err(storage_err)?;
        Ok(id)
    }

    pub fn outbox_next(&self, account: AccountId) -> Result<Option<OutboxItem>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT id, kind, message_id, raw, envelope_from, envelope_to, answered_id,
                        subject, attempts
                 FROM outbox WHERE account_id = ?1 AND failed = 0 ORDER BY id LIMIT 1",
                [account],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<Vec<u8>>>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<MessageId>>(6)?,
                        r.get::<_, String>(7)?,
                        r.get::<_, u32>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_err)?;
        let Some((id, kind, message_id, raw, from, to, answered_id, subject, attempts)) = row
        else {
            return Ok(None);
        };
        Ok(Some(OutboxItem {
            id,
            account_id: account,
            kind: OutboxKind::parse(&kind)
                .ok_or_else(|| Error::new(ErrorKind::Storage, format!("Unbekannte Art {kind}")))?,
            message_id,
            raw: raw.unwrap_or_default(),
            envelope_from: from,
            envelope_to: serde_json::from_str(&to).unwrap_or_default(),
            answered_id,
            subject,
            attempts,
        }))
    }

    /// Nächster Schritt eines Eintrags (z. B. Send → Sent nach erfolgreichem SMTP).
    pub fn outbox_set_kind(&self, id: i64, kind: OutboxKind) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE outbox SET kind = ?2, attempts = 0, last_error = NULL WHERE id = ?1",
            params![id, kind.as_str()],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    pub fn outbox_done(&self, id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM outbox WHERE id = ?1", [id])
            .map_err(storage_err)?;
        Ok(())
    }

    pub fn outbox_retry(&self, id: i64, error: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE outbox SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
            params![id, error],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    pub fn outbox_fail(&self, id: i64, error: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE outbox SET failed = 1, last_error = ?2 WHERE id = ?1",
            params![id, error],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    /// Wartet noch etwas auf den Versand? (Für den Hinweis „wird gesendet“.)
    pub fn outbox_sending(&self, account: AccountId) -> Result<u32> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE account_id = ?1 AND kind = 'send' AND failed = 0",
            [account],
            |r| r.get(0),
        )
        .map_err(storage_err)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::NewAccount;
    use kolibri_core::{Security, ServerConfig};

    fn setup() -> (Store, AccountId) {
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
        (store, a.id)
    }

    #[test]
    fn send_flow_and_counts() {
        let (store, a) = setup();
        let to = vec!["x@y.de".to_owned()];
        store
            .outbox_push(
                a,
                OutboxKind::Send,
                "m1@k",
                b"raw",
                "a@b.de",
                &to,
                Some(7),
                "Hallo",
            )
            .unwrap();
        assert_eq!(store.outbox_sending(a).unwrap(), 1);
        assert_eq!(store.queue_counts(a).unwrap().pending, 1);
        let item = store.outbox_next(a).unwrap().unwrap();
        assert_eq!(item.kind, OutboxKind::Send);
        assert_eq!(item.envelope_to, to);
        assert_eq!(item.answered_id, Some(7));
        store.outbox_set_kind(item.id, OutboxKind::Sent).unwrap();
        assert_eq!(store.outbox_sending(a).unwrap(), 0);
        assert_eq!(
            store.outbox_next(a).unwrap().unwrap().kind,
            OutboxKind::Sent
        );
        store.outbox_done(item.id).unwrap();
        assert!(store.outbox_next(a).unwrap().is_none());
    }

    #[test]
    fn newer_draft_replaces_older_unsent_version() {
        let (store, a) = setup();
        store
            .outbox_push(a, OutboxKind::Draft, "d1@k", b"v1", "", &[], None, "")
            .unwrap();
        store
            .outbox_push(a, OutboxKind::Draft, "d1@k", b"v2", "", &[], None, "")
            .unwrap();
        let item = store.outbox_next(a).unwrap().unwrap();
        assert_eq!(item.raw, b"v2");
        store.outbox_done(item.id).unwrap();
        assert!(store.outbox_next(a).unwrap().is_none());
    }

    #[test]
    fn failed_items_are_skipped_and_counted() {
        let (store, a) = setup();
        let id = store
            .outbox_push(a, OutboxKind::Send, "m@k", b"r", "", &[], None, "")
            .unwrap();
        store.outbox_fail(id, "550").unwrap();
        assert!(store.outbox_next(a).unwrap().is_none());
        assert_eq!(store.queue_counts(a).unwrap().failed, 1);
        store.clear_failed_ops(a).unwrap();
        assert_eq!(store.queue_counts(a).unwrap().failed, 0);
    }
}
