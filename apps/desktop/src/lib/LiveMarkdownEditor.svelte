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
  import { onMount } from 'svelte';

  interface Props {
    value: string;
    focusRequest?: number;
    onChange: (value: string) => void;
    onSave: () => void;
  }

  let { value, focusRequest = 0, onChange, onSave }: Props = $props();
  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let appliedFocusRequest = -1;

  const externalChange = Annotation.define<boolean>();

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
      ranges.push(Decoration.mark({ class: 'cm-live-link' }).range(start + 1, labelEnd));
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

  const editorTheme = EditorView.theme({
    '&': {
      height: '100%',
      color: '#c8d3d8',
      backgroundColor: 'transparent',
      fontSize: '13px'
    },
    '&.cm-focused': { outline: 'none' },
    '.cm-scroller': {
      overflow: 'auto',
      fontFamily: "'Inter Variable', sans-serif",
      lineHeight: '1.65',
      scrollbarColor: '#41464d transparent'
    },
    '.cm-content': { maxWidth: '920px', padding: '22px 28px 80px', caretColor: '#68d4c5' },
    '.cm-line': { padding: '0 2px' },
    '.cm-cursor': { borderLeftColor: '#68d4c5' },
    '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: '#19444b80' },
    '.cm-activeLine': { backgroundColor: '#13202755' },
    '.cm-live-heading': {
      color: '#e7eef0',
      fontFamily: "'Archivo Variable', sans-serif",
      fontWeight: '680',
      lineHeight: '1.18'
    },
    '.cm-live-heading .tok-heading': { color: 'inherit', textDecoration: 'none' },
    '.cm-live-h1': { fontSize: '26px', paddingTop: '14px', paddingBottom: '7px' },
    '.cm-live-h2': { fontSize: '20px', paddingTop: '12px', paddingBottom: '5px' },
    '.cm-live-h3': { fontSize: '16px', color: '#6ed8c9', paddingTop: '9px', paddingBottom: '3px' },
    '.cm-live-h4': { fontSize: '13px', textTransform: 'uppercase', letterSpacing: '.05em' },
    '.cm-live-h5, .cm-live-h6': { fontSize: '12px', color: '#a9bdc5' },
    '.cm-live-strong': { fontWeight: '720', color: '#e2eaed' },
    '.cm-live-emphasis': { fontStyle: 'italic', color: '#c8d6da' },
    '.cm-live-inline-code': {
      borderRadius: '3px',
      padding: '1px 4px',
      backgroundColor: '#102631',
      color: '#8fe3d6',
      fontFamily: "'JetBrains Mono Variable', monospace",
      fontSize: '.9em'
    },
    '.cm-live-link': { color: '#6ed8c9', textDecoration: 'underline', textUnderlineOffset: '3px' },
    '.cm-live-quote': {
      borderLeft: '2px solid #4fc4b6',
      paddingLeft: '13px',
      color: '#95aeb7',
      fontStyle: 'italic'
    },
    '.cm-live-list': { paddingLeft: '14px' },
    '.cm-live-bullet, .cm-live-order': {
      display: 'inline-block',
      minWidth: '18px',
      color: '#6ed8c9',
      fontFamily: "'JetBrains Mono Variable', monospace"
    },
    '.cm-live-fence': {
      borderLeft: '2px solid #274953',
      backgroundColor: '#09171d',
      color: '#7fa0a9',
      fontFamily: "'JetBrains Mono Variable', monospace"
    },
    '.cm-placeholder': { color: '#586970', fontStyle: 'italic' }
  });

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
          editorTheme,
          keymap.of([
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
</script>

<div class="editor-host" bind:this={host}></div>

<style>
  .editor-host { min-height: 0; height: 100%; overflow: hidden; background: #0c1419; }
</style>
