//! Kalender: CalDAV-Abgleich, Termine anzeigen/anlegen/ändern/löschen,
//! Einladungen aus Mails annehmen oder ablehnen.

use kolibri_core::lang::{Lang, current, tr};
use kolibri_core::{Account, AccountId, Error, ErrorKind, Invite, MessageId, Result, SecretStore};
use kolibri_dav::Url;
use kolibri_dav::{Dav, calendar_candidates, ical};
use kolibri_mail::compose::{
    build_invite_reply, build_itip, date_time_separator, escape_html, long_date, new_message_id,
};
use kolibri_store::{
    CalendarId, CalendarInfo, DavSource, EventId, EventRecord, LOCAL_URL, OutboxKind,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::accounts::AppState;
use crate::keychain::account_key;

/// Kein CalDAV gefunden → frühestens nach einem Tag erneut suchen.
const RETRY_DISCOVERY_SECS: i64 = 24 * 60 * 60;
/// Farbe für Kalender ohne eigene Farbe.
const DEFAULT_COLOR: &str = "#0a84ff";

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn record(href: String, etag: String, ics: String) -> Option<EventRecord> {
    let cal = ical::parse(&ics);
    let first = cal.events.first()?;
    let start = cal.events.iter().map(|e| e.start.unix()).min().unwrap_or(0);
    Some(EventRecord {
        href,
        etag,
        uid: first.uid.clone(),
        start,
        end_bound: cal.last_end(),
        ics,
    })
}

fn dav_for(state: &AppState, account: &Account) -> Result<Dav> {
    let password = state.secrets.password(&account_key(account.id))?;
    Dav::new(
        &account.imap.username,
        &password,
        kolibri_core::is_loopback(&account.imap.host),
    )
}

/// Gleicht alle Kalender eines Kontos ab. Liefert, ob sich etwas geändert hat.
pub async fn sync_calendars(state: &AppState, account: &Account) -> Result<bool> {
    let (home, checked) = state.store.caldav_home(account.id)?;
    let dav = dav_for(state, account)?;
    let home = match home {
        Some(h) if !h.is_empty() => h,
        Some(_) if checked.is_some_and(|c| now() - c < RETRY_DISCOVERY_SECS) => return Ok(false),
        _ => {
            let found = dav
                .discover_calendar_home(&calendar_candidates(&account.email, &account.imap.host))
                .await?;
            state
                .store
                .set_caldav_home(account.id, found.as_deref(), now())?;
            match found {
                Some(h) => h,
                None => return Ok(false),
            }
        }
    };

    let remote = dav.calendars(&home).await?;
    state.store.replace_calendars(
        account.id,
        &remote
            .iter()
            .map(|c| (c.url.clone(), c.name.clone(), c.color.clone()))
            .collect::<Vec<_>>(),
    )?;
    let mut changed = false;
    for cal in state.store.calendars()? {
        if cal.account_id != Some(account.id) {
            continue;
        }
        changed |= sync_events(state, &dav, &cal).await?;
    }
    Ok(changed)
}

/// Termine eines Kalenders abgleichen.
async fn sync_events(state: &AppState, dav: &Dav, cal: &CalendarInfo) -> Result<bool> {
    let mut changed = false;
    {
        let known = state.store.event_etags(cal.id)?;
        let changes = dav.sync_calendar(&cal.url, &known).await?;
        if !(changes.changed.is_empty() && changes.removed.is_empty()) {
            let records: Vec<EventRecord> = changes
                .changed
                .into_iter()
                .filter_map(|c| record(c.href, c.etag, c.vcard))
                .collect();
            state
                .store
                .apply_events(cal.id, &records, &changes.removed)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Eingegebene Server-Adresse prüfen (nur https, außer localhost).
fn parse_dav_url(url: &str) -> Result<Url> {
    let raw = url.trim().replacen("webcal://", "https://", 1);
    let with_scheme = if raw.contains("://") {
        raw
    } else {
        format!("https://{raw}")
    };
    let parsed = Url::parse(&with_scheme)
        .map_err(|_| Error::new(ErrorKind::Invalid, "Die Server-Adresse ist ungültig."))?;
    let local = parsed.host_str().is_some_and(kolibri_core::is_loopback);
    if parsed.scheme() != "https" && !local {
        return Err(Error::new(
            ErrorKind::Invalid,
            "Nur verschlüsselte Adressen (https://) sind erlaubt.",
        ));
    }
    Ok(parsed)
}

/// Kalender-Verzeichnis zu einer Adresse: die Adresse selbst (oder das
/// Verzeichnis darüber), sonst .well-known und die Wurzel des Servers.
async fn find_home(dav: &Dav, url: &Url) -> Result<Option<String>> {
    if let Some(h) = dav.calendar_home_for(url.as_str()).await? {
        return Ok(Some(h));
    }
    let more: Vec<String> = ["/.well-known/caldav", "/"]
        .iter()
        .filter_map(|p| url.join(p).ok().map(String::from))
        .collect();
    dav.discover_calendar_home(&more).await
}

/// Schlüsselbund-Eintrag eines verbundenen Kontos.
fn source_key(id: i64) -> String {
    format!("dav-{id}")
}

fn source_dav(state: &AppState, src: &DavSource) -> Result<Dav> {
    let password = state.secrets.password(&source_key(src.id))?;
    let local = Url::parse(&src.url)
        .ok()
        .and_then(|u| u.host_str().map(kolibri_core::is_loopback))
        .unwrap_or(false);
    Dav::new(&src.username, &password, local)
}

/// Kalender aller verbundenen Konten abgleichen.
pub async fn sync_sources(state: &AppState) -> Result<bool> {
    let mut changed = false;
    for src in state.store.dav_sources()? {
        let dav = source_dav(state, &src)?;
        let remote = dav.calendars(&src.calendar_home).await?;
        state.store.replace_source_calendars(
            src.id,
            &remote
                .iter()
                .map(|c| (c.url.clone(), c.name.clone(), c.color.clone()))
                .collect::<Vec<_>>(),
        )?;
        for cal in state.store.calendars()? {
            if cal.source_id == Some(src.id) {
                changed |= sync_events(state, &dav, &cal).await?;
            }
        }
    }
    Ok(changed)
}

/// Im Komplettabgleich. Fehler dürfen den Mail-Sync nie stören.
pub async fn refresh(app: &AppHandle, account_id: AccountId) {
    let state = app.state::<AppState>();
    let Ok(account) = state.store.account(account_id) else {
        return;
    };
    match sync_calendars(&state, &account).await {
        Ok(true) => {
            let _ = app.emit("calendar-changed", account_id);
        }
        Ok(false) => {}
        Err(e) if e.kind == ErrorKind::Auth => {
            log::warn!("CalDAV für Konto {account_id} abgelehnt: {e}");
            let _ = state.store.set_caldav_home(account_id, None, now());
        }
        Err(e) => log::warn!("Kalender von Konto {account_id} nicht abgeglichen: {e}"),
    }
    // Verbundene Konten einmal pro Durchlauf (beim ersten Konto).
    if state
        .store
        .accounts()
        .ok()
        .and_then(|a| a.first().map(|a| a.id))
        == Some(account_id)
    {
        match sync_sources(&state).await {
            Ok(true) => {
                let _ = app.emit("calendar-changed", 0);
            }
            Ok(false) => {}
            Err(e) => log::warn!("Verbundene Kalender nicht abgeglichen: {e}"),
        }
    }
}

// ---- Anzeigen -----------------------------------------------------------------

/// Ein Vorkommen für die Kalenderansicht.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub event_id: EventId,
    pub calendar_id: CalendarId,
    pub color: String,
    #[serde(flatten)]
    pub instance: ical::Instance,
}

impl AppState {
    pub fn occurrences(&self, from: i64, to: i64) -> Result<Vec<Occurrence>> {
        let calendars = self.store.calendars()?;
        let mut out = Vec::new();
        for stored in self.store.events_in_range(from, to)? {
            let color = calendars
                .iter()
                .find(|c| c.id == stored.calendar_id)
                .map(|c| c.color.clone())
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| DEFAULT_COLOR.into());
            for instance in ical::parse(&stored.ics).instances(from, to) {
                if instance.cancelled {
                    continue;
                }
                out.push(Occurrence {
                    event_id: stored.id,
                    calendar_id: stored.calendar_id,
                    color: color.clone(),
                    instance,
                });
            }
        }
        out.sort_by_key(|o| (o.instance.start, std::cmp::Reverse(o.instance.end)));
        Ok(out)
    }
}

/// Alles zu einem Termin für den Bearbeiten-Dialog.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDetail {
    pub id: EventId,
    pub calendar_id: CalendarId,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub all_day: bool,
    /// Beginn/Ende der (ersten) Serie bzw. des Termins.
    pub start: i64,
    pub end: i64,
    pub recurring: bool,
    pub organizer: Option<ical::Person>,
    pub attendees: Vec<ical::Person>,
    /// Von jemand anderem eingeladen: Teilnehmende nicht änderbar.
    pub invited_by_other: bool,
}

/// Eingeladene Person aus dem Termin-Dialog.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttendeeInput {
    #[serde(default)]
    pub name: String,
    pub email: String,
}

