import { de, type MessageKey } from './de';

type Messages = Record<MessageKey, string>;

const locales: Record<string, Messages> = { de };
let current: Messages = de;

export function setLocale(code: string): void {
  current = locales[code] ?? de;
}

/** Übersetzt einen Schlüssel. Platzhalter: `{name}` → params.name */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const text = current[key];
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (_, name: string) => String(params[name] ?? `{${name}}`));
}

export type { MessageKey };
