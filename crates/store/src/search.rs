//! Volltextsuche über den FTS5-Index mit einfachen Filtern.
//!
//! Eingabe wie in Apple Mail/Gmail, auf Deutsch und Englisch:
//!   rechnung von:stadtwerke hat:anhang nach:2026-01-01
//!   "genaue phrase" an:anna ist:ungelesen
//! Freie Wörter werden als Präfix gesucht („rech“ findet „Rechnung“),
//! Umlaute/Akzente sind egal („muller“ findet „Müller“).

use kolibri_core::{FolderId, MessageSummary, Result};
use rusqlite::params_from_iter;
use rusqlite::types::Value;

use crate::messages::{SUMMARY_COLUMNS, summary_from_row};
use crate::{Store, storage_err};

/// Zerlegte Suchanfrage.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Query {
    /// Freie Begriffe (Wörter oder Phrasen).
    pub terms: Vec<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub subject: Vec<String>,
    pub has_attachment: bool,
    pub unread: bool,
    pub flagged: bool,
    /// Unix-Sekunden (inklusive).
    pub after: Option<i64>,
    /// Unix-Sekunden (exklusive).
    pub before: Option<i64>,
}

impl Query {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// „2026-01-31“ oder „31.01.2026“ → Unix-Sekunden (Mitternacht UTC).
fn parse_date(s: &str) -> Option<i64> {
    let (y, m, d) = if let Some((d, rest)) = s.split_once('.') {
        let (m, y) = rest.split_once('.')?;
        (
            y.parse::<i64>().ok()?,
            m.parse::<i64>().ok()?,
            d.parse::<i64>().ok()?,
        )
    } else {
        let mut it = s.split('-');
        (
            it.next()?.parse().ok()?,
            it.next()?.parse().ok()?,
            it.next()?.parse().ok()?,
        )
    };
    if !(1970..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Tage seit 1970 (Algorithmus von Howard Hinnant, „days_from_civil“).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400)
}

/// Wörter und "Phrasen" trennen; Anführungszeichen dürfen auch nach `von:` stehen.
fn tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in input.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn parse(input: &str) -> Query {
    let mut q = Query::default();
    for tok in tokens(input) {
        let Some((key, value)) = tok
            .split_once(':')
            .filter(|(k, v)| !k.is_empty() && !v.is_empty())
        else {
            q.terms.push(tok);
            continue;
        };
        let value = value.to_owned();
        match key.to_lowercase().as_str() {
            "von" | "from" => q.from.push(value),
            "an" | "to" => q.to.push(value),
            "betreff" | "subject" => q.subject.push(value),
            "hat" | "has"
                if matches!(
                    value.to_lowercase().as_str(),
                    "anhang" | "attachment" | "anhänge"
                ) =>
            {
                q.has_attachment = true;
            }
            "ist" | "is" => match value.to_lowercase().as_str() {
                "ungelesen" | "unread" => q.unread = true,
                "markiert" | "flagged" | "starred" => q.flagged = true,
                _ => q.terms.push(tok),
            },
            "nach" | "after" | "seit" | "since" => match parse_date(&value) {
                Some(t) => q.after = Some(t),
                None => q.terms.push(tok),
            },
            "vor" | "before" => match parse_date(&value) {
                Some(t) => q.before = Some(t),
                None => q.terms.push(tok),
            },
            // Unbekannter Schlüssel (z. B. eine Uhrzeit „12:30“): als normales Wort suchen.
            _ => q.terms.push(tok),
        }
    }
    q
}

/// Ein Begriff als sicherer FTS5-Ausdruck. Anführungszeichen werden verdoppelt,
/// damit Eingaben nie als Such-Syntax (AND/OR/NEAR, Spalten) wirken.
fn fts_term(column: Option<&str>, text: &str) -> Option<String> {
    // Satzzeichen trennt FTS5 ohnehin; leere Begriffe weglassen.
    let cleaned: String = text.chars().filter(|c| !c.is_control()).collect();
    if !cleaned.chars().any(char::is_alphanumeric) {
        return None;
    }
    let quoted = format!("\"{}\"", cleaned.replace('"', "\"\""));
    // Einzelwort → Präfixsuche; Phrase (mit Leerzeichen) → genau.
    let expr = if cleaned.contains(char::is_whitespace) {
        quoted
    } else {
        format!("{quoted}*")
    };
    Some(match column {
        Some(c) => format!("{c} : {expr}"),
        None => expr,
    })
}

impl Query {
    /// FTS5-MATCH-Ausdruck (alle Bedingungen müssen zutreffen) oder `None`.
    pub fn match_expr(&self) -> Option<String> {
        let parts: Vec<String> = self
            .terms
            .iter()
            .filter_map(|t| fts_term(None, t))
            .chain(self.from.iter().filter_map(|t| fts_term(Some("sender"), t)))
            .chain(
                self.to
                    .iter()
                    .filter_map(|t| fts_term(Some("recipients"), t)),
            )
            .chain(
                self.subject
                    .iter()
                    .filter_map(|t| fts_term(Some("subject"), t)),
            )
            .collect();
        (!parts.is_empty()).then(|| parts.join(" AND "))
    }
}

/// Ergebnis einer Suche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage {
    pub total: u32,
    pub items: Vec<MessageSummary>,
}

