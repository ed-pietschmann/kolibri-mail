# Kolibri – leichter Mail-Client für macOS

Mail, Kalender und Kontakte in einer schlanken, schnellen, nativ wirkenden Mac-App.
Reduziert auf das Wesentliche: weniger Features, dafür jedes richtig gut.

## Für Claude: Arbeitsweise

- Der Nutzer programmiert etwas, ist aber kein Profi. Wichtige Entscheidungen kurz (2–3 Sätze)
  begründen und genaue Befehle nennen. Kommunikation auf **Deutsch**. Neue UI-Texte immer in `de.ts` und `en.ts`.
- Meilenstein für Meilenstein. Am Ende jedes Meilensteins: App lauffähig, selbst getestet,
  `pnpm lint` grün, `pnpm measure` ausgeführt, Git-Commit, kurze Zusammenfassung
  (gebaut, Messwerte, bekannte Probleme, nächster Schritt).
- Keine stillen Annahmen: bei Unklarheiten fragen oder Wahl begründen.
- Neue Dependencies vorher auf Größe und Wartungszustand prüfen.
- Testen **nie** gegen das echte Postfach des Nutzers (Purelymail), sondern gegen den lokalen
  Testserver (Stalwart, ab M2).
- Diese Datei bei Architekturänderungen und am Ende jedes Meilensteins aktualisieren.
- **Prioritäten des Nutzers:** 1. wenig RAM (deutlich unter Apple Mail – sein RAM ist oft voll), 2. schönes Design – **niemals Design für Performance opfern**. Startzeit ist zweitrangig,
  nicht daran herumoptimieren, solange sie im Rahmen bleibt.

## Getroffene Entscheidungen

| Thema                     | Entscheidung                                                                                                                                                                    |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Name / Bundle-ID          | Kolibri / `app.kolibri.mail`                                                                                                                                                    |
| Stack                     | Tauri 2 (Rust) + Svelte 5 + TypeScript (strict) + Vite, keine UI-Bibliothek                                                                                                     |
| Design                    | Hell + Dunkel nach Systemeinstellung, Vorbild des Nutzers („Casepost“-Stil): flache Seitenleiste, „Tinte“-Knöpfe, Karten-Liste – siehe `DESIGN.md`                              |
| Mail-Anbieter des Nutzers | Purelymail (IMAP/SMTP, vermutlich CalDAV/CardDAV – in M8/M9 prüfen)                                                                                                             |
| Server-Erkennung          | Eingebaute Liste (Purelymail) → Anbieter-Autoconfig → ISPDB → MX → DNS-SRV → Raten → manuell. Nie die volle Adresse senden                                                      |
| Testserver                | Stalwart v1.1.1, wird bei Bedarf mit `scripts/test-server.sh install` in `.dev/` geladen (Server und Test-Mails auf Wunsch gelöscht, zuletzt geprüft 22.11.2026), nur 127.0.0.1 |
| Editor                    | Tiptap 3 (StarterKit + Placeholder), 384 KB / 125 KB gz, **nur per `import()` beim Verfassen geladen** (`src/lib/editor.ts`)                                                    |
| Cache                     | Letzte 6 Monate komplett offline, ältere nur Header                                                                                                                             |
| Threads                   | Ordnerübergreifend (inkl. Gesendet)                                                                                                                                             |
| Signierung                | Kein Apple-Developer-Account → Ad-hoc-signierter Build; Windows unsigniert                                                                                                      |
| DB                        | `rusqlite` (bundled, FTS5) + eigene Migrationen über `user_version`; Mutex-Verbindung (Schreib-Thread ab M3)                                                                    |
| Schlüsselbund             | `security-framework` direkt (statt `keyring` 4.x), hinter Trait `SecretStore`; Dienst `app.kolibri.mail`, Konto `account-<id>`                                                  |
| TLS                       | rustls + aws-lc-rs + `rustls-platform-verifier` (macOS-Zertifikatsspeicher) für IMAP, SMTP und HTTP                                                                             |
| Verfassen                 | Schwebendes Fenster unten rechts im Hauptfenster (kein zweites Fenster = kein extra WebKit-Prozess); Lesen nebenbei möglich                                                     |
| Signaturen/Vorlagen       | Signatur = festes Tiptap-Element (`div[data-kolibri-signature]`, HTML unverändert); Vorlagen = HTML mit `{{inhalt}}`, eingesetzt in Rust beim Senden; data:-Bilder → `cid:`     |
| Symbole                   | Lucide (ISC), nur benötigte Formen in `Icon.svelte` eingebettet, keine Abhängigkeit                                                                                             |
| Lint Frontend             | Prettier + svelte-check (bewusst kein ESLint: viele Abhängigkeiten, TS strict deckt das Wichtigste ab)                                                                          |

## Architektur

