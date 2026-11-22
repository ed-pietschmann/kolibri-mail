<script lang="ts">
  import { kbd } from '../platform';
  import type { Account } from '../api';
  import { accounts } from '../accounts.svelte';
  import { mail } from '../mail.svelte';
  import { describeError } from '../errors';
  import { folderIcon, folderLabel, folderRows } from '../folders';
  import { t, type MessageKey } from '../i18n';
  import Icon, { type IconName } from './Icon.svelte';
  import logo from '../../assets/kolibri-64.png';

  type Item = { id: string; label: MessageKey; icon: IconName };

  const mailboxes: Item[] = [
    { id: 'unified:inbox', label: 'sidebar.allInboxes', icon: 'inbox' },
    { id: 'unified:flagged', label: 'sidebar.flagged', icon: 'flag' },
    { id: 'unified:drafts', label: 'sidebar.drafts', icon: 'draft' },
    { id: 'unified:sent', label: 'sidebar.sent', icon: 'sent' },
  ];
  const other: Item[] = [
    { id: 'view:today', label: 'sidebar.today', icon: 'today' },
    { id: 'view:calendar', label: 'sidebar.calendar', icon: 'calendar' },
    { id: 'view:contacts', label: 'sidebar.contacts', icon: 'person' },
  ];

  let {
    selected = $bindable('unified:inbox'),
    oncompose,
    onaddaccount,
    onremoveaccount,
    onsignature,
    ontemplates,
  }: {
    selected?: string;
    oncompose: () => void;
    onaddaccount: () => void;
    onremoveaccount: (account: Account) => void;
    onsignature: (account: Account) => void;
    ontemplates: () => void;
  } = $props();

  let collapsed = $state<Record<number, boolean>>({});

  /** Ungelesene in allen Posteingängen. */
  const unifiedUnread = $derived(
    accounts.list.reduce((sum, a) => {
      const inbox = a.folders.find((f) => f.role === 'inbox');
      return sum + (inbox ? (mail.unread[inbox.id] ?? 0) : 0);
    }, 0),
  );
  let menuFor = $state<number | null>(null);

  function closeMenu() {
    menuFor = null;
  }
</script>

<svelte:window
  onclick={closeMenu}
  onkeydown={(e) => {
    if (e.key === 'Escape') closeMenu();
  }}
/>