/// Eingabe aus dem Termin-Dialog.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventInput {
    /// None = neu.
    pub id: Option<EventId>,
    pub calendar_id: CalendarId,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub all_day: bool,
    pub start: i64,
    pub end: i64,
    #[serde(default)]
    pub attendees: Vec<AttendeeInput>,
}

/// Welche Termin-Mail verschickt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InviteMail {
    New,
    Changed,
    Cancelled,
}

/// Erster (Haupt-)Termin einer Datei samt erstem Vorkommen (Beginn, Ende).
fn main_event(cal: &ical::Calendar) -> Option<(ical::Event, i64, i64)> {
    let e = cal
        .events
        .iter()
        .find(|e| e.recurrence_id.is_none())
        .or(cal.events.first())?;
    let first = cal
        .instances(e.start.unix() - 1, i64::MAX / 4)
        .into_iter()
        .next();
    let start = first.as_ref().map_or_else(|| e.start.unix(), |i| i.start);
    let end = first.as_ref().map_or(start, |i| i.end);
    Some((e.clone(), start, end))
}

/// „Freitag, 20. November 2026, 14:00–15:00 Uhr“ bzw. nur das Datum.
fn when_text(start: i64, end: i64, all_day: bool) -> String {
    let sep = date_time_separator();
    // „Uhr“ nur im Deutschen.
    let uhr = tr(" Uhr", "");
    let s = long_date(start);
    let (day, time) = s.split_once(sep).unwrap_or((s.as_str(), ""));
    if all_day {
        return day.to_owned();
    }
    let e = long_date(end);
    let (end_day, end_time) = e.split_once(sep).unwrap_or((e.as_str(), ""));
    if end_day == day {
        format!("{day}, {time}–{end_time}{uhr}")
    } else {
        format!("{day}, {time}{uhr} – {end_day}, {end_time}{uhr}")
    }
}

