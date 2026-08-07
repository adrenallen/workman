<script lang="ts">
  import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
  import { defaultHighlightStyle, syntaxHighlighting } from '@codemirror/language';
  import { markdown } from '@codemirror/lang-markdown';
  import { Annotation, EditorState, type Range } from '@codemirror/state';
  import {
    Decoration,
    EditorView,
    ViewPlugin,
    WidgetType,
    drawSelection,
    dropCursor,
    highlightActiveLine,
    highlightSpecialChars,
    keymap,
    placeholder as editorPlaceholder,
    type DecorationSet,
    type ViewUpdate
  } from '@codemirror/view';
  import BoldIcon from '@lucide/svelte/icons/bold';
  import CheckSquare2Icon from '@lucide/svelte/icons/square-check-big';
  import Code2Icon from '@lucide/svelte/icons/code-2';
  import Heading2Icon from '@lucide/svelte/icons/heading-2';
  import Heading3Icon from '@lucide/svelte/icons/heading-3';
  import ItalicIcon from '@lucide/svelte/icons/italic';
  import ImageIcon from '@lucide/svelte/icons/image';
  import LinkIcon from '@lucide/svelte/icons/link';
  import ListIcon from '@lucide/svelte/icons/list';
  import ListOrderedIcon from '@lucide/svelte/icons/list-ordered';
  import MinusIcon from '@lucide/svelte/icons/minus';
  import StrikethroughIcon from '@lucide/svelte/icons/strikethrough';
  import TextQuoteIcon from '@lucide/svelte/icons/text-quote';
  import { onMount } from 'svelte';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import {
    EXTERNAL_LINK_TOOLTIP,
    markdownLinkAt,
    openExternalUrl
  } from './externalLinks';

  interface Props {
    value: string;
    focusRequest?: number;
    flow?: boolean;
    toolbar?: boolean;
    scrollRequest?: { key: number; line: number } | null;
    onChange: (value: string) => void;
    onSave: () => void;
    onViewportLineChange?: (line: number) => void;
  }

  let {
    value,
    focusRequest = 0,
    flow = false,
    toolbar = true,
    scrollRequest = null,
    onChange,
    onSave,
    onViewportLineChange
  }: Props = $props();
  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let appliedFocusRequest = -1;
  let appliedScrollRequest = -1;

  const externalChange = Annotation.define<boolean>();

  function replaceSelection(prefix: string, suffix = prefix, fallback = 'text'): void {
    if (!view) return;
    const selection = view.state.selection.main;
    const selected = view.state.sliceDoc(selection.from, selection.to);
    const content = selected || fallback;
    view.dispatch({
      changes: { from: selection.from, to: selection.to, insert: `${prefix}${content}${suffix}` },
      selection: {
        anchor: selection.from + prefix.length,
        head: selection.from + prefix.length + content.length
      },
      scrollIntoView: true
    });
    view.focus();
  }

  function prefixLines(prefix: string, heading = false): void {
    if (!view) return;
    const selection = view.state.selection.main;
    const firstLine = view.state.doc.lineAt(selection.from);
    const lastLine = view.state.doc.lineAt(selection.to);
    const source = view.state.sliceDoc(firstLine.from, lastLine.to);
    const next = source
      .split('\n')
      .map((line) => `${prefix}${heading ? line.replace(/^#{1,6}\s+/, '') : line}`)
      .join('\n');
    view.dispatch({
      changes: { from: firstLine.from, to: lastLine.to, insert: next },
      selection: { anchor: firstLine.from + next.length },
      scrollIntoView: true
    });
    view.focus();
  }

  function insertRule(): void {
    if (!view) return;
    const position = view.state.selection.main.head;
    const insert = `${position > 0 ? '\n\n' : ''}---\n\n`;
    view.dispatch({
      changes: { from: position, insert },
      selection: { anchor: position + insert.length },
      scrollIntoView: true
    });
    view.focus();
  }

  function reportViewportLine(editor: EditorView): void {
    onViewportLineChange?.(editor.state.doc.lineAt(editor.viewport.from).number);
  }

  class MarkerWidget extends WidgetType {
    readonly label: string;
    readonly className: string;

    constructor(label: string, className: string) {
      super();
      this.label = label;
      this.className = className;
    }

    toDOM(): HTMLElement {
      const marker = document.createElement('span');
      marker.className = this.className;
      marker.textContent = this.label;
      return marker;
    }

    eq(other: MarkerWidget): boolean {
      return this.label === other.label && this.className === other.className;
    }
  }

  function cursorTouches(view: EditorView, from: number, to: number): boolean {
    return view.state.selection.ranges.some((range) => range.head >= from && range.head <= to);
  }

  function replaceMarker(
    ranges: Range<Decoration>[],
    editor: EditorView,
    from: number,
    to: number,
    widget?: MarkerWidget
  ): void {
    if (cursorTouches(editor, from, to)) return;
    ranges.push(
      Decoration.replace(widget ? { widget, inclusive: false } : { inclusive: false }).range(from, to)
    );
  }

  function decorateInline(
    ranges: Range<Decoration>[],
    editor: EditorView,
    lineFrom: number,
    text: string
  ): void {
    for (const match of text.matchAll(/\*\*([^*\n]+)\*\*/g)) {
      const start = lineFrom + (match.index ?? 0);
      const end = start + match[0].length;
      replaceMarker(ranges, editor, start, start + 2);
      ranges.push(Decoration.mark({ class: 'cm-live-strong' }).range(start + 2, end - 2));
      replaceMarker(ranges, editor, end - 2, end);
    }
    for (const match of text.matchAll(/(?<!\*)\*([^*\n]+)\*(?!\*)/g)) {
      const start = lineFrom + (match.index ?? 0);
      const end = start + match[0].length;
      replaceMarker(ranges, editor, start, start + 1);
      ranges.push(Decoration.mark({ class: 'cm-live-emphasis' }).range(start + 1, end - 1));
      replaceMarker(ranges, editor, end - 1, end);
    }
    for (const match of text.matchAll(/`([^`\n]+)`/g)) {
      const start = lineFrom + (match.index ?? 0);
      const end = start + match[0].length;
      replaceMarker(ranges, editor, start, start + 1);
      ranges.push(Decoration.mark({ class: 'cm-live-inline-code' }).range(start + 1, end - 1));
      replaceMarker(ranges, editor, end - 1, end);
    }
    for (const match of text.matchAll(/\[([^\]\n]+)\]\(([^)\n]+)\)/g)) {
      const start = lineFrom + (match.index ?? 0);
      const labelEnd = start + match[1].length + 1;
      const end = start + match[0].length;
      replaceMarker(ranges, editor, start, start + 1);
      ranges.push(
        Decoration.mark({
          attributes: { title: EXTERNAL_LINK_TOOLTIP },
          class: 'cm-live-link'
        }).range(start + 1, labelEnd)
      );
      replaceMarker(ranges, editor, labelEnd, end);
    }
  }

  function liveDecorations(editor: EditorView): DecorationSet {
    const ranges: Range<Decoration>[] = [];
    for (const { from, to } of editor.visibleRanges) {
      let position = from;
      while (position <= to) {
        const line = editor.state.doc.lineAt(position);
        const heading = /^(#{1,6})(\s+)/.exec(line.text);
        const quote = /^(\s*>)(\s?)/.exec(line.text);
        const bullet = /^(\s*)([-+*])(\s+)/.exec(line.text);
        const ordered = /^(\s*)(\d+\.)(\s+)/.exec(line.text);
        const fence = /^(\s*```)/.exec(line.text);

        if (heading) {
          const markerEnd = line.from + heading[0].length;
          ranges.push(
            Decoration.line({ class: `cm-live-heading cm-live-h${heading[1].length}` }).range(line.from)
          );
          replaceMarker(ranges, editor, line.from, markerEnd);
          decorateInline(ranges, editor, markerEnd, line.text.slice(heading[0].length));
        } else if (quote) {
          const markerEnd = line.from + quote[0].length;
          ranges.push(Decoration.line({ class: 'cm-live-quote' }).range(line.from));
          replaceMarker(ranges, editor, line.from, markerEnd);
          decorateInline(ranges, editor, markerEnd, line.text.slice(quote[0].length));
        } else if (bullet) {
          const markerStart = line.from + bullet[1].length;
          const markerEnd = line.from + bullet[0].length;
          ranges.push(Decoration.line({ class: 'cm-live-list' }).range(line.from));
          replaceMarker(
            ranges,
            editor,
            markerStart,
            markerEnd,
            new MarkerWidget('•', 'cm-live-bullet')
          );
          decorateInline(ranges, editor, markerEnd, line.text.slice(bullet[0].length));
        } else if (ordered) {
          const markerStart = line.from + ordered[1].length;
          const markerEnd = line.from + ordered[0].length;
          ranges.push(Decoration.line({ class: 'cm-live-list' }).range(line.from));
          replaceMarker(
            ranges,
            editor,
            markerStart,
            markerEnd,
            new MarkerWidget(ordered[2], 'cm-live-order')
          );
          decorateInline(ranges, editor, markerEnd, line.text.slice(ordered[0].length));
        } else if (fence) {
          ranges.push(Decoration.line({ class: 'cm-live-fence' }).range(line.from));
          replaceMarker(ranges, editor, line.from, line.from + fence[0].length);
        } else {
          decorateInline(ranges, editor, line.from, line.text);
        }

        if (line.to >= to || line.number === editor.state.doc.lines) break;
        position = line.to + 1;
      }
    }
    return Decoration.set(ranges, true);
  }

  const liveMarkdown = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      constructor(editor: EditorView) {
        this.decorations = liveDecorations(editor);
      }

      update(update: ViewUpdate): void {
        if (update.docChanged || update.selectionSet || update.viewportChanged) {
          this.decorations = liveDecorations(update.view);
        }
      }
    },
    { decorations: (plugin) => plugin.decorations }
  );

  function createEditorTheme(flowLayout: boolean) {
    return EditorView.theme({
    '&': {
      height: flowLayout ? 'auto' : '100%',
      minHeight: flowLayout ? 'inherit' : '0',
      color: 'var(--foreground)',
      backgroundColor: 'transparent',
      fontSize: '13px'
    },
    '&.cm-focused': { outline: 'none' },
    '.cm-scroller': {
      overflow: flowLayout ? 'visible' : 'auto',
      fontFamily: "'Inter Variable', sans-serif",
      lineHeight: '1.65',
      scrollbarColor: 'var(--border-strong) transparent'
    },
    '.cm-content': {
      maxWidth: '920px',
      padding: flowLayout ? '20px 4px 48px' : '22px 28px 80px',
      caretColor: 'var(--ring)'
    },
    '.cm-line': { padding: '0 2px' },
    '.cm-cursor': { borderLeftColor: 'var(--ring)' },
    '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: 'color-mix(in srgb, var(--ring) 22%, transparent)' },
    '.cm-activeLine': { backgroundColor: 'color-mix(in srgb, var(--accent) 55%, transparent)' },
    '.cm-live-heading': {
      color: 'var(--foreground)',
      fontFamily: "'Archivo Variable', sans-serif",
      fontWeight: '680',
      lineHeight: '1.18'
    },
    '.cm-live-heading .tok-heading': { color: 'inherit', textDecoration: 'none' },
    '.cm-live-h1': { fontSize: '26px', paddingTop: '14px', paddingBottom: '7px' },
    '.cm-live-h2': { fontSize: '20px', paddingTop: '12px', paddingBottom: '5px' },
    '.cm-live-h3': { fontSize: '16px', color: 'var(--text-soft)', paddingTop: '9px', paddingBottom: '3px' },
    '.cm-live-h4': { fontSize: '13px', textTransform: 'uppercase', letterSpacing: '.05em' },
    '.cm-live-h5, .cm-live-h6': { fontSize: '12px', color: 'var(--muted-foreground)' },
    '.cm-live-strong': { fontWeight: '720', color: 'var(--foreground)' },
    '.cm-live-emphasis': { fontStyle: 'italic', color: 'var(--text-soft)' },
    '.cm-live-inline-code': {
      borderRadius: '3px',
      padding: '1px 4px',
      backgroundColor: 'var(--card)',
      color: 'var(--foreground)',
      fontFamily: "'JetBrains Mono Variable', monospace",
      fontSize: '.9em'
    },
    '.cm-live-link': { color: 'var(--ring)', textDecoration: 'underline', textUnderlineOffset: '3px' },
    '.cm-live-quote': {
      borderLeft: '2px solid var(--border-strong)',
      paddingLeft: '13px',
      color: 'var(--muted-foreground)',
      fontStyle: 'italic'
    },
    '.cm-live-list': { paddingLeft: '14px' },
    '.cm-live-bullet, .cm-live-order': {
      display: 'inline-block',
      minWidth: '18px',
      color: 'var(--muted-foreground)',
      fontFamily: "'JetBrains Mono Variable', monospace"
    },
    '.cm-live-fence': {
      borderLeft: '2px solid var(--border-strong)',
      backgroundColor: 'var(--background)',
      color: 'var(--muted-foreground)',
      fontFamily: "'JetBrains Mono Variable', monospace"
    },
    '.cm-placeholder': { color: 'var(--muted-foreground)', fontStyle: 'italic' }
    });
  }

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          EditorState.tabSize.of(2),
          history(),
          drawSelection(),
          dropCursor(),
          highlightActiveLine(),
          highlightSpecialChars(),
          markdown(),
          syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
          EditorView.lineWrapping,
          editorPlaceholder('Start writing Markdown…'),
          liveMarkdown,
          createEditorTheme(flow),
          EditorView.domEventHandlers({
            click: (event, editor) => {
              if (!event.metaKey || event.button !== 0) return false;
              const position = editor.posAtCoords({ x: event.clientX, y: event.clientY });
              if (position === null) return false;
              const href = markdownLinkAt(editor.state.doc.toString(), position);
              if (!href) return false;
              event.preventDefault();
              openExternalUrl(href);
              return true;
            }
          }),
          keymap.of([
            {
              key: 'Mod-b',
              run: () => {
                replaceSelection('**');
                return true;
              }
            },
            {
              key: 'Mod-i',
              run: () => {
                replaceSelection('*');
                return true;
              }
            },
            {
              key: 'Mod-s',
              run: () => {
                onSave();
                return true;
              }
            },
            ...defaultKeymap,
            ...historyKeymap,
            indentWithTab
          ]),
          EditorView.updateListener.of((update) => {
            if (update.docChanged || update.viewportChanged || update.geometryChanged) {
              reportViewportLine(update.view);
            }
            if (
              update.docChanged &&
              !update.transactions.some((transaction) => transaction.annotation(externalChange))
            ) {
              onChange(update.state.doc.toString());
            }
          })
        ]
      })
    });
    if (focusRequest > 0) {
      appliedFocusRequest = focusRequest;
      queueMicrotask(() => view?.focus());
    }
    queueMicrotask(() => {
      if (view) reportViewportLine(view);
    });
    return () => {
      view?.destroy();
      view = null;
    };
  });

  $effect(() => {
    const next = value;
    if (!view || view.state.doc.toString() === next) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: next },
      annotations: externalChange.of(true)
    });
  });

  $effect(() => {
    const request = focusRequest;
    if (!view || request <= appliedFocusRequest) return;
    appliedFocusRequest = request;
    queueMicrotask(() => view?.focus());
  });

  $effect(() => {
    const request = scrollRequest;
    if (!view || !request || request.key <= appliedScrollRequest) return;
    appliedScrollRequest = request.key;
    const lineNumber = Math.max(1, Math.min(request.line, view.state.doc.lines));
    const line = view.state.doc.line(lineNumber);
    view.dispatch({
      effects: EditorView.scrollIntoView(line.from, { y: 'start', yMargin: 24 })
    });
    requestAnimationFrame(() => {
      if (view) reportViewportLine(view);
    });
  });
