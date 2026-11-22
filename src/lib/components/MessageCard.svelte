<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { save as saveDialog } from '@tauri-apps/plugin-dialog';
  import { api, ApiError, type MessageDetail, type MessageSummary } from '../api';
  import { describeError } from '../errors';
  import { displayName, fileSize, fullDate, listDate } from '../format';
  import { spanText, fmtLong, fromUnix } from '../calendar';
  import { t } from '../i18n';
  import { mail } from '../mail.svelte';
  import Icon from './Icon.svelte';
  import MailFrame from './MailFrame.svelte';

  /**
   * Eine Nachricht in der Leseansicht. Eingeklappt (in Gesprächen) nur eine
   * Kopfzeile mit Vorschau; der Inhalt wird erst beim Aufklappen geladen –
   * so liegt nie mehr als nötig im Speicher.
   */
  let {
    summary,
    expanded,
    collapsible,
    ontoggle,
  }: {
    summary: MessageSummary;
    expanded: boolean;
    /** In Gesprächen mit mehreren Nachrichten einklappbar. */
    collapsible: boolean;
    ontoggle: () => void;
  } = $props();

  let detail = $state<MessageDetail | null>(null);
  let error = $state<string | null>(null);
  let imagesAllowed = $state(false);
  let loadedFor = -1;

  async function load(id: number) {
    loadedFor = id;
    error = null;
    try {
      const d = await api.getMessage(id);
      if (loadedFor !== id) return;
      detail = d;
      imagesAllowed = d.senderTrusted;
    } catch (err) {
      if (loadedFor === id) error = describeError(err);
    }
  }

  $effect(() => {
    if (expanded && loadedFor !== summary.id) void load(summary.id);
  });

  const sender = $derived(displayName(summary.fromName, summary.fromAddr));
  /** Feste Farbe je Absender (aus der Adresse), damit Avatare wiedererkennbar sind. */
  const hue = $derived(
    [...summary.fromAddr.toLowerCase()].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7),
  );

  async function attachment(index: number, mode: 'quickLook' | 'open' | 'save') {
    try {
      if (mode === 'save') {
        const name = detail?.body.attachments[index]?.name ?? 'Anhang';
        const path = await saveDialog({ defaultPath: name });
        if (path) await api.saveAttachment(summary.id, index, path);
      } else {
        await api.openAttachment(summary.id, index, mode);
      }
    } catch (err) {
      mail.notice =
        err instanceof ApiError && err.kind === 'invalid' ? err.detail : describeError(err);
    }
  }

  let inviteState = $state<'idle' | 'busy' | 'accepted' | 'declined'>('idle');
  let inviteError = $state<string | null>(null);

  async function respond(accept: boolean) {
    inviteState = 'busy';
    inviteError = null;
    try {
      await api.respondInvite(summary.id, accept);
      inviteState = accept ? 'accepted' : 'declined';
    } catch (err) {
      inviteState = 'idle';
      inviteError = describeError(err);
    }
  }

  async function alwaysLoad() {
    await api.trustImageSender(summary.fromAddr).catch(() => {});
    imagesAllowed = true;
  }

  /** Text-Mails: URLs klickbar machen (ohne HTML einzuschleusen). */
  function linkify(text: string): { text: string; url?: string }[] {
    const parts: { text: string; url?: string }[] = [];
    const re = /\bhttps?:\/\/[^\s<>"')\]]+/g;
    let last = 0;
    for (const m of text.matchAll(re)) {
      if (m.index > last) parts.push({ text: text.slice(last, m.index) });
      parts.push({ text: m[0], url: m[0] });
      last = m.index + m[0].length;
    }
    if (last < text.length) parts.push({ text: text.slice(last) });
    return parts;
  }

  const recipients = (list: { name: string; addr: string }[]) =>
    list.map((a) => displayName(a.name, a.addr)).join(', ');
</script>

<article class="card" class:collapsible class:collapsed={!expanded} data-message-id={summary.id}>
  {#if collapsible && !expanded}
    <button class="head" aria-expanded="false" onclick={ontoggle}>
      <span class="avatar small-avatar" style:--hue={hue} aria-hidden="true">
        {sender.charAt(0).toUpperCase()}
      </span>
      <span class="line">
        <span class="name" class:unread={!summary.seen}>{sender}</span>
        <span class="preview">{summary.preview || summary.subject}</span>
      </span>
      <time class="small">{listDate(summary.date)}</time>
    </button>
  {:else}
    <div class="meta">
      <span class="avatar" style:--hue={hue} aria-hidden="true">
        {sender.charAt(0).toUpperCase()}
      </span>
      <div class="who">
        {#if collapsible}
          <button class="name-toggle" aria-expanded="true" onclick={ontoggle}>
            <strong>{sender}</strong>
          </button>
        {:else}
          <strong>{sender}</strong>
        {/if}
        {#if summary.fromName}<span class="addr">&lt;{summary.fromAddr}&gt;</span>{/if}
        {#if detail?.to.length}
          <div class="small">{t('reader.to')}: {recipients(detail.to)}</div>
        {/if}
        {#if detail?.cc.length}
          <div class="small">{t('reader.cc')}: {recipients(detail.cc)}</div>
        {/if}
      </div>
      <time class="small date">{fullDate(summary.date)}</time>
    </div>

    {#if detail}
      {#if detail.body.invite}
        {@const inv = detail.body.invite}
        <div class="invite" class:cancelled={inv.method === 'CANCEL'}>
          <div class="cal-icon" aria-hidden="true">
            <span class="cal-month">
              {new Intl.DateTimeFormat('de-DE', { month: 'short' }).format(fromUnix(inv.start))}
            </span>
            <span class="cal-day">{fromUnix(inv.start).getDate()}</span>
          </div>
          <div class="inv-text">
            <strong>{inv.summary || t('event.untitled')}</strong>
            <span>
              {fmtLong.format(fromUnix(inv.start))} · {spanText(inv.start, inv.end, inv.allDay)}
            </span>
            {#if inv.location}<span>{inv.location}</span>{/if}
            {#if inv.organizerEmail}
              <span class="small">
                {t('invite.from', { name: inv.organizerName || inv.organizerEmail })}
              </span>
            {/if}
            {#if inviteError}<span class="inv-error" role="alert">{inviteError}</span>{/if}
          </div>
          <div class="inv-actions">
            {#if inv.method === 'CANCEL'}
              <span class="small">{t('invite.cancelled')}</span>
            {:else if inv.method === 'REPLY'}
              <span class="small">{t('invite.reply')}</span>
            {:else if inviteState === 'accepted'}
              <span class="ok">✓ {t('invite.accepted')}</span>
            {:else if inviteState === 'declined'}
              <span class="small">{t('invite.declined')}</span>
            {:else}
              <button
                class="btn"
                disabled={inviteState === 'busy'}
                onclick={() => void respond(false)}
              >
                {t('invite.decline')}
              </button>
              <button
                class="btn primary"
                disabled={inviteState === 'busy'}
                onclick={() => void respond(true)}
              >
                {t('invite.accept')}
              </button>
            {/if}
          </div>
        </div>
      {/if}
      {#if detail.body.attachments.length}
        <ul class="attachments" aria-label={t('reader.attachments')}>
          {#each detail.body.attachments as a, i (i)}
            <li>
              <button
                class="chip"
                title="{t('reader.quickLook')} · {a.mime}"
                onclick={() => void attachment(i, 'quickLook')}
                ondblclick={() => void attachment(i, 'open')}
              >
                <Icon name="paperclip" size={13} />
                <span class="aname">{a.name}</span>
                <span class="size">{fileSize(a.size)}</span>
              </button>
              <button
                class="chip-action"
                aria-label="{t('reader.openAttachment')}: {a.name}"
                title={t('reader.openAttachment')}
                onclick={() => void attachment(i, 'open')}
              >
                <Icon name="external" size={12} />
              </button>
              <button
                class="chip-action"
                aria-label="{t('reader.saveAttachment')}: {a.name}"
                title={t('reader.saveAttachment')}
                onclick={() => void attachment(i, 'save')}
              >
                <Icon name="download" size={12} />
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      {#if detail.body.html && detail.body.remoteImages && !imagesAllowed}
        <div class="banner" role="note">
          <Icon name="image" size={15} />
          <span>{t('reader.blocked')}</span>
          <div class="banner-actions">
            <button class="btn" onclick={() => (imagesAllowed = true)}>
              {t('reader.loadImages')}
            </button>
            <button class="link" onclick={alwaysLoad}>
              {t('reader.alwaysLoad', { sender: summary.fromAddr })}
            </button>
          </div>
        </div>
      {/if}

      <div class="body">
        {#if detail.body.html}
          <MailFrame html={detail.body.html} allowRemote={imagesAllowed} />
        {:else}
          <div class="text">
            {#each linkify(detail.body.text ?? '') as part, i (i)}
              {#if part.url}
                <a
                  href={part.url}
                  title={t('reader.openLink', { url: part.url })}
                  onclick={(e) => {
                    e.preventDefault();
                    void openUrl(part.url ?? '').catch(() => {});
                  }}
                >
                  {part.text}
                </a>
              {:else}{part.text}{/if}
            {/each}
          </div>
        {/if}
      </div>
    {:else if error}
      <div class="state error" role="alert">
        {t('reader.error')} <span class="small">{error}</span>
        <button class="btn" onclick={() => void load(summary.id)}>{t('reader.retry')}</button>
      </div>
    {:else}
      <div class="state"><span class="spinner"></span>{t('reader.loading')}</div>
    {/if}
  {/if}
</article>

<style>
  .card {
    padding: var(--space-4) 0;
    border-bottom: 1px solid var(--separator);
  }

  .card:last-child {
    border-bottom: 0;
  }

  .card.collapsed {
    padding: 0;
  }

  .head {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-3) 0;
    text-align: left;
    border-radius: var(--radius);
  }

  .head:hover {
    background: var(--hover);
  }

  .line {
    display: flex;
    gap: var(--space-2);
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }

  .name {
    flex-shrink: 0;
    font-weight: 500;
  }

  .name.unread {
    font-weight: 700;
  }

  .preview {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-secondary);
  }

  .meta {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: var(--space-3);
    align-items: start;
  }

  .avatar {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 50%;
    background: hsl(var(--hue) 45% 28%);
    color: hsl(var(--hue) 80% 86%);
    font-weight: 600;
  }

  @media (prefers-color-scheme: light) {
    .avatar {
      background: hsl(var(--hue) 70% 92%);
      color: hsl(var(--hue) 50% 32%);
    }
  }

  .small-avatar {
    width: 28px;
    height: 28px;
    font-size: 12px;
  }

  .who {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .name-toggle {
    padding: 0;
    text-align: left;
  }

  .name-toggle:hover strong {
    text-decoration: underline;
  }

  .addr,
  .small {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .date {
    white-space: nowrap;
  }

  .invite {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: var(--space-3);
    align-items: center;
    margin-top: var(--space-3);
    padding: var(--space-3);
    border-radius: 10px;
    background: var(--hover);
  }

  .invite.cancelled {
    opacity: 0.7;
  }

  .cal-icon {
    display: grid;
    width: 44px;
    border-radius: 8px;
    overflow: hidden;
    text-align: center;
    background: var(--sheet-bg);
    box-shadow: 0 0 0 1px var(--separator);
  }

  .cal-month {
    background: rgb(255 59 48);
    color: white;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    line-height: 16px;
  }

  .cal-day {
    font-size: 18px;
    font-weight: 600;
    line-height: 26px;
  }

  .inv-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 1px;
  }

  .inv-text span {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .inv-error {
    color: var(--danger) !important;
  }

  .inv-actions {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  .inv-actions .btn {
    min-width: 0;
  }

  .ok {
    color: rgb(52 199 89);
    font-weight: 600;
  }

  .attachments {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: var(--space-3) 0 0;
    padding: 0;
    list-style: none;
  }

  .attachments li {
    display: inline-flex;
    align-items: center;
    max-width: 320px;
    border-radius: var(--radius);
    background: var(--hover);
    font-size: 12px;
    overflow: hidden;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
    padding: 4px 6px 4px 10px;
  }

  .chip:hover,
  .chip-action:hover {
    background: var(--active);
  }

  .chip-action {
    display: inline-flex;
    padding: 6px;
    color: var(--text-tertiary);
    opacity: 0;
    transition: opacity var(--dur) var(--ease);
  }

  .attachments li:hover .chip-action,
  .chip-action:focus-visible {
    opacity: 1;
  }

  .aname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .size {
    color: var(--text-tertiary);
  }

  .banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: 8px;
    background: var(--hover);
    color: var(--text-secondary);
    font-size: 12px;
  }

  .banner span {
    flex: 1;
    min-width: 180px;
  }

  .banner-actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .body {
    margin-top: var(--space-4);
  }

  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: 14px;
    line-height: 1.55;
  }

  .text a {
    color: var(--accent);
  }

  .state {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-4);
    color: var(--text-tertiary);
  }

  .state.error {
    color: var(--text-secondary);
    flex-wrap: wrap;
  }
</style>
