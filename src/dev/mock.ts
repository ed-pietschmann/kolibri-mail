/**
 * NUR ENTWICKLUNG: Attrappe des Rust-Backends für die Browser-Vorschau
 * (`pnpm dev` ohne Tauri). Wird in main.ts nur im Dev-Modus und nur ohne
 * Tauri geladen und landet nicht im Release-Build.
 *
 * URL-Parameter: `?mock=empty` (ohne Konto), `?mock=5000` (Anzahl Mails).
 */
import { emit } from '@tauri-apps/api/event';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { Account, MessageDetail, MessageScope, MessageSummary, UserAction } from '../lib/api';

const params = new URLSearchParams(location.search);
const empty = params.get('mock') === 'empty';
const count = Number(params.get('mock')) || 60;

const server = (host: string, port: number) => ({
  host,
  port,
  security: 'tls' as const,
  username: 'eddie@beispiel.de',
});
const folder = (
  id: number,
  path: string,
  role: Account['folders'][number]['role'],
  delimiter = '/',
) => ({
  id,
  accountId: 1,
  path,
  name: path.split(delimiter).pop() ?? path,
  delimiter,
  role,
  selectable: true,
});

const accounts: Account[] = empty
  ? []
  : [
      {
        id: 1,
        email: 'eddie@beispiel.de',
        displayName: 'Eddie',
        signature: 'Viele Grüße\nEddie',
        imap: server('imap.purelymail.com', 993),
        smtp: server('smtp.purelymail.com', 465),
        folders: [
          folder(1, 'INBOX', 'inbox'),
          folder(2, 'Drafts', 'drafts'),
          folder(3, 'Sent', 'sent'),
          folder(4, 'Archive', 'archive'),
          folder(5, 'Junk', 'junk'),
          folder(6, 'Trash', 'trash'),
          folder(7, 'Projekte', null),
          folder(8, 'Projekte/Kolibri', null),
        ],
      },
    ];

const senders: [string, string][] = [
  ['Anna Schäfer', 'anna@beispiel.de'],
  ['GitHub', 'noreply@github.example'],
  ['Deutsche Bahn', 'service@bahn.example'],
  ['Max Braun', 'max@firma.example'],
  ['Stadtwerke', 'rechnung@stadtwerke.example'],
  ['', 'newsletter@shop.example'],
];
const subjects: [string, string][] = [
  [
    'Projekt Kolibri – Kick-off am Montag',
    'Hallo Eddie, lass uns am Montag um 10 Uhr starten. Agenda folgt.',
  ],
  [
    '[kolibri] Neuer Pull Request #42',
    'Bitte einmal drüberschauen – es geht um die virtualisierte Liste.',
  ],
  [
    'Ihre Reiseverbindung Köln → Berlin',
    'Abfahrt 08:12 Uhr, Gleis 4. Wir wünschen eine angenehme Reise.',
  ],
  ['Kurze Frage zum Angebot', 'Kannst du mir bis Freitag die überarbeitete Version schicken?'],
  ['Rechnung Oktober', 'Ihre Rechnung für Oktober liegt als PDF bei.'],
  [
    'Herbst-Angebote: bis zu 30 % Rabatt 🍂',
    'Nur diese Woche: ausgewählte Artikel stark reduziert.',
  ],
];
const now = Math.floor(Date.now() / 1000);
let messages: MessageSummary[] = Array.from({ length: empty ? 0 : count }, (_, i) => {
  const [fromName, fromAddr] = senders[i % senders.length] ?? ['', ''];
  const [subject, preview] = subjects[i % subjects.length] ?? ['', ''];
  return {
    id: i + 1,
    accountId: 1,
    folderId: 1,
    subject: i < subjects.length ? subject : `${subject} (${i + 1})`,
    fromName,
    fromAddr,
    date: now - i * 3600 * 7 - (i % 5) * 600,
    seen: i % 3 !== 0,
    flagged: i % 11 === 4,
    answered: false,
    hasAttachments: i % 6 === 4,
    preview,
    // Jede 9. Mail ist eine eigene (gesendete) – zeigt „An: …“ in der Liste.
    toDisplay: 'Anna Schäfer',
    toCount: i % 9 === 8 ? 2 : 1,
    outgoing: i % 9 === 8,
    threadCount: i === 0 ? 3 : 1,
  };
});