```
Kolibri/
├── Cargo.toml              Rust-Workspace (Lints, Profile, gemeinsame Abhängigkeiten)
├── crates/
│   ├── core/               Domänentypen, Fehler (ErrorKind), Traits MailProvider + SecretStore
│   ├── store/              SQLite-Cache; migrations/*.sql werden nur angehängt, nie geändert
│   │                       messages.rs: Nachrichten, Inhalte (getrennte Tabelle), Bilder-Whitelist
│   │                       ops.rs: Aktions-Warteschlange · outbox.rs: Postausgang/Entwürfe
│   │                       threads.rs: Gespräche (thread_root), list_threads, thread_members
│   │                       search.rs: Suchsprache (von:/an:/hat:anhang …) → FTS5-MATCH + Filter
│   │                       contacts.rs: Kontakte, gesammelte Empfänger, Adressvorschläge
│   │                       calendar.rs: Kalender, Termine (ganze ICS), Bereichsabfrage
│   │                       library.rs: Signaturen, Vorlagen, draft_meta (Editor-Stand von Entwürfen)
│   ├── dav/                CardDAV + CalDAV: Erkennung (.well-known), Adressbücher/Kalender,
│   │                       ETag-Abgleich, multiget, PUT/DELETE mit If-Match; xml.rs, vcard.rs,
│   │                       ical.rs (iCalendar lesen/schreiben, RRULE, Zeitzonen über jiff)
│   └── mail/               IMAP (async-imap), SMTP (lettre), TLS, UTF-7-Ordnernamen,
│       ├── parse.rs        mail-parser: Kopfzeilen → Liste, Inhalt → Body (cid → data:-URI), attachment_data
│       ├── compose.rs      MIME bauen (lettre), Adressen, Zitate, Re:/Fwd:, References
│       ├── sanitize.rs     ammonia-Allowlist; erkennt externe Bilder
│       ├── autoconfig/     Servererkennung: xml.rs (Thunderbird-Format), dns.rs (MX/SRV), known.rs
│       └── tests/          live_server.rs – Integrationstests gegen Stalwart
├── src-tauri/              Tauri-App `kolibri`
│   ├── src/accounts.rs     AppState-Logik (testbar) + dünne #[tauri::command]-Hüllen
│   ├── src/keychain.rs     macOS-Schlüsselbund
│   ├── src/sync.rs         Abgleich pro Ordner, Vorladen, SessionPool für interaktive Abrufe
│   ├── src/messages.rs     list_messages, get_message, sync_all (Hintergrund + Ereignisse)
│   ├── src/queue.rs        Offline-Warteschlange: plan → enqueue → drain_ops (Arbeiter je Konto)
│   ├── src/compose.rs      prepare_compose, Senden/Entwürfe (Postausgang), Signatur, Anhänge öffnen
│   ├── src/idle.rs         Push: IDLE-Wächter je Konto auf INBOX + Komplettabgleich alle 10 Min.
│   ├── src/notify.rs       Mitteilungen bei neuer Mail, Dock-Zahl
│   ├── src/contacts.rs     CardDAV-Abgleich im Komplett-Sync, list_contacts, suggest_addresses
│   ├── src/calendar.rs     CalDAV-Abgleich, occurrences, Termine speichern/löschen, Einladungen
│   ├── src/startup.rs      Startzeitmessung, Fenster erst nach Aufbau zeigen
│   └── tauri.conf.json     Fenster (Overlay-Titelleiste, Vibrancy), CSP
├── src/                    Svelte-Frontend
│   ├── app.css             Design-Tokens (Dark-first) + gemeinsame Formular-Stile
│   ├── lib/api.ts          typisierte invoke()-Wrapper, ApiError {kind, detail}
│   ├── lib/accounts.svelte.ts  Konten-Zustand (Cache sofort, Server im Hintergrund)
│   ├── lib/mail.svelte.ts  Sync-Status + `version`-Zähler aus Backend-Ereignissen
│   ├── lib/format.ts       Datum/Größe (de-DE)
│   ├── lib/errors.ts       ErrorKind → deutsche Meldung
│   ├── lib/folders.ts      Ordner-Icons, -Namen, Einrückung
│   ├── lib/i18n/           t('schlüssel') – Texte in de.ts
│   ├── lib/editor.ts       Tiptap-Fabrik (dynamisch geladen)
│   ├── lib/components/     Sidebar, MessageList (virtualisiert, Suche), ReaderPane (Gespräch), MessageCard,
│   │                       CommandPalette (⌘K), AddressInput, ContactsView, CalendarView,
│   │                       TodayView, EventSheet,
│   ├── lib/calendar.ts     Datumshelfer, Bereiche, Spalten für überlappende Termine
│   ├── lib/library.svelte.ts  Signaturen + Vorlagen (lazy), zuletzt gewähltes Design
│   ├── lib/signatureTemplates.ts  Signatur-Layouts (Felder → mailtaugliches HTML)
│   │                       MailFrame, Composer, Sheets
│   └── dev/mock.ts         NUR Entwicklung: Backend-Attrappe für die Browser-Vorschau
├── scripts/measure.sh      Startzeit, RAM, CPU, Bundle-Größe (IDLE_WAIT=60 für echten Leerlauf)
├── scripts/seed-mail.py    Test-Mails per IMAP APPEND einspielen (--bulk N für Lasttest)
├── scripts/test-all.sh     Unit-Tests + alle Servertests in fester Reihenfolge (frischer Server)
├── scripts/gen-templates.py Mail-Designs → migrations/011_designs.sql (neue Designs = neue Migration)
└── scripts/test-server.sh  lokaler Stalwart: install | reset | start | stop | status
```

