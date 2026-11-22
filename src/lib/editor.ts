/**
 * Rich-Text-Editor (Tiptap/ProseMirror). Wird nur beim Verfassen per
 * dynamischem import() geladen – im Leerlauf belegt er keinen Speicher.
 */
import { Editor, Node } from '@tiptap/core';
import Placeholder from '@tiptap/extension-placeholder';
import StarterKit from '@tiptap/starter-kit';

export type { Editor };

/**
 * Signatur als festes Element im Text: zeigt beliebiges (in Rust gesäubertes)
 * HTML unverändert an – Tabellen, Farben, Logos –, statt es in die einfachen
 * Absätze des Editors umzubauen. Löschen geht, Bearbeiten nur in den Signaturen.
 */
const SignatureNode = Node.create({
  name: 'signature',
  group: 'block',
  atom: true,
  selectable: true,
  draggable: false,
  addAttributes() {
    return { id: { default: '' }, html: { default: '' } };
  },
  parseHTML() {
    return [
      {
        tag: 'div[data-kolibri-signature]',
        getAttrs: (el) => ({
          id: el.getAttribute('data-kolibri-signature') ?? '',
          html: el.innerHTML,
        }),
      },
    ];
  },
  renderHTML({ node }) {
    const div = document.createElement('div');
    div.setAttribute('data-kolibri-signature', String(node.attrs.id));
    div.innerHTML = String(node.attrs.html);
    return div;
  },
  addNodeView() {
    return ({ node }) => {
      const dom = document.createElement('div');
      dom.className = 'kolibri-signature';
      dom.contentEditable = 'false';
      dom.innerHTML = String(node.attrs.html);
      return { dom };
    };
  },
});

/**
 * Signatur austauschen (oder mit `null` entfernen). Ohne vorhandene Signatur
 * landet sie vor dem Zitat bzw. am Ende.
 */
export function setSignature(editor: Editor, sig: { id: number; html: string } | null): void {
  const { state } = editor;
  let existing: { pos: number; size: number } | null = null;
  let quotePos: number | null = null;
  state.doc.forEach((child, offset) => {
    if (child.type.name === 'signature' && !existing)
      existing = { pos: offset, size: child.nodeSize };
    if (child.type.name === 'blockquote' && quotePos === null) quotePos = offset;
  });
  const node = sig?.html.trim()
    ? state.schema.nodes['signature']?.create({ id: String(sig.id), html: sig.html })
    : null;
  let tr = state.tr;
  const found = existing as { pos: number; size: number } | null;
  if (found) {
    tr = node
      ? tr.replaceWith(found.pos, found.pos + found.size, node)
      : tr.delete(found.pos, found.pos + found.size);
  } else if (node) {
    // Vor dem Zitat steht meist „Am … schrieb …:“ – die Signatur gehört davor.
    let at = quotePos ?? state.doc.content.size;
    if (quotePos !== null && quotePos > 0) {
      const before = state.doc.resolve(quotePos).nodeBefore;
      if (before?.type.name === 'paragraph' && /schrieb|weitergeleitet/i.test(before.textContent))
        at = quotePos - before.nodeSize;
    }
    tr = tr.insert(at, node);
  }
  editor.view.dispatch(tr);
}

export function createEditor(options: {
  element: HTMLElement;
  html: string;
  placeholder: string;
  label: string;
  onUpdate: () => void;
  onSelection: () => void;
}): Editor {
  return new Editor({
    element: options.element,
    content: options.html,
    extensions: [
      StarterKit.configure({
        heading: false,
        codeBlock: false,
        horizontalRule: false,
        link: {
          openOnClick: false,
          autolink: true,
          protocols: ['mailto'],
          HTMLAttributes: { rel: 'noopener noreferrer', target: null },
        },
      }),
      Placeholder.configure({ placeholder: options.placeholder }),
      SignatureNode,
    ],
    editorProps: {
      attributes: { 'aria-label': options.label, role: 'textbox', 'aria-multiline': 'true' },
    },
    onUpdate: options.onUpdate,
    onSelectionUpdate: options.onSelection,
    onTransaction: options.onSelection,
  });
}
