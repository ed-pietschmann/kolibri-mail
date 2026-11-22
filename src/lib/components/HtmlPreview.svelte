<script lang="ts">
  /**
   * Vorschau einer vollständigen HTML-Mail (Vorlage, eigener Code, fertige Mail).
   * Komplett abgeschottet: `sandbox` ohne Rechte (kein Skript, kein Zugriff auf
   * die App), eigene CSP im Dokument. Bilder aus dem Netz sind erlaubt – es ist
   * der eigene Entwurf. Mit `scale` als verkleinertes Vorschaubild.
   */
  let {
    html,
    title,
    scale = 1,
  }: {
    html: string;
    title: string;
    /** < 1: als Miniatur (600 px breit gerendert, verkleinert). */
    scale?: number;
  } = $props();

  const CSP =
    '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; ' +
    "style-src 'unsafe-inline'; img-src data: https:; font-src data: https:\">" +
    '<meta name="referrer" content="no-referrer">';

  const srcdoc = $derived(CSP + html);
</script>

{#if scale < 1}
  <div class="thumb" aria-hidden="true">
    <iframe
      {title}
      sandbox=""
      {srcdoc}
      tabindex="-1"
      style:width="{100 / scale}%"
      style:height="{100 / scale}%"
      style:transform="scale({scale})"
    ></iframe>
  </div>
{:else}
  <iframe class="full" {title} sandbox="" {srcdoc}></iframe>
{/if}

<style>
  iframe {
    display: block;
    border: 0;
    background: #fff;
    color-scheme: light;
  }

  .full {
    width: 100%;
    height: 100%;
  }

  .thumb {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    pointer-events: none;
  }

  .thumb iframe {
    position: absolute;
    top: 0;
    left: 0;
    transform-origin: 0 0;
  }
</style>
