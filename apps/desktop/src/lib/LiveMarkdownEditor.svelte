<script lang="ts">
  import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
  import { defaultHighlightStyle, syntaxHighlighting } from '@codemirror/language';
  import { markdown } from '@codemirror/lang-markdown';
  import { Annotation, EditorState, StateEffect, StateField, type Range } from '@codemirror/state';
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
  import { invoke } from '@tauri-apps/api/core';
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
  import type { ScratchpadComment } from './coordination';
  import {
    EXTERNAL_LINK_TOOLTIP,
    markdownLinkAt,
    openExternalUrl
  } from './externalLinks';
  import { primaryModifier } from './primaryModifier';
  import {
    resolveScratchpadAnchor,
    selectionAnchor,
    type PositionMapper,
    type ScratchpadSelectionAnchor
  } from './scratchpadAnchors';
  import { scratchpadLocalImagePath, scratchpadMarkdownImages } from './scratchpadImages';

  interface Props {
    value: string;
    focusRequest?: number;
    flow?: boolean;
    toolbar?: boolean;
    scrollRequest?: { key: number; line: number } | null;
    commentScrollRequest?: { key: number; commentId: number; fallbackLine?: number | null } | null;
    comments?: ScratchpadComment[];
    showResolvedComments?: boolean;
    focusedCommentId?: number | null;
    onChange: (value: string, changes: PositionMapper) => void;
    onSave: () => void;
    onViewportLineChange?: (line: number) => void;
    onCommentSelection?: (anchor: ScratchpadSelectionAnchor) => void;
    onCommentClick?: (commentId: number, anchor: HTMLElement) => void;
  }

  let {
    value,
    focusRequest = 0,
    flow = false,
    toolbar = true,
    scrollRequest = null,
    commentScrollRequest = null,
    comments = [],
    showResolvedComments = false,
    focusedCommentId = null,
    onChange,
    onSave,
    onViewportLineChange,
    onCommentSelection,
    onCommentClick
  }: Props = $props();
  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let appliedFocusRequest = -1;
  let appliedScrollRequest = -1;
  let appliedCommentScrollRequest = -1;
  let selectionAction = $state<({ x: number; y: number } & ScratchpadSelectionAnchor) | null>(null);
  let imageSourceDisposed = false;
  const imageSourceRequests = new Map<string, Promise<string>>();
  const imageObjectUrls = new Set<string>();

  const externalChange = Annotation.define<boolean>();
  const setCommentDecorations = StateEffect.define<DecorationSet>();
  const commentDecorationField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(decorations, transaction) {
      let next = decorations.map(transaction.changes);
      for (const effect of transaction.effects) {
        if (effect.is(setCommentDecorations)) next = effect.value;
      }
      return next;
    },
    provide: (field) => EditorView.decorations.from(field)
  });

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

  function commentDecorations(editor: EditorView): DecorationSet {
    const ranges: Range<Decoration>[] = [];
    const content = editor.state.doc.toString();
    const groups = new Map<string, {
      from: number;
      to: number;
      comments: ScratchpadComment[];
    }>();
    for (const comment of comments) {
      if (comment.resolved && !showResolvedComments) continue;
      const resolved = resolveScratchpadAnchor(content, comment);
      const from = resolved.current_start;
      const to = resolved.current_end;
      if (resolved.anchor_state !== 'anchored' || from === null || to === null) continue;
      if (from < 0 || to <= from || to > editor.state.doc.length) continue;
      const key = `${from}:${to}`;
      const group = groups.get(key);
      if (group) group.comments.push(comment);
      else groups.set(key, { from, to, comments: [comment] });
    }
    for (const { from, to, comments: groupedComments } of groups.values()) {
      const first = groupedComments[0];
      const ids = groupedComments.map((comment) => comment.id);
      const allResolved = groupedComments.every((comment) => comment.resolved);
      const excerpt = first.body.replaceAll(/\s+/g, ' ').slice(0, 120);
      const attributes = {
        'data-scratchpad-comment-id': String(first.id),
        'data-scratchpad-comment-ids': ids.join(','),
        title: groupedComments.length === 1
          ? `${first.actor}: ${excerpt}`
          : `${groupedComments.length} comments on this text`
      };
      ranges.push(
        Decoration.mark({
          class: [
            'cm-comment-highlight',
            allResolved ? 'cm-comment-resolved' : '',
            groupedComments.some((comment) => focusedCommentId === comment.id) ? 'cm-comment-focused' : ''
          ].filter(Boolean).join(' '),
          attributes,
          inclusive: true
        }).range(from, to),
        Decoration.widget({
          widget: new CommentMarkerWidget(
            ids,
            allResolved
          ),
          side: 1
        }).range(to)
      );
    }
    return Decoration.set(ranges, true);
  }

  function refreshCommentDecorations(editor: EditorView): void {
    editor.dispatch({ effects: setCommentDecorations.of(commentDecorations(editor)) });
  }

  function reportSelection(editor: EditorView): void {
    const range = editor.state.selection.main;
    if (
      range.empty ||
      !editor.hasFocus ||
      range.to < editor.viewport.from ||
      range.to > editor.viewport.to
    ) {
      selectionAction = null;
      return;
    }
    const coordinates = editor.coordsAtPos(range.to);
    if (!coordinates) {
      selectionAction = null;
      return;
    }
    selectionAction = {
      ...selectionAnchor(editor.state.doc.toString(), range.from, range.to),
      x: Math.min(window.innerWidth - 92, Math.max(8, coordinates.right + 6)),
      y: Math.min(window.innerHeight - 42, Math.max(8, coordinates.bottom + 5))
    };
  }

  function beginSelectionComment(): void {
    if (!selectionAction) return;
    const { x: _x, y: _y, ...anchor } = selectionAction;
    onCommentSelection?.(anchor);
    selectionAction = null;
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

  class CommentMarkerWidget extends WidgetType {
    readonly ids: number[];
    readonly resolved: boolean;

    constructor(ids: number[], resolved: boolean) {
      super();
      this.ids = ids;
      this.resolved = resolved;
    }

    toDOM(): HTMLElement {
      const marker = document.createElement('button');
      marker.type = 'button';
      marker.className = [
        'cm-comment-marker',
        this.resolved ? 'cm-comment-marker-resolved' : ''
      ].filter(Boolean).join(' ');
      marker.dataset.scratchpadCommentId = String(this.ids[0]);
      marker.dataset.scratchpadCommentIds = this.ids.join(',');
      marker.contentEditable = 'false';
      marker.title = this.ids.length === 1 ? 'Open comment' : `Open ${this.ids.length} comments`;
      marker.setAttribute('aria-label', marker.title);

      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
      svg.setAttribute('viewBox', '0 0 24 24');
      svg.setAttribute('width', '12');
      svg.setAttribute('height', '12');
      svg.setAttribute('fill', 'none');
      svg.setAttribute('stroke', 'currentColor');
      svg.setAttribute('stroke-width', '2');
      svg.setAttribute('stroke-linecap', 'round');
      svg.setAttribute('stroke-linejoin', 'round');
      svg.setAttribute('aria-hidden', 'true');
      const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
      path.setAttribute('d', 'M21 15a4 4 0 0 1-4 4H8l-5 3V7a4 4 0 0 1 4-4h10a4 4 0 0 1 4 4z');
      svg.append(path);
      marker.append(svg);
      if (this.ids.length > 1) {
        const count = document.createElement('span');
        count.textContent = String(this.ids.length);
        marker.append(count);
      }
      return marker;
    }

    eq(other: CommentMarkerWidget): boolean {
      return this.resolved === other.resolved &&
        this.ids.join(',') === other.ids.join(',');
    }

    ignoreEvent(): boolean {
      return false;
    }
  }

  interface AttachmentImageRead {
    bytes: number[];
    mime_type: string;
  }

  function loadLocalImageSource(path: string): Promise<string> {
    const cached = imageSourceRequests.get(path);
    if (cached) return cached;
    const request = invoke<AttachmentImageRead>('terminal_read_attachment_image', { path })
      .then((image) => {
        const source = URL.createObjectURL(new Blob(
          [new Uint8Array(image.bytes)],
          { type: image.mime_type }
        ));
        if (imageSourceDisposed) {
          URL.revokeObjectURL(source);
          throw new Error('Scratchpad image view closed');
        }
        imageObjectUrls.add(source);
        return source;
      })
      .catch((cause) => {
        imageSourceRequests.delete(path);
        throw cause;
      });
    imageSourceRequests.set(path, request);
    return request;
  }

  class LocalImageWidget extends WidgetType {
    private disposed = false;
    readonly path: string;
    readonly alt: string;

    constructor(path: string, alt: string) {
      super();
      this.path = path;
      this.alt = alt;
    }

    toDOM(editor: EditorView): HTMLElement {
      const frame = document.createElement('span');
      frame.className = 'cm-live-image';
      frame.contentEditable = 'false';
      frame.textContent = 'Loading image…';
      void loadLocalImageSource(this.path)
        .then((source) => {
          if (this.disposed || !frame.isConnected) return;
          const element = document.createElement('img');
          element.src = source;
          element.alt = this.alt || 'Embedded scratchpad image';
          element.onload = () => {
            if (!this.disposed) editor.requestMeasure();
          };
          frame.replaceChildren(element);
          editor.requestMeasure();
        })
        .catch(() => {
          if (this.disposed || !frame.isConnected) return;
          frame.classList.add('cm-live-image-error');
          frame.textContent = 'Image unavailable';
          frame.title = this.path;
          editor.requestMeasure();
        });
      return frame;
    }

    eq(other: LocalImageWidget): boolean {
      return this.path === other.path && this.alt === other.alt;
    }

    destroy(): void {
      this.disposed = true;
    }

    ignoreEvent(): boolean {
      return false;
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
    const embeddedImages = scratchpadMarkdownImages(text).flatMap((image) => {
      const path = scratchpadLocalImagePath(image.source);
      return path ? [{ ...image, path }] : [];
    });
    const overlapsEmbeddedImage = (from: number, to: number): boolean =>
      embeddedImages.some((image) => from < image.to && to > image.from);

    for (const match of text.matchAll(/\*\*([^*\n]+)\*\*/g)) {
      const localStart = match.index ?? 0;
      if (overlapsEmbeddedImage(localStart, localStart + match[0].length)) continue;
      const start = lineFrom + localStart;
      const end = start + match[0].length;
      replaceMarker(ranges, editor, start, start + 2);
      ranges.push(Decoration.mark({ class: 'cm-live-strong' }).range(start + 2, end - 2));
      replaceMarker(ranges, editor, end - 2, end);
    }
    for (const match of text.matchAll(/(?<!\*)\*([^*\n]+)\*(?!\*)/g)) {
      const localStart = match.index ?? 0;
      if (overlapsEmbeddedImage(localStart, localStart + match[0].length)) continue;
      const start = lineFrom + localStart;
      const end = start + match[0].length;
      replaceMarker(ranges, editor, start, start + 1);
      ranges.push(Decoration.mark({ class: 'cm-live-emphasis' }).range(start + 1, end - 1));
      replaceMarker(ranges, editor, end - 1, end);
    }
    for (const match of text.matchAll(/`([^`\n]+)`/g)) {
      const localStart = match.index ?? 0;
      if (overlapsEmbeddedImage(localStart, localStart + match[0].length)) continue;
      const start = lineFrom + localStart;
      const end = start + match[0].length;
      replaceMarker(ranges, editor, start, start + 1);
      ranges.push(Decoration.mark({ class: 'cm-live-inline-code' }).range(start + 1, end - 1));
      replaceMarker(ranges, editor, end - 1, end);
    }
    for (const match of text.matchAll(/\[([^\]\n]+)\]\(([^)\n]+)\)/g)) {
      const localStart = match.index ?? 0;
      if (overlapsEmbeddedImage(localStart, localStart + match[0].length)) continue;
      const start = lineFrom + localStart;
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
    for (const image of embeddedImages) {
      const start = lineFrom + image.from;
      const end = lineFrom + image.to;
      if (cursorTouches(editor, start, end)) continue;
      ranges.push(
        Decoration.replace({
          widget: new LocalImageWidget(image.path, image.alt),
          inclusive: false
        }).range(start, end)
      );
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
    '.cm-selectionBackground, &.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground': {
      backgroundColor: 'color-mix(in srgb, var(--ring) 22%, transparent) !important'
    },
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
    '.cm-live-image': {
      display: 'inline-flex',
      width: 'min(100%, 760px)',
      minHeight: '120px',
      alignItems: 'center',
      justifyContent: 'center',
      overflow: 'hidden',
      boxSizing: 'border-box',
      border: '1px solid var(--border)',
      borderRadius: '8px',
      margin: '6px 0',
      backgroundColor: 'var(--card)',
      color: 'var(--muted-foreground)',
      fontFamily: "'JetBrains Mono Variable', monospace",
      fontSize: '11px',
      verticalAlign: 'top'
    },
    '.cm-live-image img': {
      display: 'block',
      width: '100%',
      height: 'auto',
      maxHeight: '560px',
      objectFit: 'contain'
    },
    '.cm-live-image-error': { minHeight: '72px', borderStyle: 'dashed' },
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
    '.cm-comment-highlight': {
      borderBottom: '1px solid var(--notification-unread)',
      backgroundColor: 'color-mix(in srgb, var(--notification-unread) 12%, transparent)',
      color: 'var(--foreground)',
      cursor: 'pointer'
    },
    '.cm-comment-highlight:hover': {
      backgroundColor: 'color-mix(in srgb, var(--notification-unread) 19%, transparent)'
    },
    '.cm-comment-highlight.cm-comment-focused': {
      borderBottomWidth: '2px',
      backgroundColor: 'color-mix(in srgb, var(--notification-unread) 27%, transparent)'
    },
    '.cm-comment-highlight.cm-comment-resolved': {
      borderBottomColor: 'var(--border-strong)',
      backgroundColor: 'color-mix(in srgb, var(--muted-foreground) 8%, transparent)'
    },
    '.cm-comment-marker': {
      display: 'inline-flex',
      minWidth: '20px',
      height: '20px',
      alignItems: 'center',
      justifyContent: 'center',
      gap: '2px',
      margin: '0 2px 0 5px',
      border: '1px solid color-mix(in srgb, var(--notification-unread) 55%, var(--border))',
      borderRadius: '999px',
      padding: '0 4px',
      backgroundColor: 'color-mix(in srgb, var(--notification-unread) 12%, var(--popover))',
      color: 'var(--notification-unread-foreground)',
      fontFamily: "'JetBrains Mono Variable', monospace",
      fontSize: '9px',
      fontWeight: '700',
      lineHeight: '1',
      verticalAlign: '2px',
      cursor: 'pointer'
    },
    '.cm-comment-marker:hover': {
      borderColor: 'var(--notification-unread)',
      backgroundColor: 'color-mix(in srgb, var(--notification-unread) 22%, var(--popover))'
    },
    '.cm-comment-marker:focus-visible': {
      outline: '2px solid var(--ring)',
      outlineOffset: '1px'
    },
    '.cm-comment-marker.cm-comment-marker-resolved': {
      borderColor: 'var(--border-strong)',
      backgroundColor: 'var(--card)',
      color: 'var(--muted-foreground)'
    },
    '.cm-comment-flash': { outline: '2px solid var(--notification-unread)', outlineOffset: '1px' },
    '.cm-placeholder': { color: 'var(--muted-foreground)', fontStyle: 'italic' }
    });
  }

  onMount(() => {
    const hideSelectionAction = (): void => { selectionAction = null; };
    window.addEventListener('scroll', hideSelectionAction, true);
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
          commentDecorationField,
          createEditorTheme(flow),
          EditorView.domEventHandlers({
            click: (event, editor) => {
              const target = event.target instanceof Element
                ? event.target.closest<HTMLElement>('[data-scratchpad-comment-id]')
                : null;
              const commentId = Number(target?.dataset.scratchpadCommentId);
              if (target && Number.isInteger(commentId)) {
                event.preventDefault();
                const marker = target.classList.contains('cm-comment-marker')
                  ? target
                  : [...editor.dom.querySelectorAll<HTMLElement>('.cm-comment-marker')]
                      .find((candidate) => candidate.dataset.scratchpadCommentIds
                        ?.split(',').includes(String(commentId)));
                onCommentClick?.(commentId, marker ?? target);
                return true;
              }
              if (!primaryModifier(event) || event.button !== 0) return false;
              const position = editor.posAtCoords({ x: event.clientX, y: event.clientY });
              if (position === null) return false;
              const href = markdownLinkAt(editor.state.doc.toString(), position);
              if (!href) return false;
              event.preventDefault();
              openExternalUrl(href);
              return true;
            },
            blur: () => {
              window.setTimeout(() => {
                if (!view?.hasFocus) selectionAction = null;
              }, 0);
              return false;
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
              update.docChanged ||
              update.selectionSet ||
              update.viewportChanged ||
              update.geometryChanged ||
              update.focusChanged
            ) {
              reportSelection(update.view);
            }
            if (
              update.docChanged &&
              !update.transactions.some((transaction) => transaction.annotation(externalChange))
            ) {
              onChange(update.state.doc.toString(), update.changes);
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
      window.removeEventListener('scroll', hideSelectionAction, true);
      view?.destroy();
      view = null;
      imageSourceDisposed = true;
      for (const source of imageObjectUrls) URL.revokeObjectURL(source);
      imageObjectUrls.clear();
      imageSourceRequests.clear();
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

  $effect(() => {
    comments;
    showResolvedComments;
    focusedCommentId;
    if (view) refreshCommentDecorations(view);
  });

  $effect(() => {
    const request = commentScrollRequest;
    if (!view || !request || request.key <= appliedCommentScrollRequest) return;
    appliedCommentScrollRequest = request.key;
    let from: number | null = null;
    let to: number | null = null;
    view.state.field(commentDecorationField).between(0, view.state.doc.length, (rangeFrom, rangeTo, decoration) => {
      const ids = decoration.spec.attributes?.['data-scratchpad-comment-ids']?.split(',') ?? [];
      if (
        decoration.spec.attributes?.['data-scratchpad-comment-id'] === String(request.commentId) ||
        ids.includes(String(request.commentId))
      ) {
        from = rangeFrom;
        to = rangeTo;
      }
    });
    if (from === null || to === null) {
      if (request.fallbackLine !== null && request.fallbackLine !== undefined) {
        const lineNumber = Math.max(1, Math.min(request.fallbackLine, view.state.doc.lines));
        const line = view.state.doc.line(lineNumber);
        view.dispatch({ effects: EditorView.scrollIntoView(line.from, { y: 'center', yMargin: 36 }) });
      }
      return;
    }
    view.dispatch({
      selection: { anchor: from, head: to },
      effects: EditorView.scrollIntoView(from, { y: 'center', yMargin: 36 })
    });
    requestAnimationFrame(() => {
      const highlight = [...(view?.dom.querySelectorAll<HTMLElement>('[data-scratchpad-comment-ids]') ?? [])]
        .find((element) => element.dataset.scratchpadCommentIds?.split(',').includes(String(request.commentId)));
      highlight?.classList.add('cm-comment-flash');
      window.setTimeout(() => highlight?.classList.remove('cm-comment-flash'), 900);
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
{#if selectionAction}
  <button
    type="button"
    class="selection-comment"
    style={`left: ${selectionAction.x}px; top: ${selectionAction.y}px`}
    onmousedown={(event) => event.preventDefault()}
    onclick={beginSelectionComment}
  >Comment</button>
{/if}

<style>
  .editor-shell { display: grid; height: 100%; min-height: 0; grid-template-rows: minmax(0, 1fr); overflow: hidden; background: var(--background); }
  .editor-shell.with-toolbar { grid-template-rows: auto minmax(0, 1fr); }
  .editor-shell.flow { height: auto; min-height: clamp(380px, calc(100vh - 290px), 640px); grid-template-rows: minmax(0, auto); overflow: visible; background: transparent; }
  .editor-shell.flow.with-toolbar { grid-template-rows: auto minmax(0, auto); }
  .editor-shell.flow .format-toolbar { background: transparent; }
  .format-toolbar { display: flex; min-width: 0; min-height: 34px; align-items: center; gap: 1px; overflow-x: auto; border-bottom: 1px solid var(--border); padding: 2px 5px; background: var(--card); scrollbar-width: none; }
  .separator { width: 1px; height: 18px; flex: none; margin: 0 3px; background: var(--border); }
  .selection-comment { position: fixed; z-index: 50; min-height: 30px; border: 1px solid var(--border-strong); border-radius: var(--radius); padding: 0 9px; background: var(--popover); color: var(--foreground); font: 620 var(--font-size-xs) var(--ui-font-family); cursor: pointer; }
  .selection-comment:hover { background: var(--accent); }
  .selection-comment:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
  .editor-host { min-height: 0; height: 100%; overflow: hidden; background: var(--background); }
  .editor-shell.flow .editor-host { height: auto; min-height: inherit; overflow: visible; background: transparent; }
</style>