function inScope(m: MessageSummary, scope: MessageScope): boolean {
  if (scope.kind === 'flagged') return m.flagged;
  if (scope.kind === 'folder') return scope.id === 1;
  return scope.role === 'inbox';
}

function detail(id: number): MessageDetail {
  const m = messages.find((x) => x.id === id) ?? messages[0];
  if (!m) throw { kind: 'notFound', detail: String(id) };
  const html = m.fromAddr.startsWith('newsletter')
    ? `<table width="100%" bgcolor="#f4f4f4" cellpadding="0"><tr><td align="center">
         <table width="560" bgcolor="#ffffff" cellpadding="24"><tr><td>
         <img src="https://picsum.photos/512/160" width="512" alt="Banner">
         <h1 style="color:#222;margin:16px 0 8px">Herbst-Angebote 🍂</h1>
         <p style="color:#555">Bis zu <b>30 %</b> auf ausgewählte Artikel. <a href="https://example.com">Jetzt ansehen</a></p>
         </td></tr></table></td></tr></table>`
    : null;
  return {
    ...m,
    to: [{ name: 'Eddie', addr: 'eddie@beispiel.de' }],
    cc: [],
    senderTrusted: false,
    body: {
      html,
      text: html
        ? null
        : `${m.preview}\n\nMehr Infos: https://example.com/info\n\nViele Grüße\n${m.fromName || m.fromAddr}`,
      remoteImages: Boolean(html),
      attachments: m.hasAttachments
        ? [{ name: 'Rechnung-Oktober.pdf', mime: 'application/pdf', size: 184_320 }]
        : [],
      preview: m.preview,
      // Mail Nr. 4 („Kurze Frage“) enthält eine Termineinladung.
      invite:
        m.id === 4
          ? {
              method: 'REQUEST',
              uid: 'mock-invite',
              summary: 'Abstimmung Angebot',
              location: 'Videocall',
              start: todayAt(15),
              end: todayAt(15) + 1800,
              allDay: false,
              organizerName: 'Max Braun',
              organizerEmail: 'max@firma.example',
              ics: '',
            }
          : null,
    },
  };
}

function todayAt(hour: number, dayOffset = 0, minute = 0): number {
  const d = new Date();
  return Math.floor(
    new Date(d.getFullYear(), d.getMonth(), d.getDate() + dayOffset, hour, minute).getTime() / 1000,
  );
}

type MockCal = {
  id: number;
  accountId: number | null;
  url: string;
  name: string;
  color: string;
  visible: boolean;
  sourceId: number | null;
};
let calendars: MockCal[] = [
  {
    id: 1,
    accountId: null,
    url: 'local',
    name: 'Auf diesem Mac',
    color: '#0a84ff',
    visible: true,
    sourceId: null,
  },
  {
    id: 2,
    accountId: 1,
    url: 'privat',
    name: 'Privat',
    color: '#34c759',
    visible: true,
    sourceId: null,
  },
  {
    id: 3,
    accountId: 1,
    url: 'arbeit',
    name: 'Arbeit',
    color: '#ff9f0a',
    visible: true,
    sourceId: null,
  },
];

let events = [
  {
    eventId: 1,
    calendarId: 3,
    summary: 'Jour fixe Team',
    location: 'Raum 2',
    start: todayAt(10),
    end: todayAt(11),
    allDay: false,
    recurring: true,
  },
  {
    eventId: 2,
    calendarId: 3,
    summary: 'Kick-off Kolibri',
    location: 'Videocall',
    start: todayAt(10, 0, 30),
    end: todayAt(12),
    allDay: false,
    recurring: false,
  },
  {
    eventId: 3,
    calendarId: 2,
    summary: 'Mittagessen mit Anna',
    location: 'Café Fleur',
    start: todayAt(12, 1, 30),
    end: todayAt(13, 1, 30),
    allDay: false,
    recurring: false,
  },
  {
    eventId: 4,
    calendarId: 2,
    summary: 'Geburtstag Lena',
    location: '',
    start: todayAt(0, 2),
    end: todayAt(0, 3),
    allDay: true,
    recurring: true,
  },
  {
    eventId: 5,
    calendarId: 1,
    summary: 'Zahnarzt',
    location: '',
    start: todayAt(8, -1),
    end: todayAt(9, -1),
    allDay: false,
    recurring: false,
  },
  {
    eventId: 6,
    calendarId: 2,
    summary: 'Urlaub',
    location: 'Ostsee',
    start: todayAt(0, 9),
    end: todayAt(0, 14),
    allDay: true,
    recurring: false,
  },
  {
    eventId: 7,
    calendarId: 3,
    summary: 'Review',
    location: '',
    start: todayAt(16),
    end: todayAt(16, 0, 45),
    allDay: false,
    recurring: false,
  },
];

