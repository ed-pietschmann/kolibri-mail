//! Minimaler vCard-Leser (RFC 6350, auch 3.0): nur was Kolibri anzeigt.

use serde::Serialize;

/// Wert mit Bezeichnung (z. B. „Arbeit“).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Labeled {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Card {
    pub uid: String,
    pub name: String,
    pub org: String,
    pub emails: Vec<Labeled>,
    pub phones: Vec<Labeled>,
}

/// Fortsetzungszeilen (beginnen mit Leerzeichen/Tab) anhängen.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
        } else if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }
    lines
}

/// `\,` `\;` `\n` `\\` auflösen.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Strukturierten Wert an unmaskierten `;` teilen.
fn split_structured(value: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut escaped = false;
    for c in value.chars() {
        if escaped {
            if let Some(last) = parts.last_mut() {
                last.push('\\');
                last.push(c);
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ';' {
            parts.push(String::new());
        } else if let Some(last) = parts.last_mut() {
            last.push(c);
        }
    }
    parts
        .iter()
        .map(|p| unescape(p).trim().to_owned())
        .collect()
}

/// TYPE-Parameter → deutsche Bezeichnung.
fn label(params: &str) -> String {
    let lower = params.to_lowercase();
    let has = |t: &str| {
        lower
            .split(';')
            .filter_map(|p| p.strip_prefix("type="))
            .flat_map(|v| v.split(','))
            .chain(lower.split(';').filter(|p| !p.contains('=')))
            .any(|v| v.trim_matches('"') == t)
    };
    if has("cell") {
        "Mobil".into()
    } else if has("work") {
        "Arbeit".into()
    } else if has("home") {
        "Privat".into()
    } else {
        String::new()
    }
}

/// Liest die erste vCard aus dem Text.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(',', "\\,")
        .replace(';', "\\;")
        .replace('\n', "\\n")
}

/// Kontakt als vCard 3.0 (versteht jeder CardDAV-Server).
pub fn write(card: &Card) -> String {
    let mut lines = vec![
        "BEGIN:VCARD".to_owned(),
        "VERSION:3.0".into(),
        "PRODID:-//Kolibri//Kolibri Mail//DE".into(),
        format!("UID:{}", card.uid),
        format!("FN:{}", esc(card.name.trim())),
    ];
    // N: Nachname;Vorname – einfach am letzten Leerzeichen trennen.
    let name = card.name.trim();
    let (given, family) = name.rsplit_once(' ').unwrap_or(("", name));
    lines.push(format!("N:{};{};;;", esc(family), esc(given)));
    if !card.org.trim().is_empty() {
        lines.push(format!("ORG:{}", esc(card.org.trim())));
    }
    for e in &card.emails {
        let kind = if e.label.is_empty() {
            "INTERNET".to_owned()
        } else {
            format!("INTERNET,{}", e.label.to_uppercase())
        };
        lines.push(format!("EMAIL;TYPE={kind}:{}", e.value.trim()));
    }
    for p in &card.phones {
        let kind = if p.label.is_empty() {
            "VOICE".to_owned()
        } else {
            p.label.to_uppercase()
        };
        lines.push(format!("TEL;TYPE={kind}:{}", p.value.trim()));
    }
    lines.push("END:VCARD".into());
    lines.iter().map(|l| format!("{l}\r\n")).collect()
}

pub fn parse(text: &str) -> Option<Card> {
    let mut card = Card::default();
    let mut in_card = false;
    let mut structured_name = None;
    for line in unfold(text) {
        let Some((head, value)) = line.split_once(':') else {
            continue;
        };
        // Gruppenpräfix („item1.EMAIL“) entfernen.
        let head = head.rsplit_once('.').map_or(head, |(_, h)| h);
        let (prop, params) = head.split_once(';').unwrap_or((head, ""));
        match prop.to_uppercase().as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VCARD") => in_card = true,
            "END" if value.eq_ignore_ascii_case("VCARD") => break,
            _ if !in_card => {}
            "UID" => card.uid = unescape(value).trim().to_owned(),
            "FN" => card.name = unescape(value).trim().to_owned(),
            "N" => {
                // Nachname;Vorname;Weitere;Präfix;Suffix
                let p = split_structured(value);
                let given = p.get(1).cloned().unwrap_or_default();
                let family = p.first().cloned().unwrap_or_default();
                structured_name = Some(format!("{given} {family}").trim().to_owned());
            }
            "ORG" => {
                card.org = split_structured(value)
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            "EMAIL" => {
                let v = unescape(value)
                    .trim()
                    .trim_start_matches("mailto:")
                    .to_owned();
                if v.contains('@') {
                    card.emails.push(Labeled {
                        value: v,
                        label: label(params),
                    });
                }
            }
            "TEL" => {
                let v = unescape(value).trim().trim_start_matches("tel:").to_owned();
                if !v.is_empty() {
                    card.phones.push(Labeled {
                        value: v,
                        label: label(params),
                    });
                }
            }
            _ => {}
        }
    }
    if !in_card {
        return None;
    }
    if card.name.is_empty() {
        card.name = structured_name
            .filter(|n| !n.is_empty())
            .or_else(|| (!card.org.is_empty()).then(|| card.org.clone()))
            .or_else(|| card.emails.first().map(|e| e.value.clone()))
            .unwrap_or_default();
    }
    Some(card)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn written_cards_parse_back() {
        let card = Card {
            uid: "k-1".into(),
            name: "Anna Maria Schäfer".into(),
            org: "Schäfer, Söhne; Co".into(),
            emails: vec![Labeled {
                value: "anna@x.de".into(),
                label: "work".into(),
            }],
            phones: vec![Labeled {
                value: "+49 221 1".into(),
                label: String::new(),
            }],
        };
        let text = write(&card);
        assert!(text.contains("N:Schäfer;Anna Maria;;;\r\n"), "{text}");
        let back = parse(&text).unwrap();
        assert_eq!(back.uid, "k-1");
        assert_eq!(back.name, card.name);
        assert_eq!(back.org, card.org);
        assert_eq!(back.emails[0].value, "anna@x.de");
        assert_eq!(back.phones[0].value, "+49 221 1");
    }

    #[test]
    fn parses_vcard4_with_folding_groups_and_escapes() {
        let text = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:1\r\nFN:Anna Sch\r\n äfer\r\n\
N:Schäfer;Anna;;;\r\nORG:Beispiel GmbH;Vertrieb\r\n\
EMAIL;TYPE=work:anna@firma.de\r\nitem1.EMAIL;type=INTERNET;type=HOME:anna@privat.de\r\n\
TEL;TYPE=\"cell,voice\":+49 170 1234567\r\nNOTE:Zeile 1\\nZeile 2\\, mit Komma\r\nEND:VCARD\r\n";
        let c = parse(text).unwrap();
        assert_eq!(c.uid, "urn:uuid:1");
        assert_eq!(c.name, "Anna Schäfer");
        assert_eq!(c.org, "Beispiel GmbH, Vertrieb");
        assert_eq!(c.emails.len(), 2);
        assert_eq!(
            c.emails[0],
            Labeled {
                value: "anna@firma.de".into(),
                label: "Arbeit".into()
            }
        );
        assert_eq!(c.emails[1].label, "Privat");
        assert_eq!(
            c.phones[0],
            Labeled {
                value: "+49 170 1234567".into(),
                label: "Mobil".into()
            }
        );
    }

    #[test]
    fn vcard3_and_name_fallbacks() {
        let c = parse("BEGIN:VCARD\nVERSION:3.0\nN:Müller;Max;;;\nEMAIL;TYPE=INTERNET,WORK:max@x.de\nEND:VCARD\n").unwrap();
        assert_eq!(c.name, "Max Müller");
        assert_eq!(c.emails[0].label, "Arbeit");
        let only_mail = parse("BEGIN:VCARD\nEMAIL:a@b.de\nEND:VCARD").unwrap();
        assert_eq!(only_mail.name, "a@b.de");
        assert!(parse("kein vcard").is_none());
        assert_eq!(unescape(r"a\,b\;c\\d"), r"a,b;c\d");
    }
}
