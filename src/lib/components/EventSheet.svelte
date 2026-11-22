<script lang="ts">
  import { untrack } from 'svelte';
  import { api, ApiError, type AttendeeInput, type CalendarInfo, type EventDetail } from '../api';
  import { addDays, fromInputs, fromUnix, toDateInput, toTimeInput, unix } from '../calendar';
  import { describeError } from '../errors';
  import { t } from '../i18n';
  import AddressInput from './AddressInput.svelte';
  import Icon, { type IconName } from './Icon.svelte';
  import Sheet from './Sheet.svelte';

  /**
   * Termin anlegen oder bearbeiten. `draft` ist entweder ein vorhandener
   * Termin (mit id) oder ein Vorschlag für einen neuen (Zeitraum aus dem Raster).
   */
  let {
    draft,
    calendars,
    onclose,
  }: {
    draft: (Partial<EventDetail> & { start: number; end: number; allDay: boolean }) | null;
    calendars: CalendarInfo[];
    onclose: (changed: boolean) => void;
  } = $props();

  let summary = $state('');
  let location = $state('');
  let description = $state('');
  let allDay = $state(false);
  let startDate = $state('');
  let startTime = $state('');
  let endDate = $state('');
  let endTime = $state('');
  let calendarId = $state(0);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let confirmDelete = $state(false);
  /** Eingeladene als Text („Name <a@b>, c@d“), wie im Verfassen-Fenster. */
  let invitees = $state('');

  const existing = $derived(draft?.id != null);
  const readOnly = $derived(draft?.recurring === true);
  const lastCalendar = (): number => {
    try {
      return Number(localStorage.getItem('kolibri.calendar')) || 0;
    } catch {
      return 0;
    }
  };

  /**
   * Felder nur füllen, wenn ein anderer Termin geöffnet wird. Alles andere
   * (z. B. die Kalenderliste, die bei jedem Abgleich neu geladen wird) darf das
   * nicht auslösen – sonst wären die Eingaben plötzlich weg.
   */
  $effect(() => {
    const d = draft;
    if (!d) return;
    untrack(() => fill(d));
  });

  function fill(d: NonNullable<typeof draft>) {
    const draft = d;
    summary = draft.summary ?? '';
    location = draft.location ?? '';
    description = draft.description ?? '';
    allDay = draft.allDay;
    const s = fromUnix(draft.start);
    // Ganztägig: Ende ist exklusiv (nächster Tag) – angezeigt wird der letzte Tag.
    const e = draft.allDay ? addDays(fromUnix(draft.end), -1) : fromUnix(draft.end);
    startDate = toDateInput(s);
    startTime = toTimeInput(s);
    endDate = toDateInput(e);
    endTime = toTimeInput(e);
    const preferred = draft.calendarId ?? lastCalendar();
    calendarId = calendars.some((c) => c.id === preferred) ? preferred : (calendars[0]?.id ?? 0);
    invitees = (draft.attendees ?? [])
      .map((p) => (p.name ? `${p.name.replace(/[",<>]/g, '')} <${p.email}>` : p.email))
      .join(', ');
    if (invitees) invitees += ', ';
    error = null;
    confirmDelete = false;
  }

  const invitedByOther = $derived(draft?.invitedByOther === true);

  /** „Anna <a@b>, c@d“ → Liste. */
  function parseInvitees(text: string): AttendeeInput[] {
    return text
      .split(/[,;]/)
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => {
        const m = /^(.*)<([^>]+)>$/.exec(part);
        return m
          ? { name: (m[1] ?? '').trim().replace(/^"|"$/g, ''), email: (m[2] ?? '').trim() }
          : { name: '', email: part };
      });
  }

  const inviteCount = $derived(parseInvitees(invitees).length);

  const statusInfo: Record<
    string,
    { icon: IconName; key: 'accepted' | 'declined' | 'tentative' | 'open' }
  > = {
    ACCEPTED: { icon: 'checkCircle', key: 'accepted' },
    DECLINED: { icon: 'xCircle', key: 'declined' },
    TENTATIVE: { icon: 'helpCircle', key: 'tentative' },
  };
  const statusOf = (s: string) =>
    statusInfo[s.toUpperCase()] ?? { icon: 'clock' as IconName, key: 'open' as const };

  function computeRange(): { start: number; end: number } {
    if (allDay) {
      const s = fromInputs(startDate, '00:00');
      const e = addDays(fromInputs(endDate || startDate, '00:00'), 1);
      return { start: unix(s), end: unix(e) };
    }
    return {
      start: unix(fromInputs(startDate, startTime)),
      end: unix(fromInputs(endDate || startDate, endTime)),
    };
  }

  /** Beginn verschoben → Ende mitziehen (gleiche Dauer), wie in Apple Kalender. */
  function onStartChange(date: string, time: string) {
    if (!draft) return;
    const before = unix(fromInputs(startDate, startTime));
    const after = unix(fromInputs(date, time));
    const end = unix(fromInputs(endDate, endTime)) + (after - before);
    startDate = date;
    startTime = time;
    endDate = toDateInput(fromUnix(end));
    endTime = toTimeInput(fromUnix(end));
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (!draft || busy || readOnly) return;
    busy = true;
    error = null;
    try {
      const { start, end } = computeRange();
      await api.saveEvent({
        id: draft.id ?? null,
        calendarId,
        summary,
        location,
        description,
        allDay,
        start,
        end,
        attendees: invitedByOther ? [] : parseInvitees(invitees),
      });
      try {
        localStorage.setItem('kolibri.calendar', String(calendarId));
      } catch {
        // egal
      }
      onclose(true);
    } catch (err) {
      error = err instanceof ApiError && err.kind === 'invalid' ? err.detail : describeError(err);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!draft?.id) return;
    busy = true;
    try {
      await api.deleteEvent(draft.id);
      onclose(true);
    } catch (err) {
      error = describeError(err);
    } finally {
      busy = false;
    }
  }

  const calendarName = (c: CalendarInfo) =>
    c.url === 'local' ? t('calendar.local') : c.name || t('calendar.unnamed');
  const selectedColor = $derived(calendars.find((c) => c.id === calendarId)?.color || '#0a84ff');
