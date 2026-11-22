<script lang="ts">
  import { kbd, mod } from '../platform';
  import { untrack } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { api, type MessageScope, type MessageSummary } from '../api';
  import { displayName, listDate } from '../format';
  import { t } from '../i18n';
  import { mail } from '../mail.svelte';
  import Icon from './Icon.svelte';

  /**
   * Virtualisierte Nachrichtenliste: Es werden nur die sichtbaren Zeilen
   * gezeichnet und nur Seiten zu je 100 Zeilen geladen. Weit entfernte
   * Seiten werden wieder verworfen – der Speicher bleibt klein, egal wie
   * groß der Ordner ist.
   */
  let {
    title,
    scope,
    hasAccounts,
    selectedId = $bindable(null),
    onaddaccount,
  }: {
    title: string;
    scope: MessageScope | null;
    hasAccounts: boolean;
    selectedId?: number | null;
    onaddaccount: () => void;
  } = $props();

  const ROW = 104;
  const PAGE = 100;
  const OVERSCAN = 6;
  const MAX_PAGES = 6;

  let total = $state(0);
  let loaded = $state(false);
  const pages = new SvelteMap<number, MessageSummary[]>();
  const inflight = new Set<number>();
  let generation = 0;

  let viewport: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let height = $state(600);

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(total, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));

  const rows = $derived.by(() => {
    const out: { index: number; msg: MessageSummary | undefined }[] = [];
    for (let i = first; i < last; i++) {
      out.push({ index: i, msg: pages.get(Math.floor(i / PAGE))?.[i % PAGE] });
    }
    return out;
  });

  async function loadPage(page: number, gen: number) {
    if (!scope || inflight.has(page)) return;
    inflight.add(page);
    try {
      const res = mail.searchActive
        ? await api.searchMessages(
            mail.searchText,
            !mail.searchEverywhere && scope.kind === 'folder' ? scope.id : null,
            page * PAGE,
            PAGE,
          )
        : await api.listMessages(scope, page * PAGE, PAGE, mail.threaded);
      if (gen !== generation) return;
      total = res.total;
      pages.set(page, res.items);
      loaded = true;
      evict(page);
    } catch {
      if (gen === generation) loaded = true;
    } finally {
      inflight.delete(page);
    }
  }

  /** Nur die Seiten um die aktuelle Position behalten. */
  function evict(center: number) {
    if (pages.size <= MAX_PAGES) return;
    const far = [...pages.keys()].sort((a, b) => Math.abs(b - center) - Math.abs(a - center));
    for (const p of far.slice(0, pages.size - MAX_PAGES)) pages.delete(p);
  }

  // Nur ein echter Wechsel des Bereichs (nicht jedes neue Objekt) setzt zurück.
  const scopeKey = $derived(
    mail.searchActive
      ? `search|${mail.searchText}|${mail.searchEverywhere}|${JSON.stringify(scope)}`
      : `${JSON.stringify(scope)}|${mail.threaded}`,
  );
  let searchEl: HTMLInputElement | undefined = $state();

  // Suche geöffnet → Eingabefeld fokussieren.
  $effect(() => {
    if (mail.searchOpen) searchEl?.focus();
  });

  const hints = ['von:', 'an:', 'betreff:', 'hat:anhang', 'ist:ungelesen', 'ist:markiert', 'nach:'];

  function addHint(hint: string) {
    const text = mail.searchInput.trim();
    mail.setSearchInput(`${text ? `${text} ` : ''}${hint}${hint.endsWith(':') ? '' : ' '}`);
    searchEl?.focus();
  }

  // Bereich wechselt → alles zurücksetzen.
  $effect(() => {
    void scopeKey;
    untrack(reset);
  });

  function reset() {
    generation++;
    pages.clear();
    inflight.clear();
    total = 0;
    loaded = false;
    selectedId = null;
    if (viewport) viewport.scrollTop = 0;
    scrollTop = 0;
    void loadPage(0, generation);
  }

  // Fehlende Seiten im sichtbaren Bereich nachladen.
  $effect(() => {
    const gen = generation;
    const need = new Set<number>();
    for (let i = first; i < Math.max(first + 1, last); i++) need.add(Math.floor(i / PAGE));
    for (const p of need) if (!pages.has(p)) void loadPage(p, gen);
  });

  // Backend meldet Änderungen → geladene Seiten frisch holen (Scrollposition bleibt).
  $effect(() => {
    if (mail.version === 0) return;
    untrack(refreshVisible);
  });

  function refreshVisible() {
    const gen = generation;
    const visible = new Set<number>([
      Math.floor(first / PAGE),
      Math.floor(Math.max(first, last - 1) / PAGE),
    ]);
    for (const p of [...pages.keys()]) if (!visible.has(p)) pages.delete(p);
    for (const p of visible) void loadPage(p, gen);
  }

  function indexOf(id: number | null): number {
    if (id === null) return -1;
    for (const [p, items] of pages) {
      const i = items.findIndex((m) => m.id === id);
      if (i >= 0) return p * PAGE + i;
    }
    return -1;
  }

  function select(index: number) {
    if (index < 0 || index >= total) return;
    const msg = pages.get(Math.floor(index / PAGE))?.[index % PAGE];
    if (!msg) return;
    selectedId = msg.id;
    // Ins Bild scrollen
    if (!viewport) return;
    const top = index * ROW;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW > viewport.scrollTop + viewport.clientHeight)
      viewport.scrollTop = top + ROW - viewport.clientHeight;
  }

  /** Auswahl um `delta` Zeilen verschieben (Pfeiltasten, J/K). */
  export function step(delta: number) {
    const current = indexOf(selectedId);
    if (current < 0) select(0);
    else select(Math.min(total - 1, Math.max(0, current + delta)));
  }

  /**
   * Welche Mail nach dem Archivieren/Löschen von `id` ausgewählt wird:
   * die nächste darunter, sonst die darüber (wie in Apple Mail).
   */
  export function neighborOf(id: number): number | null {
    const index = indexOf(id);
    if (index < 0) return null;
    const at = (i: number) => pages.get(Math.floor(i / PAGE))?.[i % PAGE]?.id ?? null;
    return at(index + 1) ?? at(index - 1);
  }

  function onkeydown(e: KeyboardEvent) {
    if (mod(e) || e.ctrlKey || e.altKey) return;
    if (e.key === 'ArrowDown' || e.key === 'j') step(1);
    else if (e.key === 'ArrowUp' || e.key === 'k') step(-1);
    else return;
    e.preventDefault();
    // Sonst bewegt das globale Tastenkürzel die Auswahl ein zweites Mal.
    e.stopPropagation();
  }