Datenfluss Konto hinzufügen: `discover_settings` → Nutzer bestätigt → `add_account` prüft IMAP
(inkl. Ordnerliste) und SMTP **parallel** → erst dann DB-Eintrag → Passwort in Schlüsselbund
(bei Fehler DB-Eintrag zurückrollen) → Ordner speichern. Fehler tragen `[imap]`/`[smtp]` im
Detail, damit die UI sagen kann, welcher Server schuld ist.

Sync (M3): pro Ordner EXAMINE → UIDVALIDITY prüfen → `UID SEARCH SINCE` (183 Tage) →
neue Kopfdaten in 200er-Häppchen (neueste zuerst) → verschwundene löschen → Flags abgleichen.
Danach Inhalte ≤ 256 KB vorladen (max. 300 pro Ordner und Durchlauf). Größere/ältere Mails
lädt `get_message` beim Öffnen über den `SessionPool` (eigene Verbindung, nie hinter dem Sync).
Backend meldet `mail-changed`, `folders-changed`, `sync-status`; das Frontend lädt dann nur
die sichtbaren Listenseiten neu. Noch **kein** Polling/IDLE (M6), Sync bei Start und ⌘R.

Mail-Anzeige: HTML wird in Rust mit ammonia gesäubert, dann im `<iframe sandbox="allow-same-origin" srcdoc>`
(ohne `allow-scripts`) mit eigener CSP angezeigt. Externe Bilder sind blockiert, bis „Bilder laden“
(einmalig) oder Absender-Whitelist. HTML-Mails auf weißem „Papier“, Text-Mails im App-Design.
Links öffnen über `tauri-plugin-opener` (nur http/https/mailto erlaubt).

Aktionen (M4): `apply_action(ids, UserAction)` → `queue::plan` gruppiert nach Konto/Ordner und
übersetzt in `Action` (SetSeen, SetFlagged, Move{to, create}, Delete) → `store.enqueue` wendet
sofort lokal an (Flags setzen bzw. `pending_move = 1` → ausgeblendet) → Arbeiter je Konto
überträgt in Reihenfolge über den `SessionPool` (SELECT schreibend; MOVE oder COPY+EXPUNGE).
Verbindungsfehler → `retry_op` + Neuversuch mit Backoff (15 s … 5 Min.); Server lehnt ab →
`fail_op` nimmt lokal zurück + Hinweis in der UI. Vor jedem Sync wird die Warteschlange
abgearbeitet; der Flag-Abgleich überspringt UIDs mit offenen Aktionen. Archiv fehlt → Ordner
„Archive“ wird angelegt. Löschen im Papierkorb → Rückfrage → endgültig. Ereignis `queue-status`.
Tastenkürzel: J/K/↑/↓, E, ⌫/⌘⌫, U/⇧⌘U, S/⇧⌘L, V (Verschieben). Öffnen markiert als gelesen.

Verfassen (M5): `prepare_compose(kind)` baut in Rust den vorbefüllten Entwurf (Empfänger,
Re:/Fwd:, In-Reply-To/References, Signatur, Zitat, Originalanhänge). `send_message`/`save_draft`
bauen sofort die MIME-Nachricht und legen sie in `outbox` (SQLite); derselbe Arbeiter wie die
Aktionen überträgt zuerst den Postausgang: `send` → SMTP → `sent` → Kopie per APPEND in
„Gesendet“ + alte Entwurfsfassungen (gleiche Message-ID, gefunden über ENVELOPE, weil Stalwart
`SEARCH HEADER Message-ID` nicht kann) löschen → Original `\Answered`. Server lehnt ab →
Nachricht wird zum Entwurf + Hinweis. Entwürfe: Autosave alle 30 s, beim Schließen Rückfrage.
Anhänge öffnen: in `~/Library/Caches/app.kolibri.mail/attachments` (beim Start geleert) mit
Quarantäne-Attribut; Quick Look über `qlmanage -p`, Öffnen über `open` (ausführbare Typen gesperrt).
Tastenkürzel neu: ⌘N neu, R/⌘R antworten, A/⇧⌘R allen, F/⇧⌘F weiterleiten, ⌘↩ senden,
⇧⌘A anhängen, ⌘K Link, ⇧⌘N Postfächer abrufen (vorher ⌘R).

Gespräche (M6): `thread_root` = Message-ID der ältesten bekannten Nachricht; beim Einfügen aus
References/In-Reply-To bestimmt (bekannte Vorgänger zuerst), Zwischenglieder werden nachträglich
zusammengeführt; alte Zeilen füllt `backfill_threads` beim Start. Kein Betreff-Raten.
`list_threads` (Fensterfunktionen): eine Zeile pro Gespräch im Bereich, neueste vertritt es,
`thread_count` zählt über alle Ordner (ohne Papierkorb/Spam, Duplikate per Message-ID).
Leseansicht lädt `get_thread` → Karten, nur aufgeklappte laden ihren Inhalt (iframe).
Aktionen wirken auf `inScope` (Gesprächs-Nachrichten im aktuellen Bereich), Markieren nur auf
die eine. Schalter „Nach Konversationen ordnen“ wird in localStorage gemerkt.

