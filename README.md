# Kolibri

Mail, Kalender und Kontakte in einer schlanken, schnellen Desktop-App für **macOS** und
**Windows** (Vorschau). Reduziert auf das Wesentliche – und mit rund 85–105 MB Arbeitsspeicher
sparsam (Details: [docs/PERFORMANCE.md](docs/PERFORMANCE.md)).

Gebaut mit [Tauri 2](https://tauri.app) (Rust) und Svelte 5. Oberfläche auf Deutsch.

## Download

Die aktuelle Version gibt es unter **[Releases](https://github.com/ed-pietschmann/kolibri-mail/releases/latest)**:

| System                                        | Datei                     |
| --------------------------------------------- | ------------------------- |
| macOS 13 oder neuer (Apple Silicon und Intel) | `Kolibri_…_universal.dmg` |
| Windows 10/11 (64 Bit) – **Vorschau**         | `Kolibri_…_x64-setup.exe` |

### macOS

1. Die `.dmg` öffnen und **Kolibri** in den Ordner **Programme** ziehen.
2. Kolibri ist nicht bei Apple notariell beglaubigt (dafür bräuchte es ein kostenpflichtiges
   Entwicklerkonto). Beim ersten Start daher **Rechtsklick → Öffnen**. Falls macOS es trotzdem
   blockiert: **Systemeinstellungen → Datenschutz & Sicherheit → „Dennoch öffnen“**.
3. Beim ersten Speichern eines Passworts fragt macOS, ob Kolibri den Schlüsselbund benutzen
   darf, beim ersten Push, ob Kolibri Mitteilungen senden darf.

### Windows (Vorschau)

1. `…_x64-setup.exe` starten. Kolibri wird nur für dein Benutzerkonto installiert, ohne
   Administratorrechte.
2. Kolibri ist nicht signiert. Bei „Der Computer wurde durch Windows geschützt“ auf
   **Weitere Informationen → Trotzdem ausführen** klicken.
3. Passwörter landen in der Windows-Anmeldeinformationsverwaltung.

Die Windows-Version ist neu und noch wenig erprobt. Unterschiede zum Mac: Tastenkürzel mit
**Strg** statt ⌘, Anhänge öffnen direkt im Standardprogramm (keine Schnellansicht), Schließen
des Fensters beendet Kolibri. Fehler bitte als [Issue](https://github.com/ed-pietschmann/kolibri-mail/issues) melden.

## Was Kolibri kann

- **Mail:** IMAP/SMTP, automatische Servererkennung, Offline-Cache der letzten 6 Monate,
  Gespräche über alle Ordner, Push (IMAP IDLE), Mitteilungen, Dock-Zahl.
- **Lesen:** sichere Anzeige (keine Skripte, externe Bilder blockiert bis zur Freigabe),
  Anhänge per Quick Look ansehen, öffnen, sichern.
- **Schreiben:** formatierter Text, Antworten/Weiterleiten, Entwürfe (automatisch gesichert),
  Anhänge, Adressvorschläge; Senden auch offline (geht raus, sobald wieder online).
- **Signaturen:** mehrere pro Konto, aus Vorlagen, gestaltet (Farbe, Größe, Links, Logo) oder als HTML-Code;
  beim Schreiben wählbar. Seitenleiste → „…“ neben dem Konto → „Signaturen …“.
- **Designs:** neun fertige Designs (Alltag, Geschäftlich, Anlässe), eigene Vorlagen aus
  HTML-Code oder eine komplette HTML-Mail einfügen – mit Vorschau, wie sie ankommt.
- **Aktionen:** archivieren, löschen, verschieben, markieren – sofort, auch offline.
- **Suche:** sofort und lokal, mit Filtern wie `von:anna hat:anhang nach:2026-01-01`.
- **Kalender:** Tag/Woche/Monat, Heute-Übersicht, lokal und CalDAV, Einladungen annehmen;
  eigene Kalender anlegen, andere Konten (iCloud, Nextcloud) verbinden („Kalender verwalten“);
  eigene Termine mit Teilnehmenden – sie bekommen eine Einladung per Mail, Zu-/Absagen
  erscheinen automatisch am Termin.
- **Kontakte:** anlegen, bearbeiten, löschen (beim Anbieter oder nur auf dem Mac), dazu alle
  früheren Empfänger.

## Tastenkürzel

Unter Windows jeweils **Strg** statt ⌘ (⇧ = Umschalt, ⌥ = Alt).

| Taste        | Aktion                         |
| ------------ | ------------------------------ |
| ⌘N           | Neue E-Mail                    |
| ⇧⌘N          | Postfächer abrufen             |
| ⌘K           | Befehlspalette                 |
| ⌘F oder /    | Suchen                         |
| J / K, ↓ / ↑ | Nächste / vorherige Mail       |
| R, ⌘R        | Antworten                      |
| A, ⇧⌘R       | Allen antworten                |
| F, ⇧⌘F       | Weiterleiten                   |
| E            | Archivieren                    |
| ⌫            | Löschen                        |
| S, ⇧⌘L       | Markieren                      |
| U, ⇧⌘U       | Gelesen / ungelesen            |
| V            | In Ordner bewegen              |
| ⌘↩           | Senden (beim Schreiben)        |
| ⇧⌘A          | Anhang hinzufügen              |
| ⌘K           | Link einfügen (beim Schreiben) |
| ⌘1 / ⌘2 / ⌘3 | Kalender: Tag / Woche / Monat  |
| ← / →, T     | Kalender: blättern, heute      |
| ⌥⌘N          | Neuer Termin                   |
| ⌘+ / ⌘−      | Größer / kleiner               |

## Datenschutz

- Kein Konto bei uns, keine Telemetrie: Kolibri spricht nur mit deinen Mail-, Kalender- und
  Kontakte-Servern.
- Passwörter liegen nur im macOS-Schlüsselbund bzw. in der Windows-Anmeldeinformationsverwaltung.
- Bei der Servererkennung verlässt höchstens die Domain der Adresse den Rechner, nie die volle Adresse.
- Externe Bilder (Tracking-Pixel) werden blockiert, bis du sie freigibst.
- Alles Weitere bleibt lokal (macOS: `~/Library/Application Support/app.kolibri.mail/`,
  Windows: `%APPDATA%\app.kolibri.mail\`).

## Barrierefreiheit

VoiceOver-Beschriftungen für alle Bedienelemente, vollständige Tastaturbedienung,
Fokus-Rahmen, „Bewegung reduzieren“ und „Kontrast erhöhen“ aus den macOS-Bedienungshilfen
werden berücksichtigt, Zoomen mit ⌘+ / ⌘−.

## Selbst bauen

Voraussetzungen: [Rust](https://rustup.rs) (≥ 1.90), Node.js 20+ und [pnpm](https://pnpm.io);
unter Windows zusätzlich die [Tauri-Voraussetzungen](https://tauri.app/start/prerequisites/)
(Visual Studio Build Tools, WebView2).

```bash
pnpm install
pnpm tauri dev        # Entwicklungsmodus
pnpm tauri build      # Installationsdateien nach target/release/bundle/
scripts/test-all.sh   # alle Tests inkl. lokalem Testserver (macOS)
```

Auf dem Mac baut, ersetzt und startet `pnpm update-app` die installierte Version in einem
Schritt. Architektur, Entscheidungen und Befehle stehen in [CLAUDE.md](CLAUDE.md), das Design in
[DESIGN.md](DESIGN.md).

Neue Version veröffentlichen: Versionsnummer in `package.json`, `Cargo.toml` und
`src-tauri/tauri.conf.json` erhöhen, committen, dann `git tag v1.4.0 && git push origin v1.4.0`.
GitHub baut macOS und Windows und legt einen Release-Entwurf an.

## Lizenz

[MIT](LICENSE)
