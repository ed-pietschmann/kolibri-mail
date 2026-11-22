//! Eingebaute Einstellungen für Anbieter, die (noch) nicht in der ISPDB stehen.
//! Schlüssel ist die Anbieter-Domain – bei eigenen Domains über den MX-Eintrag.

use kolibri_core::{Security, ServerConfig};

use super::Email;

struct Known {
    domain: &'static str,
    imap: (&'static str, u16, Security),
    smtp: (&'static str, u16, Security),
}

const KNOWN: &[Known] = &[Known {
    // https://purelymail.com/docs/clients – Anmeldename ist die volle Adresse
    domain: "purelymail.com",
    imap: ("imap.purelymail.com", 993, Security::Tls),
    smtp: ("smtp.purelymail.com", 465, Security::Tls),
}];

pub fn lookup(domain: &str, email: &Email) -> Option<(ServerConfig, ServerConfig)> {
    let k = KNOWN.iter().find(|k| k.domain == domain)?;
    let cfg = |(host, port, security): (&str, u16, Security)| ServerConfig {
        host: host.to_owned(),
        port,
        security,
        username: email.address.clone(),
    };
    Some((cfg(k.imap), cfg(k.smtp)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn purelymail_custom_domain_via_mx() {
        let email = Email::parse("me@meine-domain.de").unwrap();
        let (imap, smtp) = lookup("purelymail.com", &email).unwrap();
        assert_eq!(imap.host, "imap.purelymail.com");
        assert_eq!(imap.username, "me@meine-domain.de");
        assert_eq!(smtp.port, 465);
        assert!(lookup("example.com", &email).is_none());
    }
}