Push (M6): je Konto eine IDLE-Verbindung auf „INBOX“ (immer dieser Name, auch vor dem ersten
Sync). Meldung → `spawn_sync(Inbox)`; läuft schon ein Sync, wird danach erneut abgeglichen
(`sync_again`). IDLE alle 9 Min. erneuert; ohne IDLE-Fähigkeit Abfrage alle 2 Min.; alle
10 Min. Komplettabgleich. Mitteilungen nur für neue ungelesene INBOX-Mails ab der alten
UIDNEXT und nur, wenn das Fenster nicht vorne ist (> 3 → Sammelmitteilung). Dock-Zahl =
Ungelesene aller Posteingänge. Fenster schließen blendet nur aus (⌘Q beendet, Dock zeigt es).

Suche (M7): FTS5-Tabelle `messages_fts` inhaltslos (`content=''`, `contentless_delete=1`),
`unicode61 remove_diacritics 2`, Präfixindex 2/3. Gefüllt per Trigger: Kopfzeilen beim Einfügen,
Text (max. 20 000 Zeichen) sobald `message_bodies` geschrieben wird. Suchsprache in Rust
(`store::search::parse`): freie Wörter = Präfix, "Phrasen", von:/from:, an:/to:, betreff:,
hat:anhang, ist:ungelesen/markiert, nach:/vor: (JJJJ-MM-TT oder TT.MM.JJJJ). Nutzereingaben
landen nur in Anführungszeichen im MATCH-Ausdruck (keine Operator-Injektion). Ohne Ordner:
alle außer Papierkorb/Spam. Ergebnisse neueste zuerst, keine Gespräche. ⌘F / „/“ öffnet die
Suche, ⌘K die Befehlspalette (unscharfe Suche über Befehle + Ordner, sonst Mailsuche).
5.038 Mails: 5 Abfragen in 3 ms, Datenbank inkl. Index 3,8 MB.

Kontakte (M8): CardDAV-Suche über `/.well-known/carddav` auf Mail-Domain, IMAP-Host und
IMAP-Host ohne „imap./mail.“ (Purelymail: purelymail.com); bei IMAP auf localhost
`http://localhost:18080` (Stalwart). Ergebnis in `accounts.carddav_home` ('' = keins, nach
24 h erneut suchen). Abgleich bei jedem Komplett-Sync: PROPFIND Depth 1 (ETags) → nur
Geändertes per `addressbook-multiget`. Nur lesen – Bearbeiten noch nicht. Anmeldung mit dem
IMAP-Benutzer/Passwort, HTTPS (HTTP nur zu localhost). Gesammelte Adressen: Empfänger beim
Senden + aus den Gesendet-Ordnern (`collect_from_sent`). Vorschläge: Wortanfang im Namen oder
Adressanfang, Kontakte vor früheren Empfängern (nach Häufigkeit/Aktualität), max. 8.

Kalender (M9): CalDAV wie CardDAV (`/.well-known/caldav`, `calendar-home-set`, Farbe über
Apple-Erweiterung). Termine werden als ganze ICS-Datei gespeichert (inkl. Ausnahmen einer
Serie); `start_unix`/`end_bound` (aus `Calendar::last_end`, NULL = endlos) begrenzen die
Abfrage, Vorkommen rechnet `ical::Calendar::instances` bei jeder Abfrage aus (DAILY/WEEKLY/
MONTHLY/YEARLY, INTERVAL, COUNT, UNTIL, BYDAY inkl. „2MO“/„-1FR“, BYMONTHDAY, EXDATE,
RECURRENCE-ID). Zeitzonen: TZID (IANA + häufige Windows-Namen) über jiff mit der macOS-
Zeitzonendatenbank; Serien werden in Ortszeit gerechnet (bleiben nach Zeitumstellung gleich).
Lokaler Kalender „Auf diesem Mac“ (account_id NULL) existiert immer. Neue Termine: UTC-Zeiten
bzw. VALUE=DATE; auf dem Server per PUT `If-None-Match: *`, Ändern/Löschen mit `If-Match`.
Wiederkehrende Termine: nur anzeigen und ganze Serie löschen. Einladungen: `text/calendar`-
Teil oder `.ics`-Anhang → `Body.invite`; Annehmen trägt in den ersten Kalender des Kontos ein
und schickt eine iMIP-REPLY-Mail über den Postausgang, Ablehnen entfernt den Termin.
Ansichten: Tag/Woche (⌘1/⌘2, 48 px pro Stunde, Jetzt-Linie), Monat (⌘3), ←/→, T, ⌥⌘N neuer
Termin, Doppelklick ins Raster legt einen Termin an. Heute-Übersicht: Termine heute + 7 Tage.

Signaturen & Vorlagen (1.2): Tabelle `signatures` (mehrere je Konto, `is_default`), `templates`
(4 mitgelieferte, fest; eigene aus HTML-Code), `draft_meta` (Vorlage/Signatur/Editor-HTML/Code
eines Entwurfs – nur lokal, weil der Server nur die fertige Mail kennt). Signatur-HTML wird in
Rust mit `sanitize::clean_fragment` gesäubert (kein `<style>`, keine Klassen, kein `position`) und
im Editor als Atom-Knoten angezeigt; Verwaltung per contenteditable + execCommand (behält
Tabellen). Beim Senden: `render_document` = eigener Code | Vorlage mit `{{inhalt}}`/`{{name}}`/
`{{email}}`/`{{betreff}}` | schlichte Hülle; `build` wandelt `data:image` in `cid:`-Teile
(multipart/related). Vorschau über `render_preview` im abgeschotteten iframe (`HtmlPreview`).

