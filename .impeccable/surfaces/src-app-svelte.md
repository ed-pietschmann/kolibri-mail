---
version: 1
slug: 'src-app-svelte'
primary_target: 'src/App.svelte'
related_targets: []
---

# Mail window (App shell)

Scope: whole app window – sidebar, message list, reader, floating composer; calendar/contacts/today inherit the tokens. Mode: Operate. Build path: code-led.

Task: daily triage – scan, open, reply, archive; write while reading other mails. Constraints: low RAM, macOS manners, light + dark following the system, no labels feature.

## Direction contract

THESIS: A calm, paper-white mail desk where the content is the colour and the chrome is ink; refuses the blue-filled selected row and translucent utility look of stock mac mail clients.
OWN-WORLD: Flat cool-grey sidebar (#f5f5f7 / dark #141416), white working surfaces, ink-black primary button (inverts to near-white in dark), selection as a soft grey card with 10px radius, hairline separators, gray line icons, system accent only for focus, links and unread dots. SF Pro throughout, bold 600–700 headings.
STORY: The user sees what is new at a glance, opens it in one click, answers in a floating window without losing the mail they read.
FIRST VIEWPORT: Left 232px sidebar: wordmark row under the traffic lights, full-width black „Neue E-Mail“ button, mailbox list with white selected pill. Middle 360px: search pill in the title bar, „Posteingang“ 18px bold with count line, card rows (sender, bold subject, two-line preview). Right: action bar with icon+label buttons (Antworten, Weiterleiten …), 24px bold subject, sender row with avatar; composer floats bottom-right, 560px wide, 14px radius, soft shadow.
FORM: pinned by the user's reference image („Casepost“ shot); seed: none (pinned) — "ich hatte an sowas gedacht" + reference image; user confirmed flat sidebar, floating composer, no labels.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
