<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * Modales Blatt wie bei macOS-Sheets: erscheint oben unter der
   * Titelleiste. Baut auf <dialog> auf (Fokusfalle, Esc, VoiceOver).
   */
  let {
    open,
    label,
    width = 440,
    onclose,
    children,
  }: {
    open: boolean;
    label: string;
    width?: number;
    onclose: () => void;
    children: Snippet;
  } = $props();

  let dialog: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });
</script>

<dialog
  bind:this={dialog}
  aria-label={label}
  style:width="{width}px"
  oncancel={(e) => {
    e.preventDefault();
    onclose();
  }}
>
  {#if open}
    {@render children()}
  {/if}
</dialog>

<style>
  dialog {
    margin: var(--titlebar-h) auto auto;
    padding: var(--space-6);
    border: 0;
    border-radius: 12px;
    background: var(--sheet-bg);
    color: var(--text);
    box-shadow: var(--sheet-shadow);
    max-height: calc(100vh - var(--titlebar-h) - var(--space-6));
    overflow-y: auto;
  }

  dialog[open] {
    animation: sheet-in calc(var(--dur) + 20ms) var(--ease);
  }

  dialog::backdrop {
    background: var(--backdrop);
  }

  @keyframes sheet-in {
    from {
      opacity: 0;
      transform: translateY(-8px) scale(0.99);
    }
  }
</style>