Kontakte & Kalender (1.3): DAV-Erkennung folgt Weiterleitungen selbst (reqwest machte aus
PROPFIND bei 301/302 ein GET → Purelymail wurde nie gefunden) und fragt zusätzlich die Wurzel
des Anbieters (nicht der eigenen Mail-Domain); Fehlversuche landen als Warnung im Protokoll.
Migration 010 setzt erfolglose Suchen zurück. Kontakte: lokal (`account_id` NULL, href
`local:<uid>`) oder per vCard-PUT ins erste Adressbuch des Kontos; „Frühere Empfänger“ =
gesammelte Adressen ohne Kontakt (`collected`). Kalender: lokal (`url` `local:<zufall>`; „local“ =
Standard, nicht löschbar), beim Anbieter per MKCALENDAR/PROPPATCH/DELETE, oder aus verbundenen
CalDAV-Konten (`dav_sources`, Passwort im Schlüsselbund unter `dav-<id>`, `calendars.source_id`);
`AppState::dav_of(cal)` wählt den Zugang. Verbundene Konten gleicht der Komplett-Sync des ersten
Kontos ab. Signatur-Vorlagen: `src/lib/signatureTemplates.ts` (6 Layouts aus Feldern). Mail-Designs:
`scripts/gen-templates.py` erzeugt `011_designs.sql` (9 Designs: Alltag, Geschäftlich, Anlässe).

Einladungen (1.2): Termin mit Teilnehmenden → ORGANIZER (Konto des Kalenders, lokal: erstes
Konto) + ATTENDEE, SEQUENCE zählt hoch; iMIP-Mail (`build_itip`: Text + HTML + text/calendar
REQUEST + einladung.ics) über den Postausgang. Entfernte Teilnehmende und gelöschte Termine →
CANCEL. Eingehende REPLY (Status setzen) und CANCEL des Einladenden (Termin entfernen) werden beim
Vorladen im Sync bzw. beim Öffnen der Mail übernommen (`apply_itip`), Ereignis `calendar-changed`.
Zeitänderung setzt Zusagen zurück. Serien weiterhin nur ansehen.

Liste: virtualisiert (78 px Zeilen), lädt Seiten à 100 und behält max. 6 Seiten im Speicher.

Geplante Schichten:

- IMAP: CONDSTORE für schnelleren Flag-Abgleich großer Ordner (später, falls nötig).

## Konventionen

- Rust: `cargo clippy -D warnings`, `rustfmt`, `thiserror`. `unwrap`/`expect`/`panic` sind per
  Workspace-Lint verboten (in Tests mit `#[allow]` erlaubt). `unsafe` nur lokal begründet.
- Frontend: TS strict inkl. `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`.
  Svelte-5-Runes (`$state`, `$derived`, `$props`). Alle UI-Texte über `t()`.
- Farben/Abstände nur über CSS-Variablen aus `app.css`. Animationen ≤ 150 ms (`--dur`).
- Jeder Button braucht `aria-label` oder sichtbaren Text.
- Tauri-Rechte so knapp wie möglich (`capabilities/default.json`).
- Plattform: Tastenkürzel immer über `mod(e)` / `kbd()` aus `src/lib/platform.ts`, nie direkt
  `e.metaKey`. Mac-only-APIs in Rust hinter `#[cfg(target_os = "macos")]`.
- Commits: `M<n>: <was>` bzw. kurze Imperativ-Nachricht.

## Befehle

```bash
pnpm install                 # Abhängigkeiten
pnpm tauri dev               # App im Entwicklungsmodus (Hot Reload)
pnpm tauri build             # Release-Build (.app + .dmg) nach target/release/bundle/
pnpm lint                    # Prettier, svelte-check, rustfmt, clippy
pnpm format                  # automatisch formatieren
pnpm measure                 # Release bauen + Startzeit/RAM/CPU/Größe messen
pnpm update-app              # neue Version bauen, Kolibri beenden, in /Applications ersetzen, starten

scripts/test-all.sh          # ALLE Tests inkl. Server (richtet Stalwart frisch ein)
scripts/test-server.sh install # Testserver laden (falls .dev/ fehlt), dann reset
scripts/test-server.sh reset # Testserver frisch einrichten (test@kolibri.test / kolibri-test)
python3 -I scripts/seed-mail.py [--bulk 5000]  # Test-Mails + 4 Kontakte einspielen
cargo test --workspace       # Unit-Tests
KOLIBRI_TEST_SERVER=1 cargo test -p kolibri-mail --test live_server        # gegen Stalwart
KOLIBRI_TEST_SERVER=1 cargo test -p kolibri --lib -- --ignored             # Ende-zu-Ende + Schlüsselbund
```

