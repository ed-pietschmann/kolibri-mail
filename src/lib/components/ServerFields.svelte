<script lang="ts">
  import type { Security, ServerConfig } from '../api';
  import { t } from '../i18n';

  let {
    legend,
    config = $bindable(),
    disabled = false,
  }: { legend: string; config: ServerConfig; disabled?: boolean } = $props();

  const options: { value: Security; label: string }[] = [
    { value: 'tls', label: t('add.security.tls') },
    { value: 'startTls', label: t('add.security.startTls') },
    { value: 'none', label: t('add.security.none') },
  ];
</script>

<fieldset {disabled}>
  <legend>{legend}</legend>
  <div class="row">
    <label class="field host">
      <span>{t('add.host')}</span>
      <input type="text" bind:value={config.host} spellcheck="false" autocomplete="off" />
    </label>
    <label class="field port">
      <span>{t('add.port')}</span>
      <input type="number" bind:value={config.port} min="1" max="65535" />
    </label>
  </div>
  <div class="row">
    <label class="field">
      <span>{t('add.security')}</span>
      <select bind:value={config.security}>
        {#each options as o (o.value)}
          <option value={o.value}>{o.label}</option>
        {/each}
      </select>
    </label>
    <label class="field">
      <span>{t('add.username')}</span>
      <input type="text" bind:value={config.username} spellcheck="false" autocomplete="off" />
    </label>
  </div>
</fieldset>

<style>
  fieldset {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }

  legend {
    padding: 0;
    margin-bottom: var(--space-2);
    font-weight: 600;
  }

  .row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2);
  }

  .row:first-of-type {
    grid-template-columns: 1fr 88px;
  }
</style>
