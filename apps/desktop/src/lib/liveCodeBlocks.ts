import { StateField, type Range } from '@codemirror/state';
import { Decoration, EditorView, WidgetType } from '@codemirror/view';
import { writeClipboardText } from './clipboard';
import { markdownCodeBlocks, type MarkdownCodeBlock } from './markdownCodeBlocks';

export const codeBlocks = StateField.define<MarkdownCodeBlock[]>({
  create: state => markdownCodeBlocks(state.doc.toString()),
  update: (blocks, transaction) => transaction.docChanged
    ? markdownCodeBlocks(transaction.newDoc.toString()) : blocks
});

class CodeBlockTools extends WidgetType {
  private timer: ReturnType<typeof setTimeout> | undefined;
  constructor(readonly block: MarkdownCodeBlock, readonly content: string, readonly label: boolean) { super(); }
  eq(other: CodeBlockTools): boolean {
    return this.block.from === other.block.from && this.block.contentFrom === other.block.contentFrom
      && this.content === other.content && this.block.language === other.block.language && this.label === other.label;
  }
  toDOM(editor: EditorView): HTMLElement {
    const root = document.createElement('span');
    root.className = this.label ? 'cm-code-tools' : 'cm-code-tools cm-code-tools-editing';
    root.contentEditable = 'false';
    if (this.label) {
      const label = root.appendChild(document.createElement('span'));
      label.className = 'cm-code-language';
      label.textContent = this.block.language;
    }
    const actions = root.appendChild(document.createElement('span'));
    actions.className = 'cm-code-actions';
    const button = (label: string, title: string, action: () => void) => {
      const element = actions.appendChild(document.createElement('button'));
      element.type = 'button';
      element.textContent = label;
      element.setAttribute('aria-label', title);
      element.title = title;
      element.onmousedown = event => event.preventDefault();
      element.onclick = event => { event.preventDefault(); event.stopPropagation(); action(); };
      return element;
    };
    button('Edit', 'Edit code block', () => {
      editor.dispatch({ selection: { anchor: this.block.contentFrom } });
      editor.focus();
    });
    const copy = button('Copy', 'Copy code block', () => {
      void writeClipboardText(this.content).then(() => {
        copy.textContent = 'Copied';
        copy.setAttribute('aria-label', 'Code copied');
        clearTimeout(this.timer);
        this.timer = setTimeout(() => { copy.textContent = 'Copy'; copy.setAttribute('aria-label', 'Copy code block'); }, 1600);
      }).catch(() => {
        copy.textContent = 'Retry copy';
        copy.setAttribute('aria-label', 'Copy failed. Retry copying code');
      });
    });
    return root;
  }
  ignoreEvent(): boolean { return true; }
  destroy(): void { clearTimeout(this.timer); }
}

export function decorateCodeLine(editor: EditorView, lineFrom: number, ranges: Range<Decoration>[]): boolean {
  const block = editor.state.field(codeBlocks).find(block => lineFrom >= block.from && lineFrom <= block.to);
  if (!block) return false;
  const opening = lineFrom === block.from;
  const closing = lineFrom === block.closingFrom;
  ranges.push(Decoration.line({ class: opening ? 'cm-code-header' : closing ? 'cm-code-footer' : 'cm-code-line' }).range(lineFrom));
  if (opening) {
    const editingFence = editor.state.selection.ranges.some(range => range.head >= block.from && range.head <= block.openingTo);
    const widget = new CodeBlockTools(block, editor.state.sliceDoc(block.contentFrom, block.contentTo), !editingFence);
    ranges.push(editingFence
      ? Decoration.widget({ widget, side: 1 }).range(block.openingTo)
      : Decoration.replace({ widget, inclusive: false }).range(block.from, block.openingTo));
  } else if (closing && !editor.state.selection.ranges.some(range => range.head >= block.closingFrom! && range.head <= block.to)) {
    ranges.push(Decoration.replace({ inclusive: false }).range(block.closingFrom!, block.to));
  }
  return true;
}

export const codeBlockTheme = EditorView.theme({
  '.cm-line.cm-code-header, .cm-line.cm-code-line, .cm-line.cm-code-footer': {
    backgroundColor: 'var(--card)', borderLeft: '1px solid var(--border)', borderRight: '1px solid var(--border)',
    fontFamily: "'JetBrains Mono Variable', monospace", transition: 'background-color 140ms ease, border-color 140ms ease'
  },
  '.cm-line.cm-code-header': { borderTop: '1px solid var(--border)', borderRadius: '7px 7px 0 0', padding: '4px 10px', minHeight: '36px', color: 'var(--muted-foreground)' },
  '.cm-line.cm-code-line': { padding: '0 12px', tabSize: '2' },
  '.cm-line.cm-code-footer': { borderBottom: '1px solid var(--border)', borderRadius: '0 0 7px 7px', padding: '0 12px', minHeight: '22px', color: 'var(--muted-foreground)' },
  '.cm-code-tools': { display: 'inline-flex', width: '100%', alignItems: 'center', justifyContent: 'space-between', verticalAlign: 'middle', fontFamily: 'var(--ui-font-family)', fontSize: 'var(--font-size-xs)', userSelect: 'none' },
  '.cm-code-tools-editing': { width: 'auto', marginLeft: '12px' },
  '.cm-code-actions': { display: 'inline-flex', gap: '4px', marginLeft: 'auto' },
  '.cm-code-actions button': { border: '1px solid var(--border)', borderRadius: '4px', padding: '3px 9px', minHeight: '28px', background: 'transparent', color: 'var(--muted-foreground)', cursor: 'pointer', transition: 'background-color 140ms ease, color 140ms ease' },
  '.cm-code-actions button:hover': { background: 'var(--accent)', color: 'var(--foreground)' },
  '.cm-code-actions button:focus-visible': { outline: '2px solid var(--ring)', outlineOffset: '1px' },
  '@media (prefers-reduced-motion: reduce)': { '.cm-line.cm-code-header, .cm-line.cm-code-line, .cm-line.cm-code-footer, .cm-code-actions button': { transition: 'none' } }
});
