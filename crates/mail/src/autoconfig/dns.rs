//! DNS-Abfragen für die Servererkennung (MX und SRV).

use std::time::Duration;

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::{RData, RecordType};
use kolibri_core::{Security, ServerConfig};

use super::Email;

const DNS_TIMEOUT: Duration = Duration::from_secs(5);

fn resolver() -> Option<TokioResolver> {
    TokioResolver::builder_tokio().ok()?.build().ok()
}

fn host_of(name: &hickory_resolver::proto::rr::Name) -> String {
    name.to_utf8().trim_end_matches('.').to_ascii_lowercase()
}

/// Registrierbare Domain eines Hostnamens (ohne Public-Suffix-Liste,
/// mit den häufigsten zweistufigen Endungen).
pub fn base_domain(host: &str) -> String {
    const TWO_LEVEL: &[&str] = &[
        "co.uk", "org.uk", "com.au", "co.jp", "co.nz", "com.br", "co.at",
    ];
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    let keep = if labels.len() >= 3 && TWO_LEVEL.iter().any(|s| host.ends_with(&format!(".{s}"))) {
        3
    } else {
        2
    };
    labels[labels.len().saturating_sub(keep)..].join(".")
}

/// MX mit der höchsten Priorität → dessen Basis-Domain (z. B. `purelymail.com`).
pub async fn mx_base_domain(domain: &str) -> Option<String> {
    let resolver = resolver()?;
    let lookup = tokio::time::timeout(DNS_TIMEOUT, resolver.lookup(domain, RecordType::MX))
        .await
        .ok()?
        .ok()?;
    lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::MX(mx) => Some((mx.preference, host_of(&mx.exchange))),
            _ => None,
        })
        .min_by_key(|(pref, _)| *pref)
        .map(|(_, host)| base_domain(&host))
}

async fn srv_record(resolver: &TokioResolver, name: String) -> Option<(String, u16)> {
    let lookup = tokio::time::timeout(DNS_TIMEOUT, resolver.lookup(name, RecordType::SRV))
        .await
        .ok()?
        .ok()?;
    lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::SRV(srv) => Some((srv.priority, host_of(&srv.target), srv.port)),
            _ => None,
        })
        // Ziel "." bedeutet laut RFC 2782: Dienst nicht angeboten.
        .filter(|(_, host, port)| !host.is_empty() && *port != 0)
        .min_by_key(|(prio, _, _)| *prio)
        .map(|(_, host, port)| (host, port))
}

/// RFC 6186/8314: `_imaps._tcp`, `_imap._tcp`, `_submissions._tcp`, `_submission._tcp`.
pub async fn srv(email: &Email) -> Option<(ServerConfig, ServerConfig)> {
    let resolver = resolver()?;
    let d = &email.domain;
    let (imaps, imap, subs, sub) = tokio::join!(
        srv_record(&resolver, format!("_imaps._tcp.{d}")),
        srv_record(&resolver, format!("_imap._tcp.{d}")),
        srv_record(&resolver, format!("_submissions._tcp.{d}")),
        srv_record(&resolver, format!("_submission._tcp.{d}")),
    );
    let cfg = |(host, port): (String, u16), security| ServerConfig {
        host,
        port,
        security,
        username: email.address.clone(),
    };
    let imap = imaps
        .map(|r| cfg(r, Security::Tls))
        .or_else(|| imap.map(|r| cfg(r, Security::StartTls)))?;
    let smtp = subs
        .map(|r| cfg(r, Security::Tls))
        .or_else(|| sub.map(|r| cfg(r, Security::StartTls)))?;
    Some((imap, smtp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_domains() {
        assert_eq!(base_domain("mailserver.purelymail.com."), "purelymail.com");
        assert_eq!(base_domain("mx01.emig.gmx.net"), "gmx.net");
        assert_eq!(base_domain("mx.mail.example.co.uk"), "example.co.uk");
        assert_eq!(base_domain("posteo.de"), "posteo.de");
    }
}
