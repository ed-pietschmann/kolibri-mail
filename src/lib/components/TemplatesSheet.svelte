<script lang="ts">
  import { mod } from '../platform';
  import { tick } from 'svelte';
  import { api, ApiError, CONTENT_PLACEHOLDER, type Template } from '../api';
  import { describeError } from '../errors';
  import { t } from '../i18n';
  import { hasContentSlot, library, loadLibrary, TEMPLATE_SKELETON } from '../library.svelte';
  import HtmlPreview from './HtmlPreview.svelte';
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';

  /**
   * Vorlagen: mitgelieferte Designs (nur ansehen/kopieren) und eigene aus
   * HTML-Code. Mit {{inhalt}} wird der geschriebene Text eingesetzt, ohne ist
   * die Vorlage eine fertige Mail.
   */
  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  /** Beispieltext für die Vorschau. */
  const SAMPLE =
    '<p>Hallo Anna,</p><p>hier steht dein Text – genau so, wie du ihn im Verfassen-Fenster ' +
    'schreibst.</p><p>Viele Grüße<br>Eddie</p>';

  let editing = $state<Template | null>(null);
  let dirty = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmDelete = $state(false);
  let preview = $state('');
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  let nameInput: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (!open) return;
    error = null;
    void loadLibrary(true).then(() => select(library.templates[0] ?? null));
  });

  function sample(html: string): string {
    return html
      .replaceAll(CONTENT_PLACEHOLDER, SAMPLE)
      .replaceAll('{{name}}', 'Eddie Beispiel')
      .replaceAll('{{email}}', 'eddie@beispiel.de')
      .replaceAll('{{betreff}}', 'Neuigkeiten aus dem Projekt');
  }

  async function select(tpl: Template | null) {
    if (editing && dirty && !editing.builtin && !(await save())) return;
    editing = tpl ? { ...tpl } : null;
    preview = tpl ? sample(tpl.html) : '';
    dirty = false;
    confirmDelete = false;
    error = null;
  }

  async function create(from?: Template) {
    await select({
      id: 0,
      name: from ? t('templates.copyName', { name: from.name }) : t('templates.newName'),
      html: from?.html ?? TEMPLATE_SKELETON,
      builtin: false,
    });
    dirty = true;
    await tick();
    nameInput?.select();
  }

  function onCode() {
    dirty = true;
    clearTimeout(previewTimer);
    previewTimer = setTimeout(() => (preview = editing ? sample(editing.html) : ''), 250);
  }

  async function save(): Promise<boolean> {
    if (!editing || editing.builtin) return false;
    busy = true;
    error = null;
    try {
      const saved = await api.saveTemplate($state.snapshot(editing));
      await loadLibrary(true);
      editing = { ...saved };
      dirty = false;
      return true;
    } catch (err) {
      error = err instanceof ApiError && err.kind === 'invalid' ? err.detail : describeError(err);
      return false;
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!editing || editing.builtin) return;
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    try {
      if (editing.id) await api.deleteTemplate(editing.id);
      await loadLibrary(true);
      dirty = false;
      editing = null;
      await select(library.templates[0] ?? null);
    } catch (err) {
      error = describeError(err);
    }
  }

  function close() {
    if (dirty && editing && !editing.builtin) void save().then((ok) => ok && onclose());
    else onclose();
  }
</script>

