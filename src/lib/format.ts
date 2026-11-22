/** Datums- und Größenformatierung (in der Sprache der Oberfläche). */
import { locale, t } from './i18n';

const time = new Intl.DateTimeFormat(locale.intl, { hour: '2-digit', minute: '2-digit' });
const weekday = new Intl.DateTimeFormat(locale.intl, { weekday: 'long' });
const short = new Intl.DateTimeFormat(locale.intl, {
  day: '2-digit',
  month: '2-digit',
  year: '2-digit',
});
const full = new Intl.DateTimeFormat(locale.intl, { dateStyle: 'medium', timeStyle: 'short' });

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Für die Liste: heute → Uhrzeit, gestern, diese Woche → Wochentag, sonst Datum. */
export function listDate(unix: number, now = new Date()): string {
  const d = new Date(unix * 1000);
  const days = Math.round((startOfDay(now) - startOfDay(d)) / 86_400_000);
  if (days <= 0) return time.format(d);
  if (days === 1) return t('format.yesterday');
  if (days < 7) return weekday.format(d);
  return short.format(d);
}

export function fullDate(unix: number): string {
  return full.format(new Date(unix * 1000));
}

export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toLocaleString(locale.intl, { maximumFractionDigits: 1 })} MB`;
}

/** Anzeigename: Name oder – falls leer – Adresse. */
export function displayName(name: string, addr: string): string {
  return name.trim() || addr;
}
