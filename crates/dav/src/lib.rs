//! CardDAV (RFC 6352): Adressbücher finden und Kontakte abgleichen.
//!
//! Ablauf wie bei Apple Kontakte:
//! 1. `/.well-known/carddav` (RFC 6764) auf der Mail-Domain bzw. dem Server
//! 2. `current-user-principal` → `addressbook-home-set` → Adressbücher
//! 3. Abgleich: ETags vergleichen, Geänderte per `addressbook-multiget` holen

pub mod ical;
pub mod vcard;
pub mod xml;

use std::collections::HashMap;
use std::time::Duration;

use kolibri_core::{Error, ErrorKind, Result};
use reqwest::{Client, Method, StatusCode};

pub use reqwest::Url;
pub use vcard::{Card, Labeled};

const TIMEOUT: Duration = Duration::from_secs(20);
/// So viele Kontakte pro multiget-Anfrage.
const MULTIGET_BATCH: usize = 50;

/// Zugang zu einem DAV-Server.
pub struct Dav {
    client: Client,
    user: String,
    password: String,
}

/// Ein Adressbuch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressBook {
    pub url: String,
    pub name: String,
}

/// Ein abgerufener Kontakt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCard {
    pub href: String,
    pub etag: String,
    pub vcard: String,
}

/// Ergebnis eines Abgleichs.
#[derive(Debug, Default)]
pub struct BookChanges {
    pub changed: Vec<RemoteCard>,
    pub removed: Vec<String>,
}

