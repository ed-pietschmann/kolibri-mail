<script lang="ts">
  import { kbd, mod } from '../platform';
  import { untrack } from 'svelte';
  import {
    api,
    type CalendarInfo,
    type DavSource,
    type EventDetail,
    type Occurrence,
  } from '../api';
  import { accounts } from '../accounts.svelte';
  import {
    addDays,
    addMonths,
    fmtDayMonth,
    fmtLong,
    fmtMonthYear,
    fmtTime,
    fmtWeekdayShort,
    fromUnix,
    isoWeek,
    layoutColumns,
    range,
    sameDay,
    startOfDay,
    unix,
    type View,
  } from '../calendar';
  import { t } from '../i18n';
  import { mail } from '../mail.svelte';
  import CalendarSheet from './CalendarSheet.svelte';
  import EventSheet from './EventSheet.svelte';
  import Icon from './Icon.svelte';

  /** Kalender: Tag, Woche, Monat. Belegt die Spalten von Liste und Leseansicht. */
  const HOUR = 48;

  function readView(): View {
    try {
      const v = localStorage.getItem('kolibri.calendarView');
      return v === 'day' || v === 'month' ? v : 'week';
    } catch {
      return 'week';
    }
  }

  let view = $state<View>(readView());
  let anchor = $state(startOfDay(new Date()));
  let now = $state(new Date());
  let occurrences = $state<Occurrence[]>([]);
  let calendars = $state<CalendarInfo[]>([]);
  let menuOpen = $state(false);
  let sheetMode = $state<'add' | 'edit' | null>(null);
  let sheetCalendar = $state<CalendarInfo | null>(null);
  let sources = $state<DavSource[]>([]);
  let editing = $state<
    (Partial<EventDetail> & { start: number; end: number; allDay: boolean }) | null
  >(null);
  let grid: HTMLDivElement | undefined = $state();

  const r = $derived(range(view, anchor));
  const title = $derived.by(() => {
    if (view === 'day') return fmtLong.format(anchor);
    if (view === 'month') return fmtMonthYear.format(anchor);
    const last = addDays(r.from, 6);
    return `${fmtDayMonth.format(r.from)} – ${fmtDayMonth.format(last)} ${last.getFullYear()}`;
  });
  const subtitle = $derived(view === 'week' ? t('calendar.week', { n: isoWeek(r.from) }) : '');

  async function load() {
    try {
      const [occ, cals] = await Promise.all([
        api.occurrences(unix(r.from), unix(r.to)),
        api.listCalendars(),
      ]);
      occurrences = occ;
      calendars = cals;
      sources = await api.listDavSources().catch(() => []);
    } catch {
      occurrences = [];
    }
  }

  $effect(() => {
    void r.from.getTime();
    void r.to.getTime();
    void mail.calendarVersion;
    untrack(() => void load());
  });

  // Jetzt-Linie jede Minute aktualisieren.
  $effect(() => {
    const timer = setInterval(() => (now = new Date()), 60_000);
    return () => clearInterval(timer);
  });

  // Tag/Woche: zur Arbeitszeit (7 Uhr) bzw. zur aktuellen Uhrzeit scrollen.
  $effect(() => {
    if ((view === 'day' || view === 'week') && grid) {
      const hour = r.days.some((d) => sameDay(d, new Date()))
        ? Math.max(0, new Date().getHours() - 2)
        : 7;
      grid.scrollTop = hour * HOUR;
    }
  });

  function setView(v: View) {
    view = v;
    try {
      localStorage.setItem('kolibri.calendarView', v);
    } catch {
      // egal
    }
  }

  function step(dir: number) {
    anchor =
      view === 'month' ? addMonths(anchor, dir) : addDays(anchor, dir * (view === 'week' ? 7 : 1));
  }

  function today() {
    anchor = startOfDay(new Date());
  }

  /** Vorkommen eines Tages (ganztägige / mit Uhrzeit getrennt). */
  function forDay(day: Date) {
    const from = unix(day);
    const to = unix(addDays(day, 1));
    const list = occurrences.filter((o) => o.start < to && o.end > from);
    return {
      allDay: list.filter((o) => o.allDay || o.end - o.start >= 86_400),
      timed: list.filter((o) => !o.allDay && o.end - o.start < 86_400),
    };
  }

  async function open(o: Occurrence) {
    try {
      const d = await api.eventDetail(o.eventId);
      editing = d;
    } catch {
      // Termin inzwischen gelöscht
      void load();
    }
  }

  function create(start: Date, allDay = false) {
    const s = unix(start);
    editing = allDay
      ? { start: s, end: s + 86_400, allDay: true }
      : { start: s, end: s + 3600, allDay: false };
  }

  function createAtPointer(e: MouseEvent, day: Date) {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const minutes = Math.floor((((e.clientY - rect.top) / HOUR) * 60) / 30) * 30;
    const start = new Date(day.getFullYear(), day.getMonth(), day.getDate(), 0, minutes);
    create(start);
  }

  function top(o: Occurrence, day: Date): number {
    return Math.max(0, (o.start - unix(day)) / 3600) * HOUR;
  }

  function height(o: Occurrence, day: Date): number {
    const start = Math.max(o.start, unix(day));
    const end = Math.min(o.end, unix(addDays(day, 1)));
    return Math.max(HOUR / 2 - 2, ((end - start) / 3600) * HOUR - 2);
  }

  async function toggleCalendar(c: CalendarInfo) {
    await api.setCalendarVisible(c.id, !c.visible).catch(() => {});
    void load();
  }

  function onkeydown(e: KeyboardEvent) {
    const el = e.target as HTMLElement | null;
    if (el?.closest('input, textarea, select') || document.querySelector('dialog[open]')) return;
    if (mod(e) && ['1', '2', '3'].includes(e.key)) {
      e.preventDefault();
      setView(e.key === '1' ? 'day' : e.key === '2' ? 'week' : 'month');
    } else if (e.altKey && mod(e) && e.key.toLowerCase() === 'n') {
      e.preventDefault();
      create(
        new Date(
          anchor.getFullYear(),
          anchor.getMonth(),
          anchor.getDate(),
          new Date().getHours() + 1,
        ),
      );
    } else if (!mod(e) && e.key === 'ArrowLeft') {
      e.preventDefault();
      step(-1);
    } else if (!mod(e) && e.key === 'ArrowRight') {
      e.preventDefault();
      step(1);
    } else if (!mod(e) && e.key.toLowerCase() === 't') {
      e.preventDefault();
      today();
    }
  }

  const calName = (c: CalendarInfo) =>
    c.url === 'local' ? t('calendar.local') : c.name || t('calendar.unnamed');

  /** Gruppenname: Auf diesem Mac, Konto-Adresse oder verbundenes Konto. */
  function groupOf(c: CalendarInfo): string {
    if (c.accountId !== null)
      return accounts.list.find((a) => a.id === c.accountId)?.email ?? t('calendar.unnamed');
    if (c.sourceId !== null) return sources.find((s) => s.id === c.sourceId)?.name ?? '';
    return t('calendar.local');
  }
  const hours = Array.from({ length: 24 }, (_, i) => i);
  const weekdays = $derived(r.days.slice(0, 7));