Testserver-Ports (nur 127.0.0.1, ohne TLS): IMAP 1143, SMTP 1587, JMAP/HTTP 18080.
In der App manuell einrichten mit Server `localhost`, Verschlüsselung „Keine (nur localhost)“.

Rust liegt unter `~/.cargo` (rustup). Falls `cargo` fehlt: `source "$HOME/.cargo/env"`.

## Meilensteine

1. ✅ Setup: Tauri + Svelte, Fenster-Chrome, Vibrancy, Dark Mode, Lint, CLAUDE.md, Messskript
2. ✅ Konto hinzufügen (Autoconfig, Schlüsselbund), IMAP-Login, Ordnerliste; Stalwart-Testumgebung
3. ✅ Sync → SQLite → virtualisierte Liste + sichere Leseansicht (Bilder-Blockade, Whitelist)
4. ✅ Aktionen + Offline-Warteschlange (archivieren, löschen, markieren, verschieben), J/K/E/R
5. ✅ Schreiben/Senden (SMTP), Antworten/Weiterleiten, Entwürfe, Signatur, Anhänge, Quick Look
6. ✅ Unified Inbox, Threads, IDLE-Push, Benachrichtigungen
7. ✅ FTS5-Suche mit Filtern + Befehlspalette (⌘K)
8. ✅ CardDAV-Kontakte + gesammelte Adressen + Autovervollständigung
9. ✅ CalDAV + lokaler Kalender, Tag/Woche/Monat, Heute-Übersicht, ICS-Einladungen
10. ✅ Feinschliff: Barrierefreiheit, App-Icon, Performance-Bericht, `.dmg` (Version 1.0.0)

## Aktueller Stand

**M10 abgeschlossen – Version 1.0.0.** Alle geplanten Meilensteine fertig. M10: Kontrast-Prüfung
(hell `--text-tertiary` 0,42 → 3:1), Modus „Kontrast erhöhen“ (`prefers-contrast: more`, alle
Grautöne ≥ 4,5:1), „Bewegung reduzieren“ schaltet auch Animationen ab, Zoomen mit ⌘+/⌘−,
App-Symbol (`design/icon.svg` → `design/icon.png` → `pnpm tauri icon design/icon.png`),
`.dmg` über `pnpm dist`, Nutzer-README, Performance-Bericht in `docs/PERFORMANCE.md`.

**1.1.0:** Neues Design nach Vorbild des Nutzers (siehe `DESIGN.md`, Produktgrundlagen in
`PRODUCT.md`): flache Seitenleiste mit Marke und schwarzem „Neue E-Mail“, Suchfeld oben in der
Liste, Karten-Zeilen (104 px, zwei Vorschauzeilen), Aktionsleiste mit Beschriftungen, farbige
Initialen-Avatare. Composer schwebt unten rechts (verkleinern/vergrößern), Lesen und Navigieren
bleiben dabei möglich (ersetzt das automatische Sichern beim Wegklicken aus 1.0.1). Vibrancy
entfernt. Update-Befehl `pnpm update-app` (Tests: `KOLIBRI_APP_DST=<ordner>` installiert woanders,
ohne die laufende App zu beenden). Kontrast-Modus greift jetzt auch im hellen Modus.

**1.5.0:** Englisch zusätzlich zu Deutsch. Sprache = Systemsprache, ermittelt in Rust (`sys-locale`;
die Mac-Web-Ansicht kennt nur die Sprachen des Bundles) und beim Start gesetzt
(`kolibri_core::lang`); Oberfläche fragt `ui_language` und lädt `App.svelte` erst danach
(Datumsformate entstehen beim Import). `?lang=en` in der Vorschau. Texte: `src/lib/i18n/de.ts` +
`en.ts` (gleiche Schlüssel, TS prüft), `locale.intl` für Intl-Formate. Rust: Fehlertexte bleiben
deutsch im Code, `Error::new` übersetzt über die Tabelle in `crates/core/src/lang.rs` (Muster mit
`{}`) – neue Meldung = dort Zeile ergänzen; Mail-Texte (Zitat-Kopf, Weiterleitung, Einladungen,
Mitteilungen, `long_date`) über `lang::tr`/`current()`. Designs: Texte als `{{t:schlüssel}}`
(Rust `DESIGN_TEXTS`, Vorschau `designTexts()`), Namen über `templateName()`; Migration 013.
README.md englisch, README.de.md deutsch.

**1.4.0:** Öffentlich auf GitHub (`ed-pietschmann/kolibri-mail`, MIT) mit Windows-Vorschau.
Windows: Passwörter über `keyring` (Anmeldeinformationsverwaltung, nur unter Windows eingebunden),
Anhänge öffnen über `tauri-plugin-opener` + `Zone.Identifier`-Markierung, zusätzliche gesperrte
Endungen (.exe, .bat …), Fenster schließen beendet die App (kein Dock), `RunEvent::Reopen` nur
macOS, `tauri.windows.conf.json` (normale Titelleiste, NSIS-Installer, nur Benutzerkonto),
jiff bringt die Zeitzonen-DB mit (`tzdb-bundle-platform`). Frontend: `src/lib/platform.ts` –
`mod(e)` statt `e.metaKey` (Strg unter Windows), `kbd('⇧⌘N')` für Kürzel-Anzeigen,
`html[data-platform]`. Windows wird nur in GitHub Actions gebaut/getestet (`ci.yml`), Releases
über Tag `v*` (`release.yml`, tauri-action → Release-Entwurf mit universal .dmg + setup.exe).

