<script lang="ts">
  import { untrack } from 'svelte';
  import { api, type Occurrence } from '../api';
  import { accounts } from '../accounts.svelte';
  import {
    addDays,
    fmtLong,
    fmtMedium,
    fromUnix,
    sameDay,
    spanText,
    startOfDay,
    unix,
  } from '../calendar';
  import { t } from '../i18n';
  import { mail } from '../mail.svelte';
  import Icon from './Icon.svelte';

  /** Heute: Termine heute + nächste 7 Tage, ungelesene Mails. */
  let { ongo }: { ongo: (target: string) => void } = $props();

  let events = $state<Occurrence[]>([]);
  const today = startOfDay(new Date());

  async function load() {
    try {
      const list = await api.occurrences(unix(today), unix(addDays(today, 8)));
      events = list.sort((a, b) => a.start - b.start);
    } catch {
      events = [];
    }
  }

  $effect(() => {
    void mail.calendarVersion;
    untrack(() => void load());
  });

  const todays = $derived(events.filter((e) => e.start < unix(addDays(today, 1))));
  const upcoming = $derived(events.filter((e) => e.start >= unix(addDays(today, 1))));
  const unread = $derived(
    accounts.list.reduce((sum, a) => {
      const inbox = a.folders.find((f) => f.role === 'inbox');
      return sum + (inbox ? (mail.unread[inbox.id] ?? 0) : 0);
    }, 0),
  );

  const greeting = (() => {
    const h = new Date().getHours();
    return h < 11 ? t('today.morning') : h < 17 ? t('today.day') : t('today.evening');
  })();
</script>

<section class="today" aria-label={t('sidebar.today')}>
  <header class="toolbar" data-tauri-drag-region></header>
  <div class="content">
    <p class="greeting">{greeting}</p>
    <h1>{fmtLong.format(today)}</h1>

    <div class="cards">
      <button class="card mail" onclick={() => ongo('unified:inbox')}>
        <Icon name="inbox" size={20} />
        <span class="big">{unread}</span>
        <span>{unread === 1 ? t('today.unreadOne') : t('today.unread')}</span>
      </button>
      <button class="card" onclick={() => ongo('view:calendar')}>
        <Icon name="calendar" size={20} />
        <span class="big">{todays.length}</span>
        <span>{todays.length === 1 ? t('today.eventOne') : t('today.events')}</span>
      </button>
    </div>

    <h2>{t('today.todayHeading')}</h2>
    {#if todays.length === 0}
      <p class="empty">{t('today.free')}</p>
    {:else}
      <ul>
        {#each todays as e (`${e.eventId}-${e.start}`)}
          <li class:past={e.end * 1000 < Date.now()}>
            <span class="bar" style:background={e.color}></span>
            <span class="when">{spanText(e.start, e.end, e.allDay)}</span>
            <span class="what">
              <strong>{e.summary || t('event.untitled')}</strong>
              {#if e.location}<span class="where">{e.location}</span>{/if}
            </span>
          </li>
        {/each}
      </ul>
    {/if}

    {#if upcoming.length}
      <h2>{t('today.upcoming')}</h2>
      <ul>
        {#each upcoming as e, i (`${e.eventId}-${e.start}`)}
          {#if i === 0 || !sameDay(fromUnix(e.start), fromUnix(upcoming[i - 1]?.start ?? 0))}
            <li class="day">{fmtMedium.format(fromUnix(e.start))}</li>
          {/if}
          <li>
            <span class="bar" style:background={e.color}></span>
            <span class="when">{spanText(e.start, e.end, e.allDay)}</span>
            <span class="what">
              <strong>{e.summary || t('event.untitled')}</strong>
              {#if e.location}<span class="where">{e.location}</span>{/if}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</section>

<style>
  .today {
    grid-column: 2 / 4;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--surface-reader);
    border-left: 1px solid var(--separator);
  }

  .toolbar {
    height: var(--titlebar-h);
    flex-shrink: 0;
  }

  .content {
    flex: 1;
    overflow-y: auto;
    width: 100%;
    max-width: 640px;
    margin: 0 auto;
    padding: var(--space-2) var(--space-6) var(--space-6);
  }

  .greeting {
    margin: 0;
    color: var(--text-secondary);
  }

  h1 {
    margin: 2px 0 var(--space-6);
    font-size: 26px;
    font-weight: 700;
  }

  h2 {
    margin: var(--space-6) 0 var(--space-2);
    font-size: var(--font-size-sm);
    font-weight: 600;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .cards {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }

  .card {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: auto auto;
    column-gap: var(--space-3);
    align-items: center;
    padding: var(--space-4);
    border-radius: 12px;
    background: var(--hover);
    text-align: left;
    color: var(--text-secondary);
    transition: background-color var(--dur) var(--ease);
  }

  .card:hover {
    background: var(--active);
  }

  .card :global(svg) {
    grid-row: 1 / 3;
    color: var(--accent);
  }

  .big {
    font-size: 24px;
    font-weight: 700;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 4px 120px 1fr;
    gap: var(--space-3);
    align-items: start;
    padding: var(--space-2) 0;
  }

  li.past {
    opacity: 0.5;
  }

  li.day {
    display: block;
    padding: var(--space-3) 0 var(--space-1);
    font-weight: 600;
  }

  .bar {
    align-self: stretch;
    border-radius: 2px;
  }

  .when {
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .where {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .empty {
    color: var(--text-tertiary);
  }
</style>