</script>

<section class="list" aria-label={t('list.label')}>
  <header class="titlebar" data-tauri-drag-region>
    <div class="search-field" class:active={mail.searchOpen}>
      <Icon name="search" size={14} />
      <input
        bind:this={searchEl}
        type="text"
        value={mail.searchInput}
        disabled={!hasAccounts}
        onfocus={() => mail.openSearch()}
        oninput={(e) => mail.setSearchInput(e.currentTarget.value)}
        onkeydown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation();
            mail.closeSearch();
            e.currentTarget.blur();
          } else if (e.key === 'ArrowDown' || e.key === 'Enter') {
            e.preventDefault();
            viewport?.focus();
            if (selectedId === null) step(0);
          }
        }}
        placeholder={t('search.placeholder')}
        aria-label={t('list.search')}
        title="{t('list.search')} ({kbd('⌘F')})"
        spellcheck="false"
        autocomplete="off"
      />
      {#if mail.searchOpen}
        <button class="clear" aria-label={t('search.close')} onclick={() => mail.closeSearch()}>
          <Icon name="close" size={12} />
        </button>
      {:else}
        <kbd aria-hidden="true">{kbd('⌘F')}</kbd>
      {/if}
    </div>
  </header>

  <div class="heading-row">
    <div class="heading">
      <h1>{mail.searchOpen ? t('search.title') : title}</h1>
      {#if mail.offlinePending > 0}
        <span class="count offline" role="status">
          {t('list.offline', { n: mail.offlinePending })}
        </span>
      {:else if loaded && total > 0}
        <span class="count">{t('list.count', { n: total.toLocaleString('de-DE') })}</span>
      {/if}
    </div>
    <div class="actions">
      <button
        class="tool"
        class:on={mail.threaded}
        aria-label={t('list.threaded')}
        aria-pressed={mail.threaded}
        title={t('list.threaded')}
        onclick={() => mail.setThreaded(!mail.threaded)}
      >
        <Icon name="thread" />
      </button>
      <button
        class="tool"
        class:spinning={mail.anySyncing}
        aria-label={t('list.refresh')}
        title="{t('list.refresh')} ({kbd('⇧⌘N')})"
        onclick={() => void mail.syncAll()}
        disabled={!hasAccounts}
      >
        <Icon name="refresh" />
      </button>
    </div>
  </div>

  {#if mail.searchOpen}
    <div class="search-bar">
      <div class="segmented" role="radiogroup" aria-label={t('search.where')}>
        <button
          role="radio"
          aria-checked={mail.searchEverywhere}
          class:on={mail.searchEverywhere}
          onclick={() => (mail.searchEverywhere = true)}
        >
          {t('search.everywhere')}
        </button>
        <button
          role="radio"
          aria-checked={!mail.searchEverywhere}
          class:on={!mail.searchEverywhere}
          disabled={scope?.kind !== 'folder'}
          onclick={() => (mail.searchEverywhere = false)}
        >
          {title}
        </button>
      </div>
      {#if !mail.searchInput.trim()}
        <div class="hints" aria-label={t('search.hints')}>
          {#each hints as h (h)}
            <button class="hint" onclick={() => addHint(h)}>{h}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if !hasAccounts}
    <div class="empty" role="status">
      <Icon name="mail" size={32} />
      <p class="empty-title">{t('list.empty.title')}</p>
      <p class="empty-body">{t('list.empty.body')}</p>
      <button class="btn primary add" onclick={onaddaccount}>{t('list.empty.add')}</button>
    </div>
  {:else if !scope}
    <!-- Keine Mail-Ansicht ausgewählt -->
  {:else if loaded && total === 0}
    <div class="empty" role="status">
      <Icon name="inbox" size={32} />
      <p class="empty-title">
        {#if mail.searchActive}
          {t('search.noResults')}
        {:else}
          {mail.anySyncing ? t('list.syncing') : t('list.noMessages.title')}
        {/if}
      </p>
      {#if mail.searchActive}<p class="empty-body">{t('search.noResultsHint')}</p>{/if}
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="viewport"
      bind:this={viewport}
      bind:clientHeight={height}
      onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
      tabindex="0"
      role="listbox"
      aria-label={title}
      aria-activedescendant={selectedId !== null ? `msg-${selectedId}` : undefined}
      {onkeydown}
    >
      <div class="spacer" style:height="{total * ROW}px">
        {#each rows as { index, msg } (msg?.id ?? `p${index}`)}
          <!-- Tastatur bedient die ganze Liste (aria-activedescendant), Zeilen brauchen keinen eigenen Fokus. -->
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
          <div
            class="row"
            class:unread={msg && !msg.seen}
            class:selected={msg && msg.id === selectedId}
            style:transform="translateY({index * ROW}px)"
            role="option"
            id={msg ? `msg-${msg.id}` : undefined}
            aria-selected={msg?.id === selectedId}
            onclick={() => select(index)}
          >
            <div class="card">
              {#if msg}
                <span class="dot" aria-hidden="true"></span>
                <div class="line top">
                  <span class="from">
                    {#if msg.outgoing && msg.toDisplay}
                      {t('list.toPrefix', { name: msg.toDisplay })}{#if msg.toCount > 1}<span
                          class="more"
                        >
                          {t('list.more', { n: msg.toCount - 1 })}
                        </span>{/if}
                    {:else}
                      {displayName(msg.fromName, msg.fromAddr)}
                    {/if}
                  </span>
                  {#if msg.threadCount > 1}
                    <span class="thread" title={t('list.threadSize', { n: msg.threadCount })}>
                      {msg.threadCount}
                    </span>
                  {/if}
                  <span class="meta">
                    {#if msg.flagged}<span class="flag"><Icon name="flag" size={11} /></span>{/if}
                    {#if msg.hasAttachments}<Icon name="paperclip" size={11} />{/if}
                    <time>{listDate(msg.date)}</time>
                  </span>
                </div>
                <div class="line subject">{msg.subject || t('list.noSubject')}</div>
                <div class="line preview">{msg.preview}</div>
                <span class="visually-hidden">
                  {msg.seen ? '' : t('list.unread')}
                  {msg.flagged ? t('list.flagged') : ''}
                </span>
              {:else}
                <div class="skeleton"></div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if mail.notice || mail.failure}
    <div class="notice" role="alert">
      <Icon name="warning" size={14} />
      <span>
        {mail.notice ?? t('notice.failed', { detail: mail.failure?.error?.detail ?? '' })}
      </span>
      <button
        class="btn"
        onclick={() => {
          if (mail.notice) mail.notice = null;
          else if (mail.failure) void mail.dismissFailure(mail.failure.accountId);
        }}
      >
        {t('notice.ok')}
      </button>
    </div>
  {/if}
</section>

<style>
  .list {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--surface);
    border-left: 1px solid var(--separator);
  }

  .titlebar {
    display: flex;
    align-items: center;
    height: var(--titlebar-h);
    padding: 0 var(--space-3);
    flex-shrink: 0;
  }

  .heading-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3) var(--space-3) var(--space-4);
    flex-shrink: 0;
  }

  .heading {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  h1 {
    margin: 0;
    font-size: 19px;
    font-weight: 700;
    letter-spacing: -0.02em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .notice {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--card-radius);
    background: var(--sheet-bg);
    box-shadow: var(--sheet-shadow);
    font-size: 12px;
  }

  .notice :global(svg) {
    flex-shrink: 0;
    color: var(--warning);
  }

  .notice span {
    flex: 1;
  }

  .notice .btn {
    min-width: 0;
    height: 24px;
  }

  .count.offline {
    color: var(--warning);
  }

  .count {
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .actions {
    display: flex;
    gap: 2px;
  }

  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 8px;
    color: var(--icon);
    transition: background-color var(--dur) var(--ease);
  }

  .tool:not(:disabled):hover {
    background: var(--hover);
    color: var(--text);
  }

  .tool:disabled {
    opacity: 0.45;
  }

  .tool.spinning :global(svg) {
    animation: spin 1s linear infinite;
  }

  .viewport {
    flex: 1;
    overflow-y: auto;
    outline: none;
    contain: strict;
  }

  .spacer {
    position: relative;
  }

  .row {
    position: absolute;
    inset: 0 0 auto 0;
    height: 104px;
    padding: 0 var(--space-2);
    will-change: transform;
  }

  /* Die eigentliche Karte: eingerückt, ruhige graue Auswahl statt blauer Fläche. */
  .row > .card {
    position: relative;
    display: grid;
    grid-template-rows: auto auto auto;
    align-content: start;
    gap: 2px;
    height: 100%;
    padding: 12px var(--space-3) 0 22px;
    border-radius: var(--card-radius);
    transition: background-color var(--dur) var(--ease);
  }

  .row > .card::after {
    content: '';
    position: absolute;
    left: 22px;
    right: var(--space-3);
    bottom: 0;
    height: 1px;
    background: var(--separator);
  }

  .row:hover > .card {
    background: var(--hover);
  }

  .row.selected > .card {
    background: var(--list-selected);
  }

  .row:hover > .card::after,
  .row.selected > .card::after {
    opacity: 0;
  }

  .viewport:focus-visible .row.selected > .card {
    box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--accent) 60%, transparent);
  }

  .dot {
    position: absolute;
    left: 9px;
    top: 18px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .row.unread .dot {
    background: var(--accent);
  }

  .line {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .search-field {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    height: 32px;
    padding: 0 var(--space-2) 0 10px;
    border-radius: 9px;
    background: var(--field-search);
    color: var(--text-tertiary);
    transition: box-shadow var(--dur) var(--ease);
  }

  .search-field:focus-within {
    background: var(--field-bg);
    box-shadow:
      0 0 0 1px var(--field-border),
      0 0 0 4px color-mix(in srgb, var(--accent) 22%, transparent);
  }

  .search-field input {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
    color: var(--text);
  }

  .search-field input:focus {
    box-shadow: none;
  }

  kbd {
    font: inherit;
    font-size: 11px;
    color: var(--text-tertiary);
  }

  .clear {
    display: inline-flex;
    padding: 4px;
    border-radius: 50%;
    color: var(--text-tertiary);
  }

  .clear:hover {
    background: var(--hover);
  }

  .search-bar {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--separator);
  }

  .segmented {
    display: flex;
    padding: 2px;
    border-radius: 7px;
    background: var(--hover);
  }

  .segmented button {
    flex: 1;
    min-width: 0;
    height: 22px;
    padding: 0 var(--space-2);
    border-radius: 5px;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-secondary);
  }

  .segmented button.on {
    background: var(--sheet-bg);
    color: var(--text);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.2);
  }

  .segmented button:disabled {
    opacity: 0.5;
  }

  .hints {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .hint {
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--hover);
    color: var(--text-secondary);
    font-size: 11px;
    font-family: ui-monospace, 'SF Mono', Menlo, monospace;
  }

  .hint:hover {
    background: var(--active);
    color: var(--text);
  }

  .tool.on {
    background: var(--active);
    color: var(--text);
  }

  .more {
    margin-left: 4px;
    color: var(--text-tertiary);
    font-weight: 400;
  }

  .thread {
    flex-shrink: 0;
    margin-right: auto;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--hover);
    color: var(--text-secondary);
    font-size: 11px;
    line-height: 16px;
  }

  .from {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .row.unread .from {
    font-weight: 600;
    color: var(--text);
  }

  .meta {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }

  .flag {
    display: inline-flex;
    color: var(--flag);
  }

  .subject {
    font-size: 13px;
    font-weight: 600;
  }

  .row.unread .subject {
    font-weight: 700;
  }

  .preview {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    white-space: normal;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-secondary);
  }

  .skeleton {
    height: 88px;
    margin-top: 8px;
    border-radius: var(--card-radius);
    background: var(--hover);
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    padding: var(--space-6);
    color: var(--text-tertiary);
    text-align: center;
  }

  .empty-title {
    margin: var(--space-2) 0 0;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .empty-body {
    margin: 0;
    max-width: 220px;
  }

  .add {
    margin-top: var(--space-3);
  }
</style>