type MockSig = { id: number; accountId: number; name: string; html: string; isDefault: boolean };
let signatures: MockSig[] = [
  {
    id: 1,
    accountId: 1,
    name: 'Standard',
    html: '<p>Viele Grüße<br><b>Eddie</b></p>',
    isDefault: true,
  },
  {
    id: 2,
    accountId: 1,
    name: 'Geschäftlich',
    html:
      '<table cellpadding="0" cellspacing="0"><tr><td style="padding-right:12px;border-right:2px solid #d9542b">' +
      '<b style="font-size:15px">Eddie Beispiel</b><br><span style="color:#6e6e73">Gründer · Kolibri</span></td>' +
      '<td style="padding-left:12px;color:#6e6e73">+49 221 123456<br><a href="https://kolibri.app">kolibri.app</a></td></tr></table>',
    isDefault: false,
  },
];
const shell = (bg: string, inner: string) =>
  `<!doctype html><html><head><meta charset="utf-8"></head><body style="margin:0;padding:32px 12px;background:${bg}">` +
  `<table role="presentation" width="100%" cellpadding="0" cellspacing="0"><tr><td align="center">${inner}</td></tr></table></body></html>`;
type MockTpl = { id: number; name: string; html: string; builtin: boolean };
let templates: MockTpl[] = [
  {
    id: 1,
    name: 'Schlicht',
    builtin: true,
    html: shell(
      '#f4f4f6',
      '<table width="600" style="max-width:600px;width:100%;background:#fff;border-radius:14px"><tr><td style="padding:36px 40px;font:15px/1.6 -apple-system,Helvetica,sans-serif;color:#1d1d1f">{{inhalt}}</td></tr></table>',
    ),
  },
  {
    id: 2,
    name: 'Akzent',
    builtin: true,
    html: shell(
      '#efeff2',
      '<table width="600" style="max-width:600px;width:100%;background:#fff;border-radius:14px;overflow:hidden"><tr><td style="background:#111113;padding:28px 40px;font:700 20px -apple-system,Helvetica,sans-serif;color:#fff">{{name}}</td></tr><tr><td style="padding:36px 40px 28px;font:15px/1.6 -apple-system,Helvetica,sans-serif;color:#1d1d1f">{{inhalt}}</td></tr><tr><td style="padding:18px 40px 26px;border-top:1px solid #ececf0;font:12px -apple-system,Helvetica,sans-serif;color:#8a8a90">{{name}} · {{email}}</td></tr></table>',
    ),
  },
  {
    id: 3,
    name: 'Brief',
    builtin: true,
    html: shell(
      '#f7f3ec',
      '<table width="600" style="max-width:600px;width:100%;background:#fffdf9;border-top:3px solid #2b2620"><tr><td style="padding:40px 48px;font:16px/1.7 Georgia,serif;color:#2b2620">{{inhalt}}</td></tr></table>',
    ),
  },
  {
    id: 4,
    name: 'Ankündigung',
    builtin: true,
    html: shell(
      '#f4f4f6',
      '<table width="600" style="max-width:600px;width:100%;background:#fff;border-radius:14px;overflow:hidden"><tr><td style="height:6px;background:#d9542b"></td></tr><tr><td style="padding:36px 40px 8px;font:700 26px/1.25 -apple-system,Helvetica,sans-serif;color:#111113">{{betreff}}</td></tr><tr><td style="padding:12px 40px 36px;font:15px/1.6 -apple-system,Helvetica,sans-serif;color:#3a3a3f">{{inhalt}}</td></tr></table>',
    ),
  },
];
const escapeHtml = (x: string) =>
  x.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const fillTemplate = (tpl: string, content: string, subject: string) =>
  tpl
    .replace(/\{\{t:(\w+)\}\}/g, (_, k: string) => `[${k}]`)
    .replaceAll('{{name}}', 'Eddie Beispiel')
    .replaceAll('{{email}}', 'eddie@beispiel.de')
    .replaceAll('{{betreff}}', escapeHtml(subject.trim()))
    .replaceAll('{{inhalt}}', content);
