//! Gegen den lokalen Testserver (scripts/test-server.sh + seed-mail.py):
//! `KOLIBRI_TEST_SERVER=1 cargo test -p kolibri-dav --test live_carddav`

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;

use kolibri_core::ErrorKind;
use kolibri_dav::{Dav, candidates, vcard};

#[tokio::test]
async fn discovers_and_syncs_contacts() {
    if std::env::var("KOLIBRI_TEST_SERVER").is_err() {
        eprintln!("übersprungen");
        return;
    }
    let dav = Dav::new("test@kolibri.test", "kolibri-test", true).unwrap();
    let home = dav
        .discover_home(&candidates("test@kolibri.test", "localhost"))
        .await
        .unwrap()
        .expect("kein Adressbuch-Verzeichnis gefunden");
    let books = dav.address_books(&home).await.unwrap();
    assert!(!books.is_empty(), "keine Adressbücher");

    let changes = dav.sync_book(&books[0].url, &HashMap::new()).await.unwrap();
    let cards: Vec<_> = changes
        .changed
        .iter()
        .filter_map(|c| vcard::parse(&c.vcard))
        .collect();
    assert!(cards.len() >= 4, "{cards:?}");
    let anna = cards
        .iter()
        .find(|c| c.name == "Anna Schäfer")
        .expect("Anna fehlt");
    assert_eq!(anna.emails.len(), 2);
    assert_eq!(anna.org, "Beispiel GmbH");

    // Zweiter Abgleich mit bekannten ETags: nichts Neues; Unbekanntes gilt als gelöscht.
    let mut known: HashMap<String, String> = changes
        .changed
        .iter()
        .map(|c| (c.href.clone(), c.etag.clone()))
        .collect();
    known.insert("/dav/card/weg.vcf".into(), "\"x\"".into());
    let again = dav.sync_book(&books[0].url, &known).await.unwrap();
    assert!(again.changed.is_empty(), "{:?}", again.changed);
    assert_eq!(again.removed, ["/dav/card/weg.vcf"]);

    // Falsches Passwort wird erkannt.
    let bad = Dav::new("test@kolibri.test", "falsch", true).unwrap();
    let err = bad
        .discover_home(&candidates("test@kolibri.test", "localhost"))
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Auth);
}

#[tokio::test]
async fn calendar_roundtrip() {
    if std::env::var("KOLIBRI_TEST_SERVER").is_err() {
        return;
    }
    use kolibri_dav::{calendar_candidates, ical};
    let dav = Dav::new("test@kolibri.test", "kolibri-test", true).unwrap();
    let home = dav
        .discover_calendar_home(&calendar_candidates("test@kolibri.test", "localhost"))
        .await
        .unwrap()
        .expect("kein Kalender-Verzeichnis");
    let cals = dav.calendars(&home).await.unwrap();
    assert!(!cals.is_empty(), "keine Kalender");
    let cal = &cals[0].url;

    let uid = format!("kolibri-test-{}", std::process::id());
    let ics = ical::write_event(&ical::NewEvent {
        uid: &uid,
        summary: "Testtermin",
        location: "Köln",
        description: "",
        all_day: false,
        start: 1_800_000_000,
        end: 1_800_003_600,
        now: 1_790_000_000,
        organizer: None,
        attendees: &[],
        sequence: 0,
        method: None,
        cancelled: false,
    });
    let href = dav.put_event(cal, &uid, &ics, None).await.unwrap();
    // Doppelt anlegen geht nicht (If-None-Match).
    assert_eq!(
        dav.put_event(cal, &uid, &ics, None).await.unwrap_err().kind,
        ErrorKind::Duplicate
    );

    let changes = dav.sync_calendar(cal, &HashMap::new()).await.unwrap();
    let mine = changes
        .changed
        .iter()
        .find(|c| c.href == href)
        .expect("Termin nicht gefunden");
    let parsed = ical::parse(&mine.vcard);
    assert_eq!(parsed.events[0].summary, "Testtermin");
    assert_eq!(parsed.instances(0, i64::MAX / 2)[0].start, 1_800_000_000);

    // Ändern mit falschem ETag wird abgelehnt, mit richtigem klappt es.
    let changed = ics.replace("Testtermin", "Geändert");
    assert_eq!(
        dav.replace_event(cal, &href, &changed, "\"falsch\"")
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Duplicate
    );
    dav.replace_event(cal, &href, &changed, &mine.etag)
        .await
        .unwrap();

    dav.delete_event(cal, &href, "").await.unwrap();
    let after = dav.sync_calendar(cal, &HashMap::new()).await.unwrap();
    assert!(after.changed.iter().all(|c| c.href != href));
}
