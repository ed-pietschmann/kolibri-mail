/**
 * Sync-Zustand und Änderungs-Signale aus dem Backend.
 * Listen beobachten `version` und laden ihre sichtbaren Seiten neu.
 */
import { api, ApiError, onEvent, type MailChanged, type QueueStatus, type SyncStatus } from './api';
import { accounts } from './accounts.svelte';

function readThreaded(): boolean {
  try {
    return localStorage.getItem('kolibri.threaded') !== '0';
  } catch {
    return true;
  }
}

class MailState {
  syncing = $state<Record<number, boolean>>({});
  /** Steigt bei jeder Änderung im Cache. */
  version = $state(0);
  /** Liste nach Konversationen ordnen (wird je Mac gemerkt). */
  threaded = $state(readThreaded());
  /** Suche: offen? Eingabe (sofort) und verzögert übernommener Suchtext. */
  searchOpen = $state(false);
  searchInput = $state('');
  searchText = $state('');
  /** Überall suchen statt nur im aktuellen Ordner. */
  searchEverywhere = $state(true);
  private searchTimer: ReturnType<typeof setTimeout> | undefined;

  get searchActive(): boolean {
    return this.searchOpen && this.searchText.trim().length > 0;
  }

  openSearch(text?: string): void {
    this.searchOpen = true;
    if (text !== undefined) {
      this.searchInput = text;
      this.searchText = text;
    }
  }

  closeSearch(): void {
    clearTimeout(this.searchTimer);
    this.searchOpen = false;
    this.searchInput = '';
    this.searchText = '';
  }

  /** Tippen: erst nach kurzer Pause suchen (spart Datenbankabfragen). */
  setSearchInput(text: string): void {
    this.searchInput = text;
    clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => (this.searchText = text), 150);
  }

  /** Steigt, wenn sich Termine geändert haben. */
  calendarVersion = $state(0);
  /** Steigt, wenn sich Kontakte geändert haben. */
  contactsVersion = $state(0);
  /** Ungelesene je Ordner-ID. */
  unread = $state<Record<number, number>>({});

  setThreaded(value: boolean): void {
    this.threaded = value;
    try {
      localStorage.setItem('kolibri.threaded', value ? '1' : '0');
    } catch {
      // Ohne Speicher gilt die Einstellung nur bis zum Neustart.
    }
  }

  async loadUnread(): Promise<void> {
    try {
      const counts = await api.unreadCounts();
      this.unread = Object.fromEntries(Object.entries(counts).map(([k, v]) => [Number(k), v]));
    } catch {
      // Ohne Backend (Browser-Vorschau) bleiben die Zähler leer.
    }
  }

  /** Kurze, unaufdringliche Bestätigung (verschwindet von selbst). */
  toast = $state<string | null>(null);
  /** Kurzer Hinweis für den Nutzer (z. B. „Kein Papierkorb“). */
  notice = $state<string | null>(null);
  /** Offline-Warteschlange pro Konto. */
  queue = $state<Record<number, QueueStatus>>({});

  /** Summe wartender Aktionen, wenn mindestens ein Konto offline ist. */
  get offlinePending(): number {
    return Object.values(this.queue)
      .filter((q) => q.offline)
      .reduce((n, q) => n + q.pending, 0);
  }

  /** Erste vom Server abgelehnte Aktion (zur Anzeige). */
  get failure(): QueueStatus | undefined {
    return Object.values(this.queue).find((q) => q.failed > 0 && q.error);
  }
  private started = false;

  get anySyncing(): boolean {
    return Object.values(this.syncing).some(Boolean);
  }

  async start(): Promise<void> {
    if (this.started) return;
    this.started = true;

    let pending: ReturnType<typeof setTimeout> | undefined;
    await onEvent<MailChanged>('mail-changed', () => {
      // Mehrere Ereignisse kurz hintereinander bündeln.
      clearTimeout(pending);
      pending = setTimeout(() => {
        this.version++;
        void this.loadUnread();
      }, 150);
    });
    await onEvent<number>('folders-changed', () => void accounts.load());
    await onEvent<number>('contacts-changed', () => this.contactsVersion++);
    await onEvent<number>('calendar-changed', () => this.calendarVersion++);
    await onEvent<QueueStatus>('queue-status', (q) => {
      // Eine Fehlermeldung bleibt stehen, bis sie bestätigt wird.
      const previous = this.queue[q.accountId];
      this.queue[q.accountId] = {
        ...q,
        error: q.error ?? (q.failed > 0 ? (previous?.error ?? null) : null),
      };
    });
    await onEvent<SyncStatus>('sync-status', (s) => {
      this.syncing[s.accountId] = s.syncing;
      if (s.error) {
        accounts.syncErrors[s.accountId] = new ApiError(s.error.kind, s.error.detail);
      } else if (!s.syncing) {
        delete accounts.syncErrors[s.accountId];
      }
    });
  }

  async dismissFailure(accountId: number): Promise<void> {
    await api.dismissFailed(accountId).catch(() => {});
    const q = this.queue[accountId];
    if (q) this.queue[accountId] = { ...q, failed: 0, error: null };
  }

  async syncAll(): Promise<void> {
    void this.loadUnread();
    try {
      await api.syncAll();
    } catch {
      // Ohne Backend (Browser-Vorschau) einfach nichts tun.
    }
  }
}

export const mail = new MailState();