const attendeesByEvent = new Map<number, { name: string; email: string; status: string }[]>([
  [
    7,
    [
      { name: 'Anna Schäfer', email: 'anna@beispiel.de', status: 'ACCEPTED' },
      { name: 'Max Weber', email: 'max@firma.example', status: 'DECLINED' },
      { name: '', email: 'lea@studio.example', status: 'NEEDS-ACTION' },
    ],
  ],
]);

let davSources: { id: number; name: string; url: string; username: string }[] = [];
let collectedMock = [
  { name: 'Lea Studio', address: 'lea@studio.example', contact: false },
  { name: '', address: 'rechnung@stadtwerke.example', contact: false },
];

mockWindows('main');
mockIPC(
  (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case 'list_accounts':
        return accounts;
      case 'get_thread': {
        const id = Number(a.id);
        const m = messages.find((x) => x.id === id);
        if (!m) return { members: [], inScope: [] };
        if (id !== 1) return { members: [m], inScope: [id] };
        const earlier = (n: number, outgoing: boolean, preview: string): MessageSummary => ({
          ...m,
          id: 1000 + n,
          folderId: outgoing ? 3 : 1,
          fromName: outgoing ? 'Eddie' : m.fromName,
          fromAddr: outgoing ? 'eddie@beispiel.de' : m.fromAddr,
          outgoing,
          seen: true,
          date: m.date - (3 - n) * 3600 * 5,
          preview,
          subject: outgoing ? `Re: ${m.subject}` : m.subject,
        });
        return {
          members: [
            earlier(1, false, 'Hallo Eddie, hast du Montag Zeit für den Kick-off?'),
            earlier(2, true, 'Klar, 10 Uhr passt mir gut.'),
            { ...m, subject: `Re: ${m.subject}` },
          ],
          inScope: [1001, 1],
        };
      }
      case 'search_messages': {
        // Grobe Nachbildung: Wörter (ohne Filter) in Betreff/Absender/Vorschau.
        const words = String(a.query)
          .toLowerCase()
          .split(/\s+/)
          .filter((w) => w && !w.includes(':'));
        const hits = messages.filter((m) =>
          words.every((w) =>
            `${m.subject} ${m.fromName} ${m.fromAddr} ${m.preview}`.toLowerCase().includes(w),
          ),
        );
        const offset = Number(a.offset);
        return { total: hits.length, items: hits.slice(offset, offset + Number(a.limit)) };
      }
      case 'list_contacts': {
        const people = [
          {
            id: 1,
            accountId: 1,
            name: 'Anna Schäfer',
            org: 'Beispiel GmbH',
            emails: [
              { value: 'anna@beispiel.de', label: '' },
              { value: 'anna.schaefer@firma.example', label: 'Arbeit' },
            ],
            phones: [{ value: '+49 170 1234567', label: 'Mobil' }],
          },
          {
            id: 2,
            accountId: 1,
            name: 'Jürgen Müller',
            org: '',
            emails: [{ value: 'juergen@beispiel.de', label: '' }],
            phones: [],
          },
          {
            id: 3,
            accountId: 1,
            name: 'Lena Wolf',
            org: '',
            emails: [{ value: 'lena@beispiel.de', label: '' }],
            phones: [{ value: '+49 151 555000', label: 'Mobil' }],
          },
          {
            id: 4,
            accountId: 1,
            name: 'Max Braun',
            org: 'Firma AG',
            emails: [{ value: 'max@firma.example', label: '' }],
            phones: [],
          },
        ];
        const f = String(a.filter).toLowerCase();
        return people.filter((p) => !f || JSON.stringify(p).toLowerCase().includes(f));
      }
      case 'suggest_addresses': {
        const all = [
          { name: 'Anna Schäfer', address: 'anna@beispiel.de', contact: true },
          { name: 'Anna Schäfer', address: 'anna.schaefer@firma.example', contact: true },
          { name: 'Andrea Klein', address: 'andrea@beispiel.de', contact: false },
          { name: 'Max Braun', address: 'max@firma.example', contact: true },
        ];
        const q = String(a.prefix).toLowerCase();
        return all.filter(
          (s) =>
            s.name
              .toLowerCase()
              .split(' ')
              .some((w) => w.startsWith(q)) || s.address.startsWith(q),
        );
      }
      case 'list_calendars':
        return calendars;
      case 'calendar_accounts':
        return [1];
      case 'link_account_calendar':
        if (!String(a.url).includes('/'))
          throw { kind: 'invalid', detail: 'Die Server-Adresse ist ungültig.' };
        return { calendars: 1, contacts: true };
      case 'list_dav_sources':
        return davSources;
      case 'create_calendar': {
        if (!String(a.name).trim()) throw { kind: 'invalid', detail: 'Bitte einen Namen angeben.' };
        const id = Date.now();
        calendars = [
          ...calendars,
          {
            id,
            accountId: (a.accountId as number | null) ?? null,
            url: `new-${id}`,
            name: String(a.name),
            color: String(a.color),
            visible: true,
            sourceId: null,
          },
        ];
        return id;
      }
      case 'update_calendar':
        calendars = calendars.map((c) =>
          c.id === Number(a.id) ? { ...c, name: String(a.name), color: String(a.color) } : c,
        );
        return null;
      case 'delete_calendar':
        calendars = calendars.filter((c) => c.id !== Number(a.id) || c.url === 'local');
        return null;
      case 'connect_dav_source': {
        if (String(a.password) !== 'test')
          throw {
            kind: 'auth',
            detail: 'Anmeldung abgelehnt – Benutzername oder Passwort stimmen nicht.',
          };
        const id = Date.now();
        davSources = [
          ...davSources,
          {
            id,
            name: String(a.name) || 'iCloud',
            url: String(a.url),
            username: String(a.username),
          },
        ];
        calendars = [
          ...calendars,
          {
            id: id + 1,
            accountId: null,
            url: `src-${id}`,
            name: 'Familie',
            color: '#af52de',
            visible: true,
            sourceId: id,
          },
        ];
        return id;
      }
      case 'disconnect_dav_source':
        davSources = davSources.filter((x) => x.id !== Number(a.id));
        calendars = calendars.filter((c) => c.sourceId !== Number(a.id));
        return null;
      case 'list_collected': {
        const f = String(a.filter ?? '').toLowerCase();
        return collectedMock.filter(
          (r) => !f || r.address.includes(f) || r.name.toLowerCase().includes(f),
        );
      }
      case 'forget_collected':
        collectedMock = collectedMock.filter((r) => r.address !== a.address);
        return null;
      case 'contact_accounts':
        return [1];
      case 'save_contact': {
        const i = a.input as { name: string; emails: { value: string }[] };
        if (!i.name.trim() && !i.emails.some((e) => e.value.trim()))
          throw { kind: 'invalid', detail: 'Bitte einen Namen oder eine E-Mail-Adresse angeben.' };
        return 1;
      }
      case 'delete_contact':
        return null;
      case 'set_calendar_visible': {
        const c = calendars.find((x) => x.id === Number(a.id));
        if (c) c.visible = Boolean(a.visible);
        return null;
      }
      case 'occurrences':
        return events
          .filter((e) => calendars.find((c) => c.id === e.calendarId)?.visible)
          .filter((e) => e.start < Number(a.to) && e.end > Number(a.from))
          .map((e) => ({
            ...e,
            uid: String(e.eventId),
            color: calendars.find((c) => c.id === e.calendarId)?.color ?? '',
            cancelled: false,
          }));
      case 'event_detail': {
        const e = events.find((x) => x.eventId === Number(a.id));
        if (!e) throw { kind: 'notFound', detail: 'Termin' };
        return {
          id: e.eventId,
          calendarId: e.calendarId,
          summary: e.summary,
          location: e.location,
          description: '',
          allDay: e.allDay,
          start: e.start,
          end: e.end,
          recurring: e.recurring,
          organizer: attendeesByEvent.has(e.eventId)
            ? { name: 'Eddie Beispiel', email: 'eddie@beispiel.de', status: '' }
            : null,
          attendees: attendeesByEvent.get(e.eventId) ?? [],
          invitedByOther: false,
        };
      }
      case 'save_event': {
        const i = a.input as {
          id: number | null;
          calendarId: number;
          summary: string;
          location: string;
          allDay: boolean;
          start: number;
          end: number;
          attendees: { name: string; email: string }[];
        };
        if (!i.summary.trim()) throw { kind: 'invalid', detail: 'Bitte einen Titel eingeben.' };
        const bad = i.attendees.find((p) => !p.email.includes('@'));
        if (bad) throw { kind: 'invalid', detail: `Ungültige Adresse: ${bad.email}` };
        const ev = {
          eventId: i.id ?? Date.now(),
          calendarId: i.calendarId,
          summary: i.summary,
          location: i.location,
          start: i.start,
          end: i.end,
          allDay: i.allDay,
          recurring: false,
        };
        events = [...events.filter((e) => e.eventId !== i.id), ev];
        const before = attendeesByEvent.get(ev.eventId) ?? [];
        attendeesByEvent.set(
          ev.eventId,
          i.attendees.map((p) => ({
            ...p,
            status: before.find((b) => b.email === p.email)?.status ?? 'NEEDS-ACTION',
          })),
        );
        return ev.eventId;
      }
      case 'delete_event':
        events = events.filter((e) => e.eventId !== Number(a.id));
        return null;
      case 'respond_invite':
        if (a.accept)
          events.push({
            eventId: 99,
            calendarId: 3,
            summary: 'Abstimmung Angebot',
            location: 'Videocall',
            start: todayAt(15),
            end: todayAt(15) + 1800,
            allDay: false,
            recurring: false,
          });
        return null;
      case 'unread_counts':
        return { 1: messages.filter((m) => !m.seen).length, 8: 2 };
      case 'list_messages': {
        const scope = a.scope as MessageScope;
        const all = messages.filter((m) => inScope(m, scope));
        const offset = Number(a.offset);
        return { total: all.length, items: all.slice(offset, offset + Number(a.limit)) };
      }
      case 'get_message':
        if (Number(a.id) > 1000) return detail(1);
        return new Promise((r) => setTimeout(() => r(detail(Number(a.id))), 120));
      case 'apply_action': {
        const ids = a.ids as number[];
        const action = a.action as UserAction;
        if (action.type === 'trash' && params.get('notrash') !== null)
          throw { kind: 'notFound', detail: 'Dieses Konto hat keinen Papierkorb-Ordner' };
        if (action.type === 'markSeen' || action.type === 'flag') {
          for (const m of messages)
            if (ids.includes(m.id)) {
              if (action.type === 'markSeen') m.seen = action.value;
              else m.flagged = action.value;
            }
        } else {
          messages = messages.filter((m) => !ids.includes(m.id));
        }
        setTimeout(() => void emit('mail-changed', { accountId: 1, folderId: 1 }), 0);
        return null;
      }
      case 'prepare_compose': {
        const kind = a.kind as { type: string; messageId?: number };
        const src = kind.messageId ? detail(kind.messageId) : null;
        const def = signatures.find((x) => x.accountId === 1 && x.isDefault);
        const sig = def ? `<div data-kolibri-signature="${def.id}">${def.html}</div>` : '';
        const quote = src
          ? `<p>Am Mittwoch, 19. November 2026 um 14:05 schrieb ${src.fromName || src.fromAddr}:</p><blockquote><p>${src.preview}</p></blockquote>`
          : '';
        return {
          accountId: 1,
          draftId: `mock-${Date.now()}@beispiel.de`,
          to: kind.type === 'reply' || kind.type === 'replyAll' ? (src?.fromAddr ?? '') : '',
          cc: kind.type === 'replyAll' ? 'max@firma.example' : '',
          bcc: '',
          subject: src ? (kind.type === 'forward' ? 'Fwd: ' : 'Re: ') + src.subject : '',
          html: `<p></p>${sig}${quote}`,
          attachments:
            kind.type === 'forward' && src?.body.attachments.length
              ? [
                  {
                    kind: 'original',
                    messageId: src.id,
                    index: 0,
                    name: 'Rechnung-Oktober.pdf',
                    size: 184_320,
                  },
                ]
              : [],
          inReplyTo: null,
          references: null,
          replyToMessage: kind.messageId ?? null,
          signatureId: def?.id ?? null,
          templateId: null,
          code: null,
        };
      }
      case 'send_message':
      case 'save_draft':
        if ((a.draft as { to: string }).to.includes('kaputt'))
          throw { kind: 'invalid', detail: 'Ungültige Adresse: kaputt' };
        return null;
      case 'file_infos':
        return (a.paths as string[]).map((path) => ({
          kind: 'file',
          path,
          name: path.split('/').pop(),
          size: 48_213,
        }));
      case 'plugin:dialog|open':
        return ['/Users/test/Dokumente/Angebot.pdf'];
      case 'plugin:dialog|save':
        return null;
      case 'render_preview': {
        const d = a.draft as {
          html: string;
          subject: string;
          templateId: number | null;
          code: string | null;
        };
        if (d.code !== null) return fillTemplate(d.code, '', d.subject);
        const tpl = templates.find((x) => x.id === d.templateId);
        return tpl
          ? fillTemplate(tpl.html, d.html, d.subject)
          : `<!doctype html><html><body style="font:14px/1.5 -apple-system,sans-serif;color:#1d1d1f">${d.html}</body></html>`;
      }
      case 'list_signatures':
        return signatures;
      case 'save_signature': {
        const sig = a.signature as MockSig;
        if (!sig.name.trim()) throw { kind: 'invalid', detail: 'Bitte einen Namen angeben.' };
        const html = sig.html.replace(/<script[\s\S]*?<\/script>/gi, '');
        const first = !signatures.some((x) => x.accountId === sig.accountId);
        const saved = {
          ...sig,
          html,
          id: sig.id || Date.now(),
          isDefault: sig.isDefault || first,
        };
        signatures = signatures
          .filter((x) => x.id !== saved.id)
          .map((x) =>
            saved.isDefault && x.accountId === saved.accountId ? { ...x, isDefault: false } : x,
          );
        signatures.push(saved);
        return saved;
      }
      case 'delete_signature':
        signatures = signatures.filter((x) => x.id !== Number(a.id));
        return null;
      case 'set_default_signature':
        signatures = signatures.map((x) =>
          x.accountId === Number(a.accountId) ? { ...x, isDefault: x.id === a.id } : x,
        );
        return null;
      case 'list_templates':
        return templates;
      case 'save_template': {
        const tpl = a.template as MockTpl;
        if (tpl.builtin) throw { kind: 'invalid', detail: 'Mitgelieferte Vorlagen sind fest.' };
        if (!tpl.name.trim()) throw { kind: 'invalid', detail: 'Bitte einen Namen angeben.' };
        const saved = { ...tpl, id: tpl.id || Date.now() };
        templates = [...templates.filter((x) => x.id !== saved.id), saved];
        return saved;
      }
      case 'delete_template':
        templates = templates.filter((x) => x.builtin || x.id !== Number(a.id));
        return null;
      case 'discard_draft':
      case 'open_attachment':
      case 'save_attachment':
      case 'dismiss_failed':
      case 'sync_all':
      case 'sync_account':
      case 'trust_image_sender':
      case 'report_ready':
        return null;
      case 'discover_settings':
        throw { kind: 'notFound', detail: 'Attrappe' };
      default:
        throw { kind: 'internal', detail: `Attrappe kennt ${cmd} nicht` };
    }
  },
  { shouldMockEvents: true },
);