**1.3.3:** Termin- und Kalender-Dialog leerten sich bei jedem Abgleich (der `$effect` zum Füllen
las auch die Kalender-/Kontenliste, die bei Sync neu geladen wird). Jetzt nur beim Öffnen gefüllt
(`untrack`). Regel: Formular-Effekte nur von der Identität des geöffneten Objekts abhängig machen.

**1.3.2:** „Kalender hinzufügen → Adresse eintragen“: CalDAV-Adresse (auch eines einzelnen
Kalenders, z. B. Purelymail `…/webdav/<id>/caldav/default/`) mit den Zugangsdaten eines Mail-Kontos
verknüpfen (`link_account_calendar`); das Verzeichnis darüber wird als Kalender-Home genommen,
die Kontakte daneben (`…/carddav/…`) gleich mit. Andere Zugangsdaten → verbundenes Konto wie bisher.

**1.3.1:** Erkennung bricht nicht mehr beim ersten abgelehnten Server ab (imap.purelymail.com
antwortet 401, purelymail.com ist der richtige) – erst wenn kein Kandidat passt, zählt die Ablehnung.

**1.3.0:** Purelymail-Kontakte/-Kalender werden gefunden (Weiterleitungs-Fehler behoben, ungeprüft
gegen den echten Server), Kontakte anlegen/bearbeiten/löschen + „Frühere Empfänger“, Kalender
anlegen/umbenennen/umfärben/löschen, andere CalDAV-Konten verbinden (iCloud, Nextcloud),
Signatur-Vorlagen, 9 neue Mail-Designs.

**1.2.0:** Lucide-Symbole, mehrere HTML-Signaturen je Konto (gestalten oder Code, Logo wird
eingebettet), HTML-Vorlagen (mitgeliefert + eigene + komplette HTML-Mail per Code) mit Vorschau,
Teilnehmende zu Terminen einladen inkl. automatischer Zu-/Absagen. Testserver-Daten gelöscht
(neu mit `scripts/test-server.sh install`).

**1.0.1:** Offener Entwurf blockierte Leseansicht und Kalender (rechts blieb der Composer, die
Liste zeigte einen Platzhalter). Jetzt: Wechsel zu anderer Mail/Ansicht → `Composer.stash()`
sichert Änderungen als Entwurf (Hinweis „Entwurf gesichert“) und schließt; ⌘N aus einer
Ansicht wechselt erst in die Mail-Ansicht. JS-Fehler der Oberfläche landen im App-Protokoll
(`report_frontend_error`).

Noch nicht: wiederkehrende Termine bearbeiten oder neu anlegen, Erinnerungen (VALARM),
Termine per Ziehen verschieben, Aufgaben (VTODO), Klick auf Mitteilung öffnet die Mail, CardDAV/CalDAV bei Purelymail
ungeprüft, RAM beim Schreiben nicht gemessen.

Bekannte Punkte:

- Plattenplatz: Release-Build braucht ~5 GB in `target/`. Vor `pnpm measure` ggf.
  `cargo clean --profile dev` (Debug-Ausgabe, wird neu gebaut).
- `pnpm lint` kann > 10 Min. dauern, wenn clippy neu bauen muss – im Hintergrund starten.
- `.dmg` immer mit `pnpm dist` bauen (= `CI=true tauri build`) – ohne `CI=true` läuft das
  Finder-AppleScript für das DMG-Layout in eine Zeitüberschreitung.
- Ad-hoc-Signatur (`signingIdentity: "-"`): auf anderen Macs einmal Rechtsklick → Öffnen.
- `requestAnimationFrame` feuert im unsichtbaren Startfenster nicht → in `main.ts` `tick()` nutzen.
- Startzeit direkt nach einem Neubau ist 1–2,5 s (macOS prüft das neue Binary einmalig);
  `measure.sh` macht deshalb einen Aufwärm-Start und nimmt den Median aus drei Starts.
- Stalwart v1 speichert Einstellungen in seiner DB, nicht in einer Datei. Einrichtung nur über
  Recovery-Modus + JMAP-Verwaltung (`x:Domain/set`, `x:NetworkListener/set`, `x:Account/set`) –
  siehe `scripts/test-server.sh`. Doku: stalw.art/docs (stalwart.email ist abgeschaltet).
- Nach jedem Neubau kann macOS einmal nach Schlüsselbund-Zugriff fragen (Ad-hoc-Signatur ändert sich).
- Plattenplatz beim Nutzer ist knapp: Debug-Profil ohne volle Debug-Infos; `target/` im Auge behalten.
- App-Bundle wuchs auf 9,3 MB (aws-lc-rs, hickory, reqwest, lettre) – unter Ziel, aber beobachten.
- Stalwart über `scripts/test-server.sh` steuern; das Skript erkennt Instanzen am
  Konfigurationspfad. Eine früher von Hand gestartete Instanz hatte Tests verfälscht
  („stop“ wirkte nicht) – `start` bricht jetzt ab, wenn Port 1143 schon belegt ist.
