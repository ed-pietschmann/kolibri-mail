<script lang="ts">
  import { tick } from 'svelte';
  import { api, ApiError, type Contact, type ContactInput, type Suggestion } from '../api';
  import { accounts } from '../accounts.svelte';
  import { describeError } from '../errors';
  import { t } from '../i18n';
  import { mail } from '../mail.svelte';
  import Icon from './Icon.svelte';

  /** Kontaktliste + Detail – belegt die Spalten von Liste und Leseansicht. */
  let { onwrite }: { onwrite: (address: string) => void } = $props();

  let contacts = $state<Contact[]>([]);
  /** Frühere Empfänger, die noch kein Kontakt sind. */
  let collected = $state<Suggestion[]>([]);
  let filter = $state('');
  /** „c:<id>“ für Kontakte, „r:<adresse>“ für frühere Empfänger. */
  let selectedKey = $state<string | null>(null);
  let loaded = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** Bearbeiten/Neu: Formular im rechten Bereich. */
  let editing = $state<ContactInput | null>(null);
  let targets = $state<number[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmDelete = $state(false);
  let nameInput: HTMLInputElement | undefined = $state();

  const selected = $derived(contacts.find((c) => `c:${c.id}` === selectedKey) ?? null);
  const recipient = $derived(collected.find((r) => `r:${r.address}` === selectedKey) ?? null);
  const keys = $derived([
    ...contacts.map((c) => `c:${c.id}`),
    ...collected.map((r) => `r:${r.address}`),
  ]);

  async function load() {
    try {
      [contacts, collected] = await Promise.all([
        api.listContacts(filter),
        api.listCollected(filter).catch(() => []),
      ]);
    } catch {
      contacts = [];
    } finally {
      loaded = true;
    }
  }

  function select(key: string) {
    selectedKey = key;
    editing = null;
    confirmDelete = false;
    error = null;
  }

  async function startEdit(input: ContactInput) {
    targets = await api.contactAccounts().catch(() => []);
    editing = {
      ...input,
      emails: input.emails.length ? input.emails : [{ value: '', label: '' }],
      phones: input.phones.length ? input.phones : [{ value: '', label: '' }],
    };
    if (input.id === null && input.accountId === null) editing.accountId = targets[0] ?? null;
    error = null;
    confirmDelete = false;
    await tick();
    nameInput?.focus();
  }

  function newContact(from?: Suggestion) {
    void startEdit({
      id: null,
      accountId: null,
      name: from?.name ?? '',
      org: '',
      emails: from ? [{ value: from.address, label: '' }] : [],
      phones: [],
    });
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (!editing || busy) return;
    busy = true;
    error = null;
    try {
      const id = await api.saveContact($state.snapshot(editing));
      editing = null;
      await load();
      selectedKey = `c:${id}`;
    } catch (err) {
      error = err instanceof ApiError && err.kind === 'invalid' ? err.detail : describeError(err);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!selected) return;
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    busy = true;
    try {
      await api.deleteContact(selected.id);
      selectedKey = null;
      await load();
    } catch (err) {
      error = describeError(err);
    } finally {
      busy = false;
      confirmDelete = false;
    }
  }

  async function forget() {
    if (!recipient) return;
    await api.forgetCollected(recipient.address).catch(() => {});
    selectedKey = null;
    await load();
  }

  const accountLabel = (id: number | null) =>
    id === null
      ? t('contacts.onThisMac')
      : (accounts.list.find((a) => a.id === id)?.email ?? t('contacts.onThisMac'));

  $effect(() => {
    void mail.contactsVersion;
    void load();
  });

  function onfilter(text: string) {
    filter = text;
    clearTimeout(timer);
    timer = setTimeout(() => void load(), 120);
  }

  function initials(name: string): string {
    const parts = name.trim().split(/\s+/);
    return (
      (parts[0]?.[0] ?? '') + (parts.length > 1 ? (parts.at(-1)?.[0] ?? '') : '')
    ).toUpperCase();
  }

  /** Erster Buchstabe für die Abschnitte der Liste. */
  function letter(name: string): string {
    const c = name.trim().charAt(0).toUpperCase();
    return /\p{L}/u.test(c) ? c.normalize('NFD').charAt(0) : '#';
  }

  function onkeydown(e: KeyboardEvent) {
    const i = selectedKey ? keys.indexOf(selectedKey) : -1;
    if (e.key === 'ArrowDown' || e.key === 'j') {
      e.preventDefault();
      e.stopPropagation();
      const next = keys[Math.min(keys.length - 1, i + 1)];
      if (next) select(next);
    } else if (e.key === 'ArrowUp' || e.key === 'k') {
      e.preventDefault();
      e.stopPropagation();
      const prev = keys[Math.max(0, i - 1)];
      if (prev) select(prev);
    }
  }

  /** Feste Avatarfarbe je Name. */
  function hueOf(name: string): number {
    return [...name.toLowerCase()].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);
  }
