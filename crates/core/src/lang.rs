//! Sprache für Texte aus dem Backend (Fehlermeldungen, Zitat-Kopf, Einladungs-Mails,
//! Mitteilungen). Die Oberfläche setzt sie beim Start nach der Systemsprache.
//!
//! Quelltexte bleiben deutsch; englische Fassungen stehen in `TABLE` (Fehler) bzw.
//! werden mit [`tr`] direkt gewählt. `{}` in einem Muster steht für einen variablen Teil.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(
        match lang {
            Lang::De => 0,
            Lang::En => 1,
        },
        Ordering::Relaxed,
    );
}

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Lang::En
    } else {
        Lang::De
    }
}

/// „de“ → Deutsch, alles andere → Englisch.
pub fn from_code(code: &str) -> Lang {
    if code.to_ascii_lowercase().starts_with("de") {
        Lang::De
    } else {
        Lang::En
    }
}

/// Deutsch oder Englisch je nach aktueller Sprache.
pub fn tr(de: &'static str, en: &'static str) -> &'static str {
    match current() {
        Lang::De => de,
        Lang::En => en,
    }
}

/// Deutsche Fehlermeldungen → Englisch (`{}` = variabler Teil, gleiche Reihenfolge).
const TABLE: &[(&str, &str)] = &[
    (
        "{}: Weiterleitung nicht verfolgbar",
        "{}: cannot follow redirect",
    ),
    ("Ungültige E-Mail-Adresse", "Invalid email address"),
    (
        "Keine Servereinstellungen für {} gefunden",
        "No server settings found for {}",
    ),
    ("Ungültige Absenderadresse {}", "Invalid sender address {}"),
    ("Ungültige Adresse: {}", "Invalid address: {}"),
    ("Ungültige Adresse", "Invalid address"),
    (
        "Bitte mindestens einen Empfänger angeben.",
        "Please enter at least one recipient.",
    ),
    (
        "Nachricht konnte nicht gebaut werden: {}",
        "Could not build the message: {}",
    ),
    (
        "Keine Empfänger für die Termin-Mail.",
        "No recipients for the event email.",
    ),
    (
        "Termin-Mail konnte nicht gebaut werden: {}",
        "Could not build the event email: {}",
    ),
    ("Keine Begrüßung vom Server", "No greeting from the server"),
    (
        "Unverschlüsselte Verbindungen sind nur zu localhost erlaubt",
        "Unencrypted connections are only allowed to localhost",
    ),
    (
        "Nachricht {} nicht mehr auf dem Server",
        "Message {} is no longer on the server",
    ),
    ("{}: Server antwortet nicht", "{}: server does not respond"),
    ("Ungültiger Hostname {}: {}", "Invalid host name {}: {}"),
    ("Bitte einen Namen angeben.", "Please enter a name."),
    ("Signatur nicht gefunden", "Signature not found"),
    ("Die Vorlage ist leer.", "The template is empty."),
    (
        "Mitgelieferte Vorlagen können nicht geändert werden – bitte kopieren.",
        "Built-in templates cannot be changed – please make a copy.",
    ),
    (
        "Servereinstellungen unvollständig",
        "Server settings incomplete",
    ),
    (
        "Die Server-Adresse ist ungültig.",
        "The server address is invalid.",
    ),
    (
        "Nur verschlüsselte Adressen (https://) sind erlaubt.",
        "Only encrypted addresses (https://) are allowed.",
    ),
    (
        "Für dieses Konto wurde (noch) kein Kalender-Server gefunden.",
        "No calendar server has been found for this account (yet).",
    ),
    ("Kalender nicht gespeichert", "Calendar not saved"),
    (
        "Der Kalender „Auf diesem Mac“ bleibt immer erhalten.",
        "The calendar “On this computer” cannot be removed.",
    ),
    (
        "Anmeldung abgelehnt – Benutzername oder Passwort stimmen nicht {}",
        "Sign-in rejected – user name or password is wrong (for iCloud, use an app-specific password).",
    ),
    (
        "Unter dieser Adresse wurden keine Kalender gefunden.",
        "No calendars were found at this address.",
    ),
    (
        "Der Server hat die Anmeldung mit den Zugangsdaten dieses Mail-Kontos abgelehnt.",
        "The server rejected the sign-in with this mail account’s credentials.",
    ),
    ("Bitte einen Titel eingeben.", "Please enter a title."),
    (
        "Das Ende liegt vor dem Beginn.",
        "The end is before the start.",
    ),
    (
        "Wiederkehrende Termine können in Kolibri noch nicht bearbeitet werden.",
        "Recurring events cannot be edited in Kolibri yet.",
    ),
    ("Kein Kalender vorhanden", "No calendar available"),
    (
        "Keine Einladung in dieser Mail",
        "No invitation in this email",
    ),
    ("Einladung ohne Termin", "Invitation without an event"),
    ("Noch kein Konto eingerichtet", "No account set up yet"),
    (
        "Die Anhänge sind zusammen größer als 25 MB – viele Server lehnen das ab.",
        "The attachments are larger than 25 MB in total – many servers reject that.",
    ),
    ("{} kann nicht gelesen werden: {}", "{} cannot be read: {}"),
    ("Anhang {} nicht gefunden", "Attachment {} not found"),
    ("Anhang nicht gefunden", "Attachment not found"),
    (
        "Die Signatur ist zu groß (höchstens 500 KB) – bitte ein kleineres Bild verwenden.",
        "The signature is too large (max. 500 KB) – please use a smaller image.",
    ),
    (
        "Die Vorlage ist zu groß (höchstens 2 MB).",
        "The template is too large (max. 2 MB).",
    ),
    (
        "Ordner können nicht angehängt werden",
        "Folders cannot be attached",
    ),
    (
        "Dieser Dateityp kann Programme ausführen und wird aus Sicherheitsgründen nicht direkt geöffnet. Du kannst ihn sichern.",
        "This file type can run programs and is not opened directly for security reasons. You can save it instead.",
    ),
    (
        "Für dieses Konto wurde (noch) kein Adressbuch gefunden.",
        "No address book has been found for this account (yet).",
    ),
    (
        "Kein Adressbuch auf dem Server.",
        "No address book on the server.",
    ),
    (
        "Bitte einen Namen oder eine E-Mail-Adresse angeben.",
        "Please enter a name or an email address.",
    ),
    (
        "Liegt bereits im Papierkorb – endgültig löschen?",
        "Already in the trash – delete permanently?",
    ),
    (
        "Verschieben zwischen Konten ist (noch) nicht möglich",
        "Moving between accounts is not possible (yet)",
    ),
    ("Keine Verbindung zum Server", "No connection to the server"),
    (
        "Kein Passwort im Schlüsselbund",
        "No password in the keychain",
    ),
    ("Kein Passwort gespeichert", "No password saved"),
    (
        "Passwort ist kein gültiges UTF-8",
        "The password is not valid UTF-8",
    ),
    (
        "Zum Einladen wird ein Mail-Konto gebraucht.",
        "A mail account is needed to send invitations.",
    ),
    (
        "Der Server erlaubt das Anlegen von Kalendern nicht.",
        "The server does not allow creating calendars.",
    ),
    ("Kalender anlegen: {}", "Create calendar: {}"),
    (
        "Wurde inzwischen anderswo geändert – bitte erneut versuchen.",
        "Was changed elsewhere in the meantime – please try again.",
    ),
];

