/**
 * Zustand der Konten (Svelte-5-Runes, als Modul-Singleton).
 * Lädt zuerst sofort aus dem lokalen Cache, dann im Hintergrund vom Server.
 */
import { api, ApiError, type Account, type AddAccountInput } from './api';

class AccountsState {
  list = $state<Account[]>([]);
  loaded = $state(false);
  /** Letzter Fehler beim Abgleich pro Konto (z. B. offline). */
  syncErrors = $state<Record<number, ApiError>>({});

  async load(): Promise<void> {
    try {
      this.list = await api.listAccounts();
    } finally {
      this.loaded = true;
    }
  }

  /** Ordnerlisten aller Konten im Hintergrund auffrischen. */
  async refreshAll(): Promise<void> {
    await Promise.all(this.list.map((a) => this.refresh(a.id)));
  }

  async refresh(accountId: number): Promise<void> {
    try {
      const folders = await api.refreshFolders(accountId);
      const account = this.list.find((a) => a.id === accountId);
      if (account) account.folders = folders;
      delete this.syncErrors[accountId];
    } catch (err) {
      if (err instanceof ApiError) this.syncErrors[accountId] = err;
    }
  }

  async add(input: AddAccountInput): Promise<Account> {
    const account = await api.addAccount(input);
    this.list.push(account);
    return account;
  }

  async remove(accountId: number): Promise<void> {
    await api.removeAccount(accountId);
    this.list = this.list.filter((a) => a.id !== accountId);
    delete this.syncErrors[accountId];
  }
}

export const accounts = new AccountsState();