impl Store {
    /// Sucht in allen Ordnern (ohne Papierkorb/Spam) oder in einem Ordner.
    /// Neueste zuerst.
    pub fn search(
        &self,
        input: &str,
        folder: Option<FolderId>,
        offset: u32,
        limit: u32,
    ) -> Result<SearchPage> {
        let q = parse(input);
        if q.is_empty() {
            return Ok(SearchPage {
                total: 0,
                items: vec![],
            });
        }
        let mut conditions = vec!["m.pending_move = 0".to_owned()];
        let mut args: Vec<Value> = Vec::new();
        if let Some(expr) = q.match_expr() {
            conditions
                .push("m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?)".into());
            args.push(Value::Text(expr));
        }
        match folder {
            Some(f) => {
                conditions.push("m.folder_id = ?".into());
                args.push(Value::Integer(f));
            }
            None => conditions.push(
                "m.folder_id NOT IN (SELECT id FROM folders WHERE role IN ('trash', 'junk'))"
                    .into(),
            ),
        }
        if q.has_attachment {
            conditions.push("m.has_attachments = 1".into());
        }
        if q.unread {
            conditions.push("m.seen = 0".into());
        }
        if q.flagged {
            conditions.push("m.flagged = 1".into());
        }
        if let Some(t) = q.after {
            conditions.push("m.date >= ?".into());
            args.push(Value::Integer(t));
        }
        if let Some(t) = q.before {
            conditions.push("m.date < ?".into());
            args.push(Value::Integer(t));
        }
        let filter = conditions.join(" AND ");

        let conn = self.conn()?;
        let total: u32 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM messages m WHERE {filter}"),
                params_from_iter(args.iter()),
                |r| r.get(0),
            )
            .map_err(storage_err)?;
        args.push(Value::Integer(i64::from(limit)));
        args.push(Value::Integer(i64::from(offset)));
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM messages m WHERE {filter}
                 ORDER BY m.date DESC, m.id DESC LIMIT ? OFFSET ?"
            ))
            .map_err(storage_err)?;
        let items = stmt
            .query_map(params_from_iter(args.iter()), summary_from_row)
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)?;
        Ok(SearchPage { total, items })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::NewAccount;
    use kolibri_core::{
        Address, Body, Flags, FolderRole, RemoteFolder, RemoteMessage, Security, ServerConfig,
    };

    #[test]
    fn parses_filters_in_german_and_english() {
        let q = parse(
            r#"rechnung von:stadtwerke an:"Anna Schäfer" hat:anhang ist:ungelesen nach:2026-01-01 vor:31.12.2026 12:30"#,
        );
        assert_eq!(q.terms, ["rechnung", "12:30"]);
        assert_eq!(q.from, ["stadtwerke"]);
        assert_eq!(q.to, ["Anna Schäfer"]);
        assert!(q.has_attachment && q.unread && !q.flagged);
        assert_eq!(q.after, Some(1_767_225_600)); // 2026-01-01
        assert_eq!(q.before, Some(1_798_675_200)); // 2026-12-31
        let e = parse("from:max is:flagged has:attachment");
        assert!(e.flagged && e.has_attachment && e.terms.is_empty());
        assert!(parse("   ").is_empty());
        assert_eq!(parse("nach:gestern").terms, ["nach:gestern"]);
    }

    #[test]
    fn match_expression_is_safe() {
        let q = parse(r#"rech "a b" von:x" AND OR NEAR( * -"#);
        let e = q.match_expr().unwrap();
        // Nutzer-Eingaben landen nur in Anführungszeichen, nie als Operator.
        assert!(e.starts_with("\"rech\"* AND \"a b\""), "{e}");
        assert!(!e.contains("\"*\""), "{e}");
        assert!(parse("!!!").match_expr().is_none());
    }

    fn setup() -> (Store, FolderId, FolderId, FolderId) {
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
        let f = |p: &str, r| RemoteFolder {
            path: p.into(),
            delimiter: Some("/".into()),
            role: Some(r),
            selectable: true,
        };
        store
            .replace_folders(
                a.id,
                &[
                    f("INBOX", FolderRole::Inbox),
                    f("Archiv", FolderRole::Archive),
                    f("Trash", FolderRole::Trash),
                ],
            )
            .unwrap();
        let id = |r| store.folder_by_role(a.id, r).unwrap().unwrap().id;
        let ids = (
            id(FolderRole::Inbox),
            id(FolderRole::Archive),
            id(FolderRole::Trash),
        );
        (store, ids.0, ids.1, ids.2)
    }

    fn msg(
        uid: u32,
        subject: &str,
        from: (&str, &str),
        date: i64,
        attach: bool,
        seen: bool,
    ) -> RemoteMessage {
        RemoteMessage {
            uid,
            message_id: Some(format!("m{uid}-{subject}@x")),
            in_reply_to: None,
            references: None,
            subject: subject.into(),
            from: Address {
                name: from.0.into(),
                addr: from.1.into(),
            },
            to: vec![Address {
                name: "Anna Schäfer".into(),
                addr: "anna@x.de".into(),
            }],
            cc: vec![],
            reply_to: vec![],
            date,
            size: 1,
            flags: Flags {
                seen,
                ..Flags::default()
            },
            has_attachments: attach,
        }
    }

    #[test]
    fn searches_headers_bodies_and_filters() {
        let (store, inbox, archive, trash) = setup();
        let day = 86_400;
        store
            .insert_messages(
                inbox,
                &[
                    msg(
                        1,
                        "Rechnung Oktober",
                        ("Stadtwerke Köln", "rechnung@stadtwerke.example"),
                        100 * day,
                        true,
                        true,
                    ),
                    msg(
                        2,
                        "Treffen nächste Woche",
                        ("Jürgen Müller", "j@x.de"),
                        200 * day,
                        false,
                        false,
                    ),
                ],
            )
            .unwrap();
        store
            .insert_messages(
                archive,
                &[msg(
                    1,
                    "Alte Rechnung",
                    ("Stadtwerke", "rechnung@stadtwerke.example"),
                    50 * day,
                    false,
                    true,
                )],
            )
            .unwrap();
        store
            .insert_messages(
                trash,
                &[msg(
                    1,
                    "Rechnung gelöscht",
                    ("Spam", "s@x.de"),
                    300 * day,
                    false,
                    true,
                )],
            )
            .unwrap();

        let titles = |q: &str, f: Option<FolderId>| -> Vec<String> {
            store
                .search(q, f, 0, 50)
                .unwrap()
                .items
                .into_iter()
                .map(|m| m.subject)
                .collect()
        };
        // Präfix, über Ordner hinweg, neueste zuerst, ohne Papierkorb.
        assert_eq!(titles("rech", None), ["Rechnung Oktober", "Alte Rechnung"]);
        assert_eq!(titles("rechnung", Some(inbox)), ["Rechnung Oktober"]);
        assert_eq!(titles("rechnung", Some(trash)), ["Rechnung gelöscht"]);
        // Umlaute egal, Absender-Filter.
        assert_eq!(titles("muller", None), ["Treffen nächste Woche"]);
        assert_eq!(titles("von:jurgen", None), ["Treffen nächste Woche"]);
        assert!(titles("von:anna", None).is_empty());
        assert_eq!(titles("an:anna", None).len(), 3);
        // Reine Filter ohne Text.
        assert_eq!(titles("hat:anhang", None), ["Rechnung Oktober"]);
        assert_eq!(titles("ist:ungelesen", None), ["Treffen nächste Woche"]);
        assert_eq!(
            titles("rechnung nach:1970-03-01", None),
            ["Rechnung Oktober"]
        );
        assert_eq!(titles("rechnung vor:1970-03-01", None), ["Alte Rechnung"]);
        // Phrase.
        assert_eq!(titles("\"nächste Woche\"", None), ["Treffen nächste Woche"]);
        assert!(titles("\"Woche nächste\"", None).is_empty());

        // Inhalt wird durchsuchbar, sobald er geladen ist.
        assert!(titles("kickoff", None).is_empty());
        let id = store.search("treffen", None, 0, 1).unwrap().items[0].id;
        store
            .save_body(
                id,
                &Body {
                    html: None,
                    text: Some("Wir planen den Kickoff im Büro.".into()),
                    remote_images: false,
                    attachments: vec![],
                    preview: String::new(),
                    invite: None,
                },
            )
            .unwrap();
        assert_eq!(titles("kickoff buro", None), ["Treffen nächste Woche"]);
        // Erneutes Speichern ersetzt den Index-Eintrag (keine Dubletten).
        store
            .save_body(
                id,
                &Body {
                    html: None,
                    text: Some("Neuer Text".into()),
                    remote_images: false,
                    attachments: vec![],
                    preview: String::new(),
                    invite: None,
                },
            )
            .unwrap();
        assert!(titles("kickoff", None).is_empty());
        assert_eq!(titles("neuer", None).len(), 1);

        // Gelöschte Nachrichten verschwinden aus dem Index.
        store.delete_uids(inbox, &[2]).unwrap();
        assert!(titles("neuer", None).is_empty());
        assert_eq!(store.search("rech", None, 0, 1).unwrap().total, 2);
        assert_eq!(store.search("", None, 0, 10).unwrap().total, 0);
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date("1970-01-02"), Some(86_400));
        assert_eq!(parse_date("02.01.1970"), Some(86_400));
        assert!(parse_date("2024-02-30").is_some()); // grob geprüft, reicht für Filter
        assert_eq!(parse_date("2024-13-01"), None);
        assert_eq!(parse_date("abc"), None);
    }
}
