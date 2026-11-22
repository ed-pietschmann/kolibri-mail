<script lang="ts">
  import { api, type Suggestion } from '../api';
  import { t } from '../i18n';
  import Icon from './Icon.svelte';

  /**
   * Adressfeld mit Vorschlägen (Kontakte + frühere Empfänger).
   * Der Wert bleibt ein Text („Name <a@b>, c@d“) – vorgeschlagen wird für den
   * Teil nach dem letzten Komma.
   */
  let {
    value = $bindable(''),
    label,
    oninput,
    inputEl = $bindable(),
  }: {
    value: string;
    label: string;
    oninput?: () => void;
    inputEl?: HTMLInputElement | undefined;
  } = $props();

  let suggestions = $state<Suggestion[]>([]);
  let active = $state(0);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let request = 0;

  const open = $derived(suggestions.length > 0);

  /** Der Teil, der gerade getippt wird. */
  function currentToken(text: string): string {
    const parts = text.split(/[,;]/);
    return (parts[parts.length - 1] ?? '').trim();
  }

  function update() {
    oninput?.();
    clearTimeout(timer);
    const token = currentToken(value);
    // Schon eine vollständige Adresse in <…>? Dann nichts vorschlagen.
    if (!token || token.includes('<')) {
      suggestions = [];
      return;
    }
    const mine = ++request;
    timer = setTimeout(async () => {
      try {
        const found = await api.suggestAddresses(token);
        if (mine !== request) return;
        // Schon eingetragene Adressen (mit oder ohne Namen) nicht noch einmal anbieten.
        const present = new Set(
          value
            .split(/[,;]/)
            .slice(0, -1)
            .map((part) => (/<([^>]+)>/.exec(part)?.[1] ?? part).trim().toLowerCase()),
        );
        suggestions = found.filter((s) => !present.has(s.address.toLowerCase()));
        active = 0;
      } catch {
        suggestions = [];
      }
    }, 80);
  }

  function format(s: Suggestion): string {
    if (!s.name) return s.address;
    const name = /[",;<>@]/.test(s.name) ? `"${s.name.replaceAll('"', '')}"` : s.name;
    return `${name} <${s.address}>`;
  }

  function accept(s: Suggestion | undefined) {
    if (!s) return;
    const cut = Math.max(value.lastIndexOf(','), value.lastIndexOf(';'));
    const before = cut >= 0 ? `${value.slice(0, cut + 1).trimEnd()} ` : '';
    value = `${before}${format(s)}, `;
    suggestions = [];
    oninput?.();
    inputEl?.focus();
  }

  function onkeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = (active + 1) % suggestions.length;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = (active - 1 + suggestions.length) % suggestions.length;
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      accept(suggestions[active]);
    } else if (e.key === 'Escape') {
      // Nur die Vorschläge schließen, nicht das ganze Verfassen-Fenster.
      e.stopPropagation();
      e.preventDefault();
      suggestions = [];
    }
  }

  const listId = `addr-${Math.random().toString(36).slice(2)}`;
</script>

<div class="wrap">
  <input
    bind:this={inputEl}
    type="text"
    bind:value
    oninput={update}
    {onkeydown}
    onblur={() => setTimeout(() => (suggestions = []), 120)}
    aria-label={label}
    role="combobox"
    aria-expanded={open}
    aria-controls={listId}
    aria-autocomplete="list"
    aria-activedescendant={open ? `${listId}-${active}` : undefined}
    spellcheck="false"
    autocomplete="off"
  />
  {#if open}
    <ul class="menu" id={listId} role="listbox" aria-label={t('compose.suggestions')}>
      {#each suggestions as s, i (s.address)}
        <li
          id="{listId}-{i}"
          role="option"
          aria-selected={i === active}
          class:active={i === active}
          onmousedown={(e) => {
            e.preventDefault();
            accept(s);
          }}
          onmousemove={() => (active = i)}
        >
          <Icon name={s.contact ? 'person' : 'sent'} size={13} />
          <span class="name">{s.name || s.address}</span>
          {#if s.name}<span class="addr">{s.address}</span>{/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    flex: 1;
    min-width: 0;
  }

  input {
    width: 100%;
    height: 30px;
    padding: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
  }

  input:focus {
    box-shadow: none;
  }

  .menu {
    position: absolute;
    top: calc(100% + 2px);
    left: -8px;
    z-index: 30;
    min-width: 320px;
    max-width: 480px;
    margin: 0;
    padding: var(--space-1);
    list-style: none;
    border-radius: 8px;
    background: var(--sheet-bg);
    box-shadow: var(--sheet-shadow);
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    padding: 0 var(--space-2);
    border-radius: 5px;
    color: var(--text-secondary);
    cursor: default;
  }

  li.active {
    background: var(--active);
    color: var(--text);
  }

  .name {
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .addr {
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
