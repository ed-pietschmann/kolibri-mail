use serde::{Deserialize, Serialize};

/// Verschlüsselung der Verbindung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Security {
    /// Direkt TLS (IMAP 993, SMTP 465).
    Tls,
    /// Klartext-Start, dann STARTTLS (IMAP 143, SMTP 587).
    StartTls,
    /// Unverschlüsselt – nur für lokale Testserver erlaubt.
    None,
}

impl Security {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tls => "tls",
            Self::StartTls => "starttls",
            Self::None => "none",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "tls" => Some(Self::Tls),
            "starttls" => Some(Self::StartTls),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// Adresse und Anmeldename eines Servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub username: String,
}

impl ServerConfig {
    /// Unverschlüsselt ist nur zu `localhost` erlaubt (Testserver).
    pub fn is_allowed(&self) -> bool {
        self.security != Security::None || is_loopback(&self.host)
    }
}

pub fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_only_on_localhost() {
        let mut cfg = ServerConfig {
            host: "localhost".into(),
            port: 143,
            security: Security::None,
            username: "a".into(),
        };
        assert!(cfg.is_allowed());
        cfg.host = "mail.example.com".into();
        assert!(!cfg.is_allowed());
        cfg.security = Security::StartTls;
        assert!(cfg.is_allowed());
    }

    #[test]
    fn security_roundtrip() {
        for s in [Security::Tls, Security::StartTls, Security::None] {
            assert_eq!(Security::parse(s.as_str()), Some(s));
        }
    }
}