/// Variable Teile eines Musters herauslösen, `None` wenn es nicht passt.
fn captures<'a>(pattern: &str, text: &'a str) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = pattern.split("{}").collect();
    let (first, rest) = parts.split_first()?;
    let mut tail = text.strip_prefix(first)?;
    let mut vars = Vec::new();
    for (i, part) in rest.iter().enumerate() {
        let last = i + 1 == rest.len();
        let end = if last {
            if part.is_empty() {
                tail.len()
            } else {
                tail.strip_suffix(part).map(str::len)?
            }
        } else {
            tail.find(part)?
        };
        vars.push(&tail[..end]);
        tail = &tail[end + part.len()..];
    }
    tail.is_empty().then_some(vars)
}

/// Fehlertext in die aktuelle Sprache bringen (Deutsch bleibt unverändert).
pub fn translate(text: &str) -> String {
    if current() == Lang::De {
        return text.to_owned();
    }
    // Präfixe wie „[imap] “ erhalten.
    let (prefix, body) = match text.split_once("] ") {
        Some((p, b)) if p.starts_with('[') && !p.contains(' ') => (&text[..p.len() + 2], b),
        _ => ("", text),
    };
    for (de, en) in TABLE {
        if let Some(vars) = captures(de, body) {
            let mut out = String::from(prefix);
            let mut vars = vars.into_iter();
            for (i, piece) in en.split("{}").enumerate() {
                if i > 0 {
                    out.push_str(vars.next().unwrap_or_default());
                }
                out.push_str(piece);
            }
            return out;
        }
    }
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_and_language() {
        assert_eq!(
            captures("Ungültige Adresse: {}", "Ungültige Adresse: x@y"),
            Some(vec!["x@y"])
        );
        assert_eq!(captures("Ungültige Adresse", "Ungültige Adresse: x"), None);
        assert_eq!(
            captures(
                "{}: Server antwortet nicht",
                "imap.x.de: Server antwortet nicht"
            ),
            Some(vec!["imap.x.de"])
        );
        assert_eq!(
            captures("Ungültiger Hostname {}: {}", "Ungültiger Hostname a b: c"),
            Some(vec!["a b", "c"])
        );

        set(Lang::En);
        assert_eq!(
            translate("Ungültige Adresse: kaputt"),
            "Invalid address: kaputt"
        );
        assert_eq!(
            translate("[smtp] Keine Verbindung zum Server"),
            "[smtp] No connection to the server"
        );
        assert_eq!(translate("Irgendwas Unbekanntes"), "Irgendwas Unbekanntes");
        assert_eq!(tr("Hallo", "Hello"), "Hello");
        set(Lang::De);
        assert_eq!(
            translate("Ungültige Adresse: kaputt"),
            "Ungültige Adresse: kaputt"
        );
        assert_eq!(from_code("de-AT"), Lang::De);
        assert_eq!(from_code("en-US"), Lang::En);
    }
}
