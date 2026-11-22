use serde::Serialize;

/// Grobe Fehlerart. Das Frontend übersetzt sie in eine verständliche Meldung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// Server nicht erreichbar (DNS, Verbindung abgelehnt, kein Netz).
    Connect,
    /// Zeitüberschreitung.
    Timeout,
    /// TLS-Fehler, z. B. ungültiges Zertifikat.
    Tls,
    /// Benutzername oder Passwort falsch.
    Auth,
    /// Server antwortet unerwartet.
    Protocol,
    /// Keine Servereinstellungen gefunden.
    NotFound,
    /// Konto existiert bereits.
    Duplicate,
    /// Ungültige Eingabe.
    Invalid,
    /// Schlüsselbund-Fehler.
    Keychain,
    /// Lokale Datenbank.
    Storage,
    /// Sonstiges.
    Internal,
}

/// Fehler mit Art und technischem Detail.
#[derive(Debug, Clone, thiserror::Error, Serialize)]
#[error("{kind:?}: {detail}")]
pub struct Error {
    pub kind: ErrorKind,
    pub detail: String,
}

impl Error {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            // In der Sprache der Oberfläche (Quelltexte sind deutsch).
            detail: crate::lang::translate(&detail.into()),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