{#snippet row(id: string, label: string, icon: IconName, depth = 0, enabled = true, count = 0)}
  <li>
    <button
      class="item"
      class:selected={selected === id}
      style:padding-left="{10 + depth * 14}px"
      aria-current={selected === id ? 'page' : undefined}
      disabled={!enabled}
      onclick={() => (selected = id)}
    >
      <span class="icon"><Icon name={icon} /></span>
      <span class="label">{label}</span>
      {#if count > 0}
        <span class="count" aria-label={t('sidebar.unread', { n: count })}>{count}</span>
      {/if}
    </button>
  </li>
{/snippet}

<nav class="sidebar" aria-label={t('sidebar.label')}>
  <div class="titlebar" data-tauri-drag-region></div>

  <div class="brand" data-tauri-drag-region>
    <img src={logo} alt="" width="22" height="22" data-tauri-drag-region />
    <span data-tauri-drag-region>Kolibri</span>
  </div>

  <div class="compose-wrap">
    <button class="compose" onclick={oncompose} title="{t('list.compose')} ({kbd('⌘N')})">
      <Icon name="compose" size={15} />
      {t('list.compose')}
    </button>
  </div>

  <div class="scroll">
    <h2 class="section">{t('sidebar.mailboxes')}</h2>
    <ul>
      {#each mailboxes as item (item.id)}
        {@render row(
          item.id,
          t(item.label),
          item.icon,
          0,
          true,
          item.id === 'unified:inbox' ? unifiedUnread : 0,
        )}
      {/each}
    </ul>

    {#each accounts.list as account (account.id)}
      {@const isCollapsed = collapsed[account.id] ?? false}
      {@const syncError = accounts.syncErrors[account.id]}
      <div class="account-header">
        <button
          class="section toggle"
          aria-expanded={!isCollapsed}
          aria-label={t(isCollapsed ? 'sidebar.expand' : 'sidebar.collapse', {
            name: account.email,
          })}
          onclick={() => (collapsed[account.id] = !isCollapsed)}
        >
          <span class="chevron" class:open={!isCollapsed}><Icon name="chevron" size={10} /></span>
          <span class="label">{account.email}</span>
        </button>
        {#if syncError}
          <span class="warn" title="{t('sidebar.syncError')} ({describeError(syncError)})">
            <Icon name="warning" size={12} />
            <span class="visually-hidden">{t('sidebar.syncError')}</span>
          </span>
        {/if}
        <button
          class="more"
          aria-label={t('sidebar.accountOptions', { name: account.email })}
          aria-haspopup="menu"
          aria-expanded={menuFor === account.id}
          onclick={(e) => {
            e.stopPropagation();
            menuFor = menuFor === account.id ? null : account.id;
          }}
        >
          <Icon name="more" size={14} />
        </button>
        {#if menuFor === account.id}
          <div class="menu" role="menu">
            <button
              role="menuitem"
              onclick={() => {
                closeMenu();
                void accounts.refresh(account.id);
              }}
            >
              {t('sidebar.refreshFolders')}
            </button>
            <button
              role="menuitem"
              onclick={() => {
                closeMenu();
                onsignature(account);
              }}
            >
              {t('sidebar.signature')}
            </button>
            <button
              role="menuitem"
              onclick={() => {
                closeMenu();
                ontemplates();
              }}
            >
              {t('sidebar.templates')}
            </button>
            <button
              role="menuitem"
              class="destructive"
              onclick={() => {
                closeMenu();
                onremoveaccount(account);
              }}
            >
              {t('sidebar.removeAccount')}
            </button>
          </div>
        {/if}
      </div>
      {#if !isCollapsed}
        <ul>
          {#each folderRows(account.folders) as { folder, depth } (folder.id)}
            {@render row(
              `folder:${folder.id}`,
              folderLabel(folder),
              folderIcon(folder),
              depth,
              folder.selectable,
              // Wie Apple Mail: Zähler für Posteingang und eigene Ordner, nicht für Gesendet/Papierkorb.
              ['sent', 'trash', 'junk', 'drafts', 'all'].includes(folder.role ?? '')
                ? 0
                : (mail.unread[folder.id] ?? 0),
            )}
          {/each}
        </ul>
      {/if}
    {/each}

    <h2 class="section">{t('sidebar.calendar')}</h2>
    <ul>
      {#each other as item (item.id)}
        {@render row(item.id, t(item.label), item.icon)}
      {/each}
    </ul>
  </div>

  <div class="footer">
    <button class="add" onclick={onaddaccount}>
      <Icon name="plus" size={14} />
      {t('sidebar.addAccount')}
    </button>
  </div>
</nav>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--sidebar-bg);
  }

  .titlebar {
    height: 38px;
    flex-shrink: 0;
  }

  /* Windows: echte Titelleiste über der Seite – kein Platz für Mac-Fensterknöpfe nötig. */
  :global(html[data-platform='windows']) .titlebar {
    height: var(--space-4);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 0 var(--space-4) var(--space-3);
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }

  .brand img {
    border-radius: 6px;
  }

  .compose-wrap {
    padding: var(--space-1) var(--space-3) var(--space-3);
  }

  .compose {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    width: 100%;
    height: 36px;
    border-radius: 9px;
    background: var(--ink);
    color: var(--ink-text);
    font-weight: 600;
    transition: background-color var(--dur) var(--ease);
  }

  .compose:hover {
    background: var(--ink-hover);
  }

  .compose:focus-visible {
    outline-offset: 2px;
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 var(--space-3) var(--space-2);
  }

  .section {
    margin: var(--space-4) var(--space-2) var(--space-1);
    font-size: 12px;
    font-weight: 600;
    color: var(--text-tertiary);
  }

  ul {
    display: grid;
    gap: 2px;
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
    padding-right: var(--space-2);
    border-radius: 8px;
    color: var(--text-secondary);
    font-weight: 500;
    text-align: left;
    transition:
      background-color var(--dur) var(--ease),
      color var(--dur) var(--ease);
  }

  .item:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }

  .item.selected,
  .item.selected:hover:not(:disabled) {
    background: var(--sidebar-selected);
    box-shadow: var(--sidebar-selected-shadow);
    color: var(--text);
    font-weight: 600;
  }

  .item:disabled {
    color: var(--text-tertiary);
  }

  .count {
    margin-left: auto;
    font-size: 12px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    color: var(--text-tertiary);
  }

  .item.selected .count {
    color: var(--text-secondary);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .icon {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--icon);
  }

  .item.selected .icon {
    color: var(--text);
  }

  .account-header {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-top: var(--space-4);
  }

  .toggle {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 1;
    min-width: 0;
    margin: 0 0 var(--space-1);
    padding: 0 0 0 var(--space-2);
    text-align: left;
    color: var(--text-tertiary);
  }

  .toggle .label {
    font-size: 12px;
    font-weight: 600;
  }

  .chevron {
    display: inline-flex;
    transition: transform var(--dur) var(--ease);
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .warn {
    display: inline-flex;
    color: var(--warning);
    margin-bottom: var(--space-1);
  }

  .more {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    margin: 0 var(--space-1) var(--space-1) 0;
    border-radius: 6px;
    color: var(--text-secondary);
    opacity: 0;
    transition: opacity var(--dur) var(--ease);
  }

  .account-header:hover .more,
  .more:focus-visible,
  .more[aria-expanded='true'] {
    opacity: 1;
  }

  .more:hover {
    background: var(--active);
  }

  .menu {
    position: absolute;
    top: 100%;
    right: var(--space-1);
    z-index: 10;
    display: grid;
    min-width: 190px;
    padding: var(--space-1);
    border-radius: var(--card-radius);
    background: var(--sheet-bg);
    box-shadow: var(--sheet-shadow);
  }

  .menu button {
    height: 28px;
    padding: 0 var(--space-2);
    border-radius: 6px;
    text-align: left;
  }

  .menu button:hover {
    background: var(--active);
  }

  .menu .destructive {
    color: var(--danger);
  }

  .footer {
    padding: var(--space-2) var(--space-4) var(--space-4);
    border-top: 1px solid var(--separator);
  }

  .add {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    color: var(--text-secondary);
    font-weight: 500;
  }

  .add:hover {
    color: var(--text);
  }
</style>