- Zitate in Antworten nutzen den Textteil der Mail (sauber im Editor); komplexe HTML-Layouts
  werden beim Weiterleiten von Tiptap vereinfacht.
- Quick Look läuft über `qlmanage -p` (kleines Vorschaufenster, kein natives Panel) – in M10 prüfen.
- `measure.sh` zählt nur WebKit-Prozesse, die in den ersten 2 s nach dem Start entstehen –
  vorher wurden fremde WebKit-Prozesse (Safari & Co.) mitgezählt (einmal 760 MB „gemessen“).
- Das App-Raster braucht `grid-template-rows: minmax(0, 1fr)` – sonst wachsen Ansichten über
  die Fensterhöhe hinaus und können nicht selbst scrollen (fiel beim Kalender auf).
- Release-Build protokolliert Warnungen/Fehler nach `~/Library/Logs/app.kolibri.mail/`.
- Stalwart drosselt standardmäßig (IMAP-`rateLimit`, JMAP-Uploads → 429); `test-server.sh reset`
  hebt das IMAP-Limit an, Test-Mails kommen per IMAP APPEND.
- Echte App ohne Klicken testen: Konto per `sqlite3` in die App-DB eintragen und Testpasswort mit
  `security add-generic-password -s app.kolibri.mail -a account-<id> -w … -T <Kolibri.app>`
  hinterlegen (nach jedem Neubau neu anlegen, sonst fragt macOS). Danach wieder entfernen.
- Computer Use ist in dieser Umgebung nicht verfügbar; UI-Prüfung über Browser-Vorschau mit
  `src/dev/mock.ts` (`pnpm dev`, `?mock=5000` / `?mock=empty`, `?lang=en`). Playwright-WebKit
  wurde am 22.11.2026 auf Wunsch gelöscht (bei Bedarf neu: `pnpm dlx playwright install webkit`).
- Echte App automatisiert bedienen (nur Debug-Build): `KOLIBRI_EVAL=<skript.js>` führt nach dem
  Start JS in der Oberfläche aus; Ausgaben per `invoke('report_frontend_error', …)` ins Protokoll.
  Mit `HOME=<testordner>` läuft die App mit eigener Datenbank (Nutzerdaten unberührt; für den
  Schlüsselbund `~/Library/Keychains` in den Testordner verlinken, Konto-ID ≠ echte Konten).
- Composer-Zustände immer mittesten (offener Entwurf + Navigation, klein/groß) – der 1.0.0-Fehler
  fiel nur auf, weil kein Test mit offenem Composer navigierte.
- `scripts/test-all.sh` braucht den Testserver: vorher `scripts/test-server.sh install`.
- RAM messen ohne echtes Konto: `HOME=<leerer Ordner> IDLE_WAIT=60 bash scripts/measure.sh --no-build`
  (die App nutzt sonst die echte Datenbank und gleicht mit Purelymail ab).
- RAM direkt nach einem großen Sync ist höher (~138 MB bei 5.000 Mails), fällt nach ~1 Min.
  auf ~88 MB. Für Vergleiche immer `IDLE_WAIT=60 pnpm measure --no-build`.
- Messwerte schwanken direkt nach `cargo build` (Spotlight/XProtect) – ggf. `--no-build` wiederholen.

### Messwerte

| Meilenstein       | Startzeit           | RAM Leerlauf        | CPU Leerlauf | .app    | .dmg   |
| ----------------- | ------------------- | ------------------- | ------------ | ------- | ------ |
| M1                | ~0,45 s             | ~76 MB              | 0 %          | 4,0 MB  | 2,1 MB |
| M2                | ~0,32 s             | ~73 MB              | 0 %          | 9,3 MB  | –      |
| M3 (5.038 Mails)  | ~0,35 s             | ~88 MB (Spitze 138) | 0 %          | 11 MB   | –      |
| M4 (leer)         | ~0,31 s             | ~71 MB              | 0 %          | 11 MB   | –      |
| M5 (leer)         | ~0,35 s             | ~74–79 MB           | 0 %          | 11 MB   | –      |
| M6 (leer)         | ~0,4 s              | ~80 MB              | 0 %          | 12 MB   | –      |
| M7 (leer)         | ~0,33 s             | ~85 MB              | 0 %          | 12 MB   | –      |
| M8 (leer)         | ~0,37 s             | ~86 MB              | 0 %          | 12 MB   | –      |
| M9 (leer)         | ~0,35 s             | ~84 MB              | 0 %          | 12 MB   | –      |
| M10 (leer)        | ~0,35 s             | ~84 MB              | 0 %          | 12,6 MB | 7,3 MB |
| M10 (5.038 Mails) | ~0,41 s             | ~100–105 MB         | 0–0,4 %      | 12,6 MB | 7,3 MB |
| 1.1.0 (leer)      | (2 s nach Neubau)   | ~89 MB              | 0 %          | 13 MB   | 7,3 MB |
| 1.2.0 (leer)      | (0,46–1,5 s Neubau) | ~86 MB              | 0 %          | 13 MB   | –      |
| 1.3.0 (leer)      | ~0,36 s             | ~82 MB              | 0 %          | 13 MB   | –      |
