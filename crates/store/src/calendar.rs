//! Kalender und Termine im lokalen Cache.

use std::collections::HashMap;

use kolibri_core::{AccountId, Error, ErrorKind, Result};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::{Store, storage_err};

pub type CalendarId = i64;
pub type EventId = i64;

/// URL des lokalen Kalenders „Auf diesem Mac“.
pub const LOCAL_URL: &str = "local";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarInfo {
    pub id: CalendarId,
    /// None = lokal oder verbundenes Konto (`source_id`).
    pub account_id: Option<AccountId>,
    /// Verbundenes CalDAV-Konto (iCloud, Nextcloud …).
    pub source_id: Option<i64>,
    pub url: String,
    pub name: String,
    pub color: String,
    pub visible: bool,
}

/// Verbundenes CalDAV-Konto (ohne Passwort – das liegt im Schlüsselbund).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DavSource {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub username: String,
    #[serde(skip)]
    pub calendar_home: String,
}

/// Termin (ganze iCalendar-Datei) mit Ort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub id: EventId,
    pub calendar_id: CalendarId,
    pub href: String,
    pub etag: String,
    pub uid: String,
    pub ics: String,
}

/// Termin, wie er gespeichert wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    pub href: String,
    pub etag: String,
    pub uid: String,
    pub ics: String,
    pub start: i64,
    /// None = Serie ohne Ende.
    pub end_bound: Option<i64>,
}

impl Store {
    pub fn caldav_home(&self, account: AccountId) -> Result<(Option<String>, Option<i64>)> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT caldav_home, caldav_checked FROM accounts WHERE id = ?1",
            [account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(storage_err)
    }

    pub fn set_caldav_home(&self, account: AccountId, home: Option<&str>, now: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE accounts SET caldav_home = ?2, caldav_checked = ?3 WHERE id = ?1",
            params![account, home.unwrap_or(""), now],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    /// Kalender eines Kontos abgleichen (neue anlegen, Namen/Farben übernehmen,
    /// verschwundene samt Terminen entfernen). Sichtbarkeit bleibt erhalten.
    pub fn replace_calendars(
        &self,
        account: AccountId,
        list: &[(String, String, String)],
    ) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        {
            let mut upsert = tx
                .prepare_cached(
                    "INSERT INTO calendars (account_id, url, name, color) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (account_id, url) DO UPDATE SET name = excluded.name,
                        color = CASE WHEN excluded.color <> '' THEN excluded.color ELSE color END",
                )
                .map_err(storage_err)?;
            for (url, name, color) in list {
                upsert
                    .execute(params![account, url, name, color])
                    .map_err(storage_err)?;
            }
        }
        let keep: Vec<&str> = list.iter().map(|(u, _, _)| u.as_str()).collect();
        let existing: Vec<(CalendarId, String)> = {
            let mut stmt = tx
                .prepare_cached("SELECT id, url FROM calendars WHERE account_id = ?1")
                .map_err(storage_err)?;
            stmt.query_map([account], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(storage_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(storage_err)?
        };
        for (id, url) in existing {
            if !keep.contains(&url.as_str()) {
                tx.execute("DELETE FROM calendars WHERE id = ?1", [id])
                    .map_err(storage_err)?;
            }
        }
        tx.commit().map_err(storage_err)
    }

    /// Alle Kalender: lokal zuerst, dann nach Konto und Name.
    pub fn calendars(&self) -> Result<Vec<CalendarInfo>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, account_id, url, name, color, visible, source_id FROM calendars
                 ORDER BY account_id IS NOT NULL OR source_id IS NOT NULL, account_id, source_id,
                    url <> 'local', name COLLATE NOCASE",
            )
            .map_err(storage_err)?;
        stmt.query_map([], |r| {
            Ok(CalendarInfo {
                id: r.get(0)?,
                account_id: r.get(1)?,
                url: r.get(2)?,
                name: r.get(3)?,
                color: r.get(4)?,
                visible: r.get(5)?,
                source_id: r.get(6)?,
            })
        })
        .map_err(storage_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_err)
    }

    pub fn calendar(&self, id: CalendarId) -> Result<CalendarInfo> {
        self.calendars()?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Kalender {id}")))
    }

