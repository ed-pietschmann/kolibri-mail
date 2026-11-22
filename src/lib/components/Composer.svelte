<script lang="ts">
  import { kbd, mod } from '../platform';
  import { onDestroy, onMount, tick } from 'svelte';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { api, ApiError, type ComposeDraft, type ComposeKind } from '../api';
  import { accounts } from '../accounts.svelte';
  import { describeError } from '../errors';
  import { fileSize } from '../format';
  import { t } from '../i18n';
  import { mail } from '../mail.svelte';
  import type { Editor } from '../editor';
  import {
    hasContentSlot,
    lastTemplate,
    library,
    loadLibrary,
    rememberTemplate,
    signaturesOf,
  } from '../library.svelte';
  import HtmlPreview from './HtmlPreview.svelte';
  import AddressInput from './AddressInput.svelte';
  import ConfirmSheet from './ConfirmSheet.svelte';
  import Icon, { type IconName } from './Icon.svelte';

  /**
   * Verfassen im rechten Bereich (statt eigenem Fenster: ein zweites Fenster
   * bräuchte einen eigenen WebKit-Prozess, also deutlich mehr RAM).
   */
  let {
    kind,
    to = '',
    onclose,
    ontemplates,
  }: {
    kind: ComposeKind;
    /** Vorbelegter Empfänger (z. B. aus den Kontakten). */
    to?: string;
    onclose: () => void;
    /** „Vorlagen bearbeiten …“ */
    ontemplates?: () => void;
  } = $props();

  const MAX_BYTES = 25 * 1024 * 1024;
  const AUTOSAVE_MS = 30_000;

  let draft = $state<ComposeDraft | null>(null);
  let loadError = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let showCc = $state(false);
  let showBcc = $state(false);
  let dirty = $state(false);
  /** Gibt es schon eine Fassung auf dem Server (Autosave oder bearbeiteter Entwurf)? */
  let savedOnce = $state(false);
  let dragging = $state(false);
  let linkOpen = $state(false);
  let linkUrl = $state('');
  let confirm = $state<'noSubject' | 'close' | null>(null);
  let savedNote = $state(false);

  let editorEl: HTMLDivElement | undefined = $state();
  let toEl: HTMLInputElement | undefined = $state();
  let editor: Editor | null = null;
  let editorMod: typeof import('../editor') | null = null;
  /** Vorschau der fertigen Mail (HTML) oder null. */
  let preview = $state<string | null>(null);
  /** Live-Vorschau im Code-Modus (verzögert aktualisiert). */
  let codePreview = $state('');
  let codeTimer: ReturnType<typeof setTimeout> | undefined;
  /** Zähler, damit die Formatierungsknöpfe ihren Zustand neu lesen. */
  let editorTick = $state(0);
  let autosaveTimer: ReturnType<typeof setInterval> | undefined;
  let unlistenDrop: (() => void) | undefined;

  const totalSize = $derived(draft?.attachments.reduce((n, a) => n + a.size, 0) ?? 0);
  const account = $derived(accounts.list.find((a) => a.id === draft?.accountId));
  const codeMode = $derived(draft !== null && draft.code !== null);
  const signatures = $derived(draft ? signaturesOf(draft.accountId) : []);
  const designValue = $derived(
    !draft
      ? 'none'
      : draft.code !== null
        ? 'code'
        : draft.templateId !== null
          ? `t:${draft.templateId}`
          : 'none',
  );

  function markDirty() {
    dirty = true;
  }

  async function start() {
    try {
      const [prepared] = await Promise.all([
        api.prepareCompose(kind),
        loadLibrary().catch(() => {}),
      ]);
      if (to && !prepared.to) prepared.to = `${to}, `;
      // Neue Mail: zuletzt benutztes Design des Kontos (falls es noch existiert).
      if (kind.type === 'new' && prepared.templateId === null) {
        const last = lastTemplate(prepared.accountId);
        if (library.templates.some((t) => t.id === last && hasContentSlot(t)))
          prepared.templateId = last;
      }
      if (prepared.code !== null) codePreview = prepared.code;
      draft = prepared;
      showCc = prepared.cc.length > 0;
      showBcc = prepared.bcc.length > 0;
      savedOnce = kind.type === 'editDraft';
      await tick();
      editorMod = await import('../editor');
      const { createEditor } = editorMod;
      if (!editorEl) return;
      editor = createEditor({
        element: editorEl,
        html: prepared.html,
        placeholder: t('compose.placeholder'),
        label: t('compose.label'),
        onUpdate: markDirty,
        onSelection: () => editorTick++,
      });
      // Neue Mail / Weiterleiten: zuerst Empfänger, Antwort: gleich schreiben.
      if (kind.type === 'reply' || kind.type === 'replyAll' || to) editor.commands.focus('start');
      else toEl?.focus();
    } catch (err) {
      loadError = describeError(err);
    }
  }

  onMount(() => {
    void start();
    autosaveTimer = setInterval(() => {
      if (dirty && !busy) void saveDraft(true);
    }, AUTOSAVE_MS);
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === 'enter' || e.payload.type === 'over') dragging = true;
        else if (e.payload.type === 'leave') dragging = false;
        else if (e.payload.type === 'drop') {
          dragging = false;
          void addPaths(e.payload.paths);
        }
      })
      .then((fn) => (unlistenDrop = fn))
      .catch(() => {});
  });

  onDestroy(() => {
    clearInterval(autosaveTimer);
    clearTimeout(codeTimer);
    unlistenDrop?.();
    editor?.destroy();
  });

  /** Aktuellen Stand inkl. Editor-HTML. */
  function snapshot(): ComposeDraft | null {
    if (!draft) return null;
    return { ...$state.snapshot(draft), html: editor?.getHTML() ?? draft.html };
  }

  async function addPaths(paths: string[]) {
    if (!draft || paths.length === 0) return;
    try {
      const infos = await api.fileInfos(paths);
      draft.attachments.push(...infos);
      markDirty();
      error = totalSize > MAX_BYTES ? t('compose.tooLarge') : null;
    } catch (err) {
      error = describeError(err);
    }
  }

  async function pickFiles() {
    try {
      const picked = await openDialog({ multiple: true, directory: false });
      if (picked) await addPaths(Array.isArray(picked) ? picked : [picked]);
    } catch (err) {
      error = describeError(err);
    }
  }

  /** Design gewählt: keins, Vorlage (mit Platz für Text), fertige Mail oder eigener Code. */
  function chooseDesign(e: Event) {
    const select = e.currentTarget as HTMLSelectElement;
    const value = select.value;
    if (!draft) return;
    if (value === 'manage') {
      select.value = designValue;
      ontemplates?.();
      return;
    }
    if (value === 'none') {
      draft.templateId = null;
      draft.code = null;
    } else if (value === 'code') {
      draft.templateId = null;
      draft.code ??= '';
    } else {
      const tpl = library.templates.find((t) => `t:${t.id}` === value);
      if (!tpl) return;
      if (hasContentSlot(tpl)) {
        draft.templateId = tpl.id;
        draft.code = null;
      } else {
        // Fertige Mail: Code übernehmen, danach frei bearbeitbar.
        draft.templateId = null;
        draft.code = tpl.html;
      }
    }
    if (draft.code !== null) codePreview = draft.code;
    if (kind.type === 'new') rememberTemplate(draft.accountId, draft.templateId);
    markDirty();
  }

  function onCodeInput() {
    markDirty();
    clearTimeout(codeTimer);
    codeTimer = setTimeout(() => (codePreview = draft?.code ?? ''), 300);
  }

  /** Signatur wechseln (oder entfernen). */
  function applySignature(id: number | null) {
    if (!draft) return;
    draft.signatureId = id;
    const sig = id === null ? null : (library.signatures.find((s) => s.id === id) ?? null);
    if (editor && editorMod) editorMod.setSignature(editor, sig);
    markDirty();
  }

  /** Anderes Absenderkonto: dessen Standard-Signatur einsetzen. */
  function onAccountChange() {
    if (!draft) return;
    const def = signaturesOf(draft.accountId).find((s) => s.isDefault) ?? null;
    applySignature(def?.id ?? null);
  }

  async function showPreview() {
    const d = snapshot();
    if (!d) return;
    try {
      preview = await api.renderPreview(d);
    } catch (err) {
      showError(err);
    }
  }

  function removeAttachment(index: number) {
    draft?.attachments.splice(index, 1);
    markDirty();
    if (totalSize <= MAX_BYTES) error = null;
  }

  function showError(err: unknown) {
    // Eingabefehler (z. B. ungültige Adresse) bringen ihren Text schon mit.
    error = err instanceof ApiError && err.kind === 'invalid' ? err.detail : describeError(err);
  }

  async function send(skipSubjectCheck = false) {
    const d = snapshot();
    if (!d || busy) return;
    if (!skipSubjectCheck && !d.subject.trim()) {
      confirm = 'noSubject';
      return;
    }
    busy = true;
    error = null;
    try {
      await api.sendMessage(d);
      dirty = false;
      mail.toast = t('compose.sending');
      onclose();
    } catch (err) {
      showError(err);
    } finally {
      busy = false;
    }
  }

  async function saveDraft(silent = false): Promise<boolean> {
    const d = snapshot();
    if (!d) return false;
    try {
      await api.saveDraft(d);
      dirty = false;
      savedOnce = true;
      if (!silent) {
        savedNote = true;
        setTimeout(() => (savedNote = false), 2000);
      }
      return true;
    } catch (err) {
      if (!silent) showError(err);
      return false;
    }
  }

  async function discard() {
    if (draft && savedOnce) await api.discardDraft(draft.accountId, draft.draftId).catch(() => {});
    dirty = false;
    onclose();
  }

  /** Fenstergröße: normal (unten rechts), klein (nur Kopfzeile) oder groß. */
  let size = $state<'normal' | 'min' | 'max'>('normal');
  const kindTitle = $derived(t(`compose.title.${kind.type}`));

  /** Erneut ⌘N o. Ä. während des Schreibens: Fenster aufklappen und fokussieren. */
  export function reveal() {
    if (size === 'min') size = 'normal';
    queueMicrotask(() => editor?.commands.focus());
  }

  /** Schließen: bei ungesicherten Änderungen nachfragen (wie Apple Mail). */
  export function requestClose() {
    if (dirty) confirm = 'close';
    else onclose();
  }

  export function sendNow() {
    void send();
  }

  function onkeydown(e: KeyboardEvent) {
    if (mod(e) && e.key === 'Enter') {
      e.preventDefault();
      void send();
    } else if (mod(e) && e.shiftKey && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      void pickFiles();
    } else if (mod(e) && !e.shiftKey && e.key.toLowerCase() === 's') {
      e.preventDefault();
      void saveDraft();
    } else if (mod(e) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      toggleLink();
    } else if (e.key === 'Escape' && preview !== null) {
      e.preventDefault();
      preview = null;
    } else if (e.key === 'Escape' && !linkOpen && !confirm) {
      e.preventDefault();
      requestClose();
    }
    // Tastenkürzel der Liste (E, S, J …) dürfen hier nicht greifen.
    e.stopPropagation();
  }

  type Format = {
    name: string;
    icon: IconName;
    label: string;
    run: () => void;
  };

  const formats: Format[] = [
    {
      name: 'bold',
      icon: 'bold',
      label: t('compose.bold'),
      run: () => editor?.chain().focus().toggleBold().run(),
    },
    {
      name: 'italic',
      icon: 'italic',
      label: t('compose.italic'),
      run: () => editor?.chain().focus().toggleItalic().run(),
    },
    {
      name: 'underline',
      icon: 'underline',
      label: t('compose.underline'),
      run: () => editor?.chain().focus().toggleUnderline().run(),
    },
    {
      name: 'strike',
      icon: 'strike',
      label: t('compose.strike'),
      run: () => editor?.chain().focus().toggleStrike().run(),
    },
    {
      name: 'bulletList',
      icon: 'list',
      label: t('compose.bulletList'),
      run: () => editor?.chain().focus().toggleBulletList().run(),
    },
    {
      name: 'orderedList',
      icon: 'listOrdered',
      label: t('compose.orderedList'),
      run: () => editor?.chain().focus().toggleOrderedList().run(),
    },
    {
      name: 'blockquote',
      icon: 'quote',
      label: t('compose.quote'),
      run: () => editor?.chain().focus().toggleBlockquote().run(),
    },
  ];

  function isActive(name: string): boolean {
    void editorTick;
    return editor?.isActive(name) ?? false;
  }

  function toggleLink() {
    if (!editor) return;
    linkUrl = (editor.getAttributes('link').href as string | undefined) ?? '';
    linkOpen = !linkOpen;
  }

  function applyLink(e: SubmitEvent) {
    e.preventDefault();
    if (!editor) return;
    let url = linkUrl.trim();
    if (url && !/^(https?:|mailto:)/i.test(url))
      url = url.includes('@') ? `mailto:${url}` : `https://${url}`;
    const chain = editor.chain().focus().extendMarkRange('link');
    if (url) chain.setLink({ href: url }).run();
    else chain.unsetLink().run();
    linkOpen = false;
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  class="composer"
  class:min={size === 'min'}
  class:max={size === 'max'}
  aria-label={t('compose.label')}
  {onkeydown}
>
  <header class="head">
    <button
      class="title"
      onclick={() => (size = size === 'min' ? 'normal' : 'min')}
      aria-label={size === 'min' ? t('compose.restore') : t('compose.minimize')}
    >
      {draft?.subject.trim() || kindTitle}
    </button>
    {#if savedNote}<span class="note" role="status">{t('compose.saved')}</span>{/if}
    <button
      class="tool"
      aria-label={size === 'min' ? t('compose.restore') : t('compose.minimize')}
      title={size === 'min' ? t('compose.restore') : t('compose.minimize')}
      onclick={() => (size = size === 'min' ? 'normal' : 'min')}
    >
      <Icon name="minimize" size={14} />
    </button>
    <button
      class="tool"
      aria-label={size === 'max' ? t('compose.unmaximize') : t('compose.maximize')}
      title={size === 'max' ? t('compose.unmaximize') : t('compose.maximize')}
      onclick={() => (size = size === 'max' ? 'normal' : 'max')}
    >
      <Icon name={size === 'max' ? 'shrink' : 'expand'} size={14} />
    </button>
    <button
      class="tool"
      aria-label={t('compose.close')}
      title="{t('compose.close')} (Esc)"
      onclick={requestClose}
    >
      <Icon name="close" size={14} />
    </button>
  </header>

  <div class="body">
    {#if loadError}
      <div class="center error" role="alert">{loadError}</div>
    {:else if !draft}
      <div class="center"><span class="spinner"></span>{t('compose.loading')}</div>
    {:else}
      <div class="fields">
        {#if accounts.list.length > 1}
          <label class="row">
            <span>{t('compose.from')}</span>
            <select
              bind:value={draft.accountId}
              onchange={() => {
                markDirty();
                onAccountChange();
              }}
            >
              {#each accounts.list as a (a.id)}
                <option value={a.id}>
                  {a.displayName ? `${a.displayName} <${a.email}>` : a.email}
                </option>
              {/each}
            </select>
          </label>
        {/if}
        <label class="row">
          <span>{t('compose.to')}</span>
          <AddressInput
            bind:inputEl={toEl}
            bind:value={draft.to}
            label={t('compose.to')}
            oninput={markDirty}
          />
          {#if !showCc || !showBcc}
            <span class="extra">
              {#if !showCc}
                <button type="button" class="link" tabindex="-1" onclick={() => (showCc = true)}>
                  {t('compose.cc')}
                </button>
              {/if}
              {#if !showBcc}
                <button type="button" class="link" tabindex="-1" onclick={() => (showBcc = true)}>
                  {t('compose.showBcc')}
                </button>
              {/if}
            </span>
          {/if}
        </label>
        {#if showCc}
          <label class="row">
            <span>{t('compose.cc')}</span>
            <AddressInput bind:value={draft.cc} label={t('compose.cc')} oninput={markDirty} />
          </label>
        {/if}
        {#if showBcc}
          <label class="row">
            <span>{t('compose.bcc')}</span>
            <AddressInput bind:value={draft.bcc} label={t('compose.bcc')} oninput={markDirty} />
          </label>
        {/if}
        <label class="row">
          <span>{t('compose.subject')}</span>
          <input type="text" class="subject" bind:value={draft.subject} oninput={markDirty} />
        </label>
        <div class="row design">
          <span id="design-label">{t('compose.design')}</span>
          <select aria-labelledby="design-label" value={designValue} onchange={chooseDesign}>
            <option value="none">{t('compose.design.none')}</option>
            {#if library.templates.length}
              <optgroup label={t('compose.design.templates')}>
                {#each library.templates as tpl (tpl.id)}
                  <option value="t:{tpl.id}">{tpl.name}</option>
                {/each}
              </optgroup>
            {/if}
            <option value="code">{t('compose.design.code')}</option>
            {#if ontemplates}<option value="manage">{t('compose.design.manage')}</option>{/if}
          </select>
          {#if !codeMode && signatures.length}
            <span class="sub" id="sig-label">{t('compose.signature')}</span>
            <select
              aria-labelledby="sig-label"
              value={draft.signatureId ?? ''}
              onchange={(e) => {
                const v = e.currentTarget.value;
                applySignature(v ? Number(v) : null);
              }}
            >
              <option value="">{t('compose.signature.none')}</option>
              {#each signatures as sig (sig.id)}
                <option value={sig.id}>{sig.name}</option>
              {/each}
            </select>
          {/if}
        </div>
      </div>

      {#if draft.attachments.length}
        <ul class="attachments">
          {#each draft.attachments as a, i (`${a.name}-${i}`)}
            <li>
              <Icon name="paperclip" size={12} />
              <span class="name">{a.name}</span>
              <span class="size">{fileSize(a.size)}</span>
              <button
                class="remove"
                aria-label={t('compose.removeAttachment', { name: a.name })}
                onclick={() => removeAttachment(i)}
              >
                <Icon name="close" size={11} />
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      {#if error}<div class="error-line" role="alert">{error}</div>{/if}
    {/if}

    <div class="editor" bind:this={editorEl} hidden={!draft || codeMode}></div>

    {#if draft && draft.code !== null}
      <div class="code-pane">
        <textarea
          class="code"
          bind:value={draft.code}
          oninput={onCodeInput}
          spellcheck="false"
          autocapitalize="off"
          aria-label={t('compose.design.code')}
          placeholder={t('compose.codePlaceholder')}></textarea>
        <div class="code-preview">
          {#if codePreview.trim()}
            <HtmlPreview html={codePreview} title={t('compose.preview')} />
          {:else}
            <p class="code-empty">{t('compose.codeEmpty')}</p>
          {/if}
        </div>
      </div>
    {/if}

    {#if preview !== null}
      <div class="preview" role="dialog" aria-label={t('compose.preview')}>
        <div class="preview-head">
          <span>{t('compose.previewTitle')}</span>
          <button class="tool" aria-label={t('compose.close')} onclick={() => (preview = null)}>
            <Icon name="close" size={14} />
          </button>
        </div>
        <div class="preview-body">
          <HtmlPreview html={preview} title={t('compose.preview')} />
        </div>
      </div>
    {/if}

    {#if draft}
      <footer class="foot">
        <button
          class="send"
          disabled={busy}
          title="{t('compose.send')} ({kbd('⌘↩')})"
          onclick={() => void send()}
        >
          {#if busy}<span class="spinner"></span>{:else}<Icon name="send" size={14} />{/if}
          {t('compose.send')}
        </button>
        <button
          class="tool"
          aria-label={t('compose.attach')}
          title="{t('compose.attach')} ({kbd('⇧⌘A')})"
          disabled={busy}
          onclick={() => void pickFiles()}
        >
          <Icon name="paperclip" />
        </button>
        <button
          class="tool"
          aria-label={t('compose.preview')}
          title={t('compose.preview')}
          aria-pressed={preview !== null}
          onclick={() => (preview === null ? void showPreview() : (preview = null))}
        >
          <Icon name="eye" />
        </button>
        <span class="sep" aria-hidden="true" hidden={codeMode}></span>
        <div class="format" role="toolbar" aria-label={t('compose.label')} hidden={codeMode}>
          {#each formats as f (f.name)}
            <button
              class="fmt"
              class:active={isActive(f.name)}
              aria-label={f.label}
              aria-pressed={isActive(f.name)}
              title={f.label}
              onclick={f.run}
            >
              <Icon name={f.icon} size={14} />
            </button>
          {/each}
          <button
            class="fmt"
            class:active={isActive('link')}
            aria-label={t('compose.link')}
            aria-expanded={linkOpen}
            title="{t('compose.link')} ({kbd('⌘K')})"
            onclick={toggleLink}
          >
            <Icon name="link" size={14} />
          </button>
          {#if linkOpen}
            <form class="link-form" onsubmit={applyLink}>
              <!-- svelte-ignore a11y_autofocus -->
              <input
                type="text"
                bind:value={linkUrl}
                placeholder={t('compose.linkPlaceholder')}
                aria-label={t('compose.link')}
                autofocus
                onkeydown={(e) => {
                  if (e.key === 'Escape') {
                    e.stopPropagation();
                    linkOpen = false;
                  }
                }}
              />
              <button class="btn" type="submit">
                {linkUrl.trim() ? t('compose.linkApply') : t('compose.linkRemove')}
              </button>
            </form>
          {/if}
        </div>
        <button
          class="tool trash"
          aria-label={t('compose.discard')}
          title={t('compose.discard')}
          disabled={busy}
          onclick={() => void discard()}
        >
          <Icon name="trash" />
        </button>
      </footer>
    {/if}
  </div>

  {#if dragging}
    <div class="drop" aria-hidden="true">
      <Icon name="paperclip" size={28} />
      {t('compose.dropHint')}
    </div>
  {/if}
</section>

<ConfirmSheet
  open={confirm === 'noSubject'}
  title={t('compose.noSubject.title')}
  body={t('compose.noSubject.body')}
  confirmLabel={t('compose.noSubject.confirm')}
  onconfirm={() => {
    confirm = null;
    void send(true);
  }}
  oncancel={() => (confirm = null)}
/>

<ConfirmSheet
  open={confirm === 'close'}
  title={t('compose.closeDirty.title')}
  body={t('compose.closeDirty.body')}
  confirmLabel={t('compose.closeDirty.save')}
  secondaryLabel={t('compose.closeDirty.discard')}
  onconfirm={async () => {
    confirm = null;
    if (await saveDraft(true)) onclose();
  }}
  onsecondary={() => {
    confirm = null;
    void discard();
  }}
  oncancel={() => (confirm = null)}
/>

<style>
  /* Schwebendes Fenster unten rechts – liegt über Leseansicht, Kalender & Co. */
  .composer {
    position: fixed;
    right: 20px;
    bottom: 20px;
    z-index: 50;
    display: flex;
    flex-direction: column;
    width: min(580px, calc(100vw - var(--sidebar-w) - 40px));
    height: min(600px, calc(100vh - var(--titlebar-h) - 40px));
    border-radius: 14px;
    background: var(--surface);
    box-shadow: var(--float-shadow);
    overflow: hidden;
    animation: rise var(--dur) var(--ease);
  }

  .composer.max {
    left: calc(var(--sidebar-w) + 20px);
    top: 20px;
    width: auto;
    height: auto;
  }

  .composer.min {
    width: 320px;
    height: auto;
  }

  .composer.min .body {
    display: none;
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(12px);
    }
  }

  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 46px;
    padding: 0 var(--space-2) 0 var(--space-4);
    border-bottom: 1px solid var(--separator);
    flex-shrink: 0;
  }

  .composer.min .head {
    border-bottom: 0;
  }

  .title {
    flex: 1;
    min-width: 0;
    height: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
    font-weight: 600;
  }

  .note {
    margin-right: var(--space-2);
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 7px;
    color: var(--icon);
    transition: background-color var(--dur) var(--ease);
  }

  .tool:not(:disabled):hover {
    background: var(--hover);
    color: var(--text);
  }

  .tool:disabled {
    opacity: 0.45;
  }

  .foot {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: var(--space-3) var(--space-3) var(--space-3) var(--space-4);
    border-top: 1px solid var(--separator);
    flex-shrink: 0;
  }

  .send {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 34px;
    margin-right: var(--space-2);
    padding: 0 var(--space-4);
    border-radius: 9px;
    background: var(--ink);
    color: var(--ink-text);
    font-weight: 600;
    transition: background-color var(--dur) var(--ease);
  }

  .send:not(:disabled):hover {
    background: var(--ink-hover);
  }

  .send:disabled {
    opacity: 0.6;
  }

  .sep {
    width: 1px;
    height: 18px;
    margin: 0 var(--space-1);
    background: var(--separator);
  }

  .trash {
    margin-left: auto;
  }

  .fields {
    display: grid;
    padding: 0 var(--space-4);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 36px;
    border-bottom: 1px solid var(--separator);
  }

  .row > span:first-child {
    flex-shrink: 0;
    width: 56px;
    color: var(--text-tertiary);
  }

  .row input,
  .row select {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
  }

  .row input:focus,
  .row select:focus {
    box-shadow: none;
  }

  .row .subject {
    font-weight: 600;
  }

  .extra {
    display: flex;
    gap: var(--space-3);
    flex-shrink: 0;
  }

  .format {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }

  .fmt {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 7px;
    color: var(--icon);
  }

  .fmt:hover {
    background: var(--hover);
  }

  .fmt.active {
    background: var(--active);
    color: var(--text);
  }

  /* Link-Eingabe schwebt über der Fußleiste */
  .link-form {
    position: absolute;
    left: var(--space-4);
    right: var(--space-4);
    bottom: calc(100% + 8px);
    display: flex;
    gap: var(--space-2);
    align-items: center;
    padding: var(--space-2);
    border-radius: var(--card-radius);
    background: var(--sheet-bg);
    box-shadow: var(--sheet-shadow);
  }

  .foot {
    position: relative;
  }

  .link-form input {
    flex: 1;
  }

  .link-form .btn {
    min-width: 0;
  }

  .attachments {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: var(--space-3) var(--space-4) 0;
    padding: 0;
    list-style: none;
  }

  .attachments li {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    max-width: 280px;
    padding: 3px 4px 3px 10px;
    border-radius: var(--radius);
    background: var(--hover);
    font-size: 12px;
  }

  .attachments .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .attachments .size {
    color: var(--text-tertiary);
  }

  .remove {
    display: inline-flex;
    padding: 4px;
    border-radius: 4px;
    color: var(--text-tertiary);
  }

  .remove:hover {
    background: var(--active);
    color: var(--text);
  }

  .error-line {
    margin: var(--space-3) var(--space-4) 0;
    color: var(--danger);
    font-size: 12px;
  }

  .editor {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-4);
    cursor: text;
  }

  .editor :global(.ProseMirror) {
    min-height: 100%;
    outline: none;
    font-size: 14px;
    line-height: 1.55;
    user-select: text;
    -webkit-user-select: text;
    overflow-wrap: anywhere;
  }

  .editor :global(.ProseMirror p) {
    margin: 0 0 0.6em;
  }

  .editor :global(.ProseMirror blockquote) {
    margin: 0.6em 0;
    padding-left: var(--space-3);
    border-left: 2px solid var(--accent);
    color: var(--text-secondary);
  }

  .editor :global(.ProseMirror a) {
    color: var(--accent);
  }

  /* Signatur: festes Element, zeigt ihr eigenes HTML unverändert. */
  .editor :global(.kolibri-signature) {
    margin: 0.8em 0;
    padding: 6px 8px;
    margin-left: -8px;
    border-radius: 8px;
    max-width: 100%;
    overflow: hidden;
    contain: layout paint;
    cursor: default;
    transition: background-color var(--dur) var(--ease);
  }

  .editor :global(.kolibri-signature:hover) {
    background: var(--hover);
  }

  .editor :global(.kolibri-signature.ProseMirror-selectednode) {
    box-shadow: 0 0 0 2px var(--accent);
  }

  .editor :global(.kolibri-signature img) {
    max-width: 100%;
    height: auto;
  }

  /* Signaturen sind für weißes Papier gemacht – im Dunkelmodus auf Papier zeigen. */
  @media (prefers-color-scheme: dark) {
    .editor :global(.kolibri-signature) {
      background: #fff;
      color: #1d1d1f;
      width: fit-content;
      margin-left: 0;
      padding: 10px 12px;
    }

    .editor :global(.kolibri-signature:hover) {
      background: #f2f2f4;
    }
  }

  .design .sub {
    flex-shrink: 0;
    color: var(--text-tertiary);
  }

  .design select {
    flex: 1 1 0;
    cursor: pointer;
  }

  .code-pane {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-rows: minmax(0, 2fr) minmax(0, 3fr);
    border-top: 1px solid var(--separator);
    margin-top: -1px;
  }

  .composer.max .code-pane {
    grid-template-rows: none;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  }

  .code {
    width: 100%;
    height: 100%;
    padding: var(--space-3) var(--space-4);
    border: 0;
    border-bottom: 1px solid var(--separator);
    background: transparent;
    color: var(--text);
    font:
      12px/1.5 ui-monospace,
      'SF Mono',
      Menlo,
      monospace;
    resize: none;
    outline: none;
    tab-size: 2;
    user-select: text;
    -webkit-user-select: text;
  }

  .composer.max .code {
    border-bottom: 0;
    border-right: 1px solid var(--separator);
  }

  .code-preview {
    min-height: 0;
    background: #fff;
  }

  .code-empty {
    margin: 0;
    padding: var(--space-6);
    color: #8a8a90;
    text-align: center;
  }

  .preview {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    animation: fade var(--dur) var(--ease);
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  .preview-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 40px;
    padding: 0 var(--space-2) 0 var(--space-4);
    border-bottom: 1px solid var(--separator);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
  }

  .preview-body {
    flex: 1;
    min-height: 0;
    background: #fff;
  }

  .editor :global(.ProseMirror p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    float: left;
    height: 0;
    color: var(--text-tertiary);
    pointer-events: none;
  }

  .center {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    color: var(--text-tertiary);
  }

  .center.error {
    color: var(--danger);
    padding: var(--space-6);
    text-align: center;
  }

  .drop {
    position: absolute;
    inset: var(--space-3);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    border: 2px dashed var(--accent);
    border-radius: 12px;
    background: color-mix(in srgb, var(--accent) 10%, var(--surface-reader));
    color: var(--accent);
    font-weight: 600;
    pointer-events: none;
  }
</style>
