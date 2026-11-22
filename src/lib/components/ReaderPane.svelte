<script lang="ts">
  import { kbd } from '../platform';
  import { untrack } from 'svelte';
  import {
    api,
    type ComposeKind,
    type MessageScope,
    type MessageSummary,
    type UserAction,
  } from '../api';
  import { accounts } from '../accounts.svelte';
  import { folderIcon, folderLabel, folderRows } from '../folders';
  import { describeError } from '../errors';
  import { t } from '../i18n';
  import { mail } from '../mail.svelte';
  import Icon from './Icon.svelte';
  import MessageCard from './MessageCard.svelte';

  let {
    messageId,
    scope,
    threaded,
    isDraft = false,
    oncompose,
    onaction,
    onloaded,
  }: {
    messageId: number | null;
    /** Bereich der Liste – Aktionen auf ein Gespräch betreffen nur diesen. */
    scope: MessageScope | null;
    threaded: boolean;
    /** Liegt die Mail im Entwürfe-Ordner? Dann „Bearbeiten“ statt Antworten. */
    isDraft?: boolean;
    /** Antworten, Weiterleiten oder Entwurf bearbeiten. */
    oncompose: (kind: ComposeKind) => void;
    /** Aktion auf die Mail bzw. die Gesprächs-Nachrichten `ids` im Bereich. */
    onaction: (action: UserAction, message: MessageSummary, ids: number[]) => void;
    /** Angezeigt (z. B. um Ungelesenes als gelesen zu markieren). */
    onloaded: (message: MessageSummary | null, unread: number[]) => void;
  } = $props();

  /** Gespräch, älteste zuerst (ohne Gespräche: nur die eine Nachricht). */
  let members = $state<MessageSummary[]>([]);
  let inScope = $state<number[]>([]);
  let expanded = $state<Set<number>>(new Set());
  let loading = $state(false);
  let error = $state<string | null>(null);
  let moveOpen = $state(false);
  let content: HTMLElement | undefined = $state();
  let request = 0;

  /** Die angeklickte Nachricht – Ziel für Antworten, Markieren usw. */
  const message = $derived(members.find((m) => m.id === messageId) ?? members.at(-1) ?? null);
  const subject = $derived(members.at(-1)?.subject ?? message?.subject ?? '');

  /** Ordner des Kontos der angezeigten Mail (ohne den aktuellen). */
  const moveTargets = $derived.by(() => {
    if (!message) return [];
    const account = accounts.list.find((a) => a.id === message?.accountId);
    return folderRows(account?.folders ?? []).filter(
      (r) => r.folder.selectable && r.folder.id !== message?.folderId,
    );
  });

  async function load(id: number, quiet = false) {
    const mine = ++request;
    if (!quiet) {
      loading = true;
      error = null;
    }
    try {
      const thread = await api.getThread(id, scope ?? { kind: 'flagged' });
      if (mine !== request) return;
      const list = threaded ? thread.members : thread.members.filter((m) => m.id === id);
      const fresh = new Set(quiet ? expanded : []);
      // Aufgeklappt: die angeklickte, die neueste und alle ungelesenen.
      fresh.add(id);
      const last = list.at(-1);
      if (last) fresh.add(last.id);
      for (const m of list) if (!m.seen) fresh.add(m.id);
      const firstLoad = !quiet;
      members = list;
      inScope = threaded && thread.inScope.length ? thread.inScope : [id];
      expanded = fresh;
      if (firstLoad) {
        onloaded(
          list.find((m) => m.id === id) ?? last ?? null,
          list.filter((m) => !m.seen).map((m) => m.id),
        );
        // In langen Gesprächen zur angeklickten Nachricht springen.
        if (list.length > 1) {
          requestAnimationFrame(() =>
            content?.querySelector(`[data-message-id="${id}"]`)?.scrollIntoView({ block: 'start' }),
          );
        }
      }
    } catch (err) {
      if (mine !== request) return;
      if (!quiet) {
        members = [];
        error = describeError(err);
      }
    } finally {
      if (mine === request) loading = false;
    }
  }

  $effect(() => {
    const id = messageId;
    void threaded;
    untrack(() => {
      if (id === null) {
        request++;
        moveOpen = false;
        members = [];
        error = null;
        loading = false;
        onloaded(null, []);
      } else {
        void load(id);
      }
    });
  });

  // Neue Antworten im offenen Gespräch erscheinen ohne Neuladen der Ansicht.
  $effect(() => {
    if (mail.version === 0) return;
    untrack(() => {
      if (messageId !== null && members.length) void load(messageId, true);
    });
  });

  function toggle(id: number) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expanded = next;
  }

  /** Tastenkürzel V öffnet das Verschieben-Menü. */
  export function openMoveMenu() {
    if (message) moveOpen = true;
  }

  /** Aktion per Tastenkürzel (wirkt wie die Knöpfe). */
  export function run(action: UserAction) {
    act(action);
  }

  function act(action: UserAction) {
    if (!message) return;
    // Markieren betrifft die eine Nachricht, alles andere das Gespräch im Bereich.
    const ids = action.type === 'flag' ? [message.id] : inScope;
    // Sofort in der Ansicht spiegeln; die Liste aktualisiert sich über Ereignisse.
    for (const m of members) {
      if (!ids.includes(m.id)) continue;
      if (action.type === 'flag') m.flagged = action.value;
      if (action.type === 'markSeen') m.seen = action.value;
    }
    moveOpen = false;
    onaction(action, message, ids);
  }

  /** Für Tastenkürzel: Umschalten wie die Knöpfe. */
  export function toggleFlag() {
    if (message) act({ type: 'flag', value: !message.flagged });
  }

  export function toggleSeen() {
    if (message) act({ type: 'markSeen', value: !message.seen });
  }

  export function compose(type: 'reply' | 'replyAll' | 'forward' | 'editDraft') {
    if (message) oncompose({ type, messageId: message.id });
  }
