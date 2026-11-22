<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';

  /**
   * Zeigt (bereits in Rust gesäubertes) Mail-HTML in einem abgeschotteten iframe:
   * - `sandbox` ohne `allow-scripts`: kein JavaScript, keine Formulare, keine Popups.
   * - Eigene CSP im Dokument: nichts lädt nach außen. Externe Bilder (https:)
   *   nur, wenn `allowRemote` gesetzt ist.
   * - Links öffnen im Standardbrowser statt im Fenster.
   * `allow-same-origin` ist nötig, damit wir die Höhe messen und Klicks abfangen
   * können – ungefährlich, weil im iframe nie ein Skript läuft.
   */
  let { html, allowRemote }: { html: string; allowRemote: boolean } = $props();

  let frame: HTMLIFrameElement | undefined = $state();
  let height = $state(200);

  const srcdoc = $derived(`<!doctype html>
<html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:${allowRemote ? ' https:' : ''}; font-src data:; form-action 'none'">
<meta name="referrer" content="no-referrer">
<style>
  html { color-scheme: light; }
  body { margin: 0; padding: 20px 24px; background: #fff; color: #1d1d1f;
    font: 14px/1.5 -apple-system, BlinkMacSystemFont, 'Helvetica Neue', sans-serif;
    overflow-wrap: anywhere; }
  img { max-width: 100%; height: auto; }
  table { max-width: 100% !important; }
  pre { white-space: pre-wrap; }
  a { color: #0a66d6; }
</style></head><body>${html}</body></html>`);

  function measure() {
    const doc = frame?.contentDocument;
    if (!doc?.documentElement) return;
    height = Math.max(80, doc.documentElement.scrollHeight);
  }

  function onload() {
    const doc = frame?.contentDocument;
    if (!doc) return;
    measure();
    // Bilder ändern die Höhe nach dem Laden.
    const observer = new ResizeObserver(measure);
    observer.observe(doc.documentElement);
    doc.addEventListener('click', (e) => {
      const target = e.target as Element | null;
      const link = target?.closest('a[href]');
      if (!link) return;
      e.preventDefault();
      const href = link.getAttribute('href') ?? '';
      if (/^(https?:|mailto:)/i.test(href)) void openUrl(href).catch(() => {});
    });
  }
</script>

<iframe
  bind:this={frame}
  title="Nachrichteninhalt"
  sandbox="allow-same-origin"
  referrerpolicy="no-referrer"
  {srcdoc}
  style:height="{height}px"
  {onload}
></iframe>

<style>
  iframe {
    display: block;
    width: 100%;
    border: 0;
    border-radius: 10px;
    background: #fff;
    box-shadow: 0 0 0 1px var(--separator);
  }
</style>
