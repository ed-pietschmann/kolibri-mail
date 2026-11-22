use serde::{Deserialize, Serialize};

use crate::{AccountId, MessageId};

/// Anhang einer neuen Nachricht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ComposeAttachment {
    /// Datei vom Mac.
    File {
        path: String,
        name: String,
        size: u64,
    },
    /// Anhang einer vorhandenen Nachricht (Weiterleiten, Entwurf bearbeiten).
    Original {
        message_id: MessageId,
        index: usize,
        name: String,
        size: u64,
    },
}

impl ComposeAttachment {
    pub fn name(&self) -> &str {
        match self {
            Self::File { name, .. } | Self::Original { name, .. } => name,
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            Self::File { size, .. } | Self::Original { size, .. } => *size,
        }
    }
}

/// Inhalt des Verfassen-Fensters. Empfänger als Text („Name <a@b.de>, c@d.de“).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeDraft {
    pub account_id: AccountId,
    /// Message-ID ohne spitze Klammern – bleibt über alle Entwurfsfassungen gleich.
    pub draft_id: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    /// HTML aus dem Editor.
    pub html: String,
    pub attachments: Vec<ComposeAttachment>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    /// Lokale Nachricht, auf die geantwortet wird (wird als beantwortet markiert).
    pub reply_to_message: Option<MessageId>,
    /// Gewählte Signatur (steht bereits im Editor-HTML; nur zum Merken).
    #[serde(default)]
    pub signature_id: Option<i64>,
    /// Vorlage, in die das Editor-HTML eingesetzt wird.
    #[serde(default)]
    pub template_id: Option<i64>,
    /// Eigener HTML-Code: ersetzt Editor und Vorlage vollständig.
    #[serde(default)]
    pub code: Option<String>,
}

/// Wie das Verfassen beginnt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ComposeKind {
    New {
        account_id: Option<AccountId>,
    },
    Reply {
        message_id: MessageId,
    },
    ReplyAll {
        message_id: MessageId,
    },
    Forward {
        message_id: MessageId,
    },
    /// Entwurf aus dem Entwürfe-Ordner weiterbearbeiten.
    EditDraft {
        message_id: MessageId,
    },
}

/// Größenlimit für alle Anhänge zusammen (übliche Server-Grenze ~25 MB).
pub const MAX_ATTACHMENTS_BYTES: u64 = 25 * 1024 * 1024;