    /// Neuer lokaler Kalender („Auf diesem Mac“).
    pub fn create_local_calendar(&self, name: &str, color: &str) -> Result<CalendarId> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO calendars (account_id, url, name, color)
             VALUES (NULL, 'local:' || lower(hex(randomblob(8))), ?1, ?2)",
            params![name, color],
        )
        .map_err(storage_err)?;
        Ok(conn.last_insert_rowid())
    }

    /// Name und Farbe ändern (Server-Kalender: nach PROPPATCH).
    pub fn update_calendar(&self, id: CalendarId, name: &str, color: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE calendars SET name = ?2, color = ?3 WHERE id = ?1",
            params![id, name, color],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    /// Kalender samt Terminen entfernen (Server-Kalender: nach DELETE).
    pub fn delete_calendar(&self, id: CalendarId) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM calendars WHERE id = ?1 AND url <> 'local'",
            [id],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    // ---- Verbundene CalDAV-Konten --------------------------------------------------

    pub fn dav_sources(&self) -> Result<Vec<DavSource>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, name, url, username, calendar_home FROM dav_sources ORDER BY id",
            )
            .map_err(storage_err)?;
        stmt.query_map([], |r| {
            Ok(DavSource {
                id: r.get(0)?,
                name: r.get(1)?,
                url: r.get(2)?,
                username: r.get(3)?,
                calendar_home: r.get(4)?,
            })
        })
        .map_err(storage_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_err)
    }

    pub fn dav_source(&self, id: i64) -> Result<DavSource> {
        self.dav_sources()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Verbindung {id}")))
    }

    pub fn insert_dav_source(&self, name: &str, url: &str, user: &str, home: &str) -> Result<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO dav_sources (name, url, username, calendar_home) VALUES (?1, ?2, ?3, ?4)",
            params![name, url, user, home],
        )
        .map_err(storage_err)?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_dav_source(&self, id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM dav_sources WHERE id = ?1", [id])
            .map_err(storage_err)?;
        Ok(())
    }

    /// Kalenderliste einer Verbindung ersetzen (wie `replace_calendars`).
    pub fn replace_source_calendars(
        &self,
        source: i64,
        list: &[(String, String, String)],
    ) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        let existing: Vec<(CalendarId, String)> = {
            let mut stmt = tx
                .prepare_cached("SELECT id, url FROM calendars WHERE source_id = ?1")
                .map_err(storage_err)?;
            stmt.query_map([source], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(storage_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(storage_err)?
        };
        for (url, name, color) in list {
            match existing.iter().find(|(_, u)| u == url) {
                Some((id, _)) => {
                    tx.execute(
                        "UPDATE calendars SET name = ?2,
                           color = CASE WHEN ?3 <> '' THEN ?3 ELSE color END WHERE id = ?1",
                        params![id, name, color],
                    )
                    .map_err(storage_err)?;
                }
                None => {
                    tx.execute(
                        "INSERT INTO calendars (account_id, source_id, url, name, color)
                         VALUES (NULL, ?1, ?2, ?3, ?4)",
                        params![source, url, name, color],
                    )
                    .map_err(storage_err)?;
                }
            }
        }
        for (id, url) in existing {
            if !list.iter().any(|(u, _, _)| *u == url) {
                tx.execute("DELETE FROM calendars WHERE id = ?1", [id])
                    .map_err(storage_err)?;
            }
        }
        tx.commit().map_err(storage_err)
    }

    pub fn set_calendar_visible(&self, id: CalendarId, visible: bool) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE calendars SET visible = ?2 WHERE id = ?1",
            params![id, visible],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    pub fn event_etags(&self, calendar: CalendarId) -> Result<HashMap<String, String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached("SELECT href, etag FROM events WHERE calendar_id = ?1")
            .map_err(storage_err)?;
        stmt.query_map([calendar], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(storage_err)?
            .collect::<rusqlite::Result<HashMap<_, _>>>()
            .map_err(storage_err)
    }

    pub fn apply_events(
        &self,
        calendar: CalendarId,
        changed: &[EventRecord],
        removed: &[String],
    ) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(storage_err)?;
        {
            let mut upsert = tx
                .prepare_cached(
                    "INSERT INTO events (calendar_id, href, etag, uid, ics, start_unix, end_bound)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (calendar_id, href) DO UPDATE SET etag = excluded.etag,
                        uid = excluded.uid, ics = excluded.ics, start_unix = excluded.start_unix,
                        end_bound = excluded.end_bound",
                )
                .map_err(storage_err)?;
            for e in changed {
                upsert
                    .execute(params![
                        calendar,
                        e.href,
                        e.etag,
                        e.uid,
                        e.ics,
                        e.start,
                        e.end_bound
                    ])
                    .map_err(storage_err)?;
            }
            let mut delete = tx
                .prepare_cached("DELETE FROM events WHERE calendar_id = ?1 AND href = ?2")
                .map_err(storage_err)?;
            for href in removed {
                delete
                    .execute(params![calendar, href])
                    .map_err(storage_err)?;
            }
        }
        tx.commit().map_err(storage_err)
    }

    /// Termine sichtbarer Kalender, die den Zeitraum [from, to) berühren können.
    pub fn events_in_range(&self, from: i64, to: i64) -> Result<Vec<StoredEvent>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT e.id, e.calendar_id, e.href, e.etag, e.uid, e.ics FROM events e
                 JOIN calendars c ON c.id = e.calendar_id
                 WHERE c.visible = 1 AND e.start_unix < ?2 AND (e.end_bound IS NULL OR e.end_bound > ?1)",
            )
            .map_err(storage_err)?;
        stmt.query_map(params![from, to], event_from_row)
            .map_err(storage_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)
    }

    pub fn event(&self, id: EventId) -> Result<StoredEvent> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT id, calendar_id, href, etag, uid, ics FROM events WHERE id = ?1",
            [id],
            event_from_row,
        )
        .optional()
        .map_err(storage_err)?
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Termin {id}")))
    }

    /// Termin mit dieser UID (z. B. um eine Einladung nicht doppelt einzutragen).
    pub fn event_by_uid(&self, uid: &str) -> Result<Option<StoredEvent>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT id, calendar_id, href, etag, uid, ics FROM events WHERE uid = ?1 LIMIT 1",
            [uid],
            event_from_row,
        )
        .optional()
        .map_err(storage_err)
    }

    pub fn delete_event_row(&self, id: EventId) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM events WHERE id = ?1", [id])
            .map_err(storage_err)?;
        Ok(())
    }
}

