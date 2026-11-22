/** Datums-Helfer für die Kalenderansichten (lokale Zeit, Woche beginnt Montag). */

export type View = 'day' | 'week' | 'month';

export function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

export function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n, d.getHours(), d.getMinutes());
}

export function addMonths(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth() + n, 1);
}

/** Montag der Woche. */
export function startOfWeek(d: Date): Date {
  const day = (d.getDay() + 6) % 7;
  return addDays(startOfDay(d), -day);
}

export function sameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

export const unix = (d: Date) => Math.floor(d.getTime() / 1000);
export const fromUnix = (s: number) => new Date(s * 1000);

/** Sichtbarer Zeitraum einer Ansicht: [Beginn, Ende) und die Tage. */
export function range(view: View, anchor: Date): { from: Date; to: Date; days: Date[] } {
  if (view === 'day') {
    const from = startOfDay(anchor);
    return { from, to: addDays(from, 1), days: [from] };
  }
  if (view === 'week') {
    const from = startOfWeek(anchor);
    return {
      from,
      to: addDays(from, 7),
      days: Array.from({ length: 7 }, (_, i) => addDays(from, i)),
    };
  }
  // Monat: immer 6 volle Wochen ab dem Montag vor dem Monatsersten.
  const from = startOfWeek(new Date(anchor.getFullYear(), anchor.getMonth(), 1));
  return {
    from,
    to: addDays(from, 42),
    days: Array.from({ length: 42 }, (_, i) => addDays(from, i)),
  };
}

/** ISO-Kalenderwoche. */
export function isoWeek(d: Date): number {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = t.getUTCDay() || 7;
  t.setUTCDate(t.getUTCDate() + 4 - day);
  const yearStart = new Date(Date.UTC(t.getUTCFullYear(), 0, 1));
  return Math.ceil(((t.getTime() - yearStart.getTime()) / 86_400_000 + 1) / 7);
}

const fmt = (o: Intl.DateTimeFormatOptions) => new Intl.DateTimeFormat('de-DE', o);
export const fmtTime = fmt({ hour: '2-digit', minute: '2-digit' });
export const fmtWeekdayShort = fmt({ weekday: 'short' });
export const fmtDayMonth = fmt({ day: 'numeric', month: 'short' });
export const fmtMonthYear = fmt({ month: 'long', year: 'numeric' });
export const fmtLong = fmt({ weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
export const fmtMedium = fmt({ weekday: 'short', day: 'numeric', month: 'short' });

/** Für <input type="date"> / <input type="time">. */
export function toDateInput(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function toTimeInput(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function fromInputs(date: string, time: string): Date {
  const [y, m, d] = date.split('-').map(Number);
  const [h, min] = (time || '00:00').split(':').map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1, h ?? 0, min ?? 0);
}

/** Zeitspanne als Text: „14:00–15:30“, „ganztägig“, mehrtägig mit Datum. */
export function spanText(start: number, end: number, allDay: boolean): string {
  const s = fromUnix(start);
  const e = fromUnix(end);
  if (allDay) {
    const last = addDays(e, -1);
    return sameDay(s, last)
      ? 'ganztägig'
      : `${fmtDayMonth.format(s)} – ${fmtDayMonth.format(last)}`;
  }
  if (sameDay(s, e)) return `${fmtTime.format(s)}–${fmtTime.format(e)}`;
  return `${fmtMedium.format(s)} ${fmtTime.format(s)} – ${fmtMedium.format(e)} ${fmtTime.format(e)}`;
}

/**
 * Überlappende Termine eines Tages nebeneinander anordnen.
 * Liefert für jeden Termin Spalte und Spaltenanzahl seiner Gruppe.
 */
export function layoutColumns<T extends { start: number; end: number }>(
  items: T[],
): { item: T; col: number; cols: number }[] {
  const sorted = [...items].sort((a, b) => a.start - b.start || b.end - a.end);
  const out: { item: T; col: number; cols: number }[] = [];
  let group: { item: T; col: number; cols: number }[] = [];
  let groupEnd = -Infinity;
  const flush = () => {
    const cols = Math.max(1, ...group.map((g) => g.col + 1));
    for (const g of group) g.cols = cols;
    out.push(...group);
    group = [];
  };
  for (const item of sorted) {
    const end = Math.max(item.end, item.start + 15 * 60);
    if (item.start >= groupEnd) {
      flush();
      groupEnd = -Infinity;
    }
    const used = new Set(
      group.filter((g) => Math.max(g.item.end, g.item.start + 900) > item.start).map((g) => g.col),
    );
    let col = 0;
    while (used.has(col)) col++;
    group.push({ item, col, cols: 1 });
    groupEnd = Math.max(groupEnd, end);
  }
  flush();
  return out;
}
