//! MIME-Parsing mit `mail-parser`: Kopfzeilen für die Liste, Inhalt für die Leseansicht.

use std::collections::HashMap;

use kolibri_core::{Address, Attachment, Body, Flags, RemoteMessage};
use mail_parser::{AddressList, MessageParser};

use crate::sanitize;

const PREVIEW_CHARS: usize = 180;
/// Eingebettete Bilder bis zu dieser Größe werden direkt ins HTML übernommen.
const MAX_INLINE_IMAGE: usize = 2 * 1024 * 1024;

fn addresses(list: Option<AddressList<'_>>) -> Vec<Address> {
    list.map(|l| {
        l.mailboxes()
            .filter_map(|m| {
                Some(Address {
                    name: m.name().unwrap_or_default().trim().to_owned(),
                    addr: m.address()?.trim().to_owned(),
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Wandelt den Kopfbereich (BODY[HEADER]) in eine Listenzeile.
pub fn parse_header(
    uid: u32,
    raw: &[u8],
    flags: Flags,
    internal_date: Option<i64>,
    size: u32,
) -> RemoteMessage {
    let parser = MessageParser::default();
    let Some(msg) = parser.parse_headers(raw) else {
        return RemoteMessage {
            uid,
            message_id: None,
            in_reply_to: None,
            references: None,
            subject: String::new(),
            from: Address {
                name: String::new(),
                addr: String::new(),
            },
            to: Vec::new(),
            cc: Vec::new(),
            reply_to: Vec::new(),
            date: internal_date.unwrap_or(0),
            size,
            flags,
            has_attachments: false,
        };
    };
    let from = addresses(msg.from()).into_iter().next().unwrap_or(Address {
        name: String::new(),
        addr: String::new(),
    });
    let join = |list: Option<mail_parser::TextList<'_>>| {
        list.map(|l| l.iter().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty())
    };
    let has_attachments = msg.content_type().is_some_and(|ct| {
        ct.ctype().eq_ignore_ascii_case("multipart")
            && ct
                .subtype()
                .is_some_and(|s| s.eq_ignore_ascii_case("mixed"))
    });
    RemoteMessage {
        uid,
        message_id: msg.message_id().map(str::to_owned),
        in_reply_to: join(msg.in_reply_to()),
        references: join(msg.references()),
        subject: msg.subject().unwrap_or_default().trim().to_owned(),
        from,
        to: addresses(msg.to()),
        cc: addresses(msg.cc()),
        reply_to: addresses(msg.reply_to()),
        date: internal_date
            .or_else(|| msg.date().map(|d| d.to_timestamp()))
            .unwrap_or(0),
        size,
        flags,
        has_attachments,
    }
}

/// Kurzvorschau: Zitate und Leerraum raus, auf eine Zeile gekürzt.
pub fn preview(text: &str) -> String {
    let mut out = String::with_capacity(PREVIEW_CHARS + 8);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('>') {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        for word in line.split_whitespace() {
            if out.chars().count() >= PREVIEW_CHARS {
                out.push('…');
                return out;
            }
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(word);
        }
    }
    out
}

/// Zerlegt eine komplette Nachricht (BODY[]) für die Leseansicht.
pub fn parse_body(raw: &[u8]) -> Body {
    let parser = MessageParser::default();
    let Some(msg) = parser.parse(raw) else {
        return Body {
            html: None,
            text: Some(String::from_utf8_lossy(raw).into_owned()),
            remote_images: false,
            attachments: Vec::new(),
            preview: String::new(),
            invite: None,
        };
    };

    // Eingebettete Bilder (cid:) als data:-URIs bereitstellen.
    let mut inline: HashMap<String, String> = HashMap::new();
    for part in msg.parts() {
        let (Some(cid), Some(ct)) = (part.content_id(), part.content_type()) else {
            continue;
        };
        if !ct.ctype().eq_ignore_ascii_case("image") || part.decoded_len() > MAX_INLINE_IMAGE {
            continue;
        }
        let mime = format!("image/{}", ct.subtype().unwrap_or("png"));
        let cid = cid.trim_matches(|c| c == '<' || c == '>').to_owned();
        inline.insert(
            cid,
            format!("data:{mime};base64,{}", base64(&part.decoded())),
        );
    }

    let attachments: Vec<Attachment> = visible_attachments(&msg)
        .map(|p| Attachment {
            name: attachment_name(&p),
            mime: attachment_mime(&p),
            size: p.decoded_len() as u64,
        })
        .collect();

    let text = msg.body_text(0).map(|t| t.into_owned());
    let has_html = msg
        .html_body()
        .any(|p| matches!(p.kind(), mail_parser::PartKind::Html));
    let html = msg.body_html(0).filter(|_| has_html);
    let (html, remote_images) = match html {
        Some(h) => {
            let clean = sanitize::clean(&h, &inline);
            (Some(clean.html), clean.remote_images)
        }
        None => (None, false),
    };

    // Termineinladung (text/calendar, auch als .ics-Anhang).
    let invite = msg
        .parts()
        .filter(|p| {
            p.is_content_type("text", "calendar")
                || p.attachment_name()
                    .is_some_and(|n| n.to_lowercase().ends_with(".ics"))
        })
        .find_map(|p| invite_from(&String::from_utf8_lossy(&p.decoded())));

    Body {
        preview: text.as_deref().map(preview).unwrap_or_default(),
        html,
        text,
        remote_images,
        attachments,
        invite,
    }
}

/// Erster Termin einer iCalendar-Datei als Einladung (ohne METHOD: wie REQUEST).
fn invite_from(ics: &str) -> Option<kolibri_core::Invite> {
    use kolibri_dav::ical;
    let cal = ical::parse(ics);
    let event = cal
        .events
        .iter()
        .find(|e| e.recurrence_id.is_none())
        .or(cal.events.first())?;
    let first = cal.instances(i64::MIN / 4, i64::MAX / 4).into_iter().next();
    let start = first
        .as_ref()
        .map_or_else(|| event.start.unix(), |i| i.start);
    let end = first.as_ref().map_or(start, |i| i.end);
    Some(kolibri_core::Invite {
        method: cal.method.clone().unwrap_or_else(|| "REQUEST".into()),
        uid: event.uid.clone(),
        summary: event.summary.clone(),
        location: event.location.clone(),
        start,
        end,
        all_day: event.all_day(),
        organizer_name: event
            .organizer
            .as_ref()
            .map(|o| o.name.clone())
            .unwrap_or_default(),
        organizer_email: event
            .organizer
            .as_ref()
            .map(|o| o.email.clone())
            .unwrap_or_default(),
        ics: ics.to_owned(),
    })
}

/// Anhänge, die als Datei gezeigt werden (eingebettete Bilder ausgenommen).
/// Die Reihenfolge ist die Index-Grundlage für `attachment_data`.
fn visible_attachments<'a>(
    msg: &'a mail_parser::Message<'a>,
) -> impl Iterator<Item = mail_parser::MessagePart<'a>> + 'a {
    msg.attachments()
        .filter(|p| !(p.is_inline() && p.content_id().is_some()))
}

fn attachment_name(p: &mail_parser::MessagePart<'_>) -> String {
    p.attachment_name()
        .map_or_else(|| "Anhang".to_owned(), str::to_owned)
}

fn attachment_mime(p: &mail_parser::MessagePart<'_>) -> String {
    p.content_type().map_or_else(
        || "application/octet-stream".to_owned(),
        |ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("octet-stream")),
    )
}

/// Inhalt des `index`-ten Anhangs: (Dateiname, MIME-Typ, Bytes).
pub fn attachment_data(raw: &[u8], index: usize) -> Option<(String, String, Vec<u8>)> {
    let msg = MessageParser::default().parse(raw)?;
    let part = visible_attachments(&msg).nth(index)?;
    Some((
        attachment_name(&part),
        attachment_mime(&part),
        part.decoded().into_owned(),
    ))
}

/// Dateiname ohne Pfadteile und Steuerzeichen (für das Ablegen auf der Platte).
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == ':' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        "Anhang".to_owned()
    } else {
        trimmed.chars().take(200).collect()
    }
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[((n >> (18 - 6 * i)) & 0x3F) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn safe_names() {
        assert_eq!(safe_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_file_name("Rechnung: März.pdf"), "Rechnung_ März.pdf");
        assert_eq!(safe_file_name("..."), "Anhang");
        assert_eq!(safe_file_name(".bashrc"), "bashrc");
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn preview_skips_quotes_and_collapses_space() {
        let p = preview("Hallo Anna,\n\n  danke   dir!\n> altes Zitat\nGruß");
        assert_eq!(p, "Hallo Anna, danke dir! Gruß");
        assert!(preview(&"wort ".repeat(200)).ends_with('…'));
    }

    #[test]
    fn parses_encoded_header() {
        let raw = b"From: =?UTF-8?Q?J=C3=BCrgen_M=C3=BCller?= <j@beispiel.de>\r\n\
To: a@b.de, \"Zweite\" <c@d.de>\r\n\
Subject: =?UTF-8?B?w5xtbMOkdXRl?=\r\n\
Message-ID: <x1@beispiel.de>\r\n\
Content-Type: multipart/mixed; boundary=x\r\n\r\n";
        let m = parse_header(7, raw, Flags::default(), Some(100), 42);
        assert_eq!(m.from.name, "Jürgen Müller");
        assert_eq!(m.from.addr, "j@beispiel.de");
        assert_eq!(m.subject, "Ümläute");
        assert_eq!(m.to.len(), 2);
        assert_eq!(m.message_id.as_deref(), Some("x1@beispiel.de"));
        assert!(m.has_attachments);
        assert_eq!(m.date, 100);
    }

    #[test]
    fn body_with_html_cid_and_attachment() {
        let raw = b"From: a@b.de\r\nSubject: t\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n\
--m\r\nContent-Type: multipart/related; boundary=\"r\"\r\n\r\n\
--r\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<p onclick=\"x()\">Hallo <img src=\"cid:logo@x\"><img src=\"https://t.example/p.gif\"></p><script>alert(1)</script>\r\n\
--r\r\nContent-Type: image/png\r\nContent-ID: <logo@x>\r\nContent-Transfer-Encoding: base64\r\n\r\nZm9v\r\n\
--r--\r\n\
--m\r\nContent-Type: application/pdf; name=\"Rechnung.pdf\"\r\nContent-Disposition: attachment; filename=\"Rechnung.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0=\r\n\
--m--\r\n";
        let body = parse_body(raw);
        let html = body.html.unwrap();
        assert!(!html.contains("script"), "{html}");
        assert!(!html.contains("onclick"), "{html}");
        assert!(html.contains("data:image/png;base64,Zm9v"), "{html}");
        assert!(body.remote_images);
        assert_eq!(body.attachments.len(), 1);
        assert_eq!(body.attachments[0].name, "Rechnung.pdf");
        let (name, mime, data) = attachment_data(raw, 0).unwrap();
        assert_eq!(
            (name.as_str(), mime.as_str()),
            ("Rechnung.pdf", "application/pdf")
        );
        assert_eq!(data, b"%PDF-");
        assert!(attachment_data(raw, 1).is_none());
        assert!(body.preview.contains("Hallo"));
        assert!(body.invite.is_none());
    }

    #[test]
    fn detects_calendar_invitation() {
        let raw = b"From: anna@beispiel.de\r\nSubject: Einladung\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nDu bist eingeladen.\r\n\
--b\r\nContent-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\r\n\
BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:inv-9\r\nSUMMARY:Planung\r\n\
DTSTART:20261201T090000Z\r\nDTEND:20261201T100000Z\r\nORGANIZER;CN=Anna:mailto:anna@beispiel.de\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n--b--\r\n";
        let body = parse_body(raw);
        let invite = body.invite.unwrap();
        assert_eq!(invite.method, "REQUEST");
        assert_eq!(invite.summary, "Planung");
        assert_eq!(invite.organizer_email, "anna@beispiel.de");
        assert_eq!(invite.end - invite.start, 3600);
        assert!(body.text.unwrap().contains("eingeladen"));
    }
}
