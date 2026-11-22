//! Automatische Servererkennung aus der E-Mail-Adresse.
//!
//! Reihenfolge (wie Thunderbird):
//! 1. Autoconfig des eigenen Anbieters (`autoconfig.<domain>`, `.well-known`)
//! 2. Mozilla-ISPDB (`autoconfig.thunderbird.net`) – es wird nur die Domain gesendet
//! 3. Eingebaute Liste bekannter Anbieter
//! 4. MX-Eintrag → Anbieter dahinter (Liste, ISPDB) – für eigene Domains
//! 5. DNS-SRV (RFC 6186 / 8314)
//! 6. Raten (`imap.<domain>`, `smtp.<domain>`), nur wenn die Ports antworten
//!
//! Datenschutz: Die vollständige E-Mail-Adresse verlässt den Rechner nie.

mod dns;
mod known;
mod xml;

use std::time::Duration;

use kolibri_core::{Error, ErrorKind, Result, Security, ServerConfig};
use serde::Serialize;

const HTTP_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_XML_BYTES: usize = 256 * 1024;
const ISPDB: &str = "https://autoconfig.thunderbird.net/v1.1/";

/// Zerlegte E-Mail-Adresse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email {
    pub address: String,
    pub local: String,
    pub domain: String,
}

impl Email {
    pub fn parse(input: &str) -> Option<Self> {
        let address = input.trim().to_owned();
        let (local, domain) = address.rsplit_once('@')?;
        let domain = domain.to_ascii_lowercase();
        let valid_domain = domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && domain
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if local.is_empty() || !valid_domain || address.chars().any(char::is_whitespace) {
            return None;
        }
        Some(Self {
            local: local.to_owned(),
            address: format!("{local}@{domain}"),
            domain,
        })
    }

    /// Ersetzt die Autoconfig-Platzhalter.
    pub fn expand(&self, template: &str) -> String {
        template
            .replace("%EMAILADDRESS%", &self.address)
            .replace("%EMAILLOCALPART%", &self.local)
            .replace("%EMAILDOMAIN%", &self.domain)
    }
}

/// Woher die Einstellungen stammen (wird in der Oberfläche angezeigt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Provider,
    Ispdb,
    Known,
    Mx,
    Srv,
    Guess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovered {
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    pub source: Source,
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent("Kolibri")
        // Nur HTTPS, auch bei Weiterleitungen.
        .https_only(true)
        .build()
        .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
}

async fn fetch_xml(
    client: &reqwest::Client,
    url: &str,
    email: &Email,
) -> Option<(ServerConfig, ServerConfig)> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_XML_BYTES as u64)
    {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    if bytes.len() > MAX_XML_BYTES {
        return None;
    }
    xml::parse(std::str::from_utf8(&bytes).ok()?, email)
}

async fn from_provider(
    client: &reqwest::Client,
    email: &Email,
) -> Option<(ServerConfig, ServerConfig)> {
    let d = &email.domain;
    let primary = format!("https://autoconfig.{d}/mail/config-v1.1.xml");
    let well_known = format!("https://{d}/.well-known/autoconfig/mail/config-v1.1.xml");
    let (a, b) = tokio::join!(
        fetch_xml(client, &primary, email),
        fetch_xml(client, &well_known, email)
    );
    a.or(b)
}

async fn from_ispdb(
    client: &reqwest::Client,
    domain: &str,
    email: &Email,
) -> Option<(ServerConfig, ServerConfig)> {
    fetch_xml(client, &format!("{ISPDB}{domain}"), email).await
}

/// Sucht die Servereinstellungen für eine Adresse.
pub async fn discover(address: &str) -> Result<Discovered> {
    let email = Email::parse(address)
        .ok_or_else(|| Error::new(ErrorKind::Invalid, "Ungültige E-Mail-Adresse"))?;
    let client = http_client()?;
    let found =
        |(imap, smtp): (ServerConfig, ServerConfig), source| Discovered { imap, smtp, source };

    if let Some(known) = known::lookup(&email.domain, &email) {
        return Ok(found(known, Source::Known));
    }

    // Alles Unabhängige parallel starten.
    let (provider, ispdb, mx, srv) = tokio::join!(
        from_provider(&client, &email),
        from_ispdb(&client, &email.domain, &email),
        dns::mx_base_domain(&email.domain),
        dns::srv(&email),
    );
    if let Some(cfg) = provider {
        return Ok(found(cfg, Source::Provider));
    }
    if let Some(cfg) = ispdb {
        return Ok(found(cfg, Source::Ispdb));
    }
    if let Some(mx_domain) = mx.filter(|m| *m != email.domain) {
        log::info!("Autoconfig: MX zeigt auf {mx_domain}");
        if let Some(cfg) = known::lookup(&mx_domain, &email) {
            return Ok(found(cfg, Source::Mx));
        }
        if let Some(cfg) = from_ispdb(&client, &mx_domain, &email).await {
            return Ok(found(cfg, Source::Mx));
        }
    }
    if let Some(cfg) = srv {
        return Ok(found(cfg, Source::Srv));
    }
    if let Some(cfg) = guess(&email).await {
        return Ok(found(cfg, Source::Guess));
    }
    Err(Error::new(
        ErrorKind::NotFound,
        format!("Keine Servereinstellungen für {} gefunden", email.domain),
    ))
}

/// Letzter Versuch: übliche Hostnamen, nur wenn die Ports wirklich antworten.
async fn guess(email: &Email) -> Option<(ServerConfig, ServerConfig)> {
    let imap_host = format!("imap.{}", email.domain);
    let smtp_host = format!("smtp.{}", email.domain);
    let reachable = |host: String, port: u16| async move {
        tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpStream::connect((host.as_str(), port)),
        )
        .await
        .is_ok_and(|r| r.is_ok())
    };
    let (imap_ok, smtps_ok, submission_ok) = tokio::join!(
        reachable(imap_host.clone(), 993),
        reachable(smtp_host.clone(), 465),
        reachable(smtp_host.clone(), 587),
    );
    if !imap_ok || !(smtps_ok || submission_ok) {
        return None;
    }
    let (port, security) = if smtps_ok {
        (465, Security::Tls)
    } else {
        (587, Security::StartTls)
    };
    Some((
        ServerConfig {
            host: imap_host,
            port: 993,
            security: Security::Tls,
            username: email.address.clone(),
        },
        ServerConfig {
            host: smtp_host,
            port,
            security,
            username: email.address.clone(),
        },
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_emails() {
        let e = Email::parse("  Eddie@Example.COM ").unwrap();
        assert_eq!(e.local, "Eddie");
        assert_eq!(e.domain, "example.com");
        assert_eq!(e.address, "Eddie@example.com");
        assert!(Email::parse("kein-at").is_none());
        assert!(Email::parse("a@localhost").is_none());
        assert!(Email::parse("a b@c.de").is_none());
        assert!(Email::parse("@c.de").is_none());
    }

    #[test]
    fn expands_placeholders() {
        let e = Email::parse("eddie@example.com").unwrap();
        assert_eq!(
            e.expand("%EMAILLOCALPART%@mx.%EMAILDOMAIN%"),
            "eddie@mx.example.com"
        );
    }

    /// Echter Netzwerktest – nur auf Wunsch: `cargo test -p kolibri-mail -- --ignored`
    #[tokio::test]
    #[ignore = "braucht Internet"]
    async fn discovers_posteo_via_ispdb() {
        let d = discover("test@posteo.de").await.unwrap();
        assert_eq!(d.imap.host, "posteo.de");
    }
}
