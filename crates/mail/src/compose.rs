//! Neue Nachrichten bauen (MIME über `lettre`) und Antworten/Zitate vorbereiten.

use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};

use kolibri_core::{Account, Address, ComposeDraft, Error, ErrorKind, Result};
use lettre::Message;
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, Mailboxes, MultiPart, SinglePart};

/// Fertig gebaute Nachricht.
#[derive(Debug, Clone)]
pub struct Built {
    pub raw: Vec<u8>,
    /// Absender für den SMTP-Umschlag.
    pub from: String,
    /// Alle Empfänger (An, Kopie, Blindkopie) für den SMTP-Umschlag.
    pub to: Vec<String>,
}

/// Inhalt eines Anhangs (bereits gelesen).
#[derive(Debug, Clone)]
pub struct FilePart {
    pub name: String,
    pub data: Vec<u8>,
}

fn invalid(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::Invalid, msg)
}

/// „Name <a@b.de>, c@d.de“ → Liste. Leer erlaubt.
pub fn parse_addresses(input: &str) -> Result<Vec<Mailbox>> {
    let cleaned = input.trim().trim_end_matches([',', ';']).replace(';', ",");
    if cleaned.trim().is_empty() {
        return Ok(Vec::new());
    }
    Mailboxes::from_str(&cleaned)
        .map(|m| m.into_iter().collect())
        .map_err(|_| invalid(format!("Ungültige Adresse: {}", input.trim())))
}

/// Neue, weltweit eindeutige Message-ID (ohne Klammern).
pub fn new_message_id(email: &str) -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let domain = email.rsplit_once('@').map_or("kolibri.local", |(_, d)| d);
    format!("{nanos:x}.{:x}.{n}.kolibri@{domain}", std::process::id())
}

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Klartext → Absätze mit Zeilenumbrüchen.
pub fn text_to_html(text: &str) -> String {
    text.trim_end()
        .split("\n\n")
        .map(|p| format!("<p>{}</p>", escape_html(p).replace('\n', "<br>")))
        .collect()
}

/// Signatur als HTML-Absatz (leer → nichts).
pub fn signature_html(signature: &str) -> String {
    if signature.trim().is_empty() {
        String::new()
    } else {
        format!(
            "<p>{}</p>",
            escape_html(signature.trim()).replace('\n', "<br>")
        )
    }
}

/// „Re: “ / „Fwd: “ voranstellen, außer es steht schon da (auch AW/WG).
pub fn prefixed_subject(prefix: &str, subject: &str) -> String {
    let lower = subject.trim_start().to_lowercase();
    let already = match prefix {
        "Re" => ["re:", "aw:", "antw:"].iter().any(|p| lower.starts_with(p)),
        _ => ["fwd:", "fw:", "wg:"].iter().any(|p| lower.starts_with(p)),
    };
    if already {
        subject.trim().to_owned()
    } else {
        format!("{prefix}: {}", subject.trim())
    }
}