/// Dateiname aus einer UID (nur sichere Zeichen).
fn safe_name(uid: &str) -> String {
    uid.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn net_err(e: &reqwest::Error) -> Error {
    let kind = if e.is_timeout() {
        ErrorKind::Timeout
    } else {
        ErrorKind::Connect
    };
    Error::new(kind, e.to_string())
}

/// Wo nach CardDAV gesucht wird (Reihenfolge = Priorität).
/// Für den lokalen Testserver (IMAP auf localhost) zusätzlich dessen HTTP-Port.
pub fn candidates(email: &str, imap_host: &str) -> Vec<String> {
    let domain = email
        .rsplit_once('@')
        .map(|(_, d)| d.to_lowercase())
        .unwrap_or_default();
    if is_loopback(imap_host) {
        return vec![format!("http://{imap_host}:18080/.well-known/carddav")];
    }
    let mut hosts = vec![domain.clone(), imap_host.to_lowercase()];
    // imap.purelymail.com → purelymail.com, mail.example.de → example.de
    if let Some((first, rest)) = imap_host.split_once('.')
        && matches!(first, "imap" | "mail" | "imaps" | "mx")
        && rest.contains('.')
    {
        hosts.push(rest.to_lowercase());
    }
    let mut seen = Vec::new();
    for h in hosts {
        if !h.is_empty() && !seen.contains(&h) {
            seen.push(h);
        }
    }
    // Zuerst überall .well-known, danach die Wurzel (manche Server – z. B.
    // Purelymail – nennen den Principal dort, ohne .well-known umzuleiten).
    let mut out: Vec<String> = seen
        .iter()
        .map(|h| format!("https://{h}/.well-known/carddav"))
        .collect();
    // Die Wurzel nur beim Mail-Anbieter (IMAP-Server und dessen Domain), nicht
    // auf der eigenen Mail-Domain – dort steht oft eine fremde Website.
    out.extend(
        seen.iter()
            .filter(|h| **h != domain || imap_host.eq_ignore_ascii_case(h))
            .map(|h| format!("https://{h}/")),
    );
    out
}

impl Dav {
    pub fn new(user: &str, password: &str, allow_http: bool) -> Result<Self> {
        let client = Client::builder()
            .timeout(TIMEOUT)
            .user_agent("Kolibri")
            .redirect(reqwest::redirect::Policy::none())
            .https_only(!allow_http)
            .build()
            .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        Ok(Self {
            client,
            user: user.to_owned(),
            password: password.to_owned(),
        })
    }

    async fn request(
        &self,
        method: &str,
        url: &Url,
        depth: &str,
        body: String,
    ) -> Result<(Url, String)> {
        let method = Method::from_bytes(method.as_bytes())
            .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        // Weiterleitungen selbst verfolgen: reqwest würde aus PROPFIND bei 301/302
        // ein GET machen (dann kommt eine HTML-Seite statt der Antwort zurück).
        let origin = url.clone();
        let mut current = url.clone();
        let mut hops = 0;
        let response = loop {
            let mut req = self
                .client
                .request(method.clone(), current.clone())
                .header("Depth", depth)
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(body.clone());
            if current.host_str() == origin.host_str() {
                req = req.basic_auth(&self.user, Some(&self.password));
            }
            let response = req.send().await.map_err(|e| net_err(&e))?;
            if !response.status().is_redirection() {
                break response;
            }
            let next = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|loc| current.join(loc).ok());
            match next {
                Some(next) if hops < MAX_REDIRECTS && redirect_allowed(&current, &next) => {
                    hops += 1;
                    current = next;
                }
                _ => {
                    return Err(Error::new(
                        ErrorKind::NotFound,
                        format!("{url}: Weiterleitung nicht verfolgbar"),
                    ));
                }
            }
        };
        let status = response.status();
        let final_url = response.url().clone();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(Error::new(ErrorKind::Auth, format!("{url}: {status}")));
        }
        if !(status.is_success() || status.as_u16() == 207) {
            return Err(Error::new(ErrorKind::NotFound, format!("{url}: {status}")));
        }
        let text = response.text().await.map_err(|e| net_err(&e))?;
        Ok((final_url, text))
    }

    async fn propfind(
        &self,
        url: &Url,
        depth: &str,
        props: &str,
    ) -> Result<(Url, Vec<xml::Response>)> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:carddav"><d:prop>{props}</d:prop></d:propfind>"#
        );
        let (final_url, text) = self.request("PROPFIND", url, depth, body).await?;
        Ok((final_url, xml::parse_multistatus(&text)))
    }

    /// Findet das Adressbuch-Verzeichnis (addressbook-home-set) des Nutzers.
    pub async fn discover_home(&self, candidates: &[String]) -> Result<Option<String>> {
        let mut last_err = None;
        let mut auth_err = None;
        for c in candidates {
            let Ok(url) = Url::parse(c) else { continue };
            match self.home_from(&url).await {
                Ok(Some(home)) => return Ok(Some(home)),
                Ok(None) => log::warn!("CardDAV: {c} nennt kein Adressbuch"),
                // Ein Server lehnt ab (z. B. imap.purelymail.com), ein anderer
                // (purelymail.com) ist der richtige – also weitersuchen.
                Err(e) if e.kind == ErrorKind::Auth => {
                    log::warn!("CardDAV: {c} – {e}");
                    auth_err = Some(e);
                }
                Err(e) => {
                    log::warn!("CardDAV: {c} – {e}");
                    last_err = Some(e);
                }
            }
        }
        if let Some(e) = auth_err {
            return Err(e);
        }
        match last_err {
            Some(e) if matches!(e.kind, ErrorKind::Connect | ErrorKind::Timeout) => Err(e),
            _ => Ok(None),
        }
    }

    async fn home_from(&self, start: &Url) -> Result<Option<String>> {
        let (base, r) = self
            .propfind(
                start,
                "0",
                "<d:current-user-principal/><c:addressbook-home-set/>",
            )
            .await?;
        let first = r.first();
        if let Some(home) = first.and_then(|r| r.prop_href("addressbook-home-set")) {
            return Ok(base.join(home).ok().map(String::from));
        }
        let Some(principal) = first.and_then(|r| r.prop_href("current-user-principal")) else {
            return Ok(None);
        };
        let principal = base
            .join(principal)
            .map_err(|e| Error::new(ErrorKind::Protocol, e.to_string()))?;
        let (base, r) = self
            .propfind(&principal, "0", "<c:addressbook-home-set/>")
            .await?;
        Ok(r.first()
            .and_then(|r| r.prop_href("addressbook-home-set"))
            .and_then(|h| base.join(h).ok())
            .map(String::from))
    }

    /// Alle Adressbücher im Verzeichnis.
    pub async fn address_books(&self, home: &str) -> Result<Vec<AddressBook>> {
        let home = Url::parse(home).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let (base, r) = self
            .propfind(&home, "1", "<d:resourcetype/><d:displayname/>")
            .await?;
        Ok(r.iter()
            .filter(|r| r.types.iter().any(|t| t == "addressbook"))
            .filter_map(|r| {
                Some(AddressBook {
                    url: base.join(&r.href).ok()?.to_string(),
                    name: r.prop("displayname").unwrap_or_default().to_owned(),
                })
            })
            .collect())
    }

    /// Abgleich eines Adressbuchs: `known` = href → ETag aus dem Cache.
    pub async fn sync_book(
        &self,
        book: &str,
        known: &HashMap<String, String>,
    ) -> Result<BookChanges> {
        let url = Url::parse(book).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let (base, listing) = self
            .propfind(&url, "1", "<d:getetag/><d:resourcetype/>")
            .await?;
        let mut server: HashMap<String, String> = HashMap::new();
        for r in &listing {
            if r.types.iter().any(|t| t == "collection") {
                continue;
            }
            let Ok(href) = base.join(&r.href) else {
                continue;
            };
            server.insert(
                href.path().to_owned(),
                r.prop("getetag").unwrap_or_default().to_owned(),
            );
        }
        let mut changes = BookChanges {
            removed: known
                .keys()
                .filter(|h| !server.contains_key(*h))
                .cloned()
                .collect(),
            ..BookChanges::default()
        };
        let todo: Vec<&String> = server
            .iter()
            .filter(|(href, etag)| known.get(*href) != Some(etag) || etag.is_empty())
            .map(|(href, _)| href)
            .collect();
        for chunk in todo.chunks(MULTIGET_BATCH) {
            let hrefs: String = chunk
                .iter()
                .map(|h| format!("<d:href>{}</d:href>", xml::escape(h)))
                .collect();
            let body = format!(
                r#"<?xml version="1.0" encoding="utf-8"?><c:addressbook-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:carddav"><d:prop><d:getetag/><c:address-data/></d:prop>{hrefs}</c:addressbook-multiget>"#
            );
            let (base, text) = self.request("REPORT", &url, "1", body).await?;
            for r in xml::parse_multistatus(&text) {
                let (Ok(href), Some(data)) = (base.join(&r.href), r.prop("address-data")) else {
                    continue;
                };
                changes.changed.push(RemoteCard {
                    href: href.path().to_owned(),
                    etag: r.prop("getetag").unwrap_or_default().to_owned(),
                    vcard: data.to_owned(),
                });
            }
        }
        Ok(changes)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Mini-HTTP-Server: 1. Anfrage → 301, 2. → 207 mit Adressbuch-Verzeichnis.
    /// Prüft, dass PROPFIND (nicht GET) samt Anmeldung beim Ziel ankommt.
    #[tokio::test]
    async fn follows_redirects_with_propfind() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            for step in 0..2 {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let reply = if step == 0 {
                    "HTTP/1.1 301 Moved\r\nLocation: /dav/me/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
                } else {
                    let body = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:carddav"><d:response><d:href>/dav/me/</d:href><d:propstat><d:prop><c:addressbook-home-set><d:href>/dav/me/books/</d:href></c:addressbook-home-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
                    format!(
                        "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                sock.write_all(reply.as_bytes()).await.unwrap();
                seen.push(req);
            }
            seen
        });
        let dav = Dav::new("me", "pw", true).unwrap();
        let home = dav
            .discover_home(&[format!("http://localhost:{port}/.well-known/carddav")])
            .await
            .unwrap();
        assert_eq!(
            home.as_deref(),
            Some(format!("http://localhost:{port}/dav/me/books/").as_str())
        );
        let seen = server.await.unwrap();
        assert!(seen[1].starts_with("PROPFIND /dav/me/ "), "{}", seen[1]);
        assert!(
            seen[1].to_lowercase().contains("authorization: basic"),
            "{}",
            seen[1]
        );
    }

    /// Erster Server lehnt ab (401), zweiter kennt das Adressbuch → gefunden.
    #[tokio::test]
    async fn rejected_candidate_does_not_stop_search() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            for step in 0..2 {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let _ = sock.read(&mut buf).await.unwrap();
                let reply = if step == 0 {
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_owned()
                } else {
                    let body = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:carddav"><d:response><d:href>/</d:href><d:propstat><d:prop><c:addressbook-home-set><d:href>/books/</d:href></c:addressbook-home-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
                    format!(
                        "HTTP/1.1 207 Multi-Status\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                sock.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        let dav = Dav::new("me", "pw", true).unwrap();
        let home = dav
            .discover_home(&[
                format!("http://localhost:{port}/.well-known/carddav"),
                format!("http://127.0.0.1:{port}/"),
            ])
            .await
            .unwrap();
        assert_eq!(
            home.as_deref(),
            Some(format!("http://127.0.0.1:{port}/books/").as_str())
        );
    }

    #[test]
    fn parents_and_siblings() {
        let cal = "https://purelymail.com/webdav/123456/caldav/default/";
        assert_eq!(
            parent_collection(cal).as_deref(),
            Some("https://purelymail.com/webdav/123456/caldav/")
        );
        assert_eq!(parent_collection("https://x.de/").as_deref(), None);
        assert_eq!(
            sibling_carddav(cal).as_deref(),
            Some("https://purelymail.com/webdav/123456/carddav/default/")
        );
        assert_eq!(
            sibling_carddav("https://c.de/remote.php/dav/calendars/me/").as_deref(),
            Some("https://c.de/remote.php/dav/addressbooks/me/")
        );
    }

    #[test]
    fn candidate_urls() {
        assert_eq!(
            candidates("me@meine-domain.de", "imap.purelymail.com"),
            [
                "https://meine-domain.de/.well-known/carddav",
                "https://imap.purelymail.com/.well-known/carddav",
                "https://purelymail.com/.well-known/carddav",
                "https://imap.purelymail.com/",
                "https://purelymail.com/",
            ]
        );
        assert_eq!(
            candidates("a@x.de", "localhost"),
            ["http://localhost:18080/.well-known/carddav"]
        );
        assert_eq!(
            candidates("a@posteo.de", "posteo.de"),
            [
                "https://posteo.de/.well-known/carddav",
                "https://posteo.de/"
            ]
        );
    }
}

// ---- CalDAV (RFC 4791) ----------------------------------------------------------

/// Ein Kalender auf dem Server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCalendar {
    pub url: String,
    pub name: String,
    /// „#RRGGBB“ (Apple-Erweiterung), leer wenn unbekannt.
    pub color: String,
}

