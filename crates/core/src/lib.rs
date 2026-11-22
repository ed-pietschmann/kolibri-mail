//! Kolibri – Domänentypen und Provider-Traits.
//!
//! Diese Crate hat keine I/O-Abhängigkeiten. Andere Crates (IMAP, später
//! OAuth/JMAP/Exchange) implementieren die hier definierten Traits.

mod compose;
mod error;
mod folder;
mod message;
mod server;

pub use compose::{ComposeAttachment, ComposeDraft, ComposeKind, MAX_ATTACHMENTS_BYTES};
pub use error::{Error, ErrorKind, Result};
pub use folder::{FolderRole, RemoteFolder};
pub use message::{
    Action, Address, Attachment, Body, Flags, Invite, MessageDetail, MessageId, MessageScope,
    MessageSummary, PendingOp, RemoteMessage, UserAction,
};
pub use server::{Security, ServerConfig, is_loopback};

use serde::{Deserialize, Serialize};

/// Datenbank-ID eines Kontos.
pub type AccountId = i64;
/// Datenbank-ID eines Ordners.
pub type FolderId = i64;

/// Ein eingerichtetes Mail-Konto (ohne Passwort – das liegt im Schlüsselbund).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: AccountId,
    pub email: String,
    /// Absendername, z. B. „Erika Mustermann“.
    pub display_name: String,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    /// Signatur (Klartext), wird beim Verfassen eingefügt.
    #[serde(default)]
    pub signature: String,
}

/// Ein im Cache gespeicherter Ordner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: FolderId,
    pub account_id: AccountId,
    /// Voller IMAP-Pfad, bereits aus modifiziertem UTF-7 dekodiert.
    pub path: String,
    /// Letzter Pfadteil zur Anzeige.
    pub name: String,
    pub delimiter: Option<String>,
    pub role: Option<FolderRole>,
    /// Nicht auswählbar (reiner Container, `\Noselect`).
    pub selectable: bool,
}

/// Speicher für Zugangsdaten (macOS: Schlüsselbund).
pub trait SecretStore: Send + Sync {
    fn set_password(&self, key: &str, password: &str) -> Result<()>;
    fn password(&self, key: &str) -> Result<String>;
    fn delete_password(&self, key: &str) -> Result<()>;
}

/// Abstraktion eines Mail-Zugangs. Heute IMAP, später z. B. JMAP oder Exchange.
pub trait MailProvider: Send {
    /// Liest alle Ordner vom Server.
    fn list_folders(&mut self) -> impl Future<Output = Result<Vec<RemoteFolder>>> + Send;
    /// Beendet die Sitzung sauber.
    fn logout(self) -> impl Future<Output = Result<()>> + Send;
}
