# Kolibri

[Deutsch](README.de.md) · **English**

Mail, calendar and contacts in one lean, fast desktop app for **macOS** and **Windows**
(preview). Focused on the essentials – and light on memory at roughly 85–105 MB
(details: [docs/PERFORMANCE.md](docs/PERFORMANCE.md)).

Built with [Tauri 2](https://tauri.app) (Rust) and Svelte 5. The interface is available in English
and German and follows your system language.

## Download

Get the latest version from **[Releases](https://github.com/ed-pietschmann/kolibri-mail/releases/latest)**:

| System                                      | File                      |
| ------------------------------------------- | ------------------------- |
| macOS 13 or later (Apple Silicon and Intel) | `Kolibri_…_universal.dmg` |
| Windows 10/11 (64-bit) – **preview**        | `Kolibri_…_x64-setup.exe` |

### macOS

1. Open the `.dmg` and drag **Kolibri** into your **Applications** folder.
2. Kolibri is not notarized by Apple (that requires a paid developer account). On first launch,
   **right-click → Open**. If macOS still blocks it: **System Settings → Privacy & Security →
   “Open Anyway”**.
3. When you first save a password, macOS asks whether Kolibri may use the keychain; on the first
   push, whether it may send notifications.

### Windows (preview)

1. Run `…_x64-setup.exe`. Kolibri is installed for your user account only – no administrator
   rights needed.
2. Kolibri is not code-signed. If you see “Windows protected your PC”, click **More info → Run
   anyway**.
3. Passwords are stored in the Windows Credential Manager.

The Windows version is new and not widely tested yet. Differences from the Mac: shortcuts use
**Ctrl** instead of ⌘, attachments open directly in their default app (no Quick Look), and
closing the window quits Kolibri. Please report problems as an
[issue](https://github.com/ed-pietschmann/kolibri-mail/issues).

## Features

- **Mail:** IMAP/SMTP, automatic server detection, offline cache of the last 6 months,
  conversations across folders, push (IMAP IDLE), notifications, unread badge.
- **Reading:** safe display (no scripts, remote images blocked until you allow them), preview,
  open and save attachments.
- **Writing:** rich text, reply/forward, drafts (saved automatically), attachments, address
  suggestions; sending works offline too (goes out once you are back online).
- **Signatures:** several per account – from templates, designed (color, size, links, logo) or
  as HTML code; pick one while writing. Sidebar → “…” next to the account → “Signatures …”.
- **Designs:** nine built-in email designs (everyday, business, occasions), your own templates
  from HTML code, or paste a complete HTML email – with a preview of how it arrives.
- **Actions:** archive, delete, move, flag – instantly, even offline.
- **Search:** instant and local, with filters like `von:anna hat:anhang nach:2026-01-01`
  (from / has:attachment / after).
- **Calendar:** day/week/month, Today overview, local and CalDAV, accept invitations; create
  your own calendars, connect other accounts (iCloud, Nextcloud) via “Manage calendars”;
  invite people to your events – they get an email invitation, and replies show up on the event.
- **Contacts:** create, edit, delete (with your provider or only on this computer), plus
  everyone you have written to.

## Keyboard shortcuts

On Windows, use **Ctrl** instead of ⌘ (⇧ = Shift, ⌥ = Alt).

| Key          | Action                         |
| ------------ | ------------------------------ |
| ⌘N           | New email                      |
| ⇧⌘N          | Get mail                       |
| ⌘K           | Command palette                |
| ⌘F or /      | Search                         |
| J / K, ↓ / ↑ | Next / previous message        |
| R, ⌘R        | Reply                          |
| A, ⇧⌘R       | Reply all                      |
| F, ⇧⌘F       | Forward                        |
| E            | Archive                        |
| ⌫            | Delete                         |
| S, ⇧⌘L       | Flag                           |
| U, ⇧⌘U       | Read / unread                  |
| V            | Move to folder                 |
| ⌘↩           | Send (while writing)           |
| ⇧⌘A          | Add attachment                 |
| ⌘K           | Insert link (while writing)    |
| ⌘1 / ⌘2 / ⌘3 | Calendar: day / week / month   |
| ← / →, T     | Calendar: previous/next, today |
| ⌥⌘N          | New event                      |
| ⌘+ / ⌘−      | Zoom in / out                  |

## Privacy

- No account with us, no telemetry: Kolibri only talks to your own mail, calendar and contacts
  servers.
- Passwords are stored only in the macOS keychain or the Windows Credential Manager.
- During server detection, at most the domain of your address leaves your computer – never the
  full address.
- Remote images (tracking pixels) are blocked until you allow them.
- Everything else stays local (macOS: `~/Library/Application Support/app.kolibri.mail/`,
  Windows: `%APPDATA%\app.kolibri.mail\`).

## Accessibility

Screen-reader labels for all controls, full keyboard operation, focus rings, and the system’s
“Reduce motion” and “Increase contrast” settings are respected; zoom with ⌘+ / ⌘−.

## Building from source

Requirements: [Rust](https://rustup.rs) (≥ 1.90), Node.js 20+ and [pnpm](https://pnpm.io); on
Windows also the [Tauri prerequisites](https://tauri.app/start/prerequisites/) (Visual Studio
Build Tools, WebView2).

```bash
pnpm install
pnpm tauri dev        # development mode
pnpm tauri build      # installers in target/release/bundle/
scripts/test-all.sh   # all tests incl. a local test mail server (macOS)
```

On the Mac, `pnpm update-app` builds, replaces and restarts the installed app in one step.
Architecture, decisions and commands are documented (in German) in [CLAUDE.md](CLAUDE.md), the
design in [DESIGN.md](DESIGN.md). New UI texts go into both `src/lib/i18n/de.ts` and
`src/lib/i18n/en.ts`.

Releasing a new version: bump the version in `package.json`, `Cargo.toml` and
`src-tauri/tauri.conf.json`, commit, then `git tag v1.5.0 && git push origin v1.5.0`. GitHub
builds macOS and Windows and creates a draft release.

## License

[MIT](LICENSE)
