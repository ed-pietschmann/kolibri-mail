# Kolibri – Performance-Bericht (Version 1.0.0)

Stand: 20.11.2026 · MacBook (Apple Silicon), macOS · Release-Build (`pnpm dist`)

Wichtigstes Ziel war **wenig Arbeitsspeicher** – deutlich unter Apple Mail –, ohne beim Design
Abstriche zu machen. Startzeit war zweitrangig.

## Ergebnis auf einen Blick

| Messung                                                | Ergebnis               | Ziel     |
| ------------------------------------------------------ | ---------------------- | -------- |
| **RAM im Leerlauf, ohne Konto**                        | **~84 MB**             | < 150 MB |
| **RAM im Leerlauf, 5.038 Mails + Kalender + Kontakte** | **~100–105 MB**        | < 150 MB |
| CPU im Leerlauf (mit Push-Verbindung)                  | 0–0,4 %                | < 1 %    |
| Fenster sichtbar nach Start                            | ~0,35–0,45 s           | < 1 s    |
| App-Größe (.app)                                       | 12,6 MB                | < 20 MB  |
| Download (.dmg)                                        | 7,3 MB                 | –        |
| Datenbank mit 5.038 Mails + Suchindex                  | ~5–9 MB                | –        |
| Suche: 5 Abfragen über 5.038 Mails                     | 3 ms                   | –        |
| Erster Abgleich 5.038 Mails (lokaler Server)           | 0,07–0,2 s (Kopfdaten) | –        |
| Neue Mail per Push sichtbar                            | ~5 s                   | –        |

RAM = „physical footprint“ (wie die Aktivitätsanzeige „Speicher“) aller Prozesse von Kolibri:
App selbst + WebKit-Prozesse für die Oberfläche.

## Wohin der Speicher geht (mit 5.038 Mails)

| Prozess                                    | Leerlauf  |
| ------------------------------------------ | --------- |
| Kolibri (Rust: Sync, Datenbank, IMAP-Push) | ~37–50 MB |
| WebKit – Oberfläche                        | ~46 MB    |
| WebKit – Grafik + Netzwerk                 | ~20 MB    |

Eine Langzeitbeobachtung über 3 Minuten nach dem Start blieb stabil (Oberfläche 46 → 45 MB,
App 52 → 49 MB). In einer von zwei Messungen hat das Messskript einmal 157 MB ermittelt – mit
einem 98-MB-WebKit-Prozess. Dieser Wert ließ sich nicht reproduzieren; vermutlich wurde ein
gleichzeitig gestarteter WebKit-Prozess einer anderen App mitgezählt. Direkt nach einem großen
Abgleich kann der Wert außerdem kurz höher liegen (beobachtet bis ~138 MB), bis WebKit
aufgeräumt hat.

## Warum Kolibri so wenig braucht

- **Tauri statt Electron:** nutzt das in macOS eingebaute WebKit statt eines eigenen Chromiums.
- **Daten liegen auf der Platte, nicht im Speicher:** SQLite mit kleinem Seiten-Cache (8 MB).
- **Virtualisierte Liste:** gezeichnet werden nur die sichtbaren Zeilen; geladen werden Seiten
  à 100 Mails, höchstens 6 Seiten bleiben im Speicher – egal ob 100 oder 100.000 Mails.
- **Inhalte nur bei Bedarf:** Mails werden erst beim Öffnen gerendert; in Gesprächen nur die
  aufgeklappten Nachrichten.
- **Editor nur beim Schreiben:** Tiptap (384 KB) wird erst geladen, wenn eine Mail verfasst wird.
- **Kein zweites Fenster:** Verfassen passiert im Hauptfenster (ein weiteres Fenster bräuchte
  einen eigenen WebKit-Prozess).
- **Push statt Abfragen:** IMAP IDLE – der Server meldet sich; im Warten keine CPU-Last.
- **Suchindex inhaltslos:** FTS5 speichert keine zweite Kopie der Texte.
- **Zeitzonen aus macOS:** keine eigene Zeitzonen-Datenbank im Programm.

## Verlauf über die Meilensteine

| Meilenstein                   | Start  | RAM Leerlauf       | .app    |
| ----------------------------- | ------ | ------------------ | ------- |
| M1 Grundgerüst                | 0,45 s | 76 MB              | 4,0 MB  |
| M2 Konten                     | 0,32 s | 73 MB              | 9,3 MB  |
| M3 Sync + Liste (5.038 Mails) | 0,35 s | 88 MB (Spitze 138) | 11 MB   |
| M4 Aktionen                   | 0,31 s | 71 MB              | 11 MB   |
| M5 Schreiben                  | 0,35 s | 74–79 MB           | 11 MB   |
| M6 Gespräche + Push           | 0,40 s | 80 MB              | 12 MB   |
| M7 Suche                      | 0,33 s | 85 MB              | 12 MB   |
| M8 Kontakte                   | 0,37 s | 86 MB              | 12 MB   |
| M9 Kalender                   | 0,35 s | 84 MB              | 12 MB   |
| **M10 (1.0.0)**, leer         | 0,35 s | **84 MB**          | 12,6 MB |
| **M10 (1.0.0)**, 5.038 Mails  | 0,41 s | **~100–105 MB**    | 12,6 MB |

Ohne Angabe: gemessen ohne Konto. Schwankungen von ±5 MB zwischen Messungen sind normal.

## Methode

- `pnpm measure` (`scripts/measure.sh`): Release-Build, ein Aufwärm-Start (macOS prüft neue
  Programme einmalig), Startzeit = Median aus drei Starts bis „Fenster sichtbar“, dann Leerlauf
  (`IDLE_WAIT`, hier 60–90 s) und Messung mit `top` (RAM = Speicher-Spalte, CPU = Mittel aus
  drei Messpunkten). Gezählt werden nur WebKit-Prozesse, die in den ersten 2 s nach dem Start
  entstehen.
- Daten: lokaler Testserver (Stalwart) mit `scripts/seed-mail.py --bulk 5000` – 5.038 Mails,
  4 Kontakte, 1 Server-Kalender. Nie gegen das echte Postfach gemessen.
- Langzeitbeobachtung mit `footprint -p` alle 15 s.

## Nicht gemessen

- RAM während eine Mail geschrieben wird (Editor geladen) – geschätzt +5–10 MB.
- RAM mit sehr großen Postfächern (> 50.000 Mails) und mit mehreren Konten.
- Ein direkter Vergleich mit Apple Mail (auf Wunsch weggelassen).
