use serde::{Deserialize, Serialize};

/// Besondere Funktion eines Ordners (RFC 6154 SPECIAL-USE oder erkannt am Namen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderRole {
    Inbox,
    Drafts,
    Sent,
    Archive,
    Junk,
    Trash,
    All,
    Flagged,
}

impl FolderRole {
    /// Reihenfolge in der Seitenleiste.
    pub fn sort_key(self) -> u8 {
        match self {
            Self::Inbox => 0,
            Self::Flagged => 1,
            Self::Drafts => 2,
            Self::Sent => 3,
            Self::Archive => 4,
            Self::All => 5,
            Self::Junk => 6,
            Self::Trash => 7,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Drafts => "drafts",
            Self::Sent => "sent",
            Self::Archive => "archive",
            Self::Junk => "junk",
            Self::Trash => "trash",
            Self::All => "all",
            Self::Flagged => "flagged",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "inbox" => Self::Inbox,
            "drafts" => Self::Drafts,
            "sent" => Self::Sent,
            "archive" => Self::Archive,
            "junk" => Self::Junk,
            "trash" => Self::Trash,
            "all" => Self::All,
            "flagged" => Self::Flagged,
            _ => return None,
        })
    }

    /// Fallback für Server ohne SPECIAL-USE: Rolle am Ordnernamen erkennen.
    pub fn guess_from_name(name: &str) -> Option<Self> {
        let n = name.to_lowercase();
        Some(match n.as_str() {
            "inbox" => Self::Inbox,
            "drafts" | "draft" | "entwürfe" | "entwurf" => Self::Drafts,
            "sent" | "sent items" | "sent messages" | "sent mail" | "gesendet"
            | "gesendete objekte" | "gesendete elemente" => Self::Sent,
            "archive" | "archiv" | "archives" => Self::Archive,
            "junk" | "spam" | "junk e-mail" | "junk-e-mail" | "bulk mail" => Self::Junk,
            "trash"
            | "deleted"
            | "deleted items"
            | "deleted messages"
            | "papierkorb"
            | "gelöscht"
            | "gelöschte elemente"
            | "gelöschte objekte" => Self::Trash,
            _ => return None,
        })
    }
}

/// Ordner, wie er vom Server kommt (noch ohne Datenbank-ID).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFolder {
    pub path: String,
    pub delimiter: Option<String>,
    pub role: Option<FolderRole>,
    pub selectable: bool,
}

impl RemoteFolder {
    /// Letzter Teil des Pfads.
    pub fn name(&self) -> &str {
        match &self.delimiter {
            Some(d) if !d.is_empty() => self.path.rsplit(d.as_str()).next().unwrap_or(&self.path),
            _ => &self.path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_german_and_english_names() {
        assert_eq!(
            FolderRole::guess_from_name("Gesendet"),
            Some(FolderRole::Sent)
        );
        assert_eq!(
            FolderRole::guess_from_name("Sent Items"),
            Some(FolderRole::Sent)
        );
        assert_eq!(
            FolderRole::guess_from_name("Papierkorb"),
            Some(FolderRole::Trash)
        );
        assert_eq!(FolderRole::guess_from_name("Rechnungen"), None);
    }

    #[test]
    fn name_is_last_path_segment() {
        let f = RemoteFolder {
            path: "INBOX/Projekte/Kolibri".into(),
            delimiter: Some("/".into()),
            role: None,
            selectable: true,
        };
        assert_eq!(f.name(), "Kolibri");
    }

    #[test]
    fn role_roundtrip() {
        for r in [
            FolderRole::Inbox,
            FolderRole::Drafts,
            FolderRole::Sent,
            FolderRole::Archive,
            FolderRole::Junk,
            FolderRole::Trash,
            FolderRole::All,
            FolderRole::Flagged,
        ] {
            assert_eq!(FolderRole::parse(r.as_str()), Some(r));
        }
    }
}
