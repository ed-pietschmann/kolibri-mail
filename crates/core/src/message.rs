use serde::{Deserialize, Serialize};

use crate::{AccountId, FolderId, FolderRole};

/// Datenbank-ID einer Nachricht.
pub type MessageId = i64;

/// Name + Adresse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    pub name: String,
    pub addr: String,
}

/// IMAP-Flags, die Kolibri auswertet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
}

/// Kopfdaten einer Nachricht, wie sie vom Server kommen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessage {
    pub uid: u32,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub subject: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// Reply-To (leer = an den Absender antworten).
    pub reply_to: Vec<Address>,
    /// Eingang beim Server (INTERNALDATE), Unix-Sekunden.
    pub date: i64,
    pub size: u32,
    pub flags: Flags,
    /// Erste Schätzung aus dem Content-Type; wird beim Laden des Inhalts präzisiert.
    pub has_attachments: bool,
}

/// Zeile der Nachrichtenliste – bewusst schlank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: MessageId,
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub has_attachments: bool,
    pub preview: String,
    /// Erster Empfänger (Name oder Adresse) – für eigene, gesendete Mails.
    pub to_display: String,
    /// Anzahl Empfänger (An + Kopie).
    pub to_count: u32,
    /// Von einem eigenen Konto gesendet?
    pub outgoing: bool,
    /// Nachrichten im Gespräch (1 = keine Konversation).
    pub thread_count: u32,
}

/// Anhang (nur Metadaten; Inhalt wird bei Bedarf geladen, ab M5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// Aufbereiteter Inhalt einer Nachricht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    /// Bereits gesäubertes HTML – ohne Skripte, Event-Handler, Formulare.
    pub html: Option<String>,
    pub text: Option<String>,
    /// Enthält das HTML externe Bilder (Tracking-Gefahr)?
    pub remote_images: bool,
    pub attachments: Vec<Attachment>,
    /// Kurzvorschau für die Liste.
    pub preview: String,
    /// Termineinladung (text/calendar) in der Mail.
    #[serde(default)]
    pub invite: Option<Invite>,
}

/// Termineinladung aus einer Mail (iTIP REQUEST/CANCEL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    /// REQUEST (Einladung), CANCEL (Absage), REPLY (Antwort) …
    pub method: String,
    pub uid: String,
    pub summary: String,
    pub location: String,
    /// Unix-Sekunden.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub organizer_name: String,
    pub organizer_email: String,
    /// Die iCalendar-Datei selbst (zum Eintragen und Antworten).
    pub ics: String,
}

/// Alles für die Leseansicht.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub body: Body,
    /// Darf dieser Absender externe Bilder automatisch laden?
    pub sender_trusted: bool,
}

/// Welche Nachrichten eine Liste zeigt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MessageScope {
    /// Ein bestimmter Ordner.
    Folder { id: FolderId },
    /// Ordner einer Rolle über alle Konten (z. B. alle Posteingänge).
    Role { role: FolderRole },
    /// Alle markierten Nachrichten.
    Flagged,
}

/// Was die Oberfläche auslöst (bezogen auf Nachrichten-IDs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UserAction {
    MarkSeen {
        value: bool,
    },
    Flag {
        value: bool,
    },
    /// In den Archivordner (wird bei Bedarf angelegt).
    Archive,
    /// In den Papierkorb – oder endgültig, wenn schon im Papierkorb.
    Trash,
    MoveTo {
        folder_id: FolderId,
    },
    /// Endgültig löschen (nur nach Rückfrage in der Oberfläche).
    DeleteForever,
}

/// Eine Server-Operation in der Warteschlange (bezogen auf UIDs eines Ordners).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    SetSeen { value: bool },
    SetFlagged { value: bool },
    SetAnswered { value: bool },
    Move { to: String, create: bool },
    Delete,
}

impl Action {
    /// Blendet die Aktion Nachrichten bis zur Bestätigung aus?
    pub fn hides(&self) -> bool {
        matches!(self, Self::Move { .. } | Self::Delete)
    }
}

/// Eingereihte Operation mit Ortsangabe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOp {
    pub id: i64,
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub folder_path: String,
    pub uids: Vec<u32>,
    pub action: Action,
    pub attempts: u32,
}
