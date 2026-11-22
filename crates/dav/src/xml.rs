//! WebDAV-`multistatus`-Antworten lesen. Namensräume werden ignoriert
//! (nur lokale Namen) – Server benutzen beliebige Präfixe.

use quick_xml::Reader;
use quick_xml::events::Event;

/// Eine `<response>` aus einer `<multistatus>`-Antwort.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Response {
    pub href: String,
    /// Eigenschaften mit Status 200: (lokaler Name, Textinhalt).
    pub props: Vec<(String, String)>,
    /// Typen aus `<resourcetype>` (z. B. „collection“, „addressbook“).
    pub types: Vec<String>,
    /// `<href>` innerhalb von Eigenschaften (z. B. current-user-principal).
    pub prop_hrefs: Vec<(String, String)>,
}

impl Response {
    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    }

    pub fn prop_href(&self, name: &str) -> Option<&str> {
        self.prop_hrefs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Default)]
struct PropStat {
    ok: bool,
    props: Vec<(String, String)>,
    types: Vec<String>,
    hrefs: Vec<(String, String)>,
}

pub fn parse_multistatus(xml: &str) -> Vec<Response> {
    // Kein trim_text: Leerzeichen neben Entitäten (&amp;) gehören zum Text.
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<Response> = None;
    let mut propstat: Option<PropStat> = None;
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name().as_ref().to_owned();
                match name.as_str() {
                    "response" => current = Some(Response::default()),
                    "propstat" => propstat = Some(PropStat::default()),
                    _ => {}
                }
                stack.push(name);
                text.clear();
            }
            Ok(Event::Empty(e)) => {
                let name = e.local_name().as_ref().to_owned();
                if let Some(ps) = propstat.as_mut() {
                    if stack.last().is_some_and(|p| p == "resourcetype") {
                        ps.types.push(name);
                    } else if stack.last().is_some_and(|p| p == "prop") {
                        ps.props.push((name, String::new()));
                    }
                }
            }
            Ok(Event::Text(t)) => text.push_str(&t.into_inner()),
            Ok(Event::CData(t)) => text.push_str(&t.into_inner()),
            Ok(Event::GeneralRef(r)) => {
                // Entitäten wie &amp; &lt; &quot; (in vCards und ETags üblich).
                let name = r.into_inner();
                text.push_str(match name.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
            }
            Ok(Event::End(_)) => {
                let Some(name) = stack.pop() else { continue };
                let parent = stack.last().map(String::as_str);
                let value = std::mem::take(&mut text).trim().to_owned();
                match (name.as_str(), parent) {
                    ("response", _) => {
                        if let Some(r) = current.take() {
                            out.push(r);
                        }
                    }
                    ("propstat", _) => {
                        if let (Some(ps), Some(r)) = (propstat.take(), current.as_mut())
                            && ps.ok
                        {
                            r.props.extend(ps.props);
                            r.types.extend(ps.types);
                            r.prop_hrefs.extend(ps.hrefs);
                        }
                    }
                    ("status", Some("propstat")) => {
                        if let Some(ps) = propstat.as_mut() {
                            ps.ok = value.contains(" 200");
                        }
                    }
                    ("href", Some("response")) => {
                        if let Some(r) = current.as_mut() {
                            r.href = value;
                        }
                    }
                    ("href", Some(owner)) => {
                        let owner = owner.to_owned();
                        if let Some(ps) = propstat.as_mut() {
                            ps.hrefs.push((owner, value));
                        }
                    }
                    (prop, Some("prop")) => {
                        if let Some(ps) = propstat.as_mut() {
                            ps.props.push((prop.to_owned(), value));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Für XML-Anfragen: Text sicher einsetzen.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_stalwart_style_response() {
        let xml = r#"<?xml version="1.0"?><D:multistatus xmlns:D="DAV:" xmlns:B="urn:ietf:params:xml:ns:carddav">
<D:response><D:href>/dav/card/</D:href><D:propstat><D:prop>
<D:current-user-principal><D:href>/dav/pal/a%40b/</D:href></D:current-user-principal>
<B:addressbook-home-set><D:href>/dav/card/a%40b/</D:href></B:addressbook-home-set>
<D:resourcetype><D:collection/><B:addressbook/></D:resourcetype>
<D:getetag>&quot;123&quot;</D:getetag>
</D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
<D:propstat><D:prop><D:displayname/></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>
</D:response>
<D:response><D:href>/x.vcf</D:href><D:propstat><D:prop><B:address-data>BEGIN:VCARD
FN:A &amp; B
END:VCARD</B:address-data></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
</D:multistatus>"#;
        let r = parse_multistatus(xml);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].href, "/dav/card/");
        assert_eq!(
            r[0].prop_href("current-user-principal"),
            Some("/dav/pal/a%40b/")
        );
        assert_eq!(
            r[0].prop_href("addressbook-home-set"),
            Some("/dav/card/a%40b/")
        );
        assert_eq!(r[0].types, ["collection", "addressbook"]);
        assert_eq!(r[0].prop("getetag"), Some("\"123\""));
        assert_eq!(
            r[0].prop("displayname"),
            None,
            "404-Eigenschaften werden ignoriert"
        );
        assert!(r[1].prop("address-data").unwrap().contains("FN:A & B"));
    }
}
