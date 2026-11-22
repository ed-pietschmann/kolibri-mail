import { de, type MessageKey } from './de';
import { en } from './en';
import { locale, setLang, type Lang } from './locale';

type Messages = Record<MessageKey, string>;

const locales: Record<Lang, Messages> = { de, en };
let current: Messages = de;

/** Sprache setzen (vor dem Laden der App, siehe main.ts). */
export function setLocale(lang: Lang): void {
  setLang(lang);
  current = locales[lang];
}

export { locale };

/** Übersetzt einen Schlüssel. Platzhalter: `{name}` → params.name */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const text = current[key];
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (_, name: string) => String(params[name] ?? `{${name}}`));
}

/** Gibt es einen Text zu diesem Schlüssel? */
export function has(key: string): key is MessageKey {
  return key in current;
}

export type { MessageKey };
