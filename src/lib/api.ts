/**
 * Typisierte Schnittstelle zum Rust-Backend.
 * Die Typen spiegeln die Rust-Structs (serde, camelCase).
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Security = 'tls' | 'startTls' | 'none';

export interface ServerConfig {
  host: string;
  port: number;
  security: Security;
  username: string;
}

export type FolderRole =
  'inbox' | 'drafts' | 'sent' | 'archive' | 'junk' | 'trash' | 'all' | 'flagged';

export interface Folder {
  id: number;
  accountId: number;
  path: string;
  name: string;
  delimiter: string | null;
  role: FolderRole | null;
  selectable: boolean;
}

export interface Account {
  id: number;
  email: string;
  displayName: string;
  imap: ServerConfig;
  smtp: ServerConfig;
  folders: Folder[];
  signature: string;
}

export type DiscoverySource = 'provider' | 'ispdb' | 'known' | 'mx' | 'srv' | 'guess';

export interface Discovered {
  imap: ServerConfig;
  smtp: ServerConfig;
  source: DiscoverySource;
}

export type ErrorKind =
  | 'connect'
  | 'timeout'
  | 'tls'
  | 'auth'
  | 'protocol'
  | 'notFound'
  | 'duplicate'
  | 'invalid'
  | 'keychain'
  | 'storage'
  | 'internal';

/** Fehler aus dem Backend: Art + technisches Detail. */
export class ApiError extends Error {
  constructor(
    readonly kind: ErrorKind,
    readonly detail: string,
  ) {
    super(detail);
  }

  /** Welcher Server war schuld? (Backend markiert mit [imap]/[smtp]) */
  get server(): 'imap' | 'smtp' | null {
    const m = /^\[(imap|smtp)\]/.exec(this.detail);
    return m ? (m[1] as 'imap' | 'smtp') : null;
  }
}

function toApiError(err: unknown): ApiError {
  if (err && typeof err === 'object' && 'kind' in err && 'detail' in err) {
    const e = err as { kind: ErrorKind; detail: string };
    return new ApiError(e.kind, e.detail);
  }
  return new ApiError('internal', String(err));
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    throw toApiError(err);
  }
}

export interface Address {
  name: string;
  addr: string;
}

export interface MessageSummary {
  id: number;
  accountId: number;
  folderId: number;
  subject: string;
  fromName: string;
  fromAddr: string;
  /** Unix-Sekunden */
  date: number;
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  hasAttachments: boolean;
  preview: string;
  toDisplay: string;
  toCount: number;
  outgoing: boolean;
  threadCount: number;
}

export interface Attachment {
  name: string;
  mime: string;
  size: number;
}

export interface Invite {
  method: string;
  uid: string;
  summary: string;
  location: string;
  start: number;
  end: number;
  allDay: boolean;
  organizerName: string;
  organizerEmail: string;
  ics: string;
}

export interface CalendarInfo {
  id: number;
  accountId: number | null;
  url: string;
  name: string;
  color: string;
  visible: boolean;
  /** Verbundenes CalDAV-Konto */
  sourceId: number | null;
}

export interface Occurrence {
  eventId: number;
  calendarId: number;
  color: string;
  uid: string;
  summary: string;
  location: string;
  /** Unix-Sekunden */
  start: number;
  end: number;
  allDay: boolean;
  recurring: boolean;
}

export interface Person {
  email: string;
  name: string;
  status: string;
}

export interface EventDetail {
  id: number;
  calendarId: number;
  summary: string;
  location: string;
  description: string;
  allDay: boolean;
  start: number;
  end: number;
  recurring: boolean;
  organizer: Person | null;
  attendees: Person[];
  /** Von jemand anderem eingeladen: Teilnehmende nicht änderbar. */
  invitedByOther: boolean;
}

export interface AttendeeInput {
  name: string;
  email: string;
}

export interface EventInput {
  id: number | null;
  calendarId: number;
  summary: string;
  location: string;
  description: string;
  allDay: boolean;
  start: number;
  end: number;
  attendees: AttendeeInput[];
}

export interface Body {
  html: string | null;
  text: string | null;
  remoteImages: boolean;
  attachments: Attachment[];
  preview: string;
  invite: Invite | null;
}