<Sheet {open} label={t('templates.title')} width={940} onclose={close}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="layout"
    onkeydown={(e) => {
      if (mod(e) && e.key.toLowerCase() === 's') {
        e.preventDefault();
        void save();
      }
    }}
  >
    <aside class="list">
      <h2>{t('templates.title')}</h2>
      <p class="hint">{t('templates.subtitle')}</p>
      <ul>
        {#each library.templates as tpl (tpl.id)}
          <li>
            <button
              class="card"
              class:selected={editing?.id === tpl.id}
              aria-current={editing?.id === tpl.id}
              onclick={() => void select(tpl)}
            >
              <span class="thumb">
                <HtmlPreview html={sample(tpl.html)} title={tpl.name} scale={0.28} />
              </span>
              <span class="meta">
                <span class="name">{tpl.name}</span>
                <span class="kind">
                  {tpl.builtin
                    ? t('templates.builtin')
                    : hasContentSlot(tpl)
                      ? t('templates.own')
                      : t('templates.fixed')}
                </span>
              </span>
            </button>
          </li>
        {/each}
        {#if editing && editing.id === 0}
          <li>
            <button class="card selected" aria-current="true">
              <span class="thumb">
                <HtmlPreview html={preview} title={editing.name} scale={0.28} />
              </span>
              <span class="meta">
                <span class="name">{editing.name || t('templates.newName')}</span>
                <span class="kind">{t('templates.unsaved')}</span>
              </span>
            </button>
          </li>
        {/if}
      </ul>
      <button class="add" onclick={() => void create()}>
        <Icon name="plus" size={14} />
        {t('templates.add')}
      </button>
    </aside>

    <section class="edit">
      {#if editing}
        <div class="top">
          {#if editing.builtin}
            <h3>{editing.name}</h3>
            <span class="pill">{t('templates.builtin')}</span>
            <span class="spacer"></span>
            <button class="btn" onclick={() => editing && void create(editing)}>
              {t('templates.copy')}
            </button>
          {:else}
            <input
              bind:this={nameInput}
              class="name-input"
              type="text"
              bind:value={editing.name}
              oninput={() => (dirty = true)}
              aria-label={t('templates.name')}
              placeholder={t('templates.name')}
            />
          {/if}
        </div>

        <div class="split">
          <div class="pane">
            <span class="pane-label">
              <Icon name="code" size={13} />
              {t('templates.code')}
            </span>
            <textarea
              class="code"
              bind:value={editing.html}
              oninput={onCode}
              readonly={editing.builtin}
              spellcheck="false"
              aria-label={t('templates.code')}></textarea>
          </div>
          <div class="pane">
            <span class="pane-label">
              <Icon name="eye" size={13} />
              {t('compose.preview')}
            </span>
            <div class="paper">
              <HtmlPreview html={preview} title={t('compose.preview')} />
            </div>
          </div>
        </div>

        <p class="placeholders">
          {#if hasContentSlot(editing)}
            {t('templates.slotHint')}
          {:else}
            {t('templates.fixedHint')}
          {/if}
          <code>{'{{inhalt}}'}</code> · <code>{'{{name}}'}</code> · <code>{'{{email}}'}</code> ·
          <code>{'{{betreff}}'}</code>
        </p>

        {#if error}<p class="error" role="alert">{error}</p>{/if}

        <div class="buttons">
          {#if !editing.builtin}
            <button
              class="btn {confirmDelete ? 'danger' : 'delete'}"
              onclick={() => void remove()}
              disabled={busy}
            >
              {confirmDelete ? t('signature.deleteConfirm') : t('templates.delete')}
            </button>
          {/if}
          <span class="spacer"></span>
          <button class="btn" onclick={close}>{t('signature.close')}</button>
          {#if !editing.builtin}
            <button class="btn primary" onclick={() => void save()} disabled={busy || !dirty}>
              {t('signature.save')}
            </button>
          {/if}
        </div>
      {/if}
    </section>
  </div>
</Sheet>

<style>
  .layout {
    display: grid;
    grid-template-columns: 236px minmax(0, 1fr);
    gap: var(--space-6);
    height: min(640px, calc(100vh - var(--titlebar-h) - 80px));
    margin: calc(var(--space-6) * -1);
  }

  .list {
    display: flex;
    flex-direction: column;
    min-height: 0;
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

  .hint {
    margin: 2px var(--space-2) var(--space-4);
    color: var(--text-tertiary);
    font-size: 12px;
  }

  ul {
    flex: 1;
    min-height: 0;
    display: grid;
    align-content: start;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }

  .card {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-2);
    border-radius: var(--card-radius);
    text-align: left;
  }

  .card:hover {
    background: var(--hover);
  }

  .card.selected {
    background: var(--sidebar-selected);
    box-shadow: var(--sidebar-selected-shadow);
  }

  .thumb {
    flex-shrink: 0;
    width: 64px;
    height: 72px;
    border-radius: 6px;
    overflow: hidden;
    background: #fff;
    box-shadow: 0 0 0 1px var(--separator);
  }

  .meta {
    display: grid;
    min-width: 0;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }

  .kind {
    color: var(--text-tertiary);
    font-size: 12px;
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
    min-height: 0;
    padding: var(--space-6) var(--space-6) var(--space-6) 0;
  }

  .top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 32px;
  }

  h3 {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
  }

  .pill {
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--active);
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 600;
  }

  .name-input {
    flex: 1;
    font-size: 15px;
    font-weight: 600;
  }

  .split {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: var(--space-3);
  }

  .pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 0;
  }

  .pane-label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-tertiary);
    font-size: 12px;
    font-weight: 600;
  }

  .code {
    flex: 1;
    min-height: 0;
    width: 100%;
    padding: var(--space-3);
    border: 1px solid var(--field-border);
    border-radius: var(--card-radius);
    background: var(--field-bg);
    color: var(--text);
    font:
      11.5px/1.5 ui-monospace,
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

  .code[readonly] {
    color: var(--text-secondary);
  }

  .paper {
    flex: 1;
    min-height: 0;
    border-radius: var(--card-radius);
    overflow: hidden;
    background: #fff;
    box-shadow: inset 0 0 0 1px var(--separator);
  }

  .placeholders {
    margin: 0;
    color: var(--text-secondary);
    font-size: 12px;
  }

  code {
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--hover);
    font:
      11px ui-monospace,
      'SF Mono',
      Menlo,
      monospace;
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
</style>