</script>

<svelte:window {onkeydown} onclick={() => (menuOpen = false)} />

<section class="calendar" aria-label={t('calendar.label')}>
  <header class="toolbar" data-tauri-drag-region>
    <div class="heading" data-tauri-drag-region>
      <h1 data-tauri-drag-region>{title}</h1>
      {#if subtitle}<span class="sub" data-tauri-drag-region>{subtitle}</span>{/if}
    </div>
    <div class="controls">
      <div class="segmented" role="radiogroup" aria-label={t('calendar.view')}>
        {#each [['day', 'calendar.day', '⌘1'], ['week', 'calendar.weekView', '⌘2'], ['month', 'calendar.month', '⌘3']] as const as [v, key, hint] (v)}
          <button
            role="radio"
            aria-checked={view === v}
            class:on={view === v}
            title="{t(key)} ({kbd(hint)})"
            onclick={() => setView(v)}
          >
            {t(key)}
          </button>
        {/each}
      </div>
      <div class="nav">
        <button
          class="tool"
          aria-label={t('calendar.previous')}
          title="{t('calendar.previous')} (←)"
          onclick={() => step(-1)}
        >
          <Icon name="chevron" size={12} />
        </button>
        <button class="btn today" title="{t('calendar.today')} (T)" onclick={today}>
          {t('calendar.today')}
        </button>
        <button
          class="tool next"
          aria-label={t('calendar.next')}
          title="{t('calendar.next')} (→)"
          onclick={() => step(1)}
        >
          <Icon name="chevron" size={12} />
        </button>
      </div>
      <div class="menu-anchor">
        <button
          class="tool"
          aria-label={t('calendar.calendars')}
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          title={t('calendar.calendars')}
          onclick={(e) => {
            e.stopPropagation();
            menuOpen = !menuOpen;
          }}
        >
          <Icon name="calendar" />
        </button>
        {#if menuOpen}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
          <div
            class="menu"
            role="group"
            aria-label={t('calendar.calendars')}
            onclick={(e) => e.stopPropagation()}
          >
            {#each calendars as c, i (c.id)}
              {#if i === 0 || groupOf(c) !== groupOf(calendars[i - 1] ?? c)}
                <div class="menu-group">{groupOf(c)}</div>
              {/if}
              <div class="menu-row">
                <label class="menu-item">
                  <input
                    type="checkbox"
                    checked={c.visible}
                    onchange={() => void toggleCalendar(c)}
                  />
                  <span class="swatch" style:background={c.color || '#0a84ff'}></span>
                  <span class="cal-name">{calName(c)}</span>
                </label>
                <button
                  class="cal-edit"
                  aria-label={t('calcfg.editNamed', { name: calName(c) })}
                  title={t('calcfg.editTitle')}
                  onclick={() => {
                    menuOpen = false;
                    sheetCalendar = c;
                    sheetMode = 'edit';
                  }}
                >
                  <Icon name="more" size={14} />
                </button>
              </div>
            {/each}
            <div class="menu-sep" aria-hidden="true"></div>
            <button
              class="menu-item add-cal"
              onclick={() => {
                menuOpen = false;
                sheetCalendar = null;
                sheetMode = 'add';
              }}
            >
              <Icon name="plus" size={14} />
              {t('calcfg.addTitle')} …
            </button>
          </div>
        {/if}
      </div>
      <button
        class="tool"
        aria-label={t('event.new')}
        title="{t('event.new')} ({kbd('⌥⌘N')})"
        onclick={() =>
          create(
            new Date(
              anchor.getFullYear(),
              anchor.getMonth(),
              anchor.getDate(),
              Math.min(22, new Date().getHours() + 1),
            ),
          )}
      >
        <Icon name="plus" />
      </button>
    </div>
  </header>

  {#if view === 'month'}
    <div class="month">
      <div class="month-head">
        {#each weekdays as d (d.getTime())}
          <div>{fmtWeekdayShort.format(d)}</div>
        {/each}
      </div>
      <div class="month-grid">
        {#each r.days as d (d.getTime())}
          {@const items = [...forDay(d).allDay, ...forDay(d).timed]}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="cell"
            class:other={d.getMonth() !== anchor.getMonth()}
            class:weekend={d.getDay() === 0 || d.getDay() === 6}
            ondblclick={() => create(new Date(d.getFullYear(), d.getMonth(), d.getDate(), 9))}
          >
            <button
              class="date"
              class:is-today={sameDay(d, now)}
              aria-label={fmtLong.format(d)}
              onclick={() => {
                anchor = d;
                setView('day');
              }}
            >
              {d.getDate()}
            </button>
            {#each items.slice(0, 3) as o (`${o.eventId}-${o.start}`)}
              <button
                class="chip"
                class:filled={o.allDay}
                style:--c={o.color}
                onclick={() => void open(o)}
              >
                {#if !o.allDay}<span class="chip-dot"></span><span class="chip-time">
                    {fmtTime.format(fromUnix(o.start))}
                  </span>{/if}
                <span class="chip-title">{o.summary || t('event.untitled')}</span>
              </button>
            {/each}
            {#if items.length > 3}
              <button
                class="more"
                onclick={() => {
                  anchor = d;
                  setView('day');
                }}
              >
                {t('calendar.more', { n: items.length - 3 })}
              </button>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {:else}
    <div class="week" style:--days={r.days.length}>
      <div class="week-head">
        <div class="gutter"></div>
        {#each r.days as d (d.getTime())}
          <div class="day-head" class:is-today={sameDay(d, now)}>
            <span class="wd">{fmtWeekdayShort.format(d)}</span>
            <span class="num">{d.getDate()}</span>
          </div>
        {/each}
      </div>
      <div class="all-day">
        <div class="gutter label">{t('calendar.allDay')}</div>
        {#each r.days as d (d.getTime())}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="all-day-cell" ondblclick={() => create(d, true)}>
            {#each forDay(d).allDay as o (`${o.eventId}-${o.start}`)}
              <button class="chip filled" style:--c={o.color} onclick={() => void open(o)}>
                <span class="chip-title">{o.summary || t('event.untitled')}</span>
              </button>
            {/each}
          </div>
        {/each}
      </div>
      <div class="grid" bind:this={grid}>
        <div class="grid-inner" style:height="{24 * HOUR}px">
          <div class="gutter hours">
            {#each hours as h (h)}
              <span style:top="{h * HOUR}px">
                {h === 0 ? '' : `${String(h).padStart(2, '0')}:00`}
              </span>
            {/each}
          </div>
          {#each r.days as d (d.getTime())}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="column"
              class:is-today={sameDay(d, now)}
              ondblclick={(e) => createAtPointer(e, d)}
            >
              {#each layoutColumns(forDay(d).timed) as { item: o, col, cols } (`${o.eventId}-${o.start}`)}
                {@const h = height(o, d)}
                <button
                  class="event"
                  class:short={h < HOUR * 0.75}
                  style:--c={o.color}
                  style:top="{top(o, d) + 1}px"
                  style:height="{h}px"
                  style:left="calc({(col / cols) * 100}% + 2px)"
                  style:width="calc({100 / cols}% - 4px)"
                  onclick={() => void open(o)}
                  title="{o.summary} · {fmtTime.format(fromUnix(o.start))}–{fmtTime.format(
                    fromUnix(o.end),
                  )}{o.location ? ` · ${o.location}` : ''}"
                >
                  <span class="ev-title">{o.summary || t('event.untitled')}</span>
                  <span class="ev-time">
                    {fmtTime.format(fromUnix(o.start))}{o.location ? ` · ${o.location}` : ''}
                  </span>
                </button>
              {/each}
              {#if sameDay(d, now)}
                <div
                  class="now"
                  style:top="{(now.getHours() + now.getMinutes() / 60) * HOUR}px"
                  aria-hidden="true"
                ></div>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</section>

<CalendarSheet
  mode={sheetMode}
  calendar={sheetCalendar}
  {sources}
  onclose={(changed) => {
    sheetMode = null;
    if (changed) void load();
  }}
/>

<EventSheet
  draft={editing}
  {calendars}
  onclose={(changed) => {
    editing = null;
    if (changed) void load();
  }}
/>

<style>
  .calendar {
    grid-column: 2 / 4;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--surface-reader);
    border-left: 1px solid var(--separator);
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    height: var(--titlebar-h);
    padding: 0 var(--space-3) 0 var(--space-4);
    border-bottom: 1px solid var(--separator);
    flex-shrink: 0;
  }

  .heading {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  h1 {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.02em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sub {
    font-size: var(--font-size-sm);
    color: var(--text-tertiary);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .segmented {
    display: flex;
    padding: 2px;
    border-radius: 7px;
    background: var(--hover);
  }

  .segmented button {
    height: 22px;
    padding: 0 var(--space-3);
    border-radius: 5px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .segmented button.on {
    background: var(--sheet-bg);
    color: var(--text);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.2);
  }

  .nav {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .nav .tool:first-child :global(svg) {
    transform: rotate(180deg);
  }

  .today {
    min-width: 0;
    height: 24px;
    font-size: 12px;
  }

  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    color: var(--text-secondary);
  }

  .tool:hover {
    background: var(--hover);
  }

  .menu-anchor {
    position: relative;
  }

  .menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 20;
    display: grid;
    min-width: 220px;
    padding: var(--space-1);
    border-radius: 8px;
    background: var(--sheet-bg);
    box-shadow: var(--sheet-shadow);
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    padding: 0 var(--space-2);
    border-radius: 4px;
    white-space: nowrap;
  }

  .menu-item:hover {
    background: var(--hover);
  }

  .swatch {
    flex-shrink: 0;
    width: 10px;
    height: 10px;
    border-radius: 3px;
  }

  .menu-group {
    padding: var(--space-2) var(--space-2) 2px;
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .menu-row {
    display: flex;
    align-items: center;
    border-radius: 4px;
  }

  .menu-row:hover {
    background: var(--hover);
  }

  .menu-row .menu-item {
    flex: 1;
    min-width: 0;
  }

  .menu-row .menu-item:hover {
    background: transparent;
  }

  .cal-name {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .cal-edit {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    color: var(--icon);
    opacity: 0;
  }

  .menu-row:hover .cal-edit,
  .cal-edit:focus-visible {
    opacity: 1;
  }

  .cal-edit:hover {
    background: var(--active);
    color: var(--text);
  }

  .menu-sep {
    height: 1px;
    margin: var(--space-1) var(--space-2);
    background: var(--separator);
  }

  .add-cal {
    width: 100%;
    color: var(--text-secondary);
    text-align: left;
  }

  .add-cal:hover {
    color: var(--text);
  }

  /* ---- Monat ---- */

  .month {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .month-head {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    border-bottom: 1px solid var(--separator);
  }

  .month-head div {
    padding: var(--space-1) var(--space-2);
    font-size: var(--font-size-sm);
    color: var(--text-tertiary);
    text-align: right;
  }

  .month-grid {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    grid-template-rows: repeat(6, 1fr);
    min-height: 0;
  }

  .cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    min-height: 0;
    padding: 4px;
    border-right: 1px solid var(--separator);
    border-bottom: 1px solid var(--separator);
    overflow: hidden;
  }

  .cell:nth-child(7n) {
    border-right: 0;
  }

  .cell.weekend {
    background: color-mix(in srgb, var(--hover) 50%, transparent);
  }

  .cell.other .date {
    color: var(--text-tertiary);
  }

  .date {
    align-self: flex-end;
    min-width: 22px;
    height: 22px;
    padding: 0 6px;
    border-radius: 11px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .date:hover {
    background: var(--hover);
  }

  .date.is-today {
    background: rgb(255 59 48);
    color: white;
  }

  .chip {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    height: 18px;
    padding: 0 4px;
    border-radius: 4px;
    font-size: 11px;
    text-align: left;
  }

  .chip:hover {
    background: var(--hover);
  }

  .chip.filled {
    background: color-mix(in srgb, var(--c) 28%, transparent);
    border-left: 3px solid var(--c);
  }

  .chip-dot {
    width: 6px;
    height: 6px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--c);
  }

  .chip-time {
    flex-shrink: 0;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }

  .chip-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .more {
    padding: 0 4px;
    font-size: 11px;
    color: var(--text-tertiary);
    text-align: left;
  }

  /* ---- Tag / Woche ---- */

  .week {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    --gutter: 52px;
  }

  .week-head,
  .all-day {
    display: grid;
    grid-template-columns: var(--gutter) repeat(var(--days), 1fr);
    border-bottom: 1px solid var(--separator);
  }

  .day-head {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 6px;
    padding: var(--space-2) 0;
  }

  .day-head .wd {
    font-size: var(--font-size-sm);
    color: var(--text-secondary);
  }

  .day-head .num {
    font-size: var(--font-size-lg);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .day-head.is-today .num {
    display: inline-grid;
    place-items: center;
    min-width: 26px;
    height: 26px;
    padding: 0 4px;
    border-radius: 13px;
    background: rgb(255 59 48);
    color: white;
  }

  .gutter.label {
    padding: 4px 6px;
    font-size: 10px;
    color: var(--text-tertiary);
    text-align: right;
  }

  .all-day-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    min-height: 26px;
    padding: 3px 2px;
    border-left: 1px solid var(--separator);
  }

  .grid {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }

  .grid-inner {
    position: relative;
    display: grid;
    grid-template-columns: var(--gutter) repeat(var(--days), 1fr);
    background-image: repeating-linear-gradient(
      to bottom,
      var(--separator) 0,
      var(--separator) 1px,
      transparent 1px,
      transparent 48px
    );
  }

  .hours {
    position: relative;
  }

  .hours span {
    position: absolute;
    right: 8px;
    transform: translateY(-50%);
    font-size: 10px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }

  .column {
    position: relative;
    border-left: 1px solid var(--separator);
  }

  .column.is-today {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }

  .event {
    position: absolute;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 3px 6px;
    border-radius: 5px;
    border-left: 3px solid var(--c);
    background: color-mix(in srgb, var(--c) 24%, var(--surface-reader));
    font-size: 11.5px;
    line-height: 1.25;
    text-align: left;
    overflow: hidden;
  }

  .event:hover {
    background: color-mix(in srgb, var(--c) 34%, var(--surface-reader));
  }

  .event.short {
    flex-direction: row;
    gap: 6px;
    padding-top: 1px;
  }

  .ev-title {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ev-time {
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .now {
    position: absolute;
    left: -1px;
    right: 0;
    height: 2px;
    background: rgb(255 59 48);
    pointer-events: none;
  }

  .now::before {
    content: '';
    position: absolute;
    left: -4px;
    top: -3px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: rgb(255 59 48);
  }
</style>
