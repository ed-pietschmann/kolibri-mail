/**
 * Sprache der Oberfläche: „de“ oder „en“. Wird in main.ts vor dem Laden der App
 * gesetzt (Systemsprache vom Backend; `?lang=en` in der Browser-Vorschau).
 */
export type Lang = 'de' | 'en';

export const locale = {
  lang: 'de' as Lang,
  /** Für Intl-Formate (Datum, Zahlen). */
  intl: 'de-DE',
};

export function setLang(lang: Lang): void {
  locale.lang = lang;
  const nav = typeof navigator === 'undefined' ? '' : navigator.language;
  // Englisch: Datumsformat des Systems, falls es englisch ist (en-US, en-GB …).
  locale.intl = lang === 'de' ? 'de-DE' : nav.toLowerCase().startsWith('en') ? nav : 'en-GB';
  if (typeof document !== 'undefined') document.documentElement.lang = lang;
}