</script>

<section class="list" aria-label={t('contacts.label')}>
  <header class="toolbar" data-tauri-drag-region>
    <div class="heading" data-tauri-drag-region>
      <h1 data-tauri-drag-region>{t('contacts.label')}</h1>
      {#if loaded && contacts.length}
        <span class="count" data-tauri-drag-region>
          {t('contacts.count', { n: contacts.length })}
        </span>
      {/if}
    </div>
    <button
      class="tool"
      aria-label={t('contacts.new')}
      title={t('contacts.new')}
      onclick={() => newContact()}
    >
      <Icon name="userPlus" size={16} />
    </button>
  </header>
  <div class="filter">
    <Icon name="search" size={13} />
    <input
      type="text"
      value={filter}
      oninput={(e) => onfilter(e.currentTarget.value)}
      placeholder={t('contacts.search')}
      aria-label={t('contacts.search')}
      spellcheck="false"
      autocomplete="off"
    />
  </div>
  {#if loaded && contacts.length === 0 && collected.length === 0}
    <div class="empty" role="status">
      <Icon name="person" size={32} />
      <p class="empty-title">{t('contacts.empty.title')}</p>
      {#if !filter}
        <p class="empty-body">{t('contacts.empty.body')}</p>
        <button class="btn primary" onclick={() => newContact()}>{t('contacts.new')}</button>
      {/if}
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <ul class="items" role="listbox" aria-label={t('contacts.label')} tabindex="0" {onkeydown}>
      {#each contacts as c, i (c.id)}
        {#if i === 0 || letter(c.name) !== letter(contacts[i - 1]?.name ?? '')}
          <li class="letter" aria-hidden="true">{letter(c.name)}</li>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          role="option"
          aria-selected={`c:${c.id}` === selectedKey}
          class:selected={`c:${c.id}` === selectedKey}
          onclick={() => select(`c:${c.id}`)}
        >
          <span class="avatar small" style:--hue={hueOf(c.name)} aria-hidden="true">
            {initials(c.name)}
          </span>
          <span class="who">
            <span class="name">{c.name}</span>
            {#if c.org || c.emails[0]}<span class="sub">{c.org || c.emails[0]?.value}</span>{/if}
          </span>
        </li>
      {/each}
      {#if collected.length}
        <li class="letter section" aria-hidden="true">{t('contacts.recent')}</li>
        {#each collected as r (r.address)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            role="option"
            aria-selected={`r:${r.address}` === selectedKey}
            class:selected={`r:${r.address}` === selectedKey}
            onclick={() => select(`r:${r.address}`)}
          >
            <span class="avatar small ghost" aria-hidden="true">
              {initials(r.name || r.address)}
            </span>
            <span class="who">
              <span class="name">{r.name || r.address}</span>
              {#if r.name}<span class="sub">{r.address}</span>{/if}
            </span>
          </li>
        {/each}
      {/if}
    </ul>
  {/if}
</section>

<section class="detail" aria-label={selected?.name ?? recipient?.address ?? t('contacts.none')}>
  <header class="toolbar actions" data-tauri-drag-region>
    {#if selected && !editing}
      <button
        class="act"
        onclick={() =>
          selected &&
          void startEdit({
            id: selected.id,
            accountId: selected.accountId,
            name: selected.name,
            org: selected.org,
            emails: selected.emails.map((e) => ({ ...e })),
            phones: selected.phones.map((p) => ({ ...p })),
          })}
      >
        <Icon name="compose" size={14} />
        {t('contacts.edit')}
      </button>
      <button
        class="act"
        class:danger={confirmDelete}
        disabled={busy}
        onclick={() => void remove()}
      >
        <Icon name="trash" size={14} />
        {confirmDelete ? t('contacts.deleteConfirm') : t('contacts.delete')}
      </button>
    {/if}
  </header>
  {#if editing}
    <form class="card form" onsubmit={save}>
      <h2 class="form-title">{editing.id === null ? t('contacts.new') : t('contacts.edit')}</h2>
      <label class="field">
        <span>{t('contacts.name')}</span>
        <input bind:this={nameInput} type="text" bind:value={editing.name} />
      </label>
      <label class="field">
        <span>{t('contacts.org')}</span>
        <input type="text" bind:value={editing.org} />
      </label>
      <div class="field">
        <span>{t('contacts.email')}</span>
        {#each editing.emails as e, i (i)}
          <div class="multi">
            <input
              type="email"
              bind:value={e.value}
              aria-label="{t('contacts.email')} {i + 1}"
              placeholder="name@beispiel.de"
            />
            <button
              type="button"
              class="mini"
              aria-label={t('contacts.removeField')}
              onclick={() => editing?.emails.splice(i, 1)}
            >
              <Icon name="minimize" size={13} />
            </button>
          </div>
        {/each}
        <button
          type="button"
          class="add-field"
          onclick={() => editing?.emails.push({ value: '', label: '' })}
        >
          <Icon name="plus" size={12} />
          {t('contacts.addEmail')}
        </button>
      </div>
      <div class="field">
        <span>{t('contacts.phone')}</span>
        {#each editing.phones as p, i (i)}
          <div class="multi">
            <input type="tel" bind:value={p.value} aria-label="{t('contacts.phone')} {i + 1}" />
            <button
              type="button"
              class="mini"
              aria-label={t('contacts.removeField')}
              onclick={() => editing?.phones.splice(i, 1)}
            >
              <Icon name="minimize" size={13} />
            </button>
          </div>
        {/each}
        <button
          type="button"
          class="add-field"
          onclick={() => editing?.phones.push({ value: '', label: '' })}
        >
          <Icon name="plus" size={12} />
          {t('contacts.addPhone')}
        </button>
      </div>
      {#if editing.id === null}
        <label class="field">
          <span>{t('contacts.saveIn')}</span>
          <select bind:value={editing.accountId}>
            <option value={null}>{t('contacts.onThisMac')}</option>
            {#each targets as id (id)}
              <option value={id}>{accountLabel(id)}</option>
            {/each}
          </select>
        </label>
      {:else}
        <p class="hint">{t('contacts.storedIn', { where: accountLabel(editing.accountId) })}</p>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="buttons">
        <button type="button" class="btn" onclick={() => (editing = null)}>
          {t('add.cancel')}
        </button>
        <button type="submit" class="btn primary" disabled={busy}>{t('contacts.save')}</button>
      </div>
    </form>
  {:else if selected}
    <div class="card">
      <div class="head">
        <span class="avatar big" style:--hue={hueOf(selected.name)} aria-hidden="true">
          {initials(selected.name)}
        </span>
        <h2>{selected.name}</h2>
        {#if selected.org}<p class="org">{selected.org}</p>{/if}
        {#if selected.emails[0]}
          <button class="btn primary" onclick={() => onwrite(selected?.emails[0]?.value ?? '')}>
            <Icon name="compose" size={14} />
            {t('contacts.write')}
          </button>
        {/if}
      </div>
      <dl>
        {#each selected.emails as e (e.value)}
          <dt>{e.label || t('contacts.email')}</dt>
          <dd>
            <button
              class="link"
              title={t('contacts.writeTo', { addr: e.value })}
              onclick={() => onwrite(e.value)}
            >
              {e.value}
            </button>
          </dd>
        {/each}
        {#each selected.phones as p (p.value)}
          <dt>{p.label || t('contacts.phone')}</dt>
          <dd class="selectable">{p.value}</dd>
        {/each}
      </dl>
      <p class="hint">{t('contacts.storedIn', { where: accountLabel(selected.accountId) })}</p>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </div>
  {:else if recipient}
    <div class="card">
      <div class="head">
        <span class="avatar big ghost" aria-hidden="true">
          {initials(recipient.name || recipient.address)}
        </span>
        <h2>{recipient.name || recipient.address}</h2>
        <p class="org">{t('contacts.recentHint')}</p>
        <div class="row-buttons">
          <button class="btn primary" onclick={() => onwrite(recipient?.address ?? '')}>
            <Icon name="compose" size={14} />
            {t('contacts.write')}
          </button>
          <button class="btn" onclick={() => recipient && newContact(recipient)}>
            <Icon name="userPlus" size={14} />
            {t('contacts.saveAsContact')}
          </button>
        </div>
      </div>
      <dl>
        <dt>{t('contacts.email')}</dt>
        <dd class="selectable">{recipient.address}</dd>
      </dl>
      <button class="link forget" onclick={() => void forget()}>{t('contacts.forget')}</button>
    </div>
  {:else}
    <div class="empty">{t('contacts.none')}</div>
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

  .detail {
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
    flex-shrink: 0;
  }

  .list .toolbar {
    border-bottom: 1px solid var(--separator);
  }

  .heading {
    display: flex;
    flex-direction: column;
  }

  h1 {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }

  .count {
    font-size: var(--font-size-sm);
    color: var(--text-tertiary);
  }

  .filter {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-2) var(--space-3);
    padding: 0 var(--space-2);
    height: 28px;
    border-radius: var(--radius);
    background: var(--field-bg);
    box-shadow: 0 0 0 1px var(--field-border);
    color: var(--text-tertiary);
  }

  .filter:focus-within {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .filter input {
    flex: 1;
    height: 26px;
    padding: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
  }

  .items {
    flex: 1;
    overflow-y: auto;
    margin: 0;
    padding: 0 var(--space-2) var(--space-2);
    list-style: none;
    outline: none;
  }

  .letter {
    position: sticky;
    top: 0;
    padding: var(--space-2) var(--space-2) 2px;
    background: var(--surface);
    font-size: var(--font-size-sm);
    font-weight: 600;
    color: var(--text-tertiary);
  }

  li[role='option'] {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 8px var(--space-2);
    border-radius: var(--card-radius);
  }

  li[role='option']:hover {
    background: var(--hover);
  }

  li.selected,
  li.selected:hover {
    background: var(--list-selected);
  }

  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name,
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-weight: 500;
  }

  .sub {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .avatar {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    border-radius: 50%;
    background: hsl(var(--hue) 45% 28%);
    color: hsl(var(--hue) 80% 86%);
    font-weight: 600;
  }

  @media (prefers-color-scheme: light) {
    .avatar {
      background: hsl(var(--hue) 70% 92%);
      color: hsl(var(--hue) 50% 32%);
    }
  }

  .avatar.small {
    width: 30px;
    height: 30px;
    font-size: 12px;
  }

  .avatar.big {
    width: 72px;
    height: 72px;
    font-size: 26px;
  }

  .card {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-4) var(--space-6) var(--space-6);
  }

  .head {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding-bottom: var(--space-6);
    border-bottom: 1px solid var(--separator);
    text-align: center;
  }

  h2 {
    margin: var(--space-2) 0 0;
    font-size: var(--font-size-xl);
    font-weight: 600;
    user-select: text;
    -webkit-user-select: text;
  }

  .org {
    margin: 0;
    color: var(--text-secondary);
  }

  .head .btn {
    margin-top: var(--space-2);
  }

  dl {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: var(--space-3) var(--space-4);
    margin: var(--space-6) 0 0;
  }

  dt {
    color: var(--text-secondary);
    text-align: right;
    font-size: 12px;
    padding-top: 1px;
  }

  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .selectable {
    user-select: text;
    -webkit-user-select: text;
  }

  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    margin-left: auto;
    border-radius: 8px;
    color: var(--icon);
  }

  .tool:hover {
    background: var(--hover);
    color: var(--text);
  }

  .actions {
    gap: var(--space-1);
    justify-content: flex-end;
  }

  .act {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 var(--space-3);
    border-radius: 8px;
    color: var(--text-secondary);
    font-weight: 500;
  }

  .act:hover {
    background: var(--hover);
    color: var(--text);
  }

  .act.danger {
    background: var(--danger);
    color: white;
  }

  .letter.section {
    margin-top: var(--space-4);
  }

  .avatar.ghost {
    background: var(--hover);
    color: var(--text-secondary);
  }

  .form {
    display: grid;
    align-content: start;
    gap: var(--space-3);
    max-width: 440px;
  }

  .form-title {
    margin: 0 0 var(--space-2);
    font-size: 19px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .field {
    display: grid;
    gap: 6px;
  }

  .field > span {
    color: var(--text-tertiary);
    font-size: 12px;
    font-weight: 600;
  }

  .multi {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .multi input {
    flex: 1;
  }

  .mini {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    color: var(--text-tertiary);
  }

  .mini:hover {
    background: var(--hover);
    color: var(--danger);
  }

  .add-field {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    justify-self: start;
    padding: 2px 0;
    color: var(--accent);
    font-size: 12px;
    font-weight: 500;
  }

  .buttons,
  .row-buttons {
    display: flex;
    gap: var(--space-2);
  }

  .buttons {
    justify-content: flex-end;
    margin-top: var(--space-2);
  }

  .row-buttons {
    flex-wrap: wrap;
    justify-content: center;
  }

  .error {
    margin: 0;
    color: var(--danger);
  }

  .forget {
    margin-top: var(--space-4);
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .hint {
    margin-top: var(--space-6);
    font-size: 12px;
    color: var(--text-tertiary);
    text-align: center;
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
    max-width: 240px;
  }
</style>
