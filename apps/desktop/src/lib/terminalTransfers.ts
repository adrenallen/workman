import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview';

import { localPathsFromUriList, pointIsInsideRect, shellEscapePaths } from './terminalInput';

interface TerminalTransferOptions {
  element: HTMLElement;
  canInsert: () => boolean;
  insert: (text: string) => void;
  focus: () => void;
  reportError: (message: string) => void;
  setDropActive: (active: boolean) => void;
  setPasteSaving: (saving: boolean) => void;
}

interface PathFile extends File {
  path?: string;
}

export function installTerminalTransfers(options: TerminalTransferOptions): () => void {
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
  const paste = (event: ClipboardEvent) => {
    const images = Array.from(event.clipboardData?.items ?? [])
      .filter((item) => item.kind === 'file' && item.type.startsWith('image/'))
      .map((item) => item.getAsFile())
      .filter((file): file is File => file !== null);
    if (images.length === 0) return;

    event.preventDefault();
    event.stopPropagation();
    if (!options.canInsert()) {
      options.reportError('Terminal input is not ready for a clipboard image.');
      return;
    }
    options.setPasteSaving(true);
    void saveClipboardImages(images)
      .then((paths) => {
        if (!disposed) insertPaths(paths);
      })
      .catch((cause) => {
        if (!disposed) options.reportError(message(cause));
      })
      .finally(() => {
        if (!disposed) options.setPasteSaving(false);
      });
  };

  options.element.addEventListener('dragenter', dragOver);
  options.element.addEventListener('dragover', dragOver);
  options.element.addEventListener('dragleave', dragLeave);
  options.element.addEventListener('drop', drop);
  options.element.addEventListener('paste', paste, true);

  return () => {
    disposed = true;
    removeNativeListener?.();
    options.setDropActive(false);
    options.setPasteSaving(false);
    options.element.removeEventListener('dragenter', dragOver);
    options.element.removeEventListener('dragover', dragOver);
    options.element.removeEventListener('dragleave', dragLeave);
    options.element.removeEventListener('drop', drop);
    options.element.removeEventListener('paste', paste, true);
  };
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
