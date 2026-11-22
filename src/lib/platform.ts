/**
 * Plattform-Unterschiede im Frontend: Tastenkürzel (⌘ auf dem Mac, Strg unter
 * Windows/Linux) und ihre Anzeige.
 */

export const isMac =
  typeof navigator === 'undefined' || !/Windows|Linux|Android/i.test(navigator.userAgent);

/** Befehlstaste gedrückt? (Mac: ⌘, sonst Strg) */
export function mod(e: { metaKey: boolean; ctrlKey: boolean }): boolean {
  return isMac ? e.metaKey : e.ctrlKey;
}

const WIN_MODS: Record<string, string> = { '⌘': 'Strg', '⌃': 'Strg', '⌥': 'Alt', '⇧': 'Umschalt' };
const WIN_KEYS: Record<string, string> = { '↩': 'Enter', '⌫': 'Rücktaste' };
const ORDER = ['Strg', 'Alt', 'Umschalt'];

/** „⇧⌘N“ → unter Windows „Strg+Umschalt+N“; auf dem Mac unverändert. */
export function kbd(label: string): string {
  if (isMac) return label;
  return label.replace(/([⌘⌃⌥⇧]+)(\S)/g, (_, mods: string, key: string) => {
    const names = [...mods].map((m) => WIN_MODS[m] ?? m);
    names.sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
    return [...names, WIN_KEYS[key] ?? key].join('+');
  });
}