/// Betreff, Text und HTML einer Termin-Mail.
fn invite_texts(
    kind: InviteMail,
    summary: &str,
    when: &str,
    location: &str,
    organizer: &str,
) -> (String, String, String) {
    let en = current() == Lang::En;
    let (prefix, lead) = match (kind, en) {
        (InviteMail::New, false) => (
            "Einladung",
            format!("{organizer} lädt dich zu einem Termin ein."),
        ),
        (InviteMail::New, true) => (
            "Invitation",
            format!("{organizer} invited you to an event."),
        ),
        (InviteMail::Changed, false) => (
            "Geändert",
            format!("{organizer} hat einen Termin geändert."),
        ),
        (InviteMail::Changed, true) => ("Updated", format!("{organizer} updated an event.")),
        (InviteMail::Cancelled, false) => {
            ("Abgesagt", format!("{organizer} hat den Termin abgesagt."))
        }
        (InviteMail::Cancelled, true) => ("Cancelled", format!("{organizer} cancelled the event.")),
    };
    let please = tr(
        "Bitte sag in deinem Kalender zu oder ab.",
        "Please accept or decline in your calendar.",
    );
    let (l_where, l_when, l_from) = if en {
        ("Where", "When", "From")
    } else {
        ("Ort", "Wann", "Von")
    };
    let subject = format!("{prefix}: {summary}");
    let mut text = format!("{lead}\n\n{summary}\n{when}\n");
    if !location.is_empty() {
        text.push_str(&format!("{l_where}: {location}\n"));
    }
    if kind != InviteMail::Cancelled {
        text.push_str(&format!("\n{please}\n"));
    }
    let place = if location.is_empty() {
        String::new()
    } else {
        format!(
            "<tr><td style=\"padding:2px 0;color:#6e6e73;width:56px\">{l_where}</td><td style=\"padding:2px 0\">{}</td></tr>",
            escape_html(location)
        )
    };
    let title_style = if kind == InviteMail::Cancelled {
        "text-decoration:line-through;color:#6e6e73"
    } else {
        "color:#111113"
    };
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"></head><body style=\"margin:0;padding:24px 12px;background:#f4f4f6\">\
<table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\"><tr><td align=\"center\">\
<table role=\"presentation\" width=\"520\" cellpadding=\"0\" cellspacing=\"0\" style=\"width:100%;max-width:520px;background:#ffffff;border-radius:14px\">\
<tr><td style=\"padding:28px 32px;font-family:-apple-system,Helvetica,Arial,sans-serif;font-size:14px;line-height:1.5;color:#1d1d1f\">\
<p style=\"margin:0 0 6px;font-size:12px;font-weight:600;letter-spacing:.04em;text-transform:uppercase;color:#6e6e73\">{prefix}</p>\
<p style=\"margin:0 0 14px;font-size:22px;line-height:1.25;font-weight:700;{title_style}\">{summary}</p>\
<table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" style=\"font-size:14px\">\
<tr><td style=\"padding:2px 0;color:#6e6e73;width:56px\">{l_when}</td><td style=\"padding:2px 0\">{when}</td></tr>{place}\
<tr><td style=\"padding:2px 0;color:#6e6e73\">{l_from}</td><td style=\"padding:2px 0\">{organizer}</td></tr></table>\
<p style=\"margin:18px 0 0;color:#6e6e73\">{lead_html}</p></td></tr></table></td></tr></table></body></html>",
        summary = escape_html(summary),
        when = escape_html(when),
        organizer = escape_html(organizer),
        lead_html = if kind == InviteMail::Cancelled {
            tr(
                "Der Termin findet nicht statt.",
                "The event will not take place.",
            )
        } else {
            please
        },
    );
    (subject, text, html)
}

impl AppState {
    /// Konto, über das Einladungen eines Kalenders verschickt werden
    /// (Konto des Kalenders, beim lokalen Kalender das erste Konto).
    fn invite_account(&self, cal: &CalendarInfo) -> Result<Account> {
        if let Some(a) = self.account_of(cal)? {
            return Ok(a);
        }
        self.store.accounts()?.into_iter().next().ok_or_else(|| {
            Error::new(
                ErrorKind::Invalid,
                "Zum Einladen wird ein Mail-Konto gebraucht.",
            )
        })
    }

    fn own_addresses(&self) -> Result<Vec<String>> {
        Ok(self
            .store
            .accounts()?
            .iter()
            .map(|a| a.email.to_lowercase())
            .collect())
    }

    /// Termin-Mail (iMIP) in den Postausgang legen.
    fn queue_invite_mail(
        &self,
        account: &Account,
        kind: InviteMail,
        to: &[ical::Person],
        event: &ical::NewEvent<'_>,
    ) -> Result<()> {
        if to.is_empty() {
            return Ok(());
        }
        let method = if kind == InviteMail::Cancelled {
            "CANCEL"
        } else {
            "REQUEST"
        };
        let ics = ical::write_event(&ical::NewEvent {
            method: Some(method),
            cancelled: kind == InviteMail::Cancelled,
            ..*event
        });
        let organizer = if account.display_name.trim().is_empty() {
            account.email.clone()
        } else {
            account.display_name.trim().to_owned()
        };
        let when = when_text(event.start, event.end, event.all_day);
        let (subject, text, html) =
            invite_texts(kind, event.summary, &when, event.location, &organizer);
        let recipients: Vec<String> = to
            .iter()
            .map(|p| {
                if p.name.trim().is_empty() {
                    p.email.clone()
                } else {
                    format!("\"{}\" <{}>", p.name.replace('"', ""), p.email)
                }
            })
            .collect();
        let built = build_itip(
            account,
            &recipients,
            &subject,
            &text,
            Some(&html),
            &ics,
            method,
        )?;
        self.store.outbox_push(
            account.id,
            OutboxKind::Send,
            &new_message_id(&account.email),
            &built.raw,
            &built.from,
            &built.to,
            None,
            &subject,
        )?;
        Ok(())
    }

    fn account_of(&self, cal: &CalendarInfo) -> Result<Option<Account>> {
        cal.account_id.map(|a| self.store.account(a)).transpose()
    }

    /// Server-Zugang eines Kalenders (None = lokal).
    fn dav_of(&self, cal: &CalendarInfo) -> Result<Option<Dav>> {
        if let Some(account) = self.account_of(cal)? {
            return dav_for(self, &account).map(Some);
        }
        match cal.source_id {
            Some(id) => source_dav(self, &self.store.dav_source(id)?).map(Some),
            None => Ok(None),
        }
    }