/// Wo nach CalDAV gesucht wird – wie bei CardDAV.
pub fn calendar_candidates(email: &str, imap_host: &str) -> Vec<String> {
    candidates(email, imap_host)
        .into_iter()
        .map(|c| c.replace("/.well-known/carddav", "/.well-known/caldav"))
        .collect()
}

/// Übergeordnete Sammlung einer DAV-Adresse:
/// `…/caldav/default/` → `…/caldav/`. Für eingetragene Kalender-Adressen,
/// die auf einen einzelnen Kalender statt auf das Verzeichnis zeigen.
pub fn parent_collection(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let path = parsed.path().trim_end_matches('/');
    let cut = path.rfind('/')?;
    if cut == 0 {
        return None;
    }
    parsed
        .join(&format!("{}/", &path[..cut]))
        .ok()
        .map(String::from)
}

/// Zu einer CalDAV-Adresse die passende CardDAV-Adresse raten
/// (Purelymail, Nextcloud & Co. legen beide nebeneinander ab).
pub fn sibling_carddav(url: &str) -> Option<String> {
    for (from, to) in [("/caldav/", "/carddav/"), ("/calendars/", "/addressbooks/")] {
        if url.contains(from) {
            return Some(url.replacen(from, to, 1));
        }
    }
    None
}

