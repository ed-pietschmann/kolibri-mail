<script lang="ts">
  import { kbd, mod } from './lib/platform';
  import { onMount } from 'svelte';
  import {
    api,
    ApiError,
    type Account,
    type ComposeKind,
    type MessageSummary,
    type MessageScope,
    type UserAction,
  } from './lib/api';
  import { accounts } from './lib/accounts.svelte';
  import { describeError } from './lib/errors';
  import { mail } from './lib/mail.svelte';
  import { folderIcon, folderLabel } from './lib/folders';
  import { t, type MessageKey } from './lib/i18n';
  import AddAccountSheet from './lib/components/AddAccountSheet.svelte';
  import CommandPalette, { type Command } from './lib/components/CommandPalette.svelte';
  import Composer from './lib/components/Composer.svelte';
  import ContactsView from './lib/components/ContactsView.svelte';
  import CalendarView from './lib/components/CalendarView.svelte';
  import TodayView from './lib/components/TodayView.svelte';
  import SignatureSheet from './lib/components/SignatureSheet.svelte';
  import TemplatesSheet from './lib/components/TemplatesSheet.svelte';
  import ConfirmSheet from './lib/components/ConfirmSheet.svelte';
  import MessageList from './lib/components/MessageList.svelte';
  import ReaderPane from './lib/components/ReaderPane.svelte';
  import Sidebar from './lib/components/Sidebar.svelte';

  const fixedTitles: Record<string, MessageKey> = {
    'unified:inbox': 'sidebar.allInboxes',
    'unified:flagged': 'sidebar.flagged',
    'unified:drafts': 'sidebar.drafts',
    'unified:sent': 'sidebar.sent',
    'view:today': 'sidebar.today',
    'view:calendar': 'sidebar.calendar',
    'view:contacts': 'sidebar.contacts',
  };

  let selected = $state('unified:inbox');
  let paletteOpen = $state(false);

  /** Alles, was die Befehlspalette (⌘K) anbietet. */
  const commands = $derived.by((): Command[] => {
    const list: Command[] = [
      {
        id: 'new',
        label: t('palette.newMail'),
        icon: 'compose',
        hint: kbd('⌘N'),
        run: () => startCompose({ type: 'new', accountId: null }),
      },
      {
        id: 'sync',
        label: t('palette.sync'),
        icon: 'refresh',
        hint: kbd('⇧⌘N'),
        run: () => void mail.syncAll(),
      },
      {
        id: 'threads',
        label: t('palette.threads'),
        icon: 'thread',
        run: () => mail.setThreaded(!mail.threaded),
      },
    ];
    if (current) {
      const r = reader;
      list.push(
        {
          id: 'reply',
          label: t('palette.reply'),
          icon: 'reply',
          hint: kbd('⌘R'),
          run: () => r?.compose('reply'),
        },
        {
          id: 'replyAll',
          label: t('palette.replyAll'),
          icon: 'replyAll',
          hint: kbd('⇧⌘R'),
          run: () => r?.compose('replyAll'),
        },
        {
          id: 'forward',
          label: t('palette.forward'),
          icon: 'forward',
          hint: kbd('⇧⌘F'),
          run: () => r?.compose('forward'),
        },
        {
          id: 'archive',
          label: t('palette.archive'),
          icon: 'archive',
          hint: 'E',
          run: () => r?.run({ type: 'archive' }),
        },
        {
          id: 'trash',
          label: t('palette.trash'),
          icon: 'trash',
          hint: '⌫',
          run: () => r?.run({ type: 'trash' }),
        },
        {
          id: 'flag',
          label: t('palette.flag'),
          icon: 'flag',
          hint: 'S',
          run: () => r?.toggleFlag(),
        },
        {
          id: 'seen',
          label: t('palette.seen'),
          icon: 'mail',
          hint: 'U',
          run: () => r?.toggleSeen(),
        },
        {
          id: 'move',
          label: t('palette.move'),
          icon: 'folder',
          hint: 'V',
          run: () => r?.openMoveMenu(),
        },
      );
    }
    for (const [id, key] of Object.entries(fixedTitles)) {
      if (!id.startsWith('unified:')) continue;
      list.push({
        id: `go-${id}`,
        label: t('palette.goto', { folder: t(key) }),
        icon:
          id === 'unified:inbox'
            ? 'inbox'
            : id === 'unified:flagged'
              ? 'flag'
              : id === 'unified:drafts'
                ? 'draft'
                : 'sent',
        run: () => go(id),
      });
    }
    for (const a of accounts.list) {
      for (const f of a.folders.filter((f) => f.selectable)) {
        list.push({
          id: `go-folder-${f.id}`,
          label: t('palette.goto', { folder: folderLabel(f) }),
          icon: folderIcon(f),
          keywords: `${a.email} ${f.path}`,
          run: () => go(`folder:${f.id}`),
        });
      }
      list.push({
        id: `sig-${a.id}`,
        label: t('palette.signature', { email: a.email }),
        icon: 'signature',
        run: () => (signatureFor = a),
      });
    }
    list.push({
      id: 'templates',
      label: t('palette.templates'),
      icon: 'template',
      run: () => (templatesOpen = true),
    });
    list.push({
      id: 'add',
      label: t('palette.addAccount'),
      icon: 'plus',
      run: () => (addOpen = true),
    });
    return list;
  });

  function go(id: string) {
    mail.closeSearch();
    selected = id;
  }
  let selectedMessage = $state<number | null>(null);
  /** Die gerade angezeigte Mail (für Tastenkürzel und Rückfragen). */
  let current = $state<MessageSummary | null>(null);
  let list: MessageList | undefined = $state();
  let reader: ReaderPane | undefined = $state();
  /** Offenes Verfassen (null = Leseansicht). */
  let composing = $state<{ id: number; kind: ComposeKind; to: string } | null>(null);
  let composer: Composer | undefined = $state();
  let signatureFor = $state<Account | null>(null);
  let templatesOpen = $state(false);

  function startCompose(kind: ComposeKind, to = '') {
    if (accounts.list.length === 0) {
      addOpen = true;
      return;
    }
    // Schon beim Schreiben? Das schwebende Fenster wieder aufklappen und fokussieren.
    if (composing) {
      composer?.reveal();
      return;
    }
    composing = { id: Date.now(), kind, to };
  }

  // Kurze Bestätigungen ausblenden.
  $effect(() => {
    if (!mail.toast) return;
    const timer = setTimeout(() => (mail.toast = null), 3000);
    return () => clearTimeout(timer);
  });

  /** Wartet auf Bestätigung „Endgültig löschen“. */
  let confirmDelete = $state<{ message: MessageSummary; ids: number[] } | null>(null);

  /** Sidebar-Auswahl → welche Nachrichten die Liste zeigt (null = keine Mail-Ansicht). */
  const scope = $derived.by((): MessageScope | null => {
    const [kind, value] = selected.split(':');
    if (kind === 'folder') return { kind: 'folder', id: Number(value) };
    if (selected === 'unified:flagged') return { kind: 'flagged' };
    if (selected === 'unified:inbox') return { kind: 'role', role: 'inbox' };
    if (selected === 'unified:drafts') return { kind: 'role', role: 'drafts' };
    if (selected === 'unified:sent') return { kind: 'role', role: 'sent' };
    return null;
  });
  let addOpen = $state(false);
  let removing = $state<Account | null>(null);
  let removeError = $state<string | null>(null);

  const title = $derived.by(() => {
    const fixed = fixedTitles[selected];
    if (fixed) return t(fixed);
    const id = Number(selected.split(':')[1]);
    for (const a of accounts.list) {
      const folder = a.folders.find((f) => f.id === id);
      if (folder) return folderLabel(folder);
    }
    return t('sidebar.allInboxes');
  });

  onMount(() => {
    // Erst sofort aus dem Cache, dann im Hintergrund vom Server abgleichen.
    void mail
      .start()
      .then(() => accounts.load())
      .then(() => mail.syncAll());
    void mail.loadUnread();
  });

  function folderRole(m: { accountId: number; folderId: number }) {
    return accounts.list.find((a) => a.id === m.accountId)?.folders.find((f) => f.id === m.folderId)
      ?.role;
  }

  /** Führt eine Aktion aus; bei Archiv/Löschen/Verschieben geht die Auswahl weiter. */
  async function perform(action: UserAction, message: MessageSummary, ids: number[]) {
    if (action.type === 'trash' && folderRole(message) === 'trash') {
      confirmDelete = { message, ids };
      return;
    }
    const leaves = ['archive', 'trash', 'moveTo', 'deleteForever'].includes(action.type);
    const next =
      leaves && selectedMessage !== null
        ? (list?.neighborOf(selectedMessage) ?? null)
        : selectedMessage;
    try {
      await api.applyAction(ids, action);
      if (leaves) selectedMessage = next;
    } catch (err) {
      // Bei Planungsfehlern hat das Backend schon eine passende deutsche Meldung.
      mail.notice =
        err instanceof ApiError && (err.kind === 'invalid' || err.kind === 'notFound')
          ? err.detail
          : describeError(err);
    }
  }

  /** Beim Öffnen als gelesen markieren (wie Apple Mail). */
  function onLoaded(message: MessageSummary | null, unread: number[]) {
    current = message;
    if (unread.length) {
      void api.applyAction(unread, { type: 'markSeen', value: true }).catch(() => {});
    }
  }

  /** Tastenkürzel gelten nicht, während man tippt oder ein Dialog offen ist. */
  /** Tastendruck im schwebenden Verfassen-Fenster (dort gelten die Mail-Kürzel nicht). */
  function inComposer(e: KeyboardEvent): boolean {
    return !!(e.target as HTMLElement | null)?.closest?.('.composer');
  }

  function isTyping(e: KeyboardEvent): boolean {
    const el = e.target as HTMLElement | null;
    return (
      !!el?.closest('input, textarea, select, [contenteditable="true"]') ||
      !!document.querySelector('dialog[open]')
    );
  }

  function onkeydown(e: KeyboardEvent) {
    const key = e.key.toLowerCase();
    // Wie Apple Mail: ⇧⌘N Postfächer abrufen, ⌘N neue E-Mail.
    if (mod(e) && e.shiftKey && key === 'n') {
      e.preventDefault();
      void mail.syncAll();
      return;
    }
    if (mod(e) && !e.shiftKey && key === 'n') {
      e.preventDefault();
      startCompose({ type: 'new', accountId: null });
      return;
    }
    if (mod(e) && !e.shiftKey && key === 'k' && !inComposer(e)) {
      e.preventDefault();
      paletteOpen = !paletteOpen;
      return;
    }
    if (mod(e) && !e.shiftKey && key === 'f' && !inComposer(e)) {
      e.preventDefault();
      mail.openSearch();
      return;
    }
    if (key === '/' && !mod(e) && !isTyping(e) && !inComposer(e)) {
      e.preventDefault();
      mail.openSearch();
      return;
    }
    if (isTyping(e) || e.altKey || e.ctrlKey || inComposer(e) || selected.startsWith('view:'))
      return;

    // Navigation
    if (!mod(e) && (key === 'j' || key === 'arrowdown')) {
      e.preventDefault();
      list?.step(1);
      return;
    }
    if (!mod(e) && (key === 'k' || key === 'arrowup')) {
      e.preventDefault();
      list?.step(-1);
      return;
    }

    if (!current) return;
    const plain = !mod(e) && !e.shiftKey;
    if (plain && key === 'e') reader?.run({ type: 'archive' });
    else if ((plain || (mod(e) && !e.shiftKey)) && (key === 'backspace' || key === 'delete'))
      reader?.run({ type: 'trash' });
    else if ((plain && key === 'u') || (mod(e) && e.shiftKey && key === 'u')) reader?.toggleSeen();
    else if ((plain && key === 's') || (mod(e) && e.shiftKey && key === 'l')) reader?.toggleFlag();
    else if (plain && key === 'v') reader?.openMoveMenu();
    else if ((plain && key === 'r') || (mod(e) && !e.shiftKey && key === 'r'))
      reader?.compose(folderRole(current) === 'drafts' ? 'editDraft' : 'reply');
    else if ((plain && key === 'a') || (mod(e) && e.shiftKey && key === 'r'))
      reader?.compose('replyAll');
    else if ((plain && key === 'f') || (mod(e) && e.shiftKey && key === 'f'))
      reader?.compose('forward');
    else return;
    e.preventDefault();
  }

  function onAdded(account: Account) {
    addOpen = false;
    void api.syncAccount(account.id).catch(() => {});
    const inbox = account.folders.find((f) => f.role === 'inbox');
    if (inbox) selected = `folder:${inbox.id}`;
  }

  async function confirmRemove() {
    if (!removing) return;
    const account = removing;
    try {
      await accounts.remove(account.id);
      if (account.folders.some((f) => selected === `folder:${f.id}`)) selected = 'unified:inbox';
      removing = null;
      removeError = null;
    } catch (err) {
      removeError = describeError(err);
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="app">
  <Sidebar
    bind:selected
    oncompose={() => startCompose({ type: 'new', accountId: null })}
    onaddaccount={() => (addOpen = true)}
    onremoveaccount={(a) => (removing = a)}
    onsignature={(a) => (signatureFor = a)}
    ontemplates={() => (templatesOpen = true)}
  />
  {#if selected === 'view:calendar'}
    <CalendarView />
  {:else if selected === 'view:today'}
    <TodayView ongo={(target) => (selected = target)} />
  {:else if selected === 'view:contacts'}
    <ContactsView
      onwrite={(address) => {
        selected = 'unified:inbox';
        startCompose({ type: 'new', accountId: null }, address);
      }}
    />
  {:else}
    <MessageList
      bind:this={list}
      {title}
      {scope}
      bind:selectedId={selectedMessage}
      hasAccounts={!accounts.loaded || accounts.list.length > 0}
      onaddaccount={() => (addOpen = true)}
    />
    <ReaderPane
      bind:this={reader}
      messageId={selectedMessage}
      {scope}
      threaded={mail.threaded && !mail.searchActive}
      isDraft={current !== null && folderRole(current) === 'drafts'}
      oncompose={startCompose}
      onaction={(action, message, ids) => void perform(action, message, ids)}
      onloaded={onLoaded}
    />
  {/if}

  <!-- Schwebendes Verfassen-Fenster: unten rechts über Leseansicht/Kalender. -->
  {#if composing}
    {#key composing.id}
      <Composer
        bind:this={composer}
        kind={composing.kind}
        to={composing.to}
        onclose={() => (composing = null)}
        ontemplates={() => (templatesOpen = true)}
      />
    {/key}
  {/if}
</div>

{#if mail.toast}
  <div class="toast" role="status">{mail.toast}</div>
{/if}

<SignatureSheet account={signatureFor} onclose={() => (signatureFor = null)} />
<TemplatesSheet open={templatesOpen} onclose={() => (templatesOpen = false)} />

<CommandPalette
  open={paletteOpen}
  {commands}
  onsearch={(text) => mail.openSearch(text)}
  onclose={() => (paletteOpen = false)}
/>

<AddAccountSheet open={addOpen} onclose={() => (addOpen = false)} onadded={onAdded} />

<ConfirmSheet
  open={confirmDelete !== null}
  title={t('deleteForever.title')}
  body={t('deleteForever.body')}
  confirmLabel={t('reader.deleteForever')}
  destructive
  onconfirm={() => {
    const c = confirmDelete;
    confirmDelete = null;
    if (c) void perform({ type: 'deleteForever' }, c.message, c.ids);
  }}
  oncancel={() => (confirmDelete = null)}
/>

<ConfirmSheet
  open={removing !== null}
  title={t('remove.title', { email: removing?.email ?? '' })}
  body={t('remove.body')}
  confirmLabel={t('remove.confirm')}
  destructive
  error={removeError}
  onconfirm={confirmRemove}
  oncancel={() => {
    removing = null;
    removeError = null;
  }}
/>

<style>
  .toast {
    position: fixed;
    left: 50%;
    bottom: var(--space-6);
    z-index: 50;
    transform: translateX(-50%);
    padding: var(--space-2) var(--space-4);
    border-radius: 999px;
    background: var(--sheet-bg);
    box-shadow: var(--sheet-shadow);
    font-size: 12px;
    animation: toast-in calc(var(--dur) + 30ms) var(--ease);
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translate(-50%, 6px);
    }
  }

  .app {
    display: grid;
    grid-template-columns: var(--sidebar-w) var(--list-w) 1fr;
    /* Zeile nie höher als das Fenster – Inhalte scrollen selbst. */
    grid-template-rows: minmax(0, 1fr);
    height: 100%;
  }
</style>