export interface MessageDetail extends MessageSummary {
  to: Address[];
  cc: Address[];
  body: Body;
  senderTrusted: boolean;
}

export type MessageScope =
  { kind: 'folder'; id: number } | { kind: 'role'; role: FolderRole } | { kind: 'flagged' };

export interface MessagePage {
  total: number;
  items: MessageSummary[];
}

export interface MailChanged {
  accountId: number;
  folderId: number | null;
}

export interface SyncStatus {
  accountId: number;
  syncing: boolean;
  error: { kind: ErrorKind; detail: string } | null;
}

export type UserAction =
  | { type: 'markSeen'; value: boolean }
  | { type: 'flag'; value: boolean }
  | { type: 'archive' }
  | { type: 'trash' }
  | { type: 'moveTo'; folderId: number }
  | { type: 'deleteForever' };

export interface QueueStatus {
  accountId: number;
  pending: number;
  failed: number;
  offline: boolean;
  error: { kind: ErrorKind; detail: string } | null;
}

export type ComposeAttachment =
  | { kind: 'file'; path: string; name: string; size: number }
  | { kind: 'original'; messageId: number; index: number; name: string; size: number };

export interface ComposeDraft {
  accountId: number;
  draftId: string;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  html: string;
  attachments: ComposeAttachment[];
  inReplyTo: string | null;
  references: string | null;
  replyToMessage: number | null;
  signatureId: number | null;
  templateId: number | null;
  /** Eigener HTML-Code (ersetzt Editor und Vorlage). */
  code: string | null;
}

export interface ContactInput {
  id: number | null;
  /** Nur bei neuen Kontakten: Konto (CardDAV) oder null = auf diesem Mac */
  accountId: number | null;
  name: string;
  org: string;
  emails: { value: string; label: string }[];
  phones: { value: string; label: string }[];
}

export interface DavSource {
  id: number;
  name: string;
  url: string;
  username: string;
}

export interface Signature {
  /** 0 = neu */
  id: number;
  accountId: number;
  name: string;
  html: string;
  isDefault: boolean;
}

export interface Template {
  /** 0 = neu */
  id: number;
  name: string;
  html: string;
  builtin: boolean;
}

/** Platzhalter für den geschriebenen Text in einer Vorlage. */
export const CONTENT_PLACEHOLDER = '{{inhalt}}';

export type ComposeKind =
  | { type: 'new'; accountId: number | null }
  | { type: 'reply'; messageId: number }
  | { type: 'replyAll'; messageId: number }
  | { type: 'forward'; messageId: number }
  | { type: 'editDraft'; messageId: number };

export interface Suggestion {
  name: string;
  address: string;
  contact: boolean;
}

export interface Contact {
  id: number;
  /** null = nur auf diesem Mac */
  accountId: number | null;
  name: string;
  org: string;
  emails: { value: string; label: string }[];
  phones: { value: string; label: string }[];
}

export interface AddAccountInput {
  email: string;
  displayName: string;
  password: string;
  imap: ServerConfig;
  smtp: ServerConfig;
}

