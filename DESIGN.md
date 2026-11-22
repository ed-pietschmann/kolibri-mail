# Kolibri – Design

Stand 1.2.0. Vorbild vom Nutzer gewählt (helle, ruhige Mail-Oberfläche im Stil eines
„Casepost“-Entwurfs). Hell und Dunkel folgen der Systemeinstellung. Alle Werte stehen als
CSS-Variablen in `src/app.css`; Komponenten verwenden nur diese.

## Grundidee

Papierweiße Arbeitsflächen, die Inhalte tragen die Farbe, die Bedienelemente sind „Tinte“.
Keine blau gefüllten Auswahlzeilen, keine durchscheinende Seitenleiste.

## Farben (Token)

| Token                  | Hell                   | Dunkel            | Zweck                             |
| ---------------------- | ---------------------- | ----------------- | --------------------------------- |
| `--sidebar-bg`         | #f5f5f7                | #141416           | Seitenleiste, Fensterfläche       |
| `--surface`            | #ffffff                | #1b1b1e           | Liste, Leseansicht, Composer      |
| `--text`               | #111113                | weiß 92 %         | Haupttext                         |
| `--text-secondary`     | Text 62 %              | weiß 60 %         | Vorschau, Metadaten               |
| `--text-tertiary`      | Text 48 %              | weiß 40 %         | Abschnittstitel, Datum            |
| `--icon`               | Text 55 %              | weiß 55 %         | Linien-Icons                      |
| `--ink` / `--ink-text` | #111113 / weiß         | #f2f2f4 / #111113 | Hauptknöpfe (Neue E-Mail, Senden) |
| `--list-selected`      | Text 5 %               | weiß 8 %          | Ausgewählte Karte                 |
| `--sidebar-selected`   | weiß + feiner Schatten | weiß 9 %          | Gewählter Seitenleisten-Eintrag   |
| `--accent`             | Systemakzent           | Systemakzent      | Fokus, Links, Ungelesen-Punkt     |
| `--flag`               | #e68c00                | #ffb340           | Markierung                        |

„Kontrast erhöhen“ (`prefers-contrast: more`) hebt alle Grautöne auf ≥ 4,5:1 an; die Blöcke
stehen bewusst **nach** dem Hell-Block, sonst würden sie überschrieben.

## Typografie

SF Pro (Systemschrift). Basis 13 px. Überschriften fett mit leicht engerer Laufweite:
Listentitel 19 px/700, Betreff in der Leseansicht 24 px/700, Marke 17 px/700. Zahlen tabellarisch.

## Formen und Tiefe

- Karten und Menüs: `--card-radius` 10 px; Composer 14 px; Knöpfe 8–9 px.
- Tiefe nur über weiche Schatten mit Versatz (`--float-shadow`, `--sheet-shadow`), keine
  Rahmen-plus-Schatten-Kombination.
- Trennlinien 1 px `--separator`, in Listen eingerückt; bei Hover/Auswahl ausgeblendet.

## Bausteine

- **Seitenleiste** (232 px): Markenzeile unter den Fensterknöpfen, schwarzer „Neue E-Mail“-
  Knopf, Einträge 32 px mit grauen Icons; Auswahl = weiße Fläche mit feinem Schatten.
- **Liste** (360 px): Suchfeld (Pille) in der Titelleiste, darunter Titel + Anzahl;
  Zeilen 104 px als Karten (Absender, fetter Betreff, zwei Vorschauzeilen), Ungelesen = Punkt.
- **Leseansicht:** Aktionsleiste mit Icon + Beschriftung (unter 640 px Breite nur Icons),
  großer Betreff, Absender mit farbigem Initialen-Avatar (Farbe fest aus der Adresse).
- **Composer:** schwebend unten rechts (580 × 600), verkleinern auf Kopfzeile, vergrößern über
  Liste und Leseansicht; Fußleiste mit schwarzem „Senden“, Anhang, Formatierung, Papierkorb.

- **Symbole:** Lucide (Linien, runde Enden), 1,75 px Strich bis 16 px, sonst 1,6.
- **Signaturen/Vorlagen:** Bearbeitung und Vorschau immer auf weißem „Papier“ (so sieht es der
  Empfänger); im Dunkelmodus steht die Signatur im Editor auf einer weißen Karte. Verwaltung als
  breites Blatt mit grauer Liste links (wie die Seitenleiste) und Arbeitsfläche rechts.
- **Teilnehmende:** Zeilen mit Name, Adresse und Status (Zugesagt `--success`, Abgesagt
  `--danger`, Offen grau).

## Bewegung

≤ 150 ms (`--dur`), nur für Zustände (Hover, Auftauchen des Composers). „Bewegung reduzieren“
schaltet alles ab.