</script>

<div class="editor-shell" class:flow class:with-toolbar={toolbar}>
  {#if toolbar}
    <div class="format-toolbar" aria-label="Markdown formatting">
      <IconButton label="Bold" shortcut="⌘B" onclick={() => replaceSelection('**')}>
        {#snippet icon()}<BoldIcon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Italic" shortcut="⌘I" onclick={() => replaceSelection('*')}>
        {#snippet icon()}<ItalicIcon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Strikethrough" onclick={() => replaceSelection('~~')}>
        {#snippet icon()}<StrikethroughIcon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Inline code" onclick={() => replaceSelection('`')}>
        {#snippet icon()}<Code2Icon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Link" onclick={() => replaceSelection('[', '](https://)', 'link text')}>
        {#snippet icon()}<LinkIcon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <span class="separator" aria-hidden="true"></span>
      <IconButton label="Heading 2" onclick={() => prefixLines('## ', true)}>
        {#snippet icon()}<Heading2Icon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Heading 3" onclick={() => prefixLines('### ', true)}>
        {#snippet icon()}<Heading3Icon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Bulleted list" onclick={() => prefixLines('- ')}>
        {#snippet icon()}<ListIcon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Numbered list" onclick={() => prefixLines('1. ')}>
        {#snippet icon()}<ListOrderedIcon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Checklist" onclick={() => prefixLines('- [ ] ')}>
        {#snippet icon()}<CheckSquare2Icon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Quote" onclick={() => prefixLines('> ')}>
        {#snippet icon()}<TextQuoteIcon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Embed media" onclick={() => replaceSelection('![', '](https://)', 'description')}>
        {#snippet icon()}<ImageIcon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
      <IconButton label="Horizontal rule" onclick={insertRule}>
        {#snippet icon()}<MinusIcon size={14} strokeWidth={1.8} />{/snippet}
      </IconButton>
    </div>
  {/if}
  <div class="editor-host" bind:this={host}></div>
</div>

<style>
  .editor-shell { display: grid; height: 100%; min-height: 0; grid-template-rows: minmax(0, 1fr); overflow: hidden; background: var(--background); }
  .editor-shell.with-toolbar { grid-template-rows: auto minmax(0, 1fr); }
  .editor-shell.flow { height: auto; min-height: clamp(380px, calc(100vh - 290px), 640px); grid-template-rows: minmax(0, auto); overflow: visible; background: transparent; }
  .editor-shell.flow.with-toolbar { grid-template-rows: auto minmax(0, auto); }
  .editor-shell.flow .format-toolbar { background: transparent; }
  .format-toolbar { display: flex; min-width: 0; min-height: 34px; align-items: center; gap: 1px; overflow-x: auto; border-bottom: 1px solid var(--border); padding: 2px 5px; background: var(--card); scrollbar-width: none; }
  .separator { width: 1px; height: 18px; flex: none; margin: 0 3px; background: var(--border); }
  .editor-host { min-height: 0; height: 100%; overflow: hidden; background: var(--background); }
  .editor-shell.flow .editor-host { height: auto; min-height: inherit; overflow: visible; background: transparent; }
</style>
