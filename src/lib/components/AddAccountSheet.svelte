<script lang="ts">
  import { ApiError, api, type Account, type DiscoverySource, type ServerConfig } from '../api';
  import { accounts } from '../accounts.svelte';
  import { describeError } from '../errors';
  import { t, type MessageKey } from '../i18n';
  import Icon from './Icon.svelte';
  import ServerFields from './ServerFields.svelte';
  import Sheet from './Sheet.svelte';

  let {
    open,
    onclose,
    onadded,
  }: { open: boolean; onclose: () => void; onadded: (account: Account) => void } = $props();

  const blank = (): ServerConfig => ({ host: '', port: 993, security: 'tls', username: '' });

  let name = $state('');
  let email = $state('');
  let password = $state('');
  let manual = $state(false);
  let imap = $state<ServerConfig>(blank());
  let smtp = $state<ServerConfig>({ ...blank(), port: 465 });
  let phase = $state<'idle' | 'discovering' | 'signingIn'>('idle');
  let error = $state<string | null>(null);
  let source = $state<DiscoverySource | null>(null);
  let discoveredFor = $state('');

  const busy = $derived(phase !== 'idle');
  const canSubmit = $derived(email.includes('@') && password.length > 0 && !busy);

  function reset() {
    name = email = password = '';
    manual = false;
    imap = blank();
    smtp = { ...blank(), port: 465 };
    phase = 'idle';
    error = null;
    source = null;
    discoveredFor = '';
  }

  function close() {
    if (busy) return;
    reset();
    onclose();
  }

  /** Vorschlag für manuelle Einrichtung: übliche Hostnamen der Domain. */
  function prefillGuess() {
    const domain = email.split('@')[1]?.trim().toLowerCase() ?? '';
    if (!imap.host && domain) imap.host = `imap.${domain}`;
    if (!smtp.host && domain) smtp.host = `smtp.${domain}`;
    if (!imap.username) imap.username = email.trim();
    if (!smtp.username) smtp.username = email.trim();
  }

  function onEmailInput(previous: string) {
    // Benutzernamen mitziehen, solange sie der alten Adresse entsprachen.
    if (imap.username === previous) imap.username = email.trim();
    if (smtp.username === previous) smtp.username = email.trim();
  }

  function toggleManual() {
    manual = !manual;
    if (manual) prefillGuess();
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    error = null;
    const address = email.trim();

    if (!manual && discoveredFor !== address) {
      phase = 'discovering';
      try {
        const found = await api.discoverSettings(address);
        imap = found.imap;
        smtp = found.smtp;
        source = found.source;
        discoveredFor = address;
      } catch (err) {
        phase = 'idle';
        manual = true;
        prefillGuess();
        error = describeError(err);
        return;
      }
    }

    phase = 'signingIn';
    try {
      const account = await accounts.add({
        email: address,
        displayName: name.trim() || address.split('@')[0] || address,
        password,
        imap: $state.snapshot(imap),
        smtp: $state.snapshot(smtp),
      });
      reset();
      onadded(account);
    } catch (err) {
      error = describeError(err);
      // Bei Verbindungsproblemen die Einstellungen zeigen, damit man sie prüfen kann.
      if (err instanceof ApiError && err.kind !== 'auth' && err.kind !== 'duplicate') {
        manual = true;
      }
    } finally {
      phase = 'idle';
    }
  }
</script>

<Sheet {open} label={t('add.title')} width={460} onclose={close}>
  <form onsubmit={submit} novalidate>
    <header>
      <div class="badge"><Icon name="mail" size={22} /></div>
      <div>
        <h2>{t('add.title')}</h2>
        <p>{t('add.subtitle')}</p>
      </div>
    </header>

    <div class="fields">
      <label class="field">
        <span>{t('add.name')}</span>
        <input
          type="text"
          bind:value={name}
          placeholder={t('add.namePlaceholder')}
          autocomplete="name"
          disabled={busy}
        />
      </label>
      <label class="field">
        <span>{t('add.email')}</span>
        <input
          type="email"
          value={email}
          oninput={(e) => {
            const previous = email.trim();
            email = e.currentTarget.value;
            onEmailInput(previous);
          }}
          placeholder="name@beispiel.de"
          autocomplete="email"
          spellcheck="false"
          required
          disabled={busy}
        />
      </label>
      <label class="field">
        <span>{t('add.password')}</span>
        <input
          type="password"
          bind:value={password}
          autocomplete="current-password"
          required
          disabled={busy}
        />
        <small>{t('add.passwordHint')}</small>
      </label>
    </div>

    {#if manual}
      <div class="manual">
        <ServerFields legend={t('add.incoming')} bind:config={imap} disabled={busy} />
        <ServerFields legend={t('add.outgoing')} bind:config={smtp} disabled={busy} />
      </div>
    {/if}

    <div class="status" aria-live="polite">
      {#if phase === 'discovering'}
        <span class="spinner"></span>{t('add.discovering')}
      {:else if phase === 'signingIn'}
        <span class="spinner"></span>{t('add.signingIn')}
      {:else if error}
        <span class="error" role="alert">{error}</span>
      {:else if source && !manual}
        <span class="found">✓ {t(`add.found.${source}` as MessageKey)} · {imap.host}</span>
      {/if}
    </div>

    <footer>
      <button type="button" class="link" onclick={toggleManual} disabled={busy}>
        {manual ? t('add.hideManual') : t('add.manual')}
      </button>
      <div class="buttons">
        <button type="button" class="btn" onclick={close} disabled={busy}>
          {t('add.cancel')}
        </button>
        <button type="submit" class="btn primary" disabled={!canSubmit}>
          {t('add.signIn')}
        </button>
      </div>
    </footer>
  </form>
</Sheet>

<style>
  form {
    display: grid;
    gap: var(--space-4);
  }

  header {
    display: flex;
    gap: var(--space-3);
    align-items: center;
  }

  .badge {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 44px;
    height: 44px;
    border-radius: 10px;
    background: var(--accent);
    color: white;
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-lg);
    font-weight: 600;
  }

  header p {
    margin: 2px 0 0;
    color: var(--text-secondary);
  }

  .fields {
    display: grid;
    gap: var(--space-3);
  }

  small {
    color: var(--text-tertiary);
    font-size: var(--font-size-sm);
  }

  .manual {
    display: grid;
    gap: var(--space-4);
    padding-top: var(--space-2);
    border-top: 1px solid var(--separator);
  }

  .status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 18px;
    color: var(--text-secondary);
  }

  .error {
    color: var(--danger);
    user-select: text;
    -webkit-user-select: text;
  }

  .found {
    color: var(--text-secondary);
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
  }
</style>