fn event_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
    Ok(StoredEvent {
        id: r.get(0)?,
        calendar_id: r.get(1)?,
        href: r.get(2)?,
        etag: r.get(3)?,
        uid: r.get(4)?,
        ics: r.get(5)?,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::NewAccount;
    use kolibri_core::{Security, ServerConfig};

    fn rec(href: &str, start: i64, end: Option<i64>) -> EventRecord {
        EventRecord {
            href: href.into(),
            etag: "e".into(),
            uid: href.into(),
            ics: "X".into(),
            start,
            end_bound: end,
        }
    }

    #[test]
    fn local_calendar_exists_and_range_query() {
        let store = Store::open_in_memory().unwrap();
        let cals = store.calendars().unwrap();
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].url, LOCAL_URL);
        assert!(cals[0].account_id.is_none());
        let local = cals[0].id;
        store
            .apply_events(
                local,
                &[
                    rec("a", 100, Some(200)),
                    rec("serie", 50, None),
                    rec("alt", 10, Some(20)),
                ],
                &[],
            )
            .unwrap();
        let hrefs = |f, t| {
            let mut v: Vec<_> = store
                .events_in_range(f, t)
                .unwrap()
                .into_iter()
                .map(|e| e.href)
                .collect();
            v.sort();
            v
        };
        assert_eq!(hrefs(150, 160), ["a", "serie"]);
        assert_eq!(hrefs(15, 30), ["alt"]);
        assert_eq!(hrefs(1000, 2000), ["serie"]);
        store.set_calendar_visible(local, false).unwrap();
        assert!(hrefs(0, 10_000).is_empty());
        assert_eq!(store.event_by_uid("a").unwrap().unwrap().href, "a");
        let id = store.event_by_uid("a").unwrap().unwrap().id;
        store.delete_event_row(id).unwrap();
        assert!(store.event_by_uid("a").unwrap().is_none());
    }

    #[test]
    fn replace_calendars_keeps_visibility_and_removes_missing() {
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
            .unwrap()
            .id;
        store
            .replace_calendars(
                a,
                &[
                    ("u1".into(), "Privat".into(), "#ff0000".into()),
                    ("u2".into(), "Arbeit".into(), String::new()),
                ],
            )
            .unwrap();
        let privat = store
            .calendars()
            .unwrap()
            .into_iter()
            .find(|c| c.name == "Privat")
            .unwrap();
        store.set_calendar_visible(privat.id, false).unwrap();
        store
            .apply_events(privat.id, &[rec("x", 1, Some(2))], &[])
            .unwrap();
        store
            .replace_calendars(a, &[("u1".into(), "Privat (neu)".into(), String::new())])
            .unwrap();
        let cals = store.calendars().unwrap();
        assert_eq!(cals.len(), 2, "lokal + Privat");
        let p = cals.iter().find(|c| c.url == "u1").unwrap();
        assert_eq!(p.name, "Privat (neu)");
        assert_eq!(
            p.color, "#ff0000",
            "Farbe bleibt, wenn der Server keine liefert"
        );
        assert!(!p.visible);
        assert_eq!(store.event_etags(p.id).unwrap().len(), 1);
        assert!(store.caldav_home(a).unwrap().0.is_none());
        store.set_caldav_home(a, Some("https://x/cal/"), 5).unwrap();
        assert_eq!(
            store.caldav_home(a).unwrap(),
            (Some("https://x/cal/".into()), Some(5))
        );
    }
}
