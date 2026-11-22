//! HTML-Säuberung mit `ammonia` (Allowlist-basiert).
//!
//! Entfernt: Skripte, Event-Handler (`on*`), Formulare, iframes, objects,
//! `javascript:`-Links. Behalten: Layout-Attribute und Inline-Styles, damit
//! Newsletter normal aussehen. Externe Bilder bleiben im HTML stehen, werden
//! aber in der Anzeige per CSP blockiert, bis der Nutzer sie erlaubt.
//! `cid:`-Bilder werden durch data:-URIs ersetzt.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ammonia::Builder;

pub struct Clean {
    pub html: String,
    pub remote_images: bool,
}

const EXTRA_TAGS: &[&str] = &["style", "font", "center", "big", "small", "u", "s"];
const EXTRA_ATTRS: &[&str] = &[
    "style",
    "class",
    "id",
    "align",
    "valign",
    "width",
    "height",
    "bgcolor",
    "background",
    "border",
    "cellpadding",
    "cellspacing",
    "color",
    "face",
    "size",
    "dir",
    "lang",
    "title",
];

fn is_remote(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("http:") || u.starts_with("https:") || u.starts_with("//")
}

pub fn clean(html: &str, inline: &HashMap<String, String>) -> Clean {
    let remote = Arc::new(AtomicBool::new(false));
    let remote_in_filter = Arc::clone(&remote);
    let inline: HashMap<String, String> = inline.clone();

    let html = Builder::default()
        .add_tags(EXTRA_TAGS)
        .rm_clean_content_tags(&["style"])
        .add_generic_attributes(EXTRA_ATTRS)
        .url_schemes(HashSet::from([
            "http", "https", "mailto", "tel", "data", "cid",
        ]))
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| {
            match (element, attribute) {
                // cid: → data:-URI (oder entfernen, wenn unbekannt)
                ("img", "src") if value.starts_with("cid:") => {
                    let key = value.trim_start_matches("cid:");
                    inline.get(key).map(|uri| Cow::Owned(uri.clone()))
                }
                ("img", "src") => {
                    if is_remote(value) {
                        remote_in_filter.store(true, Ordering::Relaxed);
                    }
                    Some(Cow::Borrowed(value))
                }
                // data: und cid: nur für Bilder, nie für Links
                (_, "href") if value.starts_with("data:") || value.starts_with("cid:") => None,
                (_, "background") if is_remote(value) => {
                    remote_in_filter.store(true, Ordering::Relaxed);
                    Some(Cow::Borrowed(value))
                }
                _ => Some(Cow::Borrowed(value)),
            }
        })
        .clean(html)
        .to_string();

    // Hintergrundbilder in CSS (style-Attribut oder <style>)
    let lower = html.to_ascii_lowercase();
    let css_remote = ["url(http", "url('http", "url(\"http", "url(//", "@import"]
        .iter()
        .any(|p| lower.contains(p));

    Clean {
        remote_images: remote.load(Ordering::Relaxed) || css_remote,
        html,
    }
}

/// Eigenes HTML-Fragment (Signatur) säubern. Strenger als bei empfangenen Mails:
/// keine `<style>`-Blöcke, keine Klassen/IDs (das Fragment wird im App-Fenster
/// angezeigt und darf dessen Gestaltung nicht verändern), keine `position`.
pub fn clean_fragment(html: &str) -> String {
    let attrs: Vec<&str> = EXTRA_ATTRS
        .iter()
        .copied()
        .filter(|a| !matches!(*a, "class" | "id"))
        .collect();
    Builder::default()
        .add_tags(&["font", "center", "big", "small", "u", "s"])
        .add_generic_attributes(&attrs)
        .url_schemes(HashSet::from(["http", "https", "mailto", "tel", "data"]))
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("img", "src") => Some(Cow::Borrowed(value)),
            (_, "href") if value.trim_start().to_ascii_lowercase().starts_with("data:") => None,
            (_, "style") => {
                let lower = value.to_ascii_lowercase();
                (!(lower.contains("position")
                    || lower.contains("url(")
                    || lower.contains("expression")))
                .then_some(Cow::Borrowed(value))
            }
            _ => Some(Cow::Borrowed(value)),
        })
        .clean(html)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(html: &str) -> Clean {
        clean(html, &HashMap::new())
    }

    #[test]
    fn removes_active_content() {
        let out = c(r#"<p onmouseover="x()">a</p><script>alert(1)</script>
            <iframe src="https://x"></iframe><form action="https://x"><input></form>
            <a href="javascript:alert(1)">link</a><object data="x"></object>"#)
        .html;
        for bad in [
            "script",
            "onmouseover",
            "iframe",
            "<form",
            "<input",
            "javascript:",
            "object",
        ] {
            assert!(!out.contains(bad), "{bad} in {out}");
        }
    }

    #[test]
    fn keeps_layout_and_styles() {
        let out = c(r##"<style>p{color:red}</style><table width="600" bgcolor="#fff"><tr><td style="padding:4px">x</td></tr></table>"##).html;
        assert!(out.contains("p{color:red}"), "{out}");
        assert!(out.contains(r#"width="600""#), "{out}");
        assert!(out.contains("padding:4px"), "{out}");
    }

    #[test]
    fn detects_remote_images() {
        assert!(c(r#"<img src="https://t.example/p.gif">"#).remote_images);
        assert!(c(r#"<div style="background:url(https://x/y.png)">x</div>"#).remote_images);
        assert!(
            c(r#"<table><tr><td background="http://x/y.png">x</td></tr></table>"#).remote_images
        );
        assert!(!c(r#"<img src="data:image/png;base64,AAAA">"#).remote_images);
    }

    #[test]
    fn data_links_are_removed_but_data_images_kept() {
        let out =
            c(r#"<a href="data:text/html,evil">x</a><img src="data:image/png;base64,AAAA">"#).html;
        assert!(!out.contains("data:text/html"), "{out}");
        assert!(out.contains("data:image/png"), "{out}");
    }

    #[test]
    fn fragments_lose_styles_blocks_and_positioning() {
        let out = clean_fragment(
            r##"<style>body{display:none}</style><p class="x" id="y" style="color:red">Hi</p>
            <div style="position:fixed;top:0">z</div><img src="data:image/png;base64,AAAA"><script>x</script>
            <table width="300"><tr><td bgcolor="#eee">t</td></tr></table>"##,
        );
        for bad in [
            "<style",
            "display:none",
            "class=",
            "id=",
            "position",
            "<script",
        ] {
            assert!(!out.contains(bad), "{bad} in {out}");
        }
        for good in ["color:red", "data:image/png", "width=\"300\"", "bgcolor"] {
            assert!(out.contains(good), "{good} fehlt in {out}");
        }
    }

    #[test]
    fn links_get_noopener() {
        let out = c(r#"<a href="https://example.com">x</a>"#).html;
        assert!(out.contains("noopener"), "{out}");
    }
}