/// References-Kette für eine Antwort (RFC 5322, 3.6.4).
pub fn reply_references(
    references: Option<&str>,
    in_reply_to: Option<&str>,
    message_id: &str,
) -> String {
    let mut ids: Vec<String> = references
        .or(in_reply_to)
        .unwrap_or_default()
        .split_whitespace()
        .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    ids.push(message_id.trim_matches(|c| c == '<' || c == '>').to_owned());
    ids.dedup();
    ids.iter()
        .map(|id| format!("<{id}>"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Adressen als Eingabetext („Name <a@b>, …“), ohne bestimmte Adressen.
pub fn format_addresses(list: &[Address], exclude: &[&str]) -> String {
    let mut seen: Vec<String> = exclude.iter().map(|s| s.to_lowercase()).collect();
    let mut out = Vec::new();
    for a in list {
        let key = a.addr.to_lowercase();
        if a.addr.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if a.name.is_empty() {
            out.push(a.addr.clone());
        } else {
            let name = if a.name.contains([',', '"', '<', '>', ';', '@']) {
                format!("\"{}\"", a.name.replace('"', ""))
            } else {
                a.name.clone()
            };
            out.push(format!("{name} <{}>", a.addr));
        }
    }
    out.join(", ")
}

fn mime_for(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "html" | "htm" => "text/html; charset=utf-8",
        "ics" => "text/calendar; charset=utf-8",
        "vcf" => "text/vcard; charset=utf-8",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pages" | "numbers" | "key" => "application/octet-stream",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

/// „Mittwoch, 19. November 2026 um 14:05“ (Ortszeit).
pub fn german_date(unix: i64) -> String {
    use chrono::{Datelike, Local, TimeZone, Timelike};
    const DAYS: [&str; 7] = [
        "Montag",
        "Dienstag",
        "Mittwoch",
        "Donnerstag",
        "Freitag",
        "Samstag",
        "Sonntag",
    ];
    const MONTHS: [&str; 12] = [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ];
    let Some(d) = Local.timestamp_opt(unix, 0).single() else {
        return String::new();
    };
    format!(
        "{}, {}. {} {} um {:02}:{:02}",
        DAYS[d.weekday().num_days_from_monday() as usize],
        d.day(),
        MONTHS[d.month0() as usize],
        d.year(),
        d.hour(),
        d.minute()
    )
}

/// Inhalt einer Mail für Zitate: Text bevorzugt (wird im Editor sauber),
/// sonst das bereits gesäuberte HTML.
pub fn quotable(body: &kolibri_core::Body) -> String {
    match (&body.text, &body.html) {
        (Some(t), _) if !t.trim().is_empty() => text_to_html(t),
        (_, Some(h)) => h.clone(),
        _ => String::new(),
    }
}

/// Zitat für Antworten.
pub fn reply_quote(sender: &str, date: i64, body: &kolibri_core::Body) -> String {
    format!(
        "<p>Am {} schrieb {}:</p><blockquote>{}</blockquote>",
        escape_html(&german_date(date)),
        escape_html(sender),
        quotable(body)
    )
}

/// Block für Weiterleitungen.
pub fn forward_block(
    from: &str,
    subject: &str,
    date: i64,
    to: &str,
    body: &kolibri_core::Body,
) -> String {
    format!(
        "<p>Anfang der weitergeleiteten Nachricht:</p><blockquote><p><strong>Von:</strong> {}<br>\
<strong>Betreff:</strong> {}<br><strong>Datum:</strong> {}<br><strong>An:</strong> {}</p>{}</blockquote>",
        escape_html(from),
        escape_html(subject),
        escape_html(&german_date(date)),
        escape_html(to),
        quotable(body)
    )
}

/// Das HTML aus dem Editor in eine vollständige, mailtaugliche Seite packen.
pub fn wrap_html(body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"></head>\
<body style=\"font-family:-apple-system,'Helvetica Neue',Helvetica,Arial,sans-serif;\
font-size:14px;line-height:1.5;color:#1d1d1f\">{body}</body></html>"
    )
}

/// Platzhalter einer Vorlage füllen. `{{inhalt}}` bekommt das Editor-HTML,
/// die übrigen Werte werden HTML-maskiert.
pub fn fill_template(template: &str, content: &str, account: &Account, subject: &str) -> String {
    let name = if account.display_name.trim().is_empty() {
        account.email.as_str()
    } else {
        account.display_name.trim()
    };
    template
        .replace("{{name}}", &escape_html(name))
        .replace("{{email}}", &escape_html(&account.email))
        .replace("{{betreff}}", &escape_html(subject.trim()))
        .replace("{{inhalt}}", content)
}

/// Eingebettetes Bild (aus einer data:-URI), wird als `cid:` mitgeschickt.
struct InlineImage {
    cid: String,
    mime: String,
    data: Vec<u8>,
}

/// Ersetzt `src="data:image/…;base64,…"` durch `cid:`-Verweise, damit Logos in
/// Signaturen und Vorlagen überall angezeigt werden (Gmail & Co. blockieren data:).
fn extract_inline_images(html: &str, domain: &str) -> (String, Vec<InlineImage>) {
    let mut out = String::with_capacity(html.len());
    let mut images: Vec<InlineImage> = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find("data:image/") {
        let (before, tail) = rest.split_at(pos);
        out.push_str(before);
        let end = tail
            .find(|c: char| c == '"' || c == '\'' || c == ')' || c.is_whitespace())
            .unwrap_or(tail.len());
        let uri = &tail[..end];
        let parsed = uri
            .strip_prefix("data:")
            .and_then(|u| u.split_once(";base64,"))
            .and_then(|(mime, b64)| decode_base64(b64).map(|d| (mime.to_owned(), d)));
        match parsed {
            Some((mime, data)) if !data.is_empty() => {
                // Gleiches Bild nur einmal anhängen.
                let cid = match images.iter().find(|i| i.data == data) {
                    Some(i) => i.cid.clone(),
                    None => {
                        let cid = format!("bild{}.{}", images.len() + 1, new_message_id(domain));
                        images.push(InlineImage {
                            cid: cid.clone(),
                            mime,
                            data,
                        });
                        cid
                    }
                };
                out.push_str("cid:");
                out.push_str(&cid);
            }
            _ => out.push_str(uri),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    (out, images)
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &c in input.as_bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() || c == b'%' {
            continue;
        }
        buf = (buf << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buf >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

/// Baut die MIME-Nachricht aus der fertigen HTML-Seite (`document`).
/// `for_draft`: Bcc bleibt im Kopf, Empfänger dürfen fehlen.
pub fn build(
    account: &Account,
    draft: &ComposeDraft,
    document: &str,
    files: Vec<FilePart>,
    for_draft: bool,
) -> Result<Built> {
    let from_addr = account
        .email
        .parse()
        .map_err(|_| invalid(format!("Ungültige Absenderadresse {}", account.email)))?;
    let display = account.display_name.trim();
    let from = Mailbox::new((!display.is_empty()).then(|| display.to_owned()), from_addr);

    let to = parse_addresses(&draft.to)?;
    let cc = parse_addresses(&draft.cc)?;
    let bcc = parse_addresses(&draft.bcc)?;
    if !for_draft && to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(invalid("Bitte mindestens einen Empfänger angeben."));
    }

    let mut builder = Message::builder()
        .from(from.clone())
        .subject(draft.subject.trim())
        .message_id(Some(format!("<{}>", draft.draft_id)))
        .date_now()
        .user_agent("Kolibri".into());
    for m in &to {
        builder = builder.to(m.clone());
    }
    for m in &cc {
        builder = builder.cc(m.clone());
    }
    for m in &bcc {
        builder = builder.bcc(m.clone());
    }
    if let Some(id) = draft.in_reply_to.as_deref().filter(|s| !s.is_empty()) {
        builder = builder.in_reply_to(id.to_owned());
    }
    if let Some(r) = draft.references.as_deref().filter(|s| !s.is_empty()) {
        builder = builder.references(r.to_owned());
    }

    let recipients: Vec<String> = to
        .iter()
        .chain(&cc)
        .chain(&bcc)
        .map(|m| m.email.to_string())
        .collect();
    if for_draft {
        builder = builder.keep_bcc();
        // Entwürfe werden nie per SMTP verschickt; der Umschlag ist nur Formsache.
        let envelope =
            lettre::address::Envelope::new(Some(from.email.clone()), vec![from.email.clone()])
                .map_err(|e| invalid(e.to_string()))?;
        builder = builder.envelope(envelope);
    }

    let domain = account
        .email
        .rsplit_once('@')
        .map_or("kolibri.local", |(_, d)| d);
    let (document, images) = extract_inline_images(document, domain);
    let text = mail_parser::html_to_text(&document);
    let alternative = if images.is_empty() {
        MultiPart::alternative_plain_html(text, document)
    } else {
        let mut related = MultiPart::related().singlepart(SinglePart::html(document));
        for img in images {
            let ct = ContentType::parse(&img.mime)
                .or_else(|_| ContentType::parse("application/octet-stream"))
                .map_err(|e| invalid(e.to_string()))?;
            related = related.singlepart(Attachment::new_inline(img.cid).body(img.data, ct));
        }
        MultiPart::alternative()
            .singlepart(SinglePart::plain(text))
            .multipart(related)
    };
    let message = if files.is_empty() {
        builder.multipart(alternative)
    } else {
        let mut mixed = MultiPart::mixed().multipart(alternative);
        for f in files {
            let ct = ContentType::parse(mime_for(&f.name))
                .or_else(|_| ContentType::parse("application/octet-stream"))
                .map_err(|e| invalid(e.to_string()))?;
            mixed = mixed.singlepart(Attachment::new(f.name).body(f.data, ct));
        }
        builder.multipart(mixed)
    }
    .map_err(|e| invalid(format!("Nachricht konnte nicht gebaut werden: {e}")))?;

    Ok(Built {
        raw: message.formatted(),
        from: from.email.to_string(),
        to: recipients,
    })
}

/// Antwort auf eine Termineinladung (iMIP, RFC 6047): Text + text/calendar; method=REPLY.
pub fn build_invite_reply(
    account: &Account,
    organizer: &str,
    subject: &str,
    text: &str,
    ics: &str,
) -> Result<Built> {
    build_itip(
        account,
        &[organizer.to_owned()],
        subject,
        text,
        None,
        ics,
        "REPLY",
    )
}

/// Termin-Mail nach iMIP (RFC 6047): Einladung (REQUEST), Absage (CANCEL) oder
/// Antwort (REPLY). Text, optional HTML und der Termin als `text/calendar` im
/// selben multipart/alternative – so verstehen es Outlook, Apple Kalender und Gmail.
/// Zusätzlich hängt die Datei als `einladung.ics` an (für Programme ohne iMIP).
pub fn build_itip(
    account: &Account,
    to: &[String],
    subject: &str,
    text: &str,
    html: Option<&str>,
    ics: &str,
    method: &str,
) -> Result<Built> {
    let from_addr = account
        .email
        .parse()
        .map_err(|_| invalid(format!("Ungültige Absenderadresse {}", account.email)))?;
    let display = account.display_name.trim();
    let from = Mailbox::new((!display.is_empty()).then(|| display.to_owned()), from_addr);
    let mut builder = Message::builder()
        .from(from.clone())
        .subject(subject)
        .message_id(Some(format!("<{}>", new_message_id(&account.email))))
        .date_now()
        .user_agent("Kolibri".into());
    let mut recipients = Vec::new();
    for addr in to {
        let mailbox: Mailbox = addr
            .parse()
            .map_err(|_| invalid(format!("Ungültige Adresse: {addr}")))?;
        recipients.push(mailbox.email.to_string());
        builder = builder.to(mailbox);
    }
    if recipients.is_empty() {
        return Err(invalid("Keine Empfänger für die Termin-Mail."));
    }
    let calendar = ContentType::parse(&format!("text/calendar; method={method}; charset=utf-8"))
        .map_err(|e| invalid(e.to_string()))?;
    let mut alternative = MultiPart::alternative().singlepart(SinglePart::plain(text.to_owned()));
    if let Some(h) = html {
        alternative = alternative.singlepart(SinglePart::html(h.to_owned()));
    }
    alternative =
        alternative.singlepart(SinglePart::builder().header(calendar).body(ics.to_owned()));
    let message =
        if method == "REPLY" {
            builder.multipart(alternative)
        } else {
            let file_type = ContentType::parse("application/ics; name=\"einladung.ics\"")
                .map_err(|e| invalid(e.to_string()))?;
            builder.multipart(MultiPart::mixed().multipart(alternative).singlepart(
                Attachment::new("einladung.ics".into()).body(ics.to_owned(), file_type),
            ))
        }
        .map_err(|e| invalid(format!("Termin-Mail konnte nicht gebaut werden: {e}")))?;
    Ok(Built {
        raw: message.formatted(),
        from: from.email.to_string(),
        to: recipients,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use kolibri_core::{ComposeAttachment, Security, ServerConfig};

    fn account() -> Account {
        let s = ServerConfig {
            host: "h".into(),
            port: 1,
            security: Security::Tls,
            username: "u".into(),
        };
        Account {
            id: 1,
            email: "eddie@beispiel.de".into(),
            display_name: "Eddie Müller".into(),
            imap: s.clone(),
            smtp: s,
            signature: String::new(),
        }
    }

    fn draft() -> ComposeDraft {
        ComposeDraft {
            account_id: 1,
            draft_id: "abc.kolibri@beispiel.de".into(),
            to: "Anna Schäfer <anna@beispiel.de>, max@firma.example".into(),
            cc: String::new(),
            bcc: "geheim@beispiel.de".into(),
            subject: "Grüße aus Köln".into(),
            html: "<p>Hallo <b>Anna</b>,</p><p>bis Montag!</p>".into(),
            attachments: Vec::<ComposeAttachment>::new(),
            in_reply_to: Some("<orig@x>".into()),
            references: Some("<a@x> <orig@x>".into()),
            reply_to_message: None,
            signature_id: None,
            template_id: None,
            code: None,
        }
    }

    fn doc() -> String {
        wrap_html(&draft().html)
    }

    #[test]
    fn templates_fill_placeholders_and_escape() {
        let mut a = account();
        a.display_name = "Eddie <M>".into();
        let out = fill_template(
            "<h1>{{betreff}}</h1><div>{{inhalt}}</div><p>{{name}} {{email}}</p>",
            "<p><b>Hallo</b></p>",
            &a,
            " A & B ",
        );
        assert_eq!(
            out,
            "<h1>A &amp; B</h1><div><p><b>Hallo</b></p></div><p>Eddie &lt;M&gt; eddie@beispiel.de</p>"
        );
    }

    #[test]
    fn data_images_become_inline_parts() {
        // 1×1-PNG
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let html = format!(
            "<p>Hi</p><img src=\"data:image/png;base64,{png}\"><img src=\"data:image/png;base64,{png}\">"
        );
        let built = build(&account(), &draft(), &wrap_html(&html), vec![], false).unwrap();
        let raw = String::from_utf8_lossy(&built.raw);
        assert!(raw.contains("multipart/related"), "{raw}");
        assert!(!raw.contains("data:image"), "{raw}");
        let parsed = mail_parser::MessageParser::default()
            .parse(&built.raw)
            .unwrap();
        let html_part = parsed.body_html(0).unwrap();
        assert_eq!(html_part.matches("cid:bild1.").count(), 2, "{html_part}");
        let inline: Vec<_> = parsed
            .parts()
            .filter(|p| p.content_id().is_some())
            .collect();
        assert_eq!(inline.len(), 1);
        assert_eq!(&inline[0].decoded()[1..4], b"PNG");
    }

    #[test]
    fn invitations_carry_calendar_part_and_file() {
        let ics = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART:20270101T100000Z\r\nSUMMARY:Planung\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let built = build_itip(
            &account(),
            &["Anna <anna@x.de>".into(), "max@y.de".into()],
            "Einladung: Planung",
            "Du bist eingeladen.",
            Some("<p>Du bist eingeladen.</p>"),
            ics,
            "REQUEST",
        )
        .unwrap();
        assert_eq!(built.to, vec!["anna@x.de", "max@y.de"]);
        let raw = String::from_utf8_lossy(&built.raw);
        assert!(raw.contains("text/calendar; method=REQUEST"), "{raw}");
        assert!(raw.contains("einladung.ics"), "{raw}");
        let parsed = crate::parse::parse_body(&built.raw);
        let invite = parsed.invite.unwrap();
        assert_eq!(invite.method, "REQUEST");
        assert_eq!(invite.summary, "Planung");
    }

    #[test]
    fn builds_mime_and_hides_bcc() {
        let built = build(&account(), &draft(), &doc(), vec![], false).unwrap();
        let raw = String::from_utf8_lossy(&built.raw);
        assert!(
            raw.contains("Message-ID: <abc.kolibri@beispiel.de>"),
            "{raw}"
        );
        assert!(raw.contains("In-Reply-To: <orig@x>"));
        assert!(raw.contains("multipart/alternative"));
        assert!(
            !raw.to_lowercase().contains("geheim@"),
            "Bcc im Kopf: {raw}"
        );
        assert_eq!(built.to.len(), 3);
        assert!(built.to.contains(&"geheim@beispiel.de".to_owned()));
        assert_eq!(built.from, "eddie@beispiel.de");

        // Rückweg: mail-parser versteht, was wir bauen.
        let parsed = mail_parser::MessageParser::default()
            .parse(&built.raw)
            .unwrap();
        assert_eq!(parsed.subject(), Some("Grüße aus Köln"));
        assert!(parsed.body_text(0).unwrap().contains("Hallo Anna"));
    }

    #[test]
    fn draft_keeps_bcc_and_allows_no_recipients() {
        let mut d = draft();
        d.to = String::new();
        let built = build(&account(), &d, &doc(), vec![], true).unwrap();
        assert!(String::from_utf8_lossy(&built.raw).contains("geheim@beispiel.de"));
        d.bcc = String::new();
        assert!(build(&account(), &d, &doc(), vec![], true).is_ok());
        assert_eq!(
            build(&account(), &d, &doc(), vec![], false)
                .unwrap_err()
                .kind,
            ErrorKind::Invalid
        );
    }

    #[test]
    fn attachments_are_added() {
        let files = vec![FilePart {
            name: "Rechnung März.pdf".into(),
            data: b"%PDF-1.4 test".to_vec(),
        }];
        let built = build(&account(), &draft(), &doc(), files, false).unwrap();
        let parsed = mail_parser::MessageParser::default()
            .parse(&built.raw)
            .unwrap();
        let att: Vec<_> = parsed.attachments().collect();
        assert_eq!(att.len(), 1);
        assert_eq!(att[0].attachment_name(), Some("Rechnung März.pdf"));
        assert_eq!(&*att[0].decoded(), b"%PDF-1.4 test");
    }

    #[test]
    fn rejects_bad_addresses() {
        let mut d = draft();
        d.to = "kein adresse".into();
        assert_eq!(
            build(&account(), &d, &doc(), vec![], false)
                .unwrap_err()
                .kind,
            ErrorKind::Invalid
        );
        assert_eq!(parse_addresses(" a@b.de; c@d.de, ").unwrap().len(), 2);
    }

    #[test]
    fn subjects_and_references() {
        assert_eq!(prefixed_subject("Re", "Hallo"), "Re: Hallo");
        assert_eq!(prefixed_subject("Re", "AW: Hallo"), "AW: Hallo");
        assert_eq!(prefixed_subject("Fwd", "WG: Hallo"), "WG: Hallo");
        assert_eq!(prefixed_subject("Fwd", "Re: Hallo"), "Fwd: Re: Hallo");
        assert_eq!(
            reply_references(Some("<a@x> <b@x>"), None, "<c@x>"),
            "<a@x> <b@x> <c@x>"
        );
        assert_eq!(reply_references(None, Some("<a@x>"), "c@x"), "<a@x> <c@x>");
    }

    #[test]
    fn formats_and_excludes_addresses() {
        let list = vec![
            Address {
                name: "Anna".into(),
                addr: "anna@x.de".into(),
            },
            Address {
                name: String::new(),
                addr: "ICH@x.de".into(),
            },
            Address {
                name: "Müller, Max".into(),
                addr: "max@x.de".into(),
            },
            Address {
                name: "Anna".into(),
                addr: "anna@x.de".into(),
            },
        ];
        assert_eq!(
            format_addresses(&list, &["ich@x.de"]),
            "Anna <anna@x.de>, \"Müller, Max\" <max@x.de>"
        );
        // Das Ergebnis muss wieder einlesbar sein.
        assert_eq!(
            parse_addresses(&format_addresses(&list, &[]))
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn quotes() {
        let body = kolibri_core::Body {
            html: Some("<p>alt</p>".into()),
            text: Some("Hallo <du>\nZeile 2".into()),
            remote_images: false,
            attachments: vec![],
            preview: String::new(),
            invite: None,
        };
        let q = reply_quote("Anna <anna@x.de>", 0, &body);
        assert!(q.contains("schrieb Anna &lt;anna@x.de&gt;:"), "{q}");
        assert!(
            q.contains("<blockquote><p>Hallo &lt;du&gt;<br>Zeile 2</p></blockquote>"),
            "{q}"
        );
        assert!(german_date(1_700_000_000).contains("2023 um"));
        let f = forward_block("A", "Betreff", 0, "B", &body);
        assert!(f.contains("<strong>Betreff:</strong> Betreff"));
    }

    #[test]
    fn invite_reply_has_calendar_part() {
        let built = build_invite_reply(
            &account(),
            "anna@beispiel.de",
            "Zugesagt: Planung",
            "Eddie hat zugesagt.",
            "BEGIN:VCALENDAR\r\nMETHOD:REPLY\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let raw = String::from_utf8_lossy(&built.raw);
        assert!(raw.contains("text/calendar; method=REPLY"), "{raw}");
        assert_eq!(built.to, ["anna@beispiel.de"]);
        let parsed = mail_parser::MessageParser::default()
            .parse(&built.raw)
            .unwrap();
        assert!(
            parsed
                .parts()
                .any(|p| p.is_content_type("text", "calendar"))
        );
    }

    #[test]
    fn html_helpers() {
        assert_eq!(escape_html("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
        assert_eq!(text_to_html("a\nb\n\nc"), "<p>a<br>b</p><p>c</p>");
        assert_eq!(signature_html("  "), "");
        assert!(new_message_id("x@beispiel.de").ends_with("@beispiel.de"));
        assert_ne!(new_message_id("x@y.de"), new_message_id("x@y.de"));
    }
}
