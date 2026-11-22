<script lang="ts">
  import { t } from '../i18n';
  import Sheet from './Sheet.svelte';

  let {
    open,
    title,
    body,
    confirmLabel,
    destructive = false,
    error = null,
    secondaryLabel = null,
    onconfirm,
    onsecondary,
    oncancel,
  }: {
    open: boolean;
    title: string;
    body: string;
    confirmLabel: string;
    destructive?: boolean;
    error?: string | null;
    /** Optionaler dritter Knopf links (z. B. „Verwerfen“). */
    secondaryLabel?: string | null;
    onconfirm: () => void;
    onsecondary?: () => void;
    oncancel: () => void;
  } = $props();
</script>

<Sheet {open} label={title} width={380} onclose={oncancel}>
  <div class="content">
    <h2>{title}</h2>
    <p>{body}</p>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="buttons">
      {#if secondaryLabel}
        <button class="btn secondary" onclick={onsecondary}>{secondaryLabel}</button>
      {/if}
      <!-- Fokus auf die sichere Wahl: bei Gefährlichem „Abbrechen“, sonst die Hauptaktion. -->
      <!-- svelte-ignore a11y_autofocus -->
      <button class="btn" autofocus={destructive} onclick={oncancel}>{t('add.cancel')}</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button
        class="btn"
        class:danger={destructive}
        autofocus={!destructive}
        class:primary={!destructive}
        onclick={onconfirm}
      >
        {confirmLabel}
      </button>
    </div>
  </div>
</Sheet>

<style>
  .content {
    display: grid;
    gap: var(--space-3);
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-lg);
    font-weight: 600;
  }

  p {
    margin: 0;
    color: var(--text-secondary);
  }

  .error {
    color: var(--danger);
  }

  .secondary {
    margin-right: auto;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
