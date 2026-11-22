# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Tauri 2 desktop app for macOS; the UI is a WKWebView, so web design rules apply, with native macOS
affordances: traffic lights, overlay title bar, system accent colour, SF Pro.)

## Users

One person (the owner) using their own Mac every day for personal mail, calendar and contacts with a
Purelymail account. Not a professional developer. Their Mac's memory is often full, which is why the
app exists.

## Product Purpose

Kolibri is a lightweight mail client with calendar and contacts: fewer features than Apple Mail, each
done well, at clearly lower memory use (idle ~85–105 MB). Success: it is pleasant enough to replace
Apple Mail for daily use.

## Positioning

A Tauri app with an on-disk cache, a virtualised list, inline/floating composing (no second window,
no extra WebKit process) and IMAP IDLE push: it stays small, while looking like a modern, polished
mail app rather than a utility.

## Operating Context

Daily triage of an inbox: scan the list, open a mail, reply, archive, delete. Composing while still
able to read other mails. Calendar day/week/month and a today overview. Keyboard shortcuts (J/K, E,
R, ⌘N, ⌘K palette). German UI.

## Capabilities and Constraints

- Mail (IMAP/SMTP), conversations, search, offline queue, drafts, signature, attachments.
- Calendar (CalDAV + local), contacts (CardDAV), invitations.
- Priorities, in order: low RAM; beautiful design, never sacrificed for performance; startup time
  secondary.
- No labels/tags feature (explicitly not wanted, 2026-11-20).
- All colours and spacing through CSS variables in `src/app.css`; animations ≤ 150 ms.

## Brand Commitments

- Name Kolibri, hummingbird app icon (`design/icon.svg`).
- Visual reference chosen by the user (2026-11-20): a clean light mail UI in the style of a
  "Casepost" Dribbble shot, with a light flat sidebar, black Compose button, cards in the list,
  a large subject in the reader, and a floating compose window bottom-right. It must have a matching
  dark variant that follows the system setting. The sidebar is flat (no vibrancy).

## Evidence on Hand

No real customers or testimonials; single-user personal tool. The mock data in `src/dev/mock.ts` is
synthetic.

## Product Principles

1. Small footprint first: no feature may add a resident process or large always-loaded library.
2. Look finished: every view, state and empty state is designed, not default.
3. Mac manners: keyboard-first, system accent, follows light/dark.
4. Never lose the user's writing: drafts are saved automatically.

## Accessibility & Inclusion

Honour „Kontrast erhöhen“ (`prefers-contrast: more`) and „Bewegung reduzieren“; all controls are
labelled; text contrast ≥ 4.5:1 for body text.