    /// Neuer Kalender: lokal (`account_id` None) oder auf dem Server des Kontos.
    pub async fn create_calendar(
        &self,
        account_id: Option<AccountId>,
        name: &str,
        color: &str,
    ) -> Result<CalendarId> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::new(ErrorKind::Invalid, "Bitte einen Namen angeben."));
        }
        let Some(acc) = account_id else {
            return self.store.create_local_calendar(name, color);
        };
        let account = self.store.account(acc)?;
        let home = self
            .store
            .caldav_home(acc)?
            .0
            .filter(|h| !h.is_empty())
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    "Für dieses Konto wurde (noch) kein Kalender-Server gefunden.",
                )
            })?;
        let url = dav_for(self, &account)?
            .make_calendar(&home, name, color)
            .await?;
        let mut list: Vec<(String, String, String)> = self
            .store
            .calendars()?
            .into_iter()
            .filter(|c| c.account_id == Some(acc))
            .map(|c| (c.url, c.name, c.color))
            .collect();
        list.push((url.clone(), name.to_owned(), color.to_owned()));
        self.store.replace_calendars(acc, &list)?;
        self.store
            .calendars()?
            .into_iter()
            .find(|c| c.account_id == Some(acc) && c.url == url)
            .map(|c| c.id)
            .ok_or_else(|| Error::new(ErrorKind::Internal, "Kalender nicht gespeichert"))
    }

    pub async fn update_calendar(&self, id: CalendarId, name: &str, color: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::new(ErrorKind::Invalid, "Bitte einen Namen angeben."));
        }
        let cal = self.store.calendar(id)?;
        if let Some(dav) = self.dav_of(&cal)? {
            dav.update_calendar(&cal.url, name, color).await?;
        }
        self.store.update_calendar(id, name, color)
    }

    pub async fn delete_calendar(&self, id: CalendarId) -> Result<()> {
        let cal = self.store.calendar(id)?;
        if cal.url == LOCAL_URL {
            return Err(Error::new(
                ErrorKind::Invalid,
                "Der Kalender „Auf diesem Mac“ bleibt immer erhalten.",
            ));
        }
        if let Some(dav) = self.dav_of(&cal)? {
            dav.delete_collection(&cal.url).await?;
        }
        self.store.delete_calendar(id)
    }

    /// Anderes CalDAV-Konto verbinden: Adresse prüfen, Kalender finden, Passwort sichern.
    pub async fn connect_source(
        &self,
        name: &str,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<i64> {
        let parsed = parse_dav_url(url)?;
        let local = parsed.host_str().is_some_and(kolibri_core::is_loopback);
        let dav = Dav::new(username.trim(), password, local)?;
        let home = find_home(&dav, &parsed)
            .await
            .map_err(|e| {
                if e.kind == ErrorKind::Auth {
                    Error::new(
                        ErrorKind::Auth,
                        "Anmeldung abgelehnt – Benutzername oder Passwort stimmen nicht                          (bei iCloud ein app-spezifisches Passwort verwenden).",
                    )
                } else {
                    e
                }
            })?
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    "Unter dieser Adresse wurden keine Kalender gefunden.",
                )
            })?;
        let calendars = dav.calendars(&home).await?;
        let label = if name.trim().is_empty() {
            parsed.host_str().unwrap_or("Kalender").to_owned()
        } else {
            name.trim().to_owned()
        };
        let id = self
            .store
            .insert_dav_source(&label, parsed.as_str(), username.trim(), &home)?;
        if let Err(e) = self.secrets.set_password(&source_key(id), password) {
            let _ = self.store.delete_dav_source(id);
            return Err(e);
        }
        self.store.replace_source_calendars(
            id,
            &calendars
                .iter()
                .map(|c| (c.url.clone(), c.name.clone(), c.color.clone()))
                .collect::<Vec<_>>(),
        )?;
        let _ = sync_sources(self).await;
        Ok(id)
    }

    /// Eingetragene CalDAV-Adresse (z. B. von Purelymail) mit einem Mail-Konto
    /// verknüpfen – angemeldet wird mit dessen Zugangsdaten. Die Adressbücher
    /// werden gleich mitgesucht. Liefert (Kalender gefunden, Kontakte gefunden).
    pub async fn link_account_dav(
        &self,
        account_id: AccountId,
        url: &str,
    ) -> Result<(usize, bool)> {
        let parsed = parse_dav_url(url)?;
        let account = self.store.account(account_id)?;
        let dav = dav_for(self, &account)?;
        let home = find_home(&dav, &parsed).await.map_err(|e| {
            if e.kind == ErrorKind::Auth {
                Error::new(
                    ErrorKind::Auth,
                    "Der Server hat die Anmeldung mit den Zugangsdaten dieses Mail-Kontos abgelehnt.",
                )
            } else {
                e
            }
        })?;
        let home = home.ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Unter dieser Adresse wurden keine Kalender gefunden.",
            )
        })?;
        self.store.set_caldav_home(account.id, Some(&home), now())?;
        // Kontakte liegen meist daneben (…/caldav/… → …/carddav/…).
        let mut contacts = false;
        if let Some(card) = kolibri_dav::sibling_carddav(parsed.as_str())
            && let Ok(Some(book_home)) = dav.address_home_for(&card).await
        {
            self.store.set_carddav_home(account.id, Some(&book_home))?;
            contacts = true;
        }
        sync_calendars(self, &account).await?;
        let found = self
            .store
            .calendars()?
            .iter()
            .filter(|c| c.account_id == Some(account.id))
            .count();
        if contacts {
            let _ = crate::contacts::sync_contacts(self, &account).await;
        }
        Ok((found, contacts))
    }

    pub fn disconnect_source(&self, id: i64) -> Result<()> {
        let _ = self.secrets.delete_password(&source_key(id));
        self.store.delete_dav_source(id)
    }

    /// Speichert eine iCalendar-Datei im Kalender (lokal oder auf dem Server).
    async fn store_ics(
        &self,
        cal: &CalendarInfo,
        uid: &str,
        ics: String,
        existing: Option<(EventId, String, String, i64)>,
    ) -> Result<()> {
        let (href, etag) = match (self.dav_of(cal)?, &existing) {
            (None, Some((_, href, _, old_cal))) if *old_cal == cal.id => {
                (href.clone(), String::new())
            }
            (None, _) => (format!("{uid}.ics"), String::new()),
            (Some(dav), Some((_, href, etag, old_cal))) if *old_cal == cal.id => {
                dav.replace_event(&cal.url, href, &ics, etag).await?;
                (href.clone(), String::new())
            }
            (Some(dav), _) => (
                dav.put_event(&cal.url, uid, &ics, None).await?,
                String::new(),
            ),
        };
        // In anderen Kalender verschoben: alten Eintrag entfernen.
        if let Some((id, old_href, old_etag, old_cal)) = existing
            && old_cal != cal.id
        {
            self.delete_stored(id, old_cal, &old_href, &old_etag)
                .await?;
        }
        let rec = record(href, etag, ics)
            .ok_or_else(|| Error::new(ErrorKind::Internal, "Termin unlesbar"))?;
        // ETag leer: beim nächsten Abgleich holt Kolibri die Server-Fassung.
        self.store.apply_events(cal.id, &[rec], &[])
    }

    async fn delete_stored(
        &self,
        id: EventId,
        calendar: CalendarId,
        href: &str,
        etag: &str,
    ) -> Result<()> {
        let cal = self.store.calendar(calendar)?;
        if let Some(dav) = self.dav_of(&cal)? {
            dav.delete_event(&cal.url, href, etag).await?;
        }
        self.store.delete_event_row(id)
    }

    /// Speichert den Termin; mit Teilnehmenden gehen Einladungen raus.
    /// Liefert die Termin-ID und das Konto, dessen Postausgang etwas enthält.
    pub async fn save_event(&self, input: EventInput) -> Result<(EventId, Option<AccountId>)> {
        if input.summary.trim().is_empty() {
            return Err(Error::new(
                ErrorKind::Invalid,
                "Bitte einen Titel eingeben.",
            ));
        }
        if input.end < input.start {
            return Err(Error::new(
                ErrorKind::Invalid,
                "Das Ende liegt vor dem Beginn.",
            ));
        }
        let cal = self.store.calendar(input.calendar_id)?;
        let existing = match input.id {
            Some(id) => {
                let e = self.store.event(id)?;
                if ical::parse(&e.ics)
                    .events
                    .iter()
                    .any(|ev| ev.rule.is_some() || ev.recurrence_id.is_some())
                {
                    return Err(Error::new(
                        ErrorKind::Invalid,
                        "Wiederkehrende Termine können in Kolibri noch nicht bearbeitet werden.",
                    ));
                }
                Some(e)
            }
            None => None,
        };
        let uid = existing
            .as_ref()
            .map(|e| e.uid.clone())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| {
                new_message_id("kolibri@kolibri.local")
                    .replace(".kolibri@kolibri.local", "-kolibri")
            });
        let old = existing
            .as_ref()
            .and_then(|e| main_event(&ical::parse(&e.ics)));
        let own = self.own_addresses()?;
        let invited_by_other = old
            .as_ref()
            .and_then(|(e, _, _)| e.organizer.as_ref())
            .is_some_and(|o| !own.contains(&o.email.to_lowercase()));
        let times_changed = old.as_ref().is_some_and(|(e, s, en)| {
            *s != input.start || *en != input.end || e.all_day() != input.all_day
        });

        // Teilnehmende: bei fremden Einladungen unverändert lassen.
        let old_attendees = old
            .as_ref()
            .map(|(e, _, _)| e.attendees.clone())
            .unwrap_or_default();
        let attendees: Vec<ical::Person> = if invited_by_other {
            old_attendees.clone()
        } else {
            let mut seen: Vec<String> = own.clone();
            let mut list = Vec::new();
            for a in &input.attendees {
                let email = a.email.trim().to_owned();
                let key = email.to_lowercase();
                if !email.contains('@') || email.contains(char::is_whitespace) {
                    return Err(Error::new(
                        ErrorKind::Invalid,
                        format!("Ungültige Adresse: {email}"),
                    ));
                }
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key.clone());
                let status = old_attendees
                    .iter()
                    .find(|p| p.email.to_lowercase() == key)
                    .filter(|_| !times_changed)
                    .map(|p| p.status.clone())
                    .unwrap_or_default();
                list.push(ical::Person {
                    email,
                    name: a.name.trim().to_owned(),
                    status,
                });
            }
            list
        };
        let removed: Vec<ical::Person> = if invited_by_other {
            vec![]
        } else {
            old_attendees
                .iter()
                .filter(|p| {
                    !attendees
                        .iter()
                        .any(|a| a.email.eq_ignore_ascii_case(&p.email))
                })
                .cloned()
                .collect()
        };
        let sender = if !invited_by_other && (!attendees.is_empty() || !removed.is_empty()) {
            Some(self.invite_account(&cal)?)
        } else {
            None
        };
        let organizer = match (&sender, invited_by_other) {
            (_, true) => old.as_ref().and_then(|(e, _, _)| e.organizer.clone()),
            (Some(a), false) if !attendees.is_empty() => Some(ical::Person {
                email: a.email.clone(),
                name: a.display_name.trim().to_owned(),
                status: String::new(),
            }),
            _ => None,
        };
        let sequence = old.as_ref().map_or(0, |(e, _, _)| e.sequence + 1);
        let event = ical::NewEvent {
            uid: &uid,
            summary: input.summary.trim(),
            location: input.location.trim(),
            description: input.description.trim(),
            all_day: input.all_day,
            start: input.start,
            end: input.end,
            now: now(),
            organizer: organizer.as_ref(),
            attendees: &attendees,
            sequence,
            method: None,
            cancelled: false,
        };
        let ics = ical::write_event(&event);
        self.store_ics(
            &cal,
            &uid,
            ics,
            existing.map(|e| (e.id, e.href, e.etag, e.calendar_id)),
        )
        .await?;
        if let Some(account) = &sender {
            let kind = if old.is_some() {
                InviteMail::Changed
            } else {
                InviteMail::New
            };
            self.queue_invite_mail(account, kind, &attendees, &event)?;
            let gone = ical::NewEvent {
                attendees: &removed,
                ..event
            };
            self.queue_invite_mail(account, InviteMail::Cancelled, &removed, &gone)?;
        }
        Ok((
            self.store.event_by_uid(&uid)?.map_or(0, |e| e.id),
            sender.map(|a| a.id),
        ))
    }

    /// Löschen; bei eigenen Einladungen bekommen alle Teilnehmenden eine Absage.
    pub async fn delete_event(&self, id: EventId) -> Result<Option<AccountId>> {
        let e = self.store.event(id)?;
        let mut notified = None;
        if let Some((ev, start, end)) = main_event(&ical::parse(&e.ics))
            && !ev.attendees.is_empty()
            && ev.rule.is_none()
            && let Some(org) = &ev.organizer
            && self.own_addresses()?.contains(&org.email.to_lowercase())
        {
            let cal = self.store.calendar(e.calendar_id)?;
            let account = self.invite_account(&cal)?;
            let event = ical::NewEvent {
                uid: &ev.uid,
                summary: &ev.summary,
                location: &ev.location,
                description: &ev.description,
                all_day: ev.all_day(),
                start,
                end,
                now: now(),
                organizer: Some(org),
                attendees: &ev.attendees,
                sequence: ev.sequence + 1,
                method: None,
                cancelled: true,
            };
            self.queue_invite_mail(&account, InviteMail::Cancelled, &ev.attendees, &event)?;
            notified = Some(account.id);
        }
        self.delete_stored(id, e.calendar_id, &e.href, &e.etag)
            .await?;
        Ok(notified)
    }

    /// Antwort (REPLY) auf eine eigene Einladung oder Absage (CANCEL) des
    /// Einladenden aus einer eingegangenen Mail übernehmen. `true` = geändert.
    pub async fn apply_itip(&self, invite: &Invite) -> Result<bool> {
        let Some(stored) = self.store.event_by_uid(&invite.uid)? else {
            return Ok(false);
        };
        let Some((ev, _, _)) = main_event(&ical::parse(&stored.ics)) else {
            return Ok(false);
        };
        let own = self.own_addresses()?;
        let organizer = ev
            .organizer
            .as_ref()
            .map(|o| o.email.to_lowercase())
            .unwrap_or_default();
        let cal = self.store.calendar(stored.calendar_id)?;
        match invite.method.to_uppercase().as_str() {
            "REPLY" if own.contains(&organizer) => {
                let reply = ical::parse(&invite.ics);
                let mut ics = stored.ics.clone();
                let mut changed = false;
                for p in reply.events.iter().flat_map(|e| e.attendees.iter()) {
                    if p.status.is_empty() {
                        continue;
                    }
                    if let Some(next) = ical::set_attendee_status(&ics, &p.email, &p.status)
                        && next != ics
                    {
                        ics = next;
                        changed = true;
                    }
                }
                if changed {
                    self.store_ics(
                        &cal,
                        &stored.uid,
                        ics,
                        Some((stored.id, stored.href, stored.etag, stored.calendar_id)),
                    )
                    .await?;
                }
                Ok(changed)
            }
            "CANCEL"
                if !organizer.is_empty()
                    && !own.contains(&organizer)
                    && invite.organizer_email.eq_ignore_ascii_case(&organizer) =>
            {
                self.delete_stored(stored.id, stored.calendar_id, &stored.href, &stored.etag)
                    .await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn event_detail(&self, id: EventId) -> Result<EventDetail> {
        let stored = self.store.event(id)?;
        let cal = ical::parse(&stored.ics);
        let e = cal
            .events
            .iter()
            .find(|e| e.recurrence_id.is_none())
            .or(cal.events.first())
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Termin leer"))?;
        let first = cal
            .instances(e.start.unix() - 1, i64::MAX / 4)
            .into_iter()
            .next();
        Ok(EventDetail {
            id,
            calendar_id: stored.calendar_id,
            summary: e.summary.clone(),
            location: e.location.clone(),
            description: e.description.clone(),
            all_day: e.all_day(),
            start: first.as_ref().map_or_else(|| e.start.unix(), |i| i.start),
            end: first.as_ref().map_or_else(|| e.start.unix(), |i| i.end),
            recurring: e.rule.is_some(),
            organizer: e.organizer.clone(),
            attendees: e.attendees.clone(),
            invited_by_other: e.organizer.as_ref().is_some_and(|o| {
                !self
                    .own_addresses()
                    .unwrap_or_default()
                    .contains(&o.email.to_lowercase())
            }),
        })
    }

    /// Kalender für Einladungen: erster Kalender des Kontos, sonst lokal.
    fn default_calendar(&self, account: AccountId) -> Result<CalendarInfo> {
        let all = self.store.calendars()?;
        all.iter()
            .find(|c| c.account_id == Some(account))
            .or_else(|| all.iter().find(|c| c.url == LOCAL_URL))
            .cloned()
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Kein Kalender vorhanden"))
    }

    /// Einladung annehmen (eintragen + Zusage senden) oder ablehnen (nur Absage).
    pub async fn respond_invite(&self, message: MessageId, accept: bool) -> Result<()> {
        let detail = self.get_message(message).await?;
        let invite = detail
            .body
            .invite
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Keine Einladung in dieser Mail"))?;
        let account = self.store.account(detail.summary.account_id)?;
        let cal = ical::parse(&invite.ics);
        let event = cal
            .events
            .iter()
            .find(|e| e.recurrence_id.is_none())
            .or(cal.events.first())
            .ok_or_else(|| Error::new(ErrorKind::Invalid, "Einladung ohne Termin"))?;

        if accept {
            let target = self.default_calendar(account.id)?;
            // METHOD entfernen: im Kalender ist es ein normaler Termin.
            let ics: String = invite
                .ics
                .lines()
                .filter(|l| !l.to_uppercase().starts_with("METHOD:"))
                .map(|l| format!("{}\r\n", l.trim_end_matches('\r')))
                .collect();
            let existing = self.store.event_by_uid(&invite.uid)?;
            self.store_ics(
                &target,
                &invite.uid,
                ics,
                existing.map(|e| (e.id, e.href, e.etag, e.calendar_id)),
            )
            .await?;
        } else if let Some(e) = self.store.event_by_uid(&invite.uid)? {
            self.delete_stored(e.id, e.calendar_id, &e.href, &e.etag)
                .await?;
        }

        // Antwort an den Einladenden (nur wenn es einen gibt).
        let Some(organizer) = event.organizer.as_ref().filter(|o| o.email.contains('@')) else {
            return Ok(());
        };
        let reply = ical::write_reply(event, &account.email, &account.display_name, accept, now());
        let name = if account.display_name.is_empty() {
            &account.email
        } else {
            &account.display_name
        };
        let (prefix, verb) = match (accept, current()) {
            (true, Lang::De) => ("Zugesagt", "hat zugesagt"),
            (false, Lang::De) => ("Abgesagt", "hat abgesagt"),
            (true, Lang::En) => ("Accepted", "accepted"),
            (false, Lang::En) => ("Declined", "declined"),
        };
        let built = build_invite_reply(
            &account,
            &organizer.email,
            &format!("{prefix}: {}", event.summary),
            &format!("{name} {verb}: {}", event.summary),
            &reply,
        )?;
        self.store.outbox_push(
            account.id,
            OutboxKind::Send,
            &new_message_id(&account.email),
            &built.raw,
            &built.from,
            &built.to,
            None,
            &format!("{prefix}: {}", event.summary),
        )?;
        Ok(())
    }
}

// ---- Tauri-Commands ----------------------------------------------------------

#[tauri::command]
pub fn list_calendars(state: State<'_, AppState>) -> Result<Vec<CalendarInfo>> {
    state.store.calendars()
}

#[tauri::command]
pub async fn create_calendar(
    app: AppHandle,
    account_id: Option<AccountId>,
    name: String,
    color: String,
) -> Result<CalendarId> {
    let id = app
        .state::<AppState>()
        .create_calendar(account_id, &name, &color)
        .await?;
    let _ = app.emit("calendar-changed", 0);
    Ok(id)
}

#[tauri::command]
pub async fn update_calendar(
    app: AppHandle,
    id: CalendarId,
    name: String,
    color: String,
) -> Result<()> {
    app.state::<AppState>()
        .update_calendar(id, &name, &color)
        .await?;
    let _ = app.emit("calendar-changed", 0);
    Ok(())
}

#[tauri::command]
pub async fn delete_calendar(app: AppHandle, id: CalendarId) -> Result<()> {
    app.state::<AppState>().delete_calendar(id).await?;
    let _ = app.emit("calendar-changed", 0);
    Ok(())
}

/// Konten, auf deren Server neue Kalender angelegt werden können.
#[tauri::command]
pub fn calendar_accounts(state: State<'_, AppState>) -> Result<Vec<AccountId>> {
    let mut out = Vec::new();
    for a in state.store.accounts()? {
        if state
            .store
            .caldav_home(a.id)?
            .0
            .is_some_and(|h| !h.is_empty())
        {
            out.push(a.id);
        }
    }
    Ok(out)
}

/// Ergebnis von „Adresse eintragen“.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkResult {
    pub calendars: usize,
    pub contacts: bool,
}

#[tauri::command]
pub async fn link_account_calendar(
    app: AppHandle,
    account_id: AccountId,
    url: String,
) -> Result<LinkResult> {
    let (calendars, contacts) = app
        .state::<AppState>()
        .link_account_dav(account_id, &url)
        .await?;
    let _ = app.emit("calendar-changed", account_id);
    if contacts {
        let _ = app.emit("contacts-changed", account_id);
    }
    Ok(LinkResult {
        calendars,
        contacts,
    })
}

#[tauri::command]
pub fn list_dav_sources(state: State<'_, AppState>) -> Result<Vec<DavSource>> {
    state.store.dav_sources()
}

#[tauri::command]
pub async fn connect_dav_source(
    app: AppHandle,
    name: String,
    url: String,
    username: String,
    password: String,
) -> Result<i64> {
    let id = app
        .state::<AppState>()
        .connect_source(&name, &url, &username, &password)
        .await?;
    let _ = app.emit("calendar-changed", 0);
    Ok(id)
}

#[tauri::command]
pub fn disconnect_dav_source(app: AppHandle, id: i64) -> Result<()> {
    app.state::<AppState>().disconnect_source(id)?;
    let _ = app.emit("calendar-changed", 0);
    Ok(())
}

#[tauri::command]
pub fn set_calendar_visible(
    state: State<'_, AppState>,
    id: CalendarId,
    visible: bool,
) -> Result<()> {
    state.store.set_calendar_visible(id, visible)
}

#[tauri::command]
pub fn occurrences(state: State<'_, AppState>, from: i64, to: i64) -> Result<Vec<Occurrence>> {
    state.occurrences(from, to)
}

#[tauri::command]
pub fn event_detail(state: State<'_, AppState>, id: EventId) -> Result<EventDetail> {
    state.event_detail(id)
}

#[tauri::command]
pub async fn save_event(app: AppHandle, input: EventInput) -> Result<EventId> {
    let (id, mailed) = app.state::<AppState>().save_event(input).await?;
    if let Some(account) = mailed {
        crate::queue::kick(app.clone(), account);
    }
    let _ = app.emit("calendar-changed", 0);
    Ok(id)
}

#[tauri::command]
pub async fn delete_event(app: AppHandle, id: EventId) -> Result<()> {
    if let Some(account) = app.state::<AppState>().delete_event(id).await? {
        crate::queue::kick(app.clone(), account);
    }
    let _ = app.emit("calendar-changed", 0);
    Ok(())
}

#[tauri::command]
pub async fn respond_invite(app: AppHandle, message_id: MessageId, accept: bool) -> Result<()> {
    let state = app.state::<AppState>();
    state.respond_invite(message_id, accept).await?;
    let account = state.store.message_location(message_id)?.summary.account_id;
    crate::queue::kick(app.clone(), account);
    let _ = app.emit("calendar-changed", account);
    Ok(())
}

/// Gegen den lokalen Testserver: `scripts/test-all.sh`.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod server_tests {
    use super::*;
    use kolibri_core::{
        Address, Body, Flags, FolderRole, Invite, RemoteFolder, RemoteMessage, Security,
        ServerConfig,
    };
    use kolibri_store::{NewAccount, Store};

    #[tokio::test]
    #[ignore = "braucht den Testserver und schreibt in den Schlüsselbund"]
    async fn calendar_end_to_end() {
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

        sync_calendars(&state, &account).await.unwrap();
        let remote = state
            .store
            .calendars()
            .unwrap()
            .into_iter()
            .find(|c| c.account_id == Some(account.id))
            .expect("kein Server-Kalender");

        // Anlegen auf dem Server …
        let start = now() + 86_400;
        let id = state
            .save_event(EventInput {
                id: None,
                calendar_id: remote.id,
                summary: "Kolibri-Termin".into(),
                location: "Köln".into(),
                description: "Notiz".into(),
                all_day: false,
                start,
                end: start + 1800,
                attendees: vec![],
            })
            .await
            .unwrap()
            .0;
        assert!(id > 0);
        // … und in einem frischen Cache wiederfinden.
        let fresh = AppState::new(Store::open_in_memory().unwrap());
        let a2 = fresh
            .store
            .insert_account(&NewAccount {
                email: "test@kolibri.test".into(),
                display_name: "Test".into(),
                imap: cfg(1143),
                smtp: cfg(1587),
            })
            .unwrap();
        assert!(sync_calendars(&fresh, &a2).await.unwrap());
        let occ = fresh.occurrences(start - 10, start + 10).unwrap();
        assert!(
            occ.iter().any(|o| o.instance.summary == "Kolibri-Termin"),
            "{occ:?}"
        );

        // Ändern + löschen über die erste Instanz.
        state
            .save_event(EventInput {
                id: Some(id),
                calendar_id: remote.id,
                summary: "Kolibri-Termin (geändert)".into(),
                location: String::new(),
                description: String::new(),
                all_day: false,
                start,
                end: start + 3600,
                attendees: vec![AttendeeInput {
                    name: "Anna Schäfer".into(),
                    email: "anna@beispiel.de".into(),
                }],
            })
            .await
            .unwrap();
        let ev = state.store.events_in_range(start - 10, start + 10).unwrap();
        assert_eq!(ev.len(), 1, "Ändern darf keinen zweiten Termin erzeugen");

        // Einladung liegt im Postausgang (REQUEST an Anna).
        let mail = state
            .store
            .outbox_next(account.id)
            .unwrap()
            .expect("keine Einladung");
        let raw = String::from_utf8_lossy(&mail.raw).into_owned();
        assert_eq!(mail.envelope_to, vec!["anna@beispiel.de".to_owned()]);
        assert!(raw.contains("method=REQUEST"), "{raw}");
        assert!(mail.subject.starts_with("Geändert: "), "{}", mail.subject);
        state.store.outbox_done(mail.id).unwrap();

        // Anna sagt zu → Status steht am Termin.
        let reply = kolibri_dav::ical::write_reply(
            &main_event(&ical::parse(&state.store.event(ev[0].id).unwrap().ics))
                .unwrap()
                .0,
            "anna@beispiel.de",
            "Anna",
            true,
            now(),
        );
        let invite = kolibri_mail::parse::parse_body(
            format!("Content-Type: text/calendar; method=REPLY\r\n\r\n{reply}").as_bytes(),
        )
        .invite
        .expect("Antwort nicht erkannt");
        assert!(state.apply_itip(&invite).await.unwrap());
        let d = state.event_detail(ev[0].id).unwrap();
        assert_eq!(d.attendees[0].status, "ACCEPTED", "{:?}", d.attendees);
        assert!(!d.invited_by_other);
        let d = state.event_detail(ev[0].id).unwrap();
        assert_eq!(d.summary, "Kolibri-Termin (geändert)");
        assert_eq!(d.end - d.start, 3600);
        let ev = state.store.events_in_range(start - 10, start + 10).unwrap();
        assert_eq!(
            state.delete_event(ev[0].id).await.unwrap(),
            Some(account.id)
        );
        let cancel = state
            .store
            .outbox_next(account.id)
            .unwrap()
            .expect("keine Absage");
        assert!(String::from_utf8_lossy(&cancel.raw).contains("method=CANCEL"));
        state.store.outbox_done(cancel.id).unwrap();
        assert!(
            state
                .occurrences(start - 10, start + 10)
                .unwrap()
                .is_empty()
        );

        // Einladung in einer Mail annehmen → Termin im Kalender + Zusage im Postausgang.
        state
            .store
            .replace_folders(
                account.id,
                &[RemoteFolder {
                    path: "INBOX".into(),
                    delimiter: Some("/".into()),
                    role: Some(FolderRole::Inbox),
                    selectable: true,
                }],
            )
            .unwrap();
        let inbox = state
            .store
            .folder_by_role(account.id, FolderRole::Inbox)
            .unwrap()
            .unwrap()
            .id;
        state
            .store
            .insert_messages(
                inbox,
                &[RemoteMessage {
                    uid: 1,
                    message_id: Some("inv@x".into()),
                    in_reply_to: None,
                    references: None,
                    subject: "Einladung: Planung".into(),
                    from: Address {
                        name: "Orga".into(),
                        addr: "test@kolibri.test".into(),
                    },
                    to: vec![],
                    cc: vec![],
                    reply_to: vec![],
                    date: now(),
                    size: 1,
                    flags: Flags::default(),
                    has_attachments: false,
                }],
            )
            .unwrap();
        let msg = state
            .store
            .list_messages(kolibri_core::MessageScope::Folder { id: inbox }, 0, 1)
            .unwrap()[0]
            .id;
        let uid = format!("einladung-{}", std::process::id());
        let ics = format!(
            "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:Planung\r\n\
DTSTART:{s}\r\nDTEND:{e}\r\nORGANIZER;CN=Orga:mailto:test@kolibri.test\r\n\
ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:test@kolibri.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            s = "20301201T090000Z",
            e = "20301201T100000Z"
        );
        state
            .store
            .save_body(
                msg,
                &Body {
                    html: None,
                    text: Some("Einladung".into()),
                    remote_images: false,
                    attachments: vec![],
                    preview: String::new(),
                    invite: Some(Invite {
                        method: "REQUEST".into(),
                        uid: uid.clone(),
                        summary: "Planung".into(),
                        location: String::new(),
                        start: 0,
                        end: 0,
                        all_day: false,
                        organizer_name: "Orga".into(),
                        organizer_email: "test@kolibri.test".into(),
                        ics,
                    }),
                },
            )
            .unwrap();
        state.respond_invite(msg, true).await.unwrap();
        let stored = state
            .store
            .event_by_uid(&uid)
            .unwrap()
            .expect("Termin nicht eingetragen");
        assert_eq!(stored.calendar_id, remote.id);
        assert!(
            !stored.ics.contains("METHOD:"),
            "METHOD gehört nicht in den Kalender"
        );
        let reply = state
            .store
            .outbox_next(account.id)
            .unwrap()
            .expect("keine Zusage im Postausgang");
        assert!(String::from_utf8_lossy(&reply.raw).contains("PARTSTAT=ACCEPTED"));
        // Zusage wirklich senden.
        let r = crate::compose::drain_outbox(
            &state.store,
            &state.pool,
            &account,
            "kolibri-test",
            |s, e| panic!("{s}: {e}"),
        )
        .await
        .unwrap();
        assert!(matches!(r, crate::compose::OutboxResult::Done { .. }));

        // Absagen entfernt den Termin wieder.
        state.respond_invite(msg, false).await.unwrap();
        assert!(state.store.event_by_uid(&uid).unwrap().is_none());

        state
            .secrets
            .delete_password(&account_key(account.id))
            .unwrap();
    }
}