/// Höchstens so viele Weiterleitungen folgen.
const MAX_REDIRECTS: usize = 5;

/// Darf einer Weiterleitung von `from` nach `to` gefolgt werden? Nie von HTTPS
/// auf HTTP (außer localhost), Zugangsdaten nur an denselben Server.
fn redirect_allowed(from: &Url, to: &Url) -> bool {
    match to.scheme() {
        "https" => true,
        "http" => from.scheme() == "http" || to.host_str().is_some_and(is_loopback),
        _ => false,
    }
}

impl Dav {
    async fn propfind_ns(
        &self,
        url: &Url,
        depth: &str,
        props: &str,
    ) -> Result<(Url, Vec<xml::Response>)> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/"><d:prop>{props}</d:prop></d:propfind>"#
        );
        let (final_url, text) = self.request("PROPFIND", url, depth, body).await?;
        Ok((final_url, xml::parse_multistatus(&text)))
    }

    /// Findet das Kalender-Verzeichnis (calendar-home-set).
    pub async fn discover_calendar_home(&self, candidates: &[String]) -> Result<Option<String>> {
        let mut last_err = None;
        let mut auth_err = None;
        for c in candidates {
            let Ok(url) = Url::parse(c) else { continue };
            let attempt = async {
                let (base, r) = self
                    .propfind_ns(
                        &url,
                        "0",
                        "<d:current-user-principal/><c:calendar-home-set/>",
                    )
                    .await?;
                let first = r.first();
                if let Some(home) = first.and_then(|r| r.prop_href("calendar-home-set")) {
                    return Ok(base.join(home).ok().map(String::from));
                }
                let Some(principal) = first.and_then(|r| r.prop_href("current-user-principal"))
                else {
                    return Ok(None);
                };
                let principal = base
                    .join(principal)
                    .map_err(|e| Error::new(ErrorKind::Protocol, e.to_string()))?;
                let (base, r) = self
                    .propfind_ns(&principal, "0", "<c:calendar-home-set/>")
                    .await?;
                Ok::<_, Error>(
                    r.first()
                        .and_then(|r| r.prop_href("calendar-home-set"))
                        .and_then(|h| base.join(h).ok())
                        .map(String::from),
                )
            };
            match attempt.await {
                Ok(Some(home)) => return Ok(Some(home)),
                Ok(None) => log::warn!("CalDAV: {c} nennt keinen Kalender"),
                Err(e) if e.kind == ErrorKind::Auth => {
                    log::warn!("CalDAV: {c} – {e}");
                    auth_err = Some(e);
                }
                Err(e) => {
                    log::warn!("CalDAV: {c} – {e}");
                    last_err = Some(e);
                }
            }
        }
        if let Some(e) = auth_err {
            return Err(e);
        }
        match last_err {
            Some(e) if matches!(e.kind, ErrorKind::Connect | ErrorKind::Timeout) => Err(e),
            _ => Ok(None),
        }
    }

    /// Kalender-Verzeichnis zu einer eingetragenen Adresse: Principal/Home
    /// (per Abfrage), sonst das Verzeichnis über einem einzelnen Kalender,
    /// sonst die Adresse selbst.
    pub async fn calendar_home_for(&self, url: &str) -> Result<Option<String>> {
        if let Some(home) = self.discover_calendar_home(&[url.to_owned()]).await? {
            return Ok(Some(home));
        }
        for candidate in [parent_collection(url), Some(url.to_owned())]
            .into_iter()
            .flatten()
        {
            if let Ok(list) = self.calendars(&candidate).await
                && !list.is_empty()
            {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    /// Wie `calendar_home_for`, für Adressbücher.
    pub async fn address_home_for(&self, url: &str) -> Result<Option<String>> {
        if let Some(home) = self.discover_home(&[url.to_owned()]).await? {
            return Ok(Some(home));
        }
        for candidate in [parent_collection(url), Some(url.to_owned())]
            .into_iter()
            .flatten()
        {
            if let Ok(list) = self.address_books(&candidate).await
                && !list.is_empty()
            {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    /// Kalender mit Terminen (VEVENT) im Verzeichnis.
    pub async fn calendars(&self, home: &str) -> Result<Vec<RemoteCalendar>> {
        let home = Url::parse(home).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let (base, r) = self
            .propfind_ns(
                &home,
                "1",
                "<d:resourcetype/><d:displayname/><a:calendar-color/>",
            )
            .await?;
        Ok(r.iter()
            .filter(|r| r.types.iter().any(|t| t == "calendar"))
            .filter_map(|r| {
                let color = r.prop("calendar-color").unwrap_or_default();
                Some(RemoteCalendar {
                    url: base.join(&r.href).ok()?.to_string(),
                    name: r.prop("displayname").unwrap_or_default().to_owned(),
                    // „#RRGGBBAA“ → „#RRGGBB“
                    color: color
                        .get(..7)
                        .filter(|c| c.starts_with('#'))
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect())
    }

    /// Abgleich eines Kalenders (wie `sync_book`, mit calendar-multiget).
    pub async fn sync_calendar(
        &self,
        calendar: &str,
        known: &HashMap<String, String>,
    ) -> Result<BookChanges> {
        let url =
            Url::parse(calendar).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let (base, listing) = self
            .propfind_ns(&url, "1", "<d:getetag/><d:resourcetype/>")
            .await?;
        let mut server: HashMap<String, String> = HashMap::new();
        for r in &listing {
            if r.types.iter().any(|t| t == "collection") {
                continue;
            }
            let Ok(href) = base.join(&r.href) else {
                continue;
            };
            server.insert(
                href.path().to_owned(),
                r.prop("getetag").unwrap_or_default().to_owned(),
            );
        }
        let mut changes = BookChanges {
            removed: known
                .keys()
                .filter(|h| !server.contains_key(*h))
                .cloned()
                .collect(),
            ..BookChanges::default()
        };
        let todo: Vec<&String> = server
            .iter()
            .filter(|(href, etag)| known.get(*href) != Some(etag) || etag.is_empty())
            .map(|(href, _)| href)
            .collect();
        for chunk in todo.chunks(MULTIGET_BATCH) {
            let hrefs: String = chunk
                .iter()
                .map(|h| format!("<d:href>{}</d:href>", xml::escape(h)))
                .collect();
            let body = format!(
                r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop>{hrefs}</c:calendar-multiget>"#
            );
            let (base, text) = self.request("REPORT", &url, "1", body).await?;
            for r in xml::parse_multistatus(&text) {
                let (Ok(href), Some(data)) = (base.join(&r.href), r.prop("calendar-data")) else {
                    continue;
                };
                changes.changed.push(RemoteCard {
                    href: href.path().to_owned(),
                    etag: r.prop("getetag").unwrap_or_default().to_owned(),
                    vcard: data.to_owned(),
                });
            }
        }
        Ok(changes)
    }

    /// Legt einen Termin an oder ersetzt ihn. `etag`: None = neu (darf nicht
    /// existieren), Some = nur ersetzen, wenn unverändert. Liefert den Pfad.
    pub async fn put_event(
        &self,
        calendar: &str,
        uid: &str,
        ics: &str,
        etag: Option<&str>,
    ) -> Result<String> {
        let base =
            Url::parse(calendar).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let name: String = uid
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let url = base
            .join(&format!("{name}.ics"))
            .map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        self.put_at(&url, ics, etag, "text/calendar").await?;
        Ok(url.path().to_owned())
    }

    /// Neuer Kontakt im Adressbuch. Liefert den Pfad.
    pub async fn put_card(&self, book: &str, uid: &str, vcard: &str) -> Result<String> {
        let base = Url::parse(book).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let url = base
            .join(&format!("{}.vcf", safe_name(uid)))
            .map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        self.put_at(&url, vcard, None, "text/vcard").await?;
        Ok(url.path().to_owned())
    }

    /// Bekannten Kontakt ersetzen.
    pub async fn replace_card(
        &self,
        book: &str,
        href: &str,
        vcard: &str,
        etag: &str,
    ) -> Result<()> {
        let url = Url::parse(book)
            .and_then(|b| b.join(href))
            .map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        self.put_at(&url, vcard, Some(etag), "text/vcard").await
    }

    /// Neuen Kalender im Kalender-Verzeichnis anlegen (MKCALENDAR, RFC 4791).
    pub async fn make_calendar(&self, home: &str, name: &str, color: &str) -> Result<String> {
        let base = Url::parse(home).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let slug = format!(
            "kolibri-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis())
        );
        let url = base
            .join(&format!("{slug}/"))
            .map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><c:mkcalendar xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/"><d:set><d:prop><d:displayname>{}</d:displayname><a:calendar-color>{}</a:calendar-color><c:supported-calendar-component-set><c:comp name="VEVENT"/></c:supported-calendar-component-set></d:prop></d:set></c:mkcalendar>"#,
            xml_escape(name),
            xml_escape(color)
        );
        let status = self
            .client
            .request(
                Method::from_bytes(b"MKCALENDAR")
                    .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?,
                url.clone(),
            )
            .basic_auth(&self.user, Some(&self.password))
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body)
            .send()
            .await
            .map_err(|e| net_err(&e))?
            .status();
        match status.as_u16() {
            200..=299 => Ok(url.to_string()),
            401 | 403 => Err(Error::new(
                ErrorKind::Auth,
                "Der Server erlaubt das Anlegen von Kalendern nicht.",
            )),
            _ => Err(Error::new(
                ErrorKind::Protocol,
                format!("Kalender anlegen: {status}"),
            )),
        }
    }

    /// Name und Farbe eines Kalenders ändern (PROPPATCH).
    pub async fn update_calendar(&self, url: &str, name: &str, color: &str) -> Result<()> {
        let url = Url::parse(url).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:propertyupdate xmlns:d="DAV:" xmlns:a="http://apple.com/ns/ical/"><d:set><d:prop><d:displayname>{}</d:displayname><a:calendar-color>{}</a:calendar-color></d:prop></d:set></d:propertyupdate>"#,
            xml_escape(name),
            xml_escape(color)
        );
        self.request("PROPPATCH", &url, "0", body).await.map(|_| ())
    }

    /// Ganze Sammlung (Kalender) löschen.
    pub async fn delete_collection(&self, url: &str) -> Result<()> {
        let url = Url::parse(url).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let status = self
            .client
            .delete(url.clone())
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await
            .map_err(|e| net_err(&e))?
            .status();
        match status.as_u16() {
            200..=299 | 404 => Ok(()),
            401 | 403 => Err(Error::new(ErrorKind::Auth, format!("{url}: {status}"))),
            _ => Err(Error::new(ErrorKind::Protocol, format!("{url}: {status}"))),
        }
    }

    /// Ersetzt einen bekannten Termin unter seinem Pfad.
    pub async fn replace_event(
        &self,
        calendar: &str,
        href: &str,
        ics: &str,
        etag: &str,
    ) -> Result<()> {
        let base =
            Url::parse(calendar).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let url = base
            .join(href)
            .map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        self.put_at(&url, ics, Some(etag), "text/calendar").await
    }

    async fn put_at(&self, url: &Url, ics: &str, etag: Option<&str>, mime: &str) -> Result<()> {
        let mut req = self
            .client
            .put(url.clone())
            .basic_auth(&self.user, Some(&self.password))
            .header("Content-Type", format!("{mime}; charset=utf-8"))
            .body(ics.to_owned());
        req = match etag {
            None => req.header("If-None-Match", "*"),
            Some(e) if !e.is_empty() => req.header("If-Match", e),
            Some(_) => req,
        };
        let status = req.send().await.map_err(|e| net_err(&e))?.status();
        match status.as_u16() {
            200..=299 => Ok(()),
            401 | 403 => Err(Error::new(ErrorKind::Auth, format!("{url}: {status}"))),
            412 => Err(Error::new(
                ErrorKind::Duplicate,
                "Wurde inzwischen anderswo geändert – bitte erneut versuchen.",
            )),
            _ => Err(Error::new(ErrorKind::Protocol, format!("{url}: {status}"))),
        }
    }

    /// Löscht einen Termin (nur wenn unverändert, falls `etag` bekannt).
    pub async fn delete_event(&self, calendar: &str, href: &str, etag: &str) -> Result<()> {
        let base =
            Url::parse(calendar).map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let url = base
            .join(href)
            .map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
        let mut req = self
            .client
            .delete(url.clone())
            .basic_auth(&self.user, Some(&self.password));
        if !etag.is_empty() {
            req = req.header("If-Match", etag);
        }
        let status = req.send().await.map_err(|e| net_err(&e))?.status();
        match status.as_u16() {
            200..=299 | 404 => Ok(()),
            401 | 403 => Err(Error::new(ErrorKind::Auth, format!("{url}: {status}"))),
            412 => Err(Error::new(
                ErrorKind::Duplicate,
                "Termin wurde inzwischen anderswo geändert",
            )),
            _ => Err(Error::new(ErrorKind::Protocol, format!("{url}: {status}"))),
        }
    }
}
