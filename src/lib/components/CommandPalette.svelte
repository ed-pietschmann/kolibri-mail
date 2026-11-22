<script lang="ts" module>
  import type { IconName } from './Icon.svelte';

  export interface Command {
    id: string;
    label: string;
    icon: IconName;
    /** Tastenkürzel zur Anzeige, z. B. „⌘N“. */
    hint?: string;
    /** Zusätzliche Suchwörter (z. B. Kontoname bei Ordnern). */
    keywords?: string;
    run: () => void;
  }

  /**
   * Unscharfe Suche: alle Zeichen der Eingabe müssen in Reihenfolge vorkommen.
   * Treffer am Wortanfang und zusammenhängende Treffer zählen mehr.
   * Liefert null, wenn nicht passend.
   */
  export function fuzzyScore(query: string, text: string): number | null {
    const q = query.toLowerCase().normalize('NFD').replace(/\p{M}/gu, '');
    const s = text.toLowerCase().normalize('NFD').replace(/\p{M}/gu, '');
    if (!q) return 0;
    const direct = s.indexOf(q);
    if (direct >= 0) return 1000 - direct + (direct === 0 || s[direct - 1] === ' ' ? 200 : 0);
    let score = 0;
    let pos = -1;
    for (const ch of q) {
      if (ch === ' ') continue;
      const next = s.indexOf(ch, pos + 1);
      if (next < 0) return null;
      score += next === pos + 1 ? 5 : 1;
      if (next === 0 || s[next - 1] === ' ') score += 8;
      pos = next;
    }
    return score;
  }
</script>

<script lang="ts">
  import { t } from '../i18n';
  import Icon from './Icon.svelte';

  let {
    open,
    commands,
    onsearch,
    onclose,
  }: {
    open: boolean;
    commands: Command[];
    /** Eingabe als Mailsuche übernehmen. */
    onsearch: (text: string) => void;
    onclose: () => void;
  } = $props();

  let dialog: HTMLDialogElement | undefined = $state();
  let input: HTMLInputElement | undefined = $state();
  let listEl: HTMLUListElement | undefined = $state();
  let query = $state('');
  let active = $state(0);

  type Entry = Command & { score: number };

  const entries = $derived.by((): Entry[] => {
    const q = query.trim();
    const matched = commands
      .map((c) => ({ ...c, score: fuzzyScore(q, `${c.label} ${c.keywords ?? ''}`) }))
      .filter((c): c is Entry => c.score !== null)
      .sort((a, b) => b.score - a.score)
      .slice(0, 50);
    if (!q) return matched;
    // Immer auch als Mailsuche anbieten – zuerst die passenden Befehle.
    const search: Entry = {
      id: 'search',
      label: t('palette.search', { q }),
      icon: 'search',
      hint: '↩',
      score: 0,
      run: () => onsearch(q),
    };
    return matched.length ? [...matched, search] : [search];
  });

  const activeId = $derived(entries[active] ? `palette-${entries[active]?.id}` : undefined);

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      query = '';
      active = 0;
      dialog.showModal();
      input?.focus();
    }
    if (!open && dialog.open) dialog.close();
  });

  $effect(() => {
    void query;
    active = 0;
  });

  function run(entry: Entry | undefined) {
    if (!entry) return;
    onclose();
    // Erst schließen, dann ausführen – sonst landet der Fokus im falschen Feld.
    queueMicrotask(entry.run);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'n')) {
      e.preventDefault();
      active = Math.min(entries.length - 1, active + 1);
    } else if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'p')) {
      e.preventDefault();
      active = Math.max(0, active - 1);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      run(entries[active]);
    } else {
      return;
    }
    listEl?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  }
</script>

<dialog
  bind:this={dialog}
  aria-label={t('palette.label')}
  oncancel={(e) => {
    e.preventDefault();
    onclose();
  }}
  onclick={(e) => {
    if (e.target === dialog) onclose();
  }}
>
  <div class="box">
    <div class="field">
      <Icon name="search" size={16} />
      <input
        bind:this={input}
        bind:value={query}
        {onkeydown}
        type="text"
        placeholder={t('palette.placeholder')}
        aria-label={t('palette.placeholder')}
        aria-controls="palette-list"
        aria-activedescendant={activeId}
        spellcheck="false"
        autocomplete="off"
      />
      <kbd>esc</kbd>
    </div>
    <ul id="palette-list" role="listbox" bind:this={listEl}>
      {#each entries as e, i (e.id)}
        <li
          id="palette-{e.id}"
          role="option"
          aria-selected={i === active}
          class:active={i === active}
          data-index={i}
          onclick={() => run(e)}
          onkeydown={() => {}}
          onmousemove={() => (active = i)}
        >
          <span class="icon"><Icon name={e.icon} size={15} /></span>
          <span class="label">{e.label}</span>
          {#if e.hint}<kbd>{e.hint}</kbd>{/if}
        </li>
      {:else}
        <li class="none">{t('palette.empty')}</li>
      {/each}
    </ul>
  </div>
</dialog>

<style>
  dialog {
    margin: 12vh auto auto;
    padding: 0;
    width: min(560px, calc(100vw - 48px));
    border: 0;
    border-radius: 12px;
    background: var(--sheet-bg);
    color: var(--text);
    box-shadow: var(--sheet-shadow);
    overflow: hidden;
  }

  dialog[open] {
    animation: pop-in calc(var(--dur) + 20ms) var(--ease);
  }

  dialog::backdrop {
    background: var(--backdrop);
  }

  @keyframes pop-in {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
  }

  .field {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--separator);
    color: var(--text-tertiary);
  }

  .field input {
    flex: 1;
    height: 30px;
    padding: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
    font-size: 16px;
    color: var(--text);
  }

  .field input:focus {
    box-shadow: none;
  }

  ul {
    max-height: min(420px, 55vh);
    overflow-y: auto;
    margin: 0;
    padding: var(--space-1);
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: 34px;
    padding: 0 var(--space-3);
    border-radius: 7px;
    cursor: default;
  }

  li.active {
    background: var(--active);
    color: var(--text);
  }

  .icon {
    display: inline-flex;
    color: var(--text-secondary);
  }

  li.active .icon {
    color: var(--text);
  }

  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  kbd {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--hover);
    color: var(--text-tertiary);
    font:
      11px ui-monospace,
      'SF Mono',
      Menlo,
      monospace;
  }

  .none {
    justify-content: center;
    color: var(--text-tertiary);
  }
</style>
