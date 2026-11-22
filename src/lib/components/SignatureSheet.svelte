<script lang="ts">
  import { mod } from '../platform';
  import { tick } from 'svelte';
  import { api, ApiError, type Account, type Signature } from '../api';
  import { describeError } from '../errors';
  import { t } from '../i18n';
  import { loadLibrary, signaturesOf } from '../library.svelte';
  import HtmlPreview from './HtmlPreview.svelte';
  import Icon, { type IconName } from './Icon.svelte';
  import Sheet from './Sheet.svelte';
  import { SIGNATURE_LAYOUTS, signaturePage, type SignatureFields } from '../signatureTemplates';

  /**
   * Signaturen eines Kontos: mehrere, je mit Namen; gestalten (Fett, Farbe,
   * Größe, Link, Logo) oder als HTML-Code einfügen. Gespeichert wird in Rust
   * gesäubertes HTML.
   */
  let { account, onclose }: { account: Account | null; onclose: () => void } = $props();

  /** Bilder werden verkleinert (für Retina doppelt so viele Pixel wie angezeigt). */
  const IMAGE_SHOW_MAX = 240;
  const IMAGE_MAX_BYTES = 350 * 1024;

  let editing = $state<Signature | null>(null);
  let mode = $state<'visual' | 'code' | 'templates'>('visual');
  let fields = $state<SignatureFields>({
    greeting: 'Viele Grüße',
    name: '',
    title: '',
    company: '',
    phone: '',
    mobile: '',
    email: '',
    website: '',
    address: '',
    logo: '',
    accent: '#0a84ff',
  });
  let layoutId = $state('logo');
  let logoInput: HTMLInputElement | undefined = $state();
  const ACCENTS = [
    '#111113',
    '#0a84ff',
    '#30b0c7',
    '#34c759',
    '#ff9500',
    '#d9542b',
    '#ff2d55',
    '#af52de',
  ];
  let dirty = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmDelete = $state(false);
  let surface: HTMLDivElement | undefined = $state();
  let fileInput: HTMLInputElement | undefined = $state();
  let nameInput: HTMLInputElement | undefined = $state();
  let savedRange: Range | null = null;
  let linkOpen = $state(false);
  let linkUrl = $state('');

  const list = $derived(account ? signaturesOf(account.id) : []);

  $effect(() => {
    if (!account) return;
    error = null;
    void loadLibrary(true).then(() => {
      const first = signaturesOf(account.id)[0];
      select(first ?? null);
    });
  });

  function blank(): Signature {
    return {
      id: 0,
      accountId: account?.id ?? 0,
      name: list.length ? t('signature.newName') : t('signature.defaultName'),
      html: '',
      isDefault: list.length === 0,
    };
  }

  async function select(sig: Signature | null) {
    if (editing && dirty && !(await save(false))) return;
    editing = sig ? { ...sig } : null;
    dirty = false;
    confirmDelete = false;
    linkOpen = false;
    error = null;
    mode = 'visual';
    await tick();
    if (surface && editing) surface.innerHTML = editing.html;
  }

  async function create() {
    await select(blank());
    await tick();
    nameInput?.select();
  }

  function syncFromSurface() {
    if (editing && surface && mode === 'visual') editing.html = surface.innerHTML;
    dirty = true;
  }

  function openTemplates() {
    if (!fields.name) fields.name = account?.displayName || '';
    if (!fields.email) fields.email = account?.email ?? '';
    void switchMode('templates');
  }

  function applyLayout() {
    if (!editing) return;
    const layout = SIGNATURE_LAYOUTS.find((l) => l.id === layoutId);
    if (!layout) return;
    editing.html = layout.render($state.snapshot(fields));
    dirty = true;
    void switchMode('visual');
  }

  async function pickLogo(file: File) {
    error = null;
    const shrunk = await shrink(file).catch(() => null);
    if (!shrunk) error = t('signature.imageFailed');
    else if (shrunk.data.length * 0.75 > IMAGE_MAX_BYTES) error = t('signature.imageTooLarge');
    else fields.logo = shrunk.data;
  }

  async function switchMode(next: 'visual' | 'code' | 'templates') {
    if (!editing || next === mode) return;
    if (mode === 'visual' && surface) editing.html = surface.innerHTML;
    mode = next;
    await tick();
    if (next === 'visual' && surface) surface.innerHTML = editing.html;
  }

  async function save(close: boolean): Promise<boolean> {
    if (!editing || !account) return false;
    if (mode === 'visual' && surface) editing.html = surface.innerHTML;
    busy = true;
    error = null;
    try {
      const wasDefault = list.find((s) => s.id === editing?.id)?.isDefault ?? false;
      const saved = await api.saveSignature($state.snapshot(editing));
      if (wasDefault && !editing.isDefault) await api.setDefaultSignature(account.id, null);
      await loadLibrary(true);
      dirty = false;
      editing = { ...saved, isDefault: editing.isDefault };
      if (mode === 'visual' && surface) surface.innerHTML = saved.html;
      if (close) onclose();
      return true;
    } catch (err) {
      error = err instanceof ApiError && err.kind === 'invalid' ? err.detail : describeError(err);
      return false;
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!editing) return;
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    try {
      if (editing.id) await api.deleteSignature(editing.id);
      await loadLibrary(true);
      dirty = false;
      editing = null;
      await select(account ? (signaturesOf(account.id)[0] ?? null) : null);
    } catch (err) {
      error = describeError(err);
    }
  }

  // ---- Gestalten (contenteditable + execCommand: behält beliebiges HTML bei) ----

  function rememberSelection() {
    const sel = window.getSelection();
    if (sel && sel.rangeCount && surface?.contains(sel.anchorNode)) savedRange = sel.getRangeAt(0);
  }

  function restoreSelection() {
    surface?.focus();
    const sel = window.getSelection();
    if (savedRange && sel) {
      sel.removeAllRanges();
      sel.addRange(savedRange);
    }
  }

  function exec(command: string, value?: string) {
    restoreSelection();
    document.execCommand('styleWithCSS', false, 'true');
    document.execCommand(command, false, value);
    rememberSelection();
    syncFromSurface();
  }

  type Tool = { icon: IconName; label: string; run: () => void };
  const tools: Tool[] = [
    { icon: 'bold', label: t('compose.bold'), run: () => exec('bold') },
    { icon: 'italic', label: t('compose.italic'), run: () => exec('italic') },
    { icon: 'underline', label: t('compose.underline'), run: () => exec('underline') },
  ];

  function applyLink(e: SubmitEvent) {
    e.preventDefault();
    let url = linkUrl.trim();
    if (url && !/^(https?:|mailto:|tel:)/i.test(url))
      url = url.includes('@') ? `mailto:${url}` : `https://${url}`;
    if (url) exec('createLink', url);
    else exec('unlink');
    linkOpen = false;
    linkUrl = '';
  }

  /** Bild verkleinern und als data:-URI einfügen (wird beim Senden eingebettet). */
  /** Bild verkleinern (Breite höchstens IMAGE_SHOW_MAX, doppelt so viele Pixel). */
  async function shrink(file: File): Promise<{ data: string; show: number }> {
    const url = URL.createObjectURL(file);
    try {
      const img = new Image();
      img.src = url;
      await img.decode();
      const show = Math.min(img.naturalWidth, IMAGE_SHOW_MAX);
      const pixels = Math.min(img.naturalWidth, show * 2);
      const height = Math.round((img.naturalHeight / img.naturalWidth) * pixels);
      const canvas = document.createElement('canvas');
      canvas.width = pixels;
      canvas.height = height;
      canvas.getContext('2d')?.drawImage(img, 0, 0, pixels, height);
      const keepAlpha = /png|gif|webp|svg/.test(file.type);
      return { data: canvas.toDataURL(keepAlpha ? 'image/png' : 'image/jpeg', 0.88), show };
    } finally {
      URL.revokeObjectURL(url);
    }
  }

  async function insertImage(file: File) {
    error = null;
    if (!file.type.startsWith('image/')) return;
    try {
      const url = URL.createObjectURL(file);
      const img = new Image();
      img.src = url;
      await img.decode();
      URL.revokeObjectURL(url);
      const show = Math.min(img.naturalWidth, IMAGE_SHOW_MAX);
      const pixels = Math.min(img.naturalWidth, show * 2);
      const height = Math.round((img.naturalHeight / img.naturalWidth) * pixels);
      const canvas = document.createElement('canvas');
      canvas.width = pixels;
      canvas.height = height;
      canvas.getContext('2d')?.drawImage(img, 0, 0, pixels, height);
      const keepAlpha = /png|gif|webp|svg/.test(file.type);
      const data = canvas.toDataURL(keepAlpha ? 'image/png' : 'image/jpeg', 0.88);
      if (data.length * 0.75 > IMAGE_MAX_BYTES) {
        error = t('signature.imageTooLarge');
        return;
      }
      restoreSelection();
      const html = `<img src="${data}" width="${show}" alt="" style="display:block;max-width:100%;height:auto">`;
      document.execCommand('insertHTML', false, html);
      syncFromSurface();
    } catch {
      error = t('signature.imageFailed');
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (mod(e) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      void save(false);
    }
  }
</script>

<Sheet
  open={account !== null}
  label={t('signature.title', { email: account?.email ?? '' })}
  width={900}
  onclose={() => {
    if (dirty) void save(true);
    else onclose();
  }}
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="layout" {onkeydown}>
    <aside class="list">
      <h2>{t('signature.heading')}</h2>
      <p class="account">{account?.email}</p>
      <ul>
        {#each list as sig (sig.id)}
          <li>
            <button
              class="item"
              class:selected={editing?.id === sig.id}
              aria-current={editing?.id === sig.id}
              onclick={() => void select(sig)}
            >
              <Icon name="signature" size={15} />
              <span class="name">{sig.name}</span>
              {#if sig.isDefault}<span class="badge">{t('signature.default')}</span>{/if}
            </button>
          </li>
        {/each}
        {#if editing && editing.id === 0}
          <li>
            <button class="item selected" aria-current="true">
              <Icon name="signature" size={15} />
              <span class="name">{editing.name || t('signature.newName')}</span>
            </button>
          </li>
        {/if}
      </ul>
      <button class="add" onclick={() => void create()}>
        <Icon name="plus" size={14} />
        {t('signature.add')}
      </button>
    </aside>

    <section class="edit">
      {#if editing}
        <div class="top">
          <input
            bind:this={nameInput}
            class="name-input"
            type="text"
            bind:value={editing.name}
            oninput={() => (dirty = true)}
            aria-label={t('signature.name')}
            placeholder={t('signature.name')}
          />
          <div class="segmented" role="tablist" aria-label={t('signature.mode')}>
            <button
              role="tab"
              aria-selected={mode === 'visual'}
              class:on={mode === 'visual'}
              onclick={() => void switchMode('visual')}
            >
              {t('signature.visual')}
            </button>
            <button
              role="tab"
              aria-selected={mode === 'code'}
              class:on={mode === 'code'}
              onclick={() => void switchMode('code')}
            >
              <Icon name="code" size={13} />
              {t('signature.code')}
            </button>
            <button
              role="tab"
              aria-selected={mode === 'templates'}
              class:on={mode === 'templates'}
              onclick={openTemplates}
            >
              <Icon name="template" size={13} />
              {t('signature.templates')}
            </button>
          </div>
        </div>

        {#if mode === 'visual'}
          <div class="toolbar" role="toolbar" aria-label={t('signature.tools')}>
            {#each tools as tool (tool.icon)}
              <button
                class="fmt"
                aria-label={tool.label}
                title={tool.label}
                onmousedown={(e) => e.preventDefault()}
                onclick={tool.run}
              >
                <Icon name={tool.icon} size={14} />
              </button>
            {/each}
            <label class="fmt color" title={t('signature.color')}>
              <span class="swatch" aria-hidden="true">A</span>
              <input
                type="color"
                aria-label={t('signature.color')}
                onmousedown={rememberSelection}
                oninput={(e) => exec('foreColor', e.currentTarget.value)}
              />
            </label>
            <select
              class="size"
              aria-label={t('signature.size')}
              onmousedown={rememberSelection}
              onchange={(e) => {
                exec('fontSize', e.currentTarget.value);
                e.currentTarget.value = '';
              }}
            >
              <option value="">{t('signature.size')}</option>
              <option value="2">{t('signature.size.small')}</option>
              <option value="3">{t('signature.size.normal')}</option>
              <option value="4">{t('signature.size.large')}</option>
              <option value="5">{t('signature.size.xlarge')}</option>
            </select>
            <span class="sep" aria-hidden="true"></span>
            <button
              class="fmt"
              aria-label={t('compose.link')}
              title={t('compose.link')}
              aria-expanded={linkOpen}
              onmousedown={(e) => {
                e.preventDefault();
                rememberSelection();
              }}
              onclick={() => (linkOpen = !linkOpen)}
            >
              <Icon name="link" size={14} />
            </button>
            <button
              class="fmt"
              aria-label={t('signature.image')}
              title={t('signature.image')}
              onmousedown={(e) => {
                e.preventDefault();
                rememberSelection();
              }}
              onclick={() => fileInput?.click()}
            >
              <Icon name="image" size={14} />
            </button>
            <input
              bind:this={fileInput}
              type="file"
              accept="image/png,image/jpeg,image/gif,image/webp"
              hidden
              onchange={(e) => {
                const f = e.currentTarget.files?.[0];
                if (f) void insertImage(f);
                e.currentTarget.value = '';
              }}
            />
            <button
              class="fmt"
              aria-label={t('signature.clear')}
              title={t('signature.clear')}
              onmousedown={(e) => e.preventDefault()}
              onclick={() => exec('removeFormat')}
            >
              <Icon name="xCircle" size={14} />
            </button>
          </div>
          {#if linkOpen}
            <form class="link-form" onsubmit={applyLink}>
              <!-- svelte-ignore a11y_autofocus -->
              <input
                type="text"
                bind:value={linkUrl}
                placeholder={t('signature.linkPlaceholder')}
                aria-label={t('compose.link')}
                autofocus
              />
              <button class="btn" type="submit">
                {linkUrl.trim() ? t('compose.linkApply') : t('compose.linkRemove')}
              </button>
            </form>
          {/if}
          <div class="paper">
            <div
              bind:this={surface}
              class="surface"
              contenteditable="true"
              role="textbox"
              tabindex="0"
              aria-multiline="true"
              aria-label={t('signature.content')}
              data-placeholder={t('signature.placeholder')}
              oninput={syncFromSurface}
              onkeyup={rememberSelection}
              onmouseup={rememberSelection}
            ></div>
          </div>
        {:else if mode === 'templates'}
          <div class="tpl">
            <div class="tpl-form">
              <label class="f wide">
                <span>{t('signature.f.greeting')}</span>
                <input type="text" bind:value={fields.greeting} />
              </label>
              <label class="f wide">
                <span>{t('signature.f.name')}</span>
                <input type="text" bind:value={fields.name} />
              </label>
              <label class="f">
                <span>{t('signature.f.title')}</span>
                <input type="text" bind:value={fields.title} />
              </label>
              <label class="f">
                <span>{t('signature.f.company')}</span>
                <input type="text" bind:value={fields.company} />
              </label>
              <label class="f">
                <span>{t('signature.f.phone')}</span>
                <input type="tel" bind:value={fields.phone} />
              </label>
              <label class="f">
                <span>{t('signature.f.mobile')}</span>
                <input type="tel" bind:value={fields.mobile} />
              </label>
              <label class="f">
                <span>{t('signature.f.email')}</span>
                <input type="email" bind:value={fields.email} />
              </label>
              <label class="f">
                <span>{t('signature.f.website')}</span>
                <input type="text" bind:value={fields.website} />
              </label>
              <label class="f wide">
                <span>{t('signature.f.address')}</span>
                <input type="text" bind:value={fields.address} />
              </label>
              <div class="f">
                <span>{t('signature.f.logo')}</span>
                <div class="logo-row">
                  {#if fields.logo}
                    <img src={fields.logo} alt="" class="logo-thumb" />
                    <button type="button" class="link" onclick={() => (fields.logo = '')}>
                      {t('signature.f.logoRemove')}
                    </button>
                  {:else}
                    <button type="button" class="btn small" onclick={() => logoInput?.click()}>
                      <Icon name="image" size={13} />
                      {t('signature.f.logoPick')}
                    </button>
                  {/if}
                  <input
                    bind:this={logoInput}
                    type="file"
                    accept="image/png,image/jpeg,image/gif,image/webp"
                    hidden
                    onchange={(e) => {
                      const f = e.currentTarget.files?.[0];
                      if (f) void pickLogo(f);
                      e.currentTarget.value = '';
                    }}
                  />
                </div>
              </div>
              <div class="f">
                <span id="accent-label">{t('signature.f.accent')}</span>
                <div class="accents" role="radiogroup" aria-labelledby="accent-label">
                  {#each ACCENTS as c (c)}
                    <button
                      type="button"
                      role="radio"
                      aria-checked={fields.accent === c}
                      aria-label={c}
                      class="accent"
                      class:on={fields.accent === c}
                      style:--c={c}
                      onclick={() => (fields.accent = c)}
                    ></button>
                  {/each}
                </div>
              </div>
            </div>
            <div class="tpl-gallery" role="radiogroup" aria-label={t('signature.templates')}>
              {#each SIGNATURE_LAYOUTS as layout (layout.id)}
                <button
                  type="button"
                  role="radio"
                  aria-checked={layoutId === layout.id}
                  class="tpl-card"
                  class:on={layoutId === layout.id}
                  onclick={() => (layoutId = layout.id)}
                  ondblclick={applyLayout}
                >
                  <span class="tpl-name">{layout.name}</span>
                  <span class="tpl-preview">
                    <HtmlPreview
                      html={signaturePage(layout.render(fields))}
                      title={layout.name}
                      scale={0.62}
                    />
                  </span>
                </button>
              {/each}
            </div>
          </div>
          <div class="tpl-apply">
            <span class="hint-small">{t('signature.templatesHint')}</span>
            <button type="button" class="btn primary" onclick={applyLayout}>
              {t('signature.templatesApply')}
            </button>
          </div>
        {:else}
          <div class="code-split">
            <textarea
              class="code"
              bind:value={editing.html}
              oninput={() => (dirty = true)}
              spellcheck="false"
              aria-label={t('signature.code')}
              placeholder={t('signature.codePlaceholder')}></textarea>
            <div class="paper preview">
              <HtmlPreview
                html={`<body style="margin:0;padding:14px 16px;font:14px/1.5 -apple-system,Helvetica,sans-serif;color:#1d1d1f">${editing.html}</body>`}
                title={t('compose.preview')}
              />
            </div>
          </div>
        {/if}

        <label class="check">
          <input type="checkbox" bind:checked={editing.isDefault} onchange={() => (dirty = true)} />
          {t('signature.useDefault')}
        </label>

        {#if error}<p class="error" role="alert">{error}</p>{/if}

        <div class="buttons">
          <button
            class="btn {confirmDelete ? 'danger' : 'delete'}"
            onclick={() => void remove()}
            disabled={busy}
          >
            {confirmDelete ? t('signature.deleteConfirm') : t('signature.delete')}
          </button>
          <span class="spacer"></span>
          <button class="btn" onclick={onclose}>{t('signature.close')}</button>
          <button class="btn primary" onclick={() => void save(false)} disabled={busy || !dirty}>
            {t('signature.save')}
          </button>
        </div>
      {:else}
        <div class="empty">
          <Icon name="signature" size={32} />
          <h3>{t('signature.emptyTitle')}</h3>
          <p>{t('signature.emptyBody')}</p>
          <button class="btn primary" onclick={() => void create()}>{t('signature.add')}</button>
        </div>
      {/if}
    </section>
  </div>
</Sheet>

<style>
  .layout {
    display: grid;
    grid-template-columns: 210px minmax(0, 1fr);
    gap: var(--space-6);
    min-height: 440px;
    margin: calc(var(--space-6) * -1);
  }

  .list {
    display: flex;
    flex-direction: column;
    padding: var(--space-6) var(--space-3) var(--space-4);
    background: var(--sidebar-bg);
    border-radius: 12px 0 0 12px;
  }

  h2 {
    margin: 0 var(--space-2);
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .account {
    margin: 2px var(--space-2) var(--space-4);
    color: var(--text-tertiary);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  ul {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: 32px;
    padding: 0 var(--space-2);
    border-radius: 8px;
    color: var(--icon);
    text-align: left;
  }

  .item:hover {
    background: var(--hover);
  }

  .item.selected {
    background: var(--sidebar-selected);
    box-shadow: var(--sidebar-selected-shadow);
    color: var(--text);
  }

  .item .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }

  .badge {
    flex-shrink: 0;
    padding: 1px 6px;
    border-radius: 5px;
    background: var(--active);
    color: var(--text-secondary);
    font-size: 10px;
    font-weight: 600;
  }

  .add {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 32px;
    margin-top: var(--space-2);
    padding: 0 var(--space-2);
    border-radius: 8px;
    color: var(--text-secondary);
  }

  .add:hover {
    background: var(--hover);
    color: var(--text);
  }

  .edit {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
    padding: var(--space-6) var(--space-6) var(--space-6) 0;
  }

  .top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .name-input {
    flex: 1;
    min-width: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .segmented {
    display: inline-flex;
    flex-shrink: 0;
    padding: 2px;
    border-radius: 8px;
    background: var(--field-search);
  }

  .segmented button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 var(--space-3);
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
  }

  .segmented button.on {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--sidebar-selected-shadow);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .fmt {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 7px;
    color: var(--icon);
    cursor: pointer;
  }

  .fmt:hover {
    background: var(--hover);
    color: var(--text);
  }

  .color {
    position: relative;
  }

  .swatch {
    font-weight: 700;
    text-decoration: underline 3px;
    text-underline-offset: 2px;
    text-decoration-color: var(--accent);
  }

  .color input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .size {
    width: auto;
    flex: 0 0 auto;
    height: 28px;
    margin-left: var(--space-1);
    font-size: 12px;
  }

  .sep {
    width: 1px;
    height: 18px;
    margin: 0 var(--space-1);
    background: var(--separator);
  }

  .link-form {
    display: flex;
    gap: var(--space-2);
  }

  .link-form input {
    flex: 1;
  }

  /* Signaturen sehen beim Empfänger meist auf Weiß – also auch hier. */
  .paper {
    flex: 1;
    min-height: 200px;
    border-radius: var(--card-radius);
    background: #fff;
    color: #1d1d1f;
    box-shadow: inset 0 0 0 1px var(--separator);
    overflow: auto;
  }

  .surface {
    min-height: 100%;
    padding: 14px 16px;
    outline: none;
    font:
      14px/1.5 -apple-system,
      Helvetica,
      sans-serif;
    user-select: text;
    -webkit-user-select: text;
    overflow-wrap: anywhere;
  }

  .surface:focus-visible {
    box-shadow: inset 0 0 0 2px var(--accent);
    border-radius: var(--card-radius);
  }

  .surface:empty::before {
    content: attr(data-placeholder);
    color: #8a8a90;
    pointer-events: none;
  }

  .surface :global(img) {
    max-width: 100%;
    height: auto;
  }

  .surface :global(a) {
    color: #0a66d6;
  }

  .code-split {
    flex: 1;
    display: grid;
    grid-template-rows: minmax(120px, 1fr) minmax(120px, 1fr);
    gap: var(--space-3);
    min-height: 260px;
  }

  .code {
    width: 100%;
    padding: var(--space-3);
    border: 1px solid var(--field-border);
    border-radius: var(--card-radius);
    background: var(--field-bg);
    color: var(--text);
    font:
      12px/1.5 ui-monospace,
      'SF Mono',
      Menlo,
      monospace;
    resize: none;
    user-select: text;
    -webkit-user-select: text;
  }

  .code:focus {
    outline: none;
    border-color: var(--accent);
  }

  .preview {
    min-height: 0;
  }

  .tpl {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr);
    gap: var(--space-4);
  }

  .tpl-form {
    display: grid;
    grid-template-columns: 1fr 1fr;
    align-content: start;
    gap: var(--space-2) var(--space-3);
  }

  .f {
    display: grid;
    gap: 3px;
    min-width: 0;
  }

  .f.wide {
    grid-column: 1 / -1;
  }

  .f > span {
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
  }

  .f input {
    height: 28px;
  }

  .logo-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 28px;
  }

  .logo-thumb {
    max-width: 64px;
    max-height: 28px;
    border-radius: 4px;
    background: #fff;
  }

  .btn.small {
    height: 28px;
    padding: 0 var(--space-2);
    font-size: 12px;
  }

  .accents {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    min-height: 28px;
    align-items: center;
  }

  .accent {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--c);
    box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.12);
  }

  .accent.on {
    box-shadow:
      0 0 0 2px var(--sheet-bg),
      0 0 0 4px var(--c);
  }

  .tpl-gallery {
    display: grid;
    grid-template-columns: 1fr 1fr;
    align-content: start;
    gap: var(--space-2);
    max-height: 380px;
    overflow-y: auto;
    padding: 2px;
  }

  .tpl-card {
    display: grid;
    gap: 4px;
    padding: 6px;
    border-radius: var(--card-radius);
    text-align: left;
    background: var(--hover);
    transition: box-shadow var(--dur) var(--ease);
  }

  .tpl-card:hover {
    box-shadow: inset 0 0 0 1px var(--separator);
  }

  .tpl-card.on {
    box-shadow: 0 0 0 2px var(--accent);
  }

  .tpl-name {
    padding: 0 2px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .tpl-preview {
    display: block;
    height: 92px;
    border-radius: 7px;
    overflow: hidden;
    background: #fff;
  }

  .tpl-apply {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .hint-small {
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text-secondary);
  }

  .error {
    margin: 0;
    color: var(--danger);
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
  }

  .spacer {
    flex: 1;
  }

  .btn.delete {
    color: var(--danger);
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    color: var(--icon);
    text-align: center;
  }

  .empty h3 {
    margin: var(--space-2) 0 0;
    color: var(--text);
    font-size: 15px;
  }

  .empty p {
    max-width: 320px;
    margin: 0 0 var(--space-3);
    color: var(--text-secondary);
  }
</style>
