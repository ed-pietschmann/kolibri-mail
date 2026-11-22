/**
 * Signaturen und Vorlagen (klein, wird erst beim Verfassen bzw. Bearbeiten geladen).
 */
import { api, CONTENT_PLACEHOLDER, type Signature, type Template } from './api';
import { has, t } from './i18n';

export const library = $state({
  signatures: [] as Signature[],
  templates: [] as Template[],
  loaded: false,
});

let pending: Promise<void> | null = null;

/** Lädt beide Listen (einmal; `force` lädt neu). */
export function loadLibrary(force = false): Promise<void> {
  if (library.loaded && !force) return Promise.resolve();
  pending ??= Promise.all([api.listSignatures(), api.listTemplates()])
    .then(([signatures, templates]) => {
      library.signatures = signatures;
      library.templates = templates;
      library.loaded = true;
    })
    .finally(() => (pending = null));
  return pending;
}

export function signaturesOf(accountId: number): Signature[] {
  return library.signatures.filter((s) => s.accountId === accountId);
}

/** Anzeigename: mitgelieferte Designs übersetzt, eigene unverändert. */
export function templateName(tpl: Pick<Template, 'name' | 'builtin'>): string {
  if (!tpl.builtin) return tpl.name;
  const key = `design.${tpl.name}`;
  return has(key) ? t(key) : tpl.name;
}

/** `{{t:schlüssel}}` in Designs für die Vorschau ausfüllen (beim Senden macht das Rust). */
export function designTexts(html: string): string {
  return html.replace(/\{\{t:(\w+)\}\}/g, (all, name: string) => {
    const key = `designText.${name}`;
    return has(key) ? t(key) : all;
  });
}

/** Vorlage mit Platz für eigenen Text? (Sonst: fertige Mail aus Code.) */
export function hasContentSlot(t: Pick<Template, 'html'>): boolean {
  return t.html.includes(CONTENT_PLACEHOLDER);
}

/** Zuletzt gewählte Vorlage für neue Mails eines Kontos (nur Komfort). */
export function lastTemplate(accountId: number): number | null {
  try {
    const v = localStorage.getItem(`kolibri.template.${accountId}`);
    return v ? Number(v) : null;
  } catch {
    return null;
  }
}

export function rememberTemplate(accountId: number, id: number | null): void {
  try {
    if (id === null) localStorage.removeItem(`kolibri.template.${accountId}`);
    else localStorage.setItem(`kolibri.template.${accountId}`, String(id));
  } catch {
    // Ohne Speicher eben ohne Erinnerung.
  }
}

/** Grundgerüst für eine neue eigene Vorlage. */
export const TEMPLATE_SKELETON = `<!doctype html>
<html>
<head><meta charset="utf-8"></head>
<body style="margin:0;padding:24px;background:#f4f4f6">
  <div style="max-width:600px;margin:0 auto;padding:32px;background:#ffffff;border-radius:12px;
    font-family:-apple-system,Helvetica,Arial,sans-serif;font-size:15px;line-height:1.6;color:#1d1d1f">
    ${CONTENT_PLACEHOLDER}
  </div>
</body>
</html>`;
