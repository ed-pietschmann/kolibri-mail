//! Parser für das Thunderbird-Autoconfig-Format (config-v1.1.xml).

use kolibri_core::{Security, ServerConfig};
use quick_xml::Reader;
use quick_xml::events::Event;

use super::Email;

#[derive(Debug, Default)]
struct RawServer {
    kind: String,
    hostname: String,
    port: String,
    socket_type: String,
    username: String,
    authentication: Vec<String>,
}

impl RawServer {
    /// Nur Passwort-Anmeldung (kein OAuth) und nur verschlüsselt.
    fn to_config(&self, email: &Email) -> Option<ServerConfig> {
        let security = match self.socket_type.to_ascii_uppercase().as_str() {
            "SSL" | "TLS" => Security::Tls,
            "STARTTLS" => Security::StartTls,
            _ => return None,
        };
        let password_ok = self.authentication.is_empty()
            || self.authentication.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "password-cleartext" | "plain" | "password-encrypted" | "secure"
                )
            });
        if !password_ok || self.hostname.is_empty() {
            return None;
        }
        Some(ServerConfig {
            host: email.expand(&self.hostname),
            port: self.port.trim().parse().ok()?,
            security,
            username: if self.username.is_empty() {
                email.address.clone()
            } else {
                email.expand(&self.username)
            },
        })
    }
}

/// Liefert (IMAP, SMTP) – jeweils die erste brauchbare Variante.
/// TLS wird gegenüber STARTTLS bevorzugt.
pub fn parse(xml: &str, email: &Email) -> Option<(ServerConfig, ServerConfig)> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut servers: Vec<RawServer> = Vec::new();
    let mut current: Option<RawServer> = None;
    let mut field: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name().as_ref().to_owned();
                match name.as_str() {
                    "incomingServer" | "outgoingServer" => {
                        let kind = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.local_name().as_ref() == "type")
                            .map(|a| a.value.into_owned())
                            .unwrap_or_default();
                        current = Some(RawServer {
                            kind,
                            ..RawServer::default()
                        });
                    }
                    _ => field = Some(name),
                }
            }
            Ok(Event::Text(t)) => {
                if let (Some(server), Some(f)) = (current.as_mut(), field.as_deref()) {
                    let text = t.into_inner().trim().to_owned();
                    match f {
                        "hostname" => server.hostname = text,
                        "port" => server.port = text,
                        "socketType" => server.socket_type = text,
                        "username" => server.username = text,
                        "authentication" => server.authentication.push(text),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.local_name();
                if matches!(name.as_ref(), "incomingServer" | "outgoingServer") {
                    if let Some(s) = current.take() {
                        servers.push(s);
                    }
                } else {
                    field = None;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let pick = |kind: &str| -> Option<ServerConfig> {
        let mut candidates: Vec<ServerConfig> = servers
            .iter()
            .filter(|s| s.kind == kind)
            .filter_map(|s| s.to_config(email))
            .collect();
        // stabile Sortierung: TLS vor STARTTLS, sonst Reihenfolge der Datei
        candidates.sort_by_key(|c| u8::from(c.security != Security::Tls));
        candidates.into_iter().next()
    };
    Some((pick("imap")?, pick("smtp")?))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<clientConfig version="1.1">
  <emailProvider id="example.com">
    <domain>example.com</domain>
    <incomingServer type="pop3">
      <hostname>pop.example.com</hostname><port>995</port>
      <socketType>SSL</socketType><username>%EMAILADDRESS%</username>
      <authentication>password-cleartext</authentication>
    </incomingServer>
    <incomingServer type="imap">
      <hostname>imap.example.com</hostname><port>143</port>
      <socketType>STARTTLS</socketType><username>%EMAILLOCALPART%</username>
      <authentication>password-cleartext</authentication>
    </incomingServer>
    <incomingServer type="imap">
      <hostname>imap.%EMAILDOMAIN%</hostname><port>993</port>
      <socketType>SSL</socketType><username>%EMAILADDRESS%</username>
      <authentication>OAuth2</authentication>
      <authentication>password-cleartext</authentication>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.example.com</hostname><port>587</port>
      <socketType>STARTTLS</socketType><username>%EMAILADDRESS%</username>
      <authentication>password-cleartext</authentication>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

    #[test]
    fn picks_tls_imap_and_expands_placeholders() {
        let email = Email::parse("eddie@example.com").unwrap();
        let (imap, smtp) = parse(SAMPLE, &email).unwrap();
        assert_eq!(imap.host, "imap.example.com");
        assert_eq!(imap.port, 993);
        assert_eq!(imap.security, Security::Tls);
        assert_eq!(imap.username, "eddie@example.com");
        assert_eq!(smtp.port, 587);
        assert_eq!(smtp.security, Security::StartTls);
    }

    #[test]
    fn rejects_oauth_only_and_plaintext() {
        let xml = r#"<clientConfig><emailProvider>
            <incomingServer type="imap"><hostname>a</hostname><port>993</port>
              <socketType>SSL</socketType><authentication>OAuth2</authentication></incomingServer>
            <outgoingServer type="smtp"><hostname>b</hostname><port>25</port>
              <socketType>plain</socketType></outgoingServer>
        </emailProvider></clientConfig>"#;
        let email = Email::parse("a@b.de").unwrap();
        assert!(parse(xml, &email).is_none());
    }

    #[test]
    fn garbage_returns_none() {
        let email = Email::parse("a@b.de").unwrap();
        assert!(parse("<html>404</html>", &email).is_none());
    }
}
