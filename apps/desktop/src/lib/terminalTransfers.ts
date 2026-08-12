import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview';

import {
  type ClipboardImagePasteRoute,
  localPathsFromUriList,
  pointIsInsideRect,
  shellEscapePaths
} from './terminalInput';

interface TerminalTransferOptions {
  element: HTMLElement;
  canInsert: () => boolean;
  insert: (text: string) => void;
  pasteText: (text: string) => void;
  imagePasteRoute: () => ClipboardImagePasteRoute;
  forwardAgentImagePaste: () => void;
  focus: () => void;
  reportError: (message: string) => void;
  setDropActive: (active: boolean) => void;
  setPasteSaving: (saving: boolean) => void;
}

export interface TerminalTransfers {
  dispose: () => void;
  pasteFromClipboard: () => Promise<void>;
}

interface PathFile extends File {
  path?: string;
}

type NativeTerminalClipboardRead =
  | { kind: 'text'; text: string }
  | { kind: 'image'; path: string | null }
  | { kind: 'empty' };

export async function writeTerminalClipboardText(text: string): Promise<void> {
  if (isTauri()) {
    await invoke('terminal_write_clipboard_text', { text });
    return;
  }
  await navigator.clipboard.writeText(text);
}

export function installTerminalTransfers(options: TerminalTransferOptions): TerminalTransfers {
  let disposed = false;
  let removeNativeListener: (() => void) | null = null;

  const insertPaths = (paths: string[]) => {
    if (!options.canInsert()) {
      options.reportError('Terminal input is not ready for a file path.');
      return;
    }
    try {
      const escaped = shellEscapePaths(paths);
      options.focus();
      options.insert(escaped);
    } catch (cause) {
      options.reportError(message(cause));
    }
  };

  const nativeDrop = (payload: DragDropEvent) => {
    if (payload.type === 'leave') {
      options.setDropActive(false);
      return;
    }
    const inside = pointIsInsideRect(
      payload.position,
      options.element.getBoundingClientRect(),
      window.devicePixelRatio
    );
    if (payload.type === 'enter' || payload.type === 'over') {
      options.setDropActive(inside && options.canInsert());
      return;
    }
    options.setDropActive(false);
    if (inside) insertPaths(payload.paths);
  };

  // Tauri's default native file-drop handler consumes OS drops before the DOM sees them.
  // Keep it enabled for native paths and bridge its physical coordinates into this terminal.
  if (isTauri()) {
    void getCurrentWebview()
      .onDragDropEvent((event) => nativeDrop(event.payload))
      .then((unlisten) => {
        if (disposed) unlisten();
        else removeNativeListener = unlisten;
      })
      .catch((cause) => {
        if (!disposed) options.reportError(`Could not listen for terminal file drops: ${message(cause)}`);
      });
  }

  const carriesFiles = (event: DragEvent) =>
    Array.from(event.dataTransfer?.types ?? []).includes('Files');

  const dragOver = (event: DragEvent) => {
    if (!carriesFiles(event)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
    options.setDropActive(options.canInsert());
  };
  const dragLeave = (event: DragEvent) => {
    if (
      event.relatedTarget instanceof Node
      && options.element.contains(event.relatedTarget)
    ) {
      return;
    }
    options.setDropActive(false);
  };
  const drop = (event: DragEvent) => {
    if (!carriesFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    options.setDropActive(false);
    let paths = Array.from(event.dataTransfer?.files ?? [])
      .map((file) => (file as PathFile).path)
      .filter((path): path is string => Boolean(path));
    if (paths.length === 0) {
      paths = localPathsFromUriList(event.dataTransfer?.getData('text/uri-list') ?? '');
    }
    if (paths.length === 0) {
      options.reportError('The dropped files did not expose local paths.');
      return;
    }
    insertPaths(paths);
  };
  const pasteImages = async (images: File[]) => {
    if (!options.canInsert()) {
      options.reportError('Terminal input is not ready for a clipboard image.');
      return;
    }
    if (options.imagePasteRoute() === 'agent-tui') {
      // The TUI reads the image from the OS clipboard when it receives Ctrl+V. Do not save,
      // replace, or clear the clipboard here; forwarding the chord preserves its native flow.
      options.focus();
      options.forwardAgentImagePaste();
      return;
    }
    options.setPasteSaving(true);
    try {
      const paths = await saveClipboardImages(images);
      if (!disposed) insertPaths(paths);
    } catch (cause) {
      if (!disposed) options.reportError(message(cause));
    } finally {
      if (!disposed) options.setPasteSaving(false);
    }
  };
  const pasteNativeClipboard = async () => {
    const route = options.imagePasteRoute();
    const clipboard = await invoke<NativeTerminalClipboardRead>('terminal_read_clipboard', {
      saveImage: route === 'shell-path'
    });
    if (disposed || clipboard.kind === 'empty') return;
    if (clipboard.kind === 'text') {
      if (clipboard.text === '') return;
      options.focus();
      options.pasteText(clipboard.text);
      return;
    }
    if (route === 'agent-tui') {
      // The native command only observes that an image exists; the TUI reads the unchanged pasteboard.
      options.focus();
      options.forwardAgentImagePaste();
      return;
    }
    if (!clipboard.path) throw new Error('The clipboard image was not saved for terminal paste.');
    insertPaths([clipboard.path]);
  };
  const paste = (event: ClipboardEvent) => {
    const images = Array.from(event.clipboardData?.items ?? [])
      .filter((item) => item.kind === 'file' && item.type.startsWith('image/'))
      .map((item) => item.getAsFile())
      .filter((file): file is File => file !== null);
    if (images.length === 0) return;

    event.preventDefault();
    event.stopPropagation();
    void pasteImages(images);
  };

  options.element.addEventListener('dragenter', dragOver);
  options.element.addEventListener('dragover', dragOver);
  options.element.addEventListener('dragleave', dragLeave);
  options.element.addEventListener('drop', drop);
  options.element.addEventListener('paste', paste, true);

  return {
    pasteFromClipboard: async () => {
      if (!options.canInsert()) {
        options.reportError('Terminal input is not ready for clipboard paste.');
        return;
      }
      try {
        if (isTauri()) {
          await pasteNativeClipboard();
          return;
        }
        let clipboardItems: ClipboardItems;
        try {
          clipboardItems = await navigator.clipboard.read();
        } catch (readCause) {
          // Older WKWebViews can expose readText before the full ClipboardItem API.
          const text = await navigator.clipboard.readText();
          if (disposed || text === '') throw readCause;
          options.focus();
          options.pasteText(text);
          return;
        }
        if (disposed) return;
        const images = await clipboardImages(clipboardItems);
        if (disposed) return;
        if (images.length > 0) {
          await pasteImages(images);
          return;
        }

        const text = await clipboardText(clipboardItems);
        if (disposed || text === '') return;
        options.focus();
        options.pasteText(text);
      } catch (cause) {
        if (!disposed) options.reportError(`Could not read the clipboard: ${message(cause)}`);
      }
    },
    dispose: () => {
      disposed = true;
      removeNativeListener?.();
      options.setDropActive(false);
      options.setPasteSaving(false);
      options.element.removeEventListener('dragenter', dragOver);
      options.element.removeEventListener('dragover', dragOver);
      options.element.removeEventListener('dragleave', dragLeave);
      options.element.removeEventListener('drop', drop);
      options.element.removeEventListener('paste', paste, true);
    }
  };
}

async function clipboardImages(items: ClipboardItems): Promise<File[]> {
  const images: File[] = [];
  for (const item of items) {
    for (const type of item.types.filter((candidate) => candidate.startsWith('image/'))) {
      const blob = await item.getType(type);
      images.push(new File([blob], 'clipboard-image', { type }));
    }
  }
  return images;
}

async function clipboardText(items: ClipboardItems): Promise<string> {
  const parts: string[] = [];
  for (const item of items) {
    if (!item.types.includes('text/plain')) continue;
    parts.push(await (await item.getType('text/plain')).text());
  }
  return parts.join('\n');
}

async function saveClipboardImages(images: File[]): Promise<string[]> {
  const paths: string[] = [];
  for (const image of images) {
    const bytes = Array.from(new Uint8Array(await image.arrayBuffer()));
    paths.push(await invoke<string>('terminal_save_clipboard_image', {
      bytes,
      mimeType: image.type
    }));
  }
  return paths;
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
