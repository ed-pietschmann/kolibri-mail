/**
 * Plattform-Unterschiede im Frontend: Tastenkürzel (⌘ auf dem Mac, Strg unter
 * Windows/Linux) und ihre Anzeige.
 */

import { locale } from './i18n/locale';

export const isMac =
  typeof navigator === 'undefined' || !/Windows|Linux|Android/i.test(navigator.userAgent);

/** Befehlstaste gedrückt? (Mac: ⌘, sonst Strg) */
export function mod(e: { metaKey: boolean; ctrlKey: boolean }): boolean {
  return isMac ? e.metaKey : e.ctrlKey;
}

const NAMES = {
  de: { ctrl: 'Strg', alt: 'Alt', shift: 'Umschalt', enter: 'Enter', back: 'Rücktaste' },
  en: { ctrl: 'Ctrl', alt: 'Alt', shift: 'Shift', enter: 'Enter', back: 'Backspace' },
};

/** „⇧⌘N“ → unter Windows „Strg+Umschalt+N“; auf dem Mac unverändert. */
export function kbd(label: string): string {
  if (isMac) return label;
  const n = NAMES[locale.lang];
  const mods: Record<string, string> = { '⌘': n.ctrl, '⌃': n.ctrl, '⌥': n.alt, '⇧': n.shift };
  const keys: Record<string, string> = { '↩': n.enter, '⌫': n.back };
  const order = [n.ctrl, n.alt, n.shift];
  return label.replace(/([⌘⌃⌥⇧]+)(\S)/g, (_, ms: string, key: string) => {
    const names = [...ms].map((m) => mods[m] ?? m);
    names.sort((a, b) => order.indexOf(a) - order.indexOf(b));
    return [...names, keys[key] ?? key].join('+');
  });
}