</script>

<Sheet
  open={draft !== null}
  label={existing ? t('event.edit') : t('event.new')}
  width={440}
  onclose={() => onclose(false)}
>
  <form onsubmit={save}>
    <div class="title-row">
      <span class="dot" style:background={selectedColor} aria-hidden="true"></span>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="title"
        type="text"
        bind:value={summary}
        placeholder={t('event.titlePlaceholder')}
        aria-label={t('event.title')}
        autofocus={!existing}
        disabled={readOnly}
      />
    </div>
    <label class="field">
      <span>{t('event.location')}</span>
      <input type="text" bind:value={location} disabled={readOnly} />
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={allDay} disabled={readOnly} />
      {t('event.allDay')}
    </label>

    <div class="times">
      <div class="time-row">
        <span class="label">{t('event.start')}</span>
        <input
          type="date"
          value={startDate}
          onchange={(e) => onStartChange(e.currentTarget.value, startTime)}
          aria-label="{t('event.start')} {t('event.date')}"
          disabled={readOnly}
        />
        {#if !allDay}
          <input
            type="time"
            value={startTime}
            onchange={(e) => onStartChange(startDate, e.currentTarget.value)}
            aria-label="{t('event.start')} {t('event.time')}"
            disabled={readOnly}
          />
        {/if}
      </div>
      <div class="time-row">
        <span class="label">{t('event.end')}</span>
        <input
          type="date"
          bind:value={endDate}
          aria-label="{t('event.end')} {t('event.date')}"
          disabled={readOnly}
        />
        {#if !allDay}
          <input
            type="time"
            bind:value={endTime}
            aria-label="{t('event.end')} {t('event.time')}"
            disabled={readOnly}
          />
        {/if}
      </div>
    </div>

    <label class="field">
      <span>{t('event.calendar')}</span>
      <select bind:value={calendarId} disabled={readOnly}>
        {#each calendars as c (c.id)}
          <option value={c.id}>{calendarName(c)}</option>
        {/each}
      </select>
    </label>

    <label class="field">
      <span>{t('event.notes')}</span>
      <textarea bind:value={description} rows="3" disabled={readOnly}></textarea>
    </label>

    <div class="field">
      <span>{t('event.invite')}</span>
      {#if invitedByOther || readOnly}
        {#if draft?.organizer}
          <p class="small">
            {t('event.organizer')}: {draft.organizer.name || draft.organizer.email}
          </p>
        {/if}
      {:else}
        <div class="invite-field">
          <Icon name="userPlus" size={14} />
          <AddressInput bind:value={invitees} label={t('event.invite')} />
        </div>
        <p class="small">
          {inviteCount
            ? t(existing ? 'event.inviteHintUpdate' : 'event.inviteHint', { n: inviteCount })
            : t('event.inviteEmpty')}
        </p>
      {/if}
      {#if draft?.attendees?.length}
        <ul class="people">
          {#each draft.attendees as p (p.email)}
            {@const st = statusOf(p.status)}
            <li class="person {st.key}">
              <span class="who">
                <span class="pname">{p.name || p.email}</span>
                {#if p.name}<span class="mail">{p.email}</span>{/if}
              </span>
              <span class="status">
                <Icon name={st.icon} size={13} />
                {t(`event.status.${st.key}`)}
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    {#if readOnly}<p class="small">{t('event.recurringHint')}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="buttons">
      {#if existing}
        {#if confirmDelete}
          <button type="button" class="btn danger" disabled={busy} onclick={remove}>
            {readOnly ? t('event.deleteSeriesConfirm') : t('event.deleteConfirm')}
          </button>
        {:else}
          <button type="button" class="btn left" onclick={() => (confirmDelete = true)}>
            {t('event.delete')}
          </button>
        {/if}
      {/if}
      <span class="spacer"></span>
      <button type="button" class="btn" onclick={() => onclose(false)}>{t('add.cancel')}</button>
      {#if !readOnly}
        <button type="submit" class="btn primary" disabled={busy || !summary.trim()}>
          {inviteCount && !invitedByOther
            ? existing
              ? t('event.saveAndUpdate')
              : t('event.saveAndInvite')
            : t('event.save')}
        </button>
      {/if}
    </div>
  </form>
</Sheet>

<style>
  form {
    display: grid;
    gap: var(--space-3);
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .dot {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    border-radius: 50%;
  }

  .title {
    height: 34px;
    font-size: var(--font-size-lg);
    font-weight: 600;
  }

  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .times {
    display: grid;
    gap: var(--space-2);
  }

  .time-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .time-row .label {
    width: 52px;
    color: var(--text-secondary);
    font-size: var(--font-size-sm);
  }

  .time-row input {
    height: 28px;
    padding: 0 var(--space-2);
    border: 1px solid var(--field-border);
    border-radius: var(--radius);
    background: var(--field-bg);
    color: var(--text);
    font: inherit;
  }

  .time-row input[type='date'] {
    flex: 1;
  }

  textarea {
    width: 100%;
    padding: var(--space-2);
    border: 1px solid var(--field-border);
    border-radius: var(--radius);
    background: var(--field-bg);
    color: var(--text);
    font: inherit;
    resize: vertical;
  }

  .small {
    margin: 0;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .error {
    margin: 0;
    color: var(--danger);
  }

  .field > span:first-child {
    display: block;
  }

  .invite-field {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 30px;
    padding: 0 var(--space-2);
    border: 1px solid var(--field-border);
    border-radius: var(--radius);
    background: var(--field-bg);
    color: var(--icon);
  }

  .invite-field:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .invite-field :global(input) {
    border: 0;
    background: transparent;
    box-shadow: none;
    padding: 0;
    height: 28px;
  }

  .people {
    display: grid;
    gap: 2px;
    margin: var(--space-1) 0 0;
    padding: 0;
    list-style: none;
  }

  .person {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 30px;
    padding: 2px var(--space-2);
    border-radius: 7px;
    background: var(--hover);
  }

  .who {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }

  .pname,
  .mail {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mail {
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .person.accepted .status {
    color: var(--success);
  }

  .person.declined .status {
    color: var(--danger);
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  .spacer {
    flex: 1;
  }
</style>