export const api = {
  discoverSettings: (email: string) => call<Discovered>('discover_settings', { email }),
  addAccount: (input: AddAccountInput) => call<Account>('add_account', { input }),
  listAccounts: () => call<Account[]>('list_accounts'),
  refreshFolders: (accountId: number) => call<Folder[]>('refresh_folders', { accountId }),
  removeAccount: (accountId: number) => call<void>('remove_account', { accountId }),
  listMessages: (scope: MessageScope, offset: number, limit: number, threaded: boolean) =>
    call<MessagePage>('list_messages', { scope, offset, limit, threaded }),
  getThread: (id: number, scope: MessageScope) =>
    call<{ members: MessageSummary[]; inScope: number[] }>('get_thread', { id, scope }),
  unreadCounts: () => call<Record<string, number>>('unread_counts'),
  suggestAddresses: (prefix: string) => call<Suggestion[]>('suggest_addresses', { prefix }),
  listContacts: (filter: string) => call<Contact[]>('list_contacts', { filter }),
  saveContact: (input: ContactInput) => call<number>('save_contact', { input }),
  deleteContact: (id: number) => call<void>('delete_contact', { id }),
  contactAccounts: () => call<number[]>('contact_accounts'),
  listCollected: (filter: string) => call<Suggestion[]>('list_collected', { filter }),
  forgetCollected: (address: string) => call<void>('forget_collected', { address }),
  createCalendar: (accountId: number | null, name: string, color: string) =>
    call<number>('create_calendar', { accountId, name, color }),
  updateCalendar: (id: number, name: string, color: string) =>
    call<void>('update_calendar', { id, name, color }),
  deleteCalendar: (id: number) => call<void>('delete_calendar', { id }),
  calendarAccounts: () => call<number[]>('calendar_accounts'),
  listDavSources: () => call<DavSource[]>('list_dav_sources'),
  connectDavSource: (name: string, url: string, username: string, password: string) =>
    call<number>('connect_dav_source', { name, url, username, password }),
  disconnectDavSource: (id: number) => call<void>('disconnect_dav_source', { id }),
  linkAccountCalendar: (accountId: number, url: string) =>
    call<{ calendars: number; contacts: boolean }>('link_account_calendar', { accountId, url }),
  listCalendars: () => call<CalendarInfo[]>('list_calendars'),
  setCalendarVisible: (id: number, visible: boolean) =>
    call<void>('set_calendar_visible', { id, visible }),
  occurrences: (from: number, to: number) => call<Occurrence[]>('occurrences', { from, to }),
  eventDetail: (id: number) => call<EventDetail>('event_detail', { id }),
  saveEvent: (input: EventInput) => call<number>('save_event', { input }),
  deleteEvent: (id: number) => call<void>('delete_event', { id }),
  respondInvite: (messageId: number, accept: boolean) =>
    call<void>('respond_invite', { messageId, accept }),
  searchMessages: (query: string, folderId: number | null, offset: number, limit: number) =>
    call<MessagePage>('search_messages', { query, folderId, offset, limit }),
  getMessage: (id: number) => call<MessageDetail>('get_message', { id }),
  trustImageSender: (address: string) => call<void>('trust_image_sender', { address }),
  syncAll: () => call<void>('sync_all'),
  syncAccount: (accountId: number) => call<void>('sync_account', { accountId }),
  applyAction: (ids: number[], action: UserAction) => call<void>('apply_action', { ids, action }),
  dismissFailed: (accountId: number) => call<void>('dismiss_failed', { accountId }),
  prepareCompose: (kind: ComposeKind) => call<ComposeDraft>('prepare_compose', { kind }),
  sendMessage: (draft: ComposeDraft) => call<void>('send_message', { draft }),
  saveDraft: (draft: ComposeDraft) => call<void>('save_draft', { draft }),
  discardDraft: (accountId: number, draftId: string) =>
    call<void>('discard_draft', { accountId, draftId }),
  renderPreview: (draft: ComposeDraft) => call<string>('render_preview', { draft }),
  listSignatures: () => call<Signature[]>('list_signatures'),
  saveSignature: (signature: Signature) => call<Signature>('save_signature', { signature }),
  deleteSignature: (id: number) => call<void>('delete_signature', { id }),
  setDefaultSignature: (accountId: number, id: number | null) =>
    call<void>('set_default_signature', { accountId, id }),
  listTemplates: () => call<Template[]>('list_templates'),
  saveTemplate: (template: Template) => call<Template>('save_template', { template }),
  deleteTemplate: (id: number) => call<void>('delete_template', { id }),
  fileInfos: (paths: string[]) => call<ComposeAttachment[]>('file_infos', { paths }),
  openAttachment: (messageId: number, index: number, mode: 'quickLook' | 'open') =>
    call<void>('open_attachment', { messageId, index, mode }),
  saveAttachment: (messageId: number, index: number, path: string) =>
    call<void>('save_attachment', { messageId, index, path }),
};

/** Backend-Ereignisse. Im normalen Browser (ohne Tauri) passiert nichts. */
export async function onEvent<T>(name: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  try {
    return await listen<T>(name, (e) => handler(e.payload));
  } catch {
    return () => {};
  }
}
