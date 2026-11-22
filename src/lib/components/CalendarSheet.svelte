<script lang="ts">
  import { untrack } from 'svelte';
  import { api, ApiError, type CalendarInfo, type DavSource } from '../api';
  import { accounts } from '../accounts.svelte';
  import { describeError } from '../errors';
  import { t } from '../i18n';
  import { mail } from '../mail.svelte';
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';

  /**
   * Kalender anlegen (auf dem Mac oder beim Mail-Anbieter), bearbeiten/löschen
   * und andere Kalender-Konten per CalDAV verbinden.
   */
  let {
    mode,
    calendar = null,
    sources = [],
    onclose,
  }: {
    /** null = geschlossen */
    mode: 'add' | 'edit' | null;
    calendar?: CalendarInfo | null;
    sources?: DavSource[];
    onclose: (changed: boolean) => void;
  } = $props();

  const COLORS = [
    '#0a84ff',
    '#30b0c7',
    '#34c759',
    '#a2845e',
    '#ffcc00',
    '#ff9500',
    '#ff3b30',
    '#ff2d55',
    '#af52de',
    '#5856d6',
    '#8e8e93',
  ];

  let tab = $state<'new' | 'connect'>('new');
  let name = $state('');
  let color = $state(COLORS[0] ?? '#0a84ff');
  let where = $state<number | null>(null);
  let targets = $state<number[]>([]);
  let url = $state('');
  /** Anmelden mit einem Mail-Konto (id) oder eigenen Zugangsdaten (null). */
  let signIn = $state<number | null>(null);
  let username = $state('');
  let password = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmDelete = $state(false);

  // Nur beim Öffnen füllen – nicht, wenn sich Konten oder Kalender im Hintergrund ändern.
  $effect(() => {
    const m = mode;
    const cal = calendar;
    if (m) untrack(() => reset(m, cal));
  });

  function reset(mode: 'add' | 'edit', calendar: CalendarInfo | null) {
    error = null;
    busy = false;
    confirmDelete = false;
    if (mode === 'edit' && calendar) {
      name = calendar.name;
      color = calendar.color || COLORS[0] || '#0a84ff';
    } else {
      tab = 'new';
      name = '';
      color = COLORS[Math.floor(Math.random() * COLORS.length)] ?? '#0a84ff';
      url = '';
      username = '';
      password = '';
      signIn = accounts.list[0]?.id ?? null;
      void api
        .calendarAccounts()
        .then((ids) => {
          targets = ids;
          where = ids[0] ?? null;
        })
        .catch(() => (targets = []));
    }
  }

  const source = $derived(
    calendar?.sourceId != null ? sources.find((s) => s.id === calendar?.sourceId) : undefined,
  );
  const isDefaultLocal = $derived(calendar?.url === 'local');
  const accountEmail = (id: number) => accounts.list.find((a) => a.id === id)?.email ?? '';

  function fail(err: unknown) {
    error =
      err instanceof ApiError && ['invalid', 'auth', 'notFound'].includes(err.kind)
        ? err.detail
        : describeError(err);
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    try {
      if (mode === 'edit' && calendar) await api.updateCalendar(calendar.id, name, color);
      else if (tab === 'new') await api.createCalendar(where, name, color);
      else if (signIn !== null) {
        const r = await api.linkAccountCalendar(signIn, url);
        mail.toast = r.contacts
          ? t('calcfg.linkedBoth', { n: r.calendars })
          : t('calcfg.linked', { n: r.calendars });
      } else await api.connectDavSource(name, url, username, password);
      password = '';
      onclose(true);
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!calendar) return;
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    busy = true;
    try {
      if (source) await api.disconnectDavSource(source.id);
      else await api.deleteCalendar(calendar.id);
      onclose(true);
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }
</script>

<Sheet
  open={mode !== null}
  label={mode === 'edit' ? t('calcfg.editTitle') : t('calcfg.addTitle')}
  width={460}
  onclose={() => onclose(false)}
>
  <form onsubmit={submit}>
    <h2>{mode === 'edit' ? t('calcfg.editTitle') : t('calcfg.addTitle')}</h2>

    {#if mode === 'add'}
      <div class="segmented" role="tablist" aria-label={t('calcfg.addTitle')}>
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'new'}
          class:on={tab === 'new'}
          onclick={() => (tab = 'new')}
        >
          <Icon name="plus" size={13} />
          {t('calcfg.tabNew')}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'connect'}
          class:on={tab === 'connect'}
          onclick={() => (tab = 'connect')}
        >
          <Icon name="link" size={13} />
          {t('calcfg.tabConnect')}
        </button>
      </div>
    {/if}

    {#if mode === 'edit' || tab === 'new'}
      <label class="field">
        <span>{t('calcfg.name')}</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input type="text" bind:value={name} placeholder={t('calcfg.namePlaceholder')} autofocus />
      </label>
      <div class="field">
        <span id="color-label">{t('calcfg.color')}</span>
        <div class="colors" role="radiogroup" aria-labelledby="color-label">
          {#each COLORS as c (c)}
            <button
              type="button"
              role="radio"
              aria-checked={color === c}
              aria-label={c}
              class="color"
              class:on={color === c}
              style:--c={c}
              onclick={() => (color = c)}
            ></button>
          {/each}
        </div>
      </div>
      {#if mode === 'add'}
        <label class="field">
          <span>{t('calcfg.where')}</span>
          <select bind:value={where}>
            {#each targets as id (id)}
              <option value={id}>{accountEmail(id)}</option>
            {/each}
            <option value={null}>{t('calendar.local')}</option>
          </select>
          <small>
            {where === null ? t('calcfg.whereLocalHint') : t('calcfg.whereServerHint')}
          </small>
        </label>
      {:else if source}
        <p class="hint">{t('calcfg.fromSource', { name: source.name })}</p>
      {/if}
    {:else}
      <label class="field">
        <span>{t('calcfg.url')}</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          bind:value={url}
          placeholder="https://purelymail.com/webdav/…/caldav/default/"
          autocapitalize="off"
          spellcheck="false"
          autofocus
        />
      </label>
      <label class="field">
        <span>{t('calcfg.signIn')}</span>
        <select bind:value={signIn}>
          {#each accounts.list as a (a.id)}
            <option value={a.id}>{t('calcfg.signInAccount', { email: a.email })}</option>
          {/each}
          <option value={null}>{t('calcfg.signInOther')}</option>
        </select>
        <small>
          {signIn !== null ? t('calcfg.signInAccountHint') : t('calcfg.connectHint')}
        </small>
      </label>
      {#if signIn === null}
        <label class="field">
          <span>{t('calcfg.sourceName')}</span>
          <input type="text" bind:value={name} placeholder="iCloud" />
        </label>
        <label class="field">
          <span>{t('calcfg.user')}</span>
          <input type="text" bind:value={username} autocapitalize="off" spellcheck="false" />
        </label>
        <label class="field">
          <span>{t('calcfg.password')}</span>
          <input type="password" bind:value={password} autocomplete="off" />
        </label>
      {/if}
      <ul class="examples">
        <li><b>Purelymail:</b> {t('calcfg.examplePurelymail')}</li>
        <li><b>iCloud:</b> {t('calcfg.exampleIcloud')}</li>
        <li><b>Nextcloud:</b> {t('calcfg.exampleNextcloud')}</li>
        <li><b>Google:</b> {t('calcfg.exampleGoogle')}</li>
      </ul>
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="buttons">
      {#if mode === 'edit' && !isDefaultLocal}
        <button
          type="button"
          class="btn {confirmDelete ? 'danger' : 'delete'}"
          disabled={busy}
          onclick={() => void remove()}
        >
          {confirmDelete
            ? t('calcfg.deleteConfirm')
            : source
              ? t('calcfg.disconnect')
              : t('calcfg.delete')}
        </button>
      {/if}
      <span class="spacer"></span>
      <button type="button" class="btn" onclick={() => onclose(false)}>{t('add.cancel')}</button>
      <button type="submit" class="btn primary" disabled={busy}>
        {#if busy}<span class="spinner"></span>{/if}
        {mode === 'edit'
          ? t('calcfg.save')
          : tab === 'new'
            ? t('calcfg.create')
            : t('calcfg.connect')}
      </button>
    </div>
  </form>
</Sheet>

<style>
  form {
    display: grid;
    gap: var(--space-4);
  }

  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .segmented {
    display: grid;
    grid-template-columns: 1fr 1fr;
    padding: 2px;
    border-radius: 9px;
    background: var(--field-search);
  }

  .segmented button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 28px;
    border-radius: 7px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
  }

  .segmented button.on {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--sidebar-selected-shadow);
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

  small {
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .colors {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .color {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--c);
    box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.1);
    transition: transform var(--dur) var(--ease);
  }

  .color:hover {
    transform: scale(1.1);
  }

  .color.on {
    box-shadow:
      0 0 0 2px var(--sheet-bg),
      0 0 0 4px var(--c);
  }

  .hint {
    margin: 0;
    color: var(--text-secondary);
  }

  .examples {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: var(--space-3) var(--space-3) var(--space-3) var(--space-6);
    border-radius: var(--card-radius);
    background: var(--hover);
    color: var(--text-secondary);
    font-size: 12px;
  }

  .examples b {
    color: var(--text);
  }

  .error {
    margin: 0;
    color: var(--danger);
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
  }

  .spacer {
    flex: 1;
  }

  .btn.delete {
    color: var(--danger);
  }
</style>