</script>

<svelte:window
  onclick={() => (moveOpen = false)}
  onkeydown={(e) => {
    if (e.key === 'Escape') moveOpen = false;
  }}
/>

<section class="reader" aria-label={t('reader.label')}>
  <header class="toolbar" data-tauri-drag-region>
    <div class="actions">
      {#if isDraft}
        <button class="act" disabled={!message} onclick={() => compose('editDraft')}>
          <Icon name="draft" size={15} />
          <span>{t('reader.editDraft')}</span>
        </button>
      {:else}
        <button
          class="act"
          aria-label={t('reader.reply')}
          title="{t('reader.reply')} ({kbd('R, ⌘R')})"
          disabled={!message}
          onclick={() => compose('reply')}
        >
          <Icon name="reply" size={15} />
          <span>{t('reader.replyShort')}</span>
        </button>
        <button
          class="act"
          aria-label={t('reader.replyAll')}
          title="{t('reader.replyAll')} ({kbd('A, ⇧⌘R')})"
          disabled={!message}
          onclick={() => compose('replyAll')}
        >
          <Icon name="replyAll" size={15} />
          <span>{t('reader.replyAllShort')}</span>
        </button>
        <button
          class="act"
          aria-label={t('reader.forward')}
          title="{t('reader.forward')} ({kbd('F, ⇧⌘F')})"
          disabled={!message}
          onclick={() => compose('forward')}
        >
          <Icon name="forward" size={15} />
          <span>{t('reader.forwardShort')}</span>
        </button>
      {/if}
      <span class="divider" aria-hidden="true"></span>
      <button
        class="act"
        aria-label={t('reader.archive')}
        title="{t('reader.archive')} (E)"
        disabled={!message}
        onclick={() => act({ type: 'archive' })}
      >
        <Icon name="archive" size={15} />
        <span>{t('reader.archive')}</span>
      </button>
      <button
        class="act"
        aria-label={t('reader.delete')}
        title="{t('reader.delete')} (⌫)"
        disabled={!message}
        onclick={() => act({ type: 'trash' })}
      >
        <Icon name="trash" size={15} />
        <span>{t('reader.delete')}</span>
      </button>
      <div class="menu-anchor">
        <button
          class="act icon-only"
          aria-label={t('reader.move')}
          title="{t('reader.move')} (V)"
          aria-haspopup="menu"
          aria-expanded={moveOpen}
          disabled={!message}
          onclick={(e) => {
            e.stopPropagation();
            moveOpen = !moveOpen;
          }}
        >
          <Icon name="folder" size={15} />
        </button>
        {#if moveOpen && message}
          <div class="menu" role="menu" aria-label={t('reader.moveMenu')}>
            {#each moveTargets as { folder, depth } (folder.id)}
              <button
                role="menuitem"
                style:padding-left="{8 + depth * 14}px"
                onclick={() => act({ type: 'moveTo', folderId: folder.id })}
              >
                <Icon name={folderIcon(folder)} size={13} />
                {folderLabel(folder)}
              </button>
            {/each}
          </div>
        {/if}
      </div>
      <button
        class="act icon-only"
        class:active={message?.flagged}
        aria-label={message?.flagged ? t('reader.unflag') : t('reader.flag')}
        aria-pressed={message?.flagged ?? false}
        title="{message?.flagged ? t('reader.unflag') : t('reader.flag')} (S)"
        disabled={!message}
        onclick={toggleFlag}
      >
        <Icon name="flag" size={15} />
      </button>
      <button
        class="act icon-only"
        aria-label={message && !message.seen ? t('reader.markRead') : t('reader.markUnread')}
        title="{message && !message.seen ? t('reader.markRead') : t('reader.markUnread')} (U)"
        disabled={!message}
        onclick={toggleSeen}
      >
        <Icon name={message && !message.seen ? 'mailOpen' : 'mail'} size={15} />
      </button>
    </div>
  </header>

  {#if message}
    <div class="content" bind:this={content} aria-busy={loading}>
      <h1 class="subject">
        {subject || t('list.noSubject')}
        {#if members.length > 1}
          <span class="count">{t('reader.threadCount', { n: members.length })}</span>
        {/if}
      </h1>
      {#each members as m (m.id)}
        <MessageCard
          summary={m}
          expanded={expanded.has(m.id) || members.length === 1}
          collapsible={members.length > 1}
          ontoggle={() => toggle(m.id)}
        />
      {/each}
    </div>
  {:else if loading}
    <div class="empty"><span class="spinner"></span>{t('reader.loading')}</div>
  {:else if error}
    <div class="empty error" role="alert">
      <p>{t('reader.error')}</p>
      <p class="small">{error}</p>
      {#if messageId !== null}
        <button class="btn" onclick={() => messageId !== null && void load(messageId)}>
          {t('reader.retry')}
        </button>
      {/if}
    </div>
  {:else}
    <div class="empty">{t('reader.empty')}</div>
  {/if}
</section>

<style>
  .reader {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--surface-reader);
    border-left: 1px solid var(--separator);
  }

  .toolbar {
    display: flex;
    align-items: center;
    height: var(--titlebar-h);
    padding: 0 var(--space-4);
    border-bottom: 1px solid var(--separator);
    flex-shrink: 0;
    container-type: inline-size;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }

  .act {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 10px;
    border-radius: 8px;
    color: var(--text-secondary);
    font-size: 12.5px;
    font-weight: 500;
    white-space: nowrap;
    transition:
      background-color var(--dur) var(--ease),
      color var(--dur) var(--ease);
  }

  .act :global(svg) {
    color: var(--icon);
  }

  .act.icon-only {
    width: 30px;
    padding: 0;
    justify-content: center;
  }

  .act:not(:disabled):hover {
    background: var(--hover);
    color: var(--text);
  }

  .act:disabled {
    opacity: 0.4;
  }

  .act.active :global(svg) {
    color: var(--flag);
  }

  /* Schmale Leseansicht: Beschriftungen ausblenden, Symbole bleiben. */
  @container (max-width: 640px) {
    .act span {
      display: none;
    }

    .act {
      width: 30px;
      padding: 0;
      justify-content: center;
    }
  }

  .divider {
    width: 1px;
    height: 18px;
    margin: 0 var(--space-2);
    background: var(--separator);
  }

  .menu-anchor {
    position: relative;
  }

  .menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 20;
    display: grid;
    min-width: 200px;
    max-height: 60vh;
    overflow-y: auto;
    padding: var(--space-1);
    border-radius: var(--card-radius);
    background: var(--sheet-bg);
    box-shadow: var(--sheet-shadow);
  }

  .menu button {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 26px;
    padding-right: var(--space-3);
    border-radius: 4px;
    text-align: left;
    white-space: nowrap;
  }

  .menu button:hover {
    background: var(--active);
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6) 28px 120px;
    user-select: text;
    -webkit-user-select: text;
    cursor: auto;
  }

  .subject {
    margin: 0 0 var(--space-3);
    font-size: 24px;
    font-weight: 700;
    letter-spacing: -0.025em;
    line-height: 1.2;
    text-wrap: balance;
  }

  .count {
    margin-left: var(--space-2);
    font-size: var(--font-size-sm);
    font-weight: 500;
    color: var(--text-tertiary);
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    color: var(--text-tertiary);
    font-size: var(--font-size-lg);
  }

  .empty.error {
    color: var(--text-secondary);
    font-size: var(--font-size);
    text-align: center;
    padding: var(--space-6);
  }

  .empty p {
    margin: 0;
  }
</style>
