import type { ProcessKind } from './daemon';
import { normalizeAgentToolType } from './agentToolType.ts';

const safePathCharacter = /^[\p{L}\p{N}_./-]$/u;

export type ClipboardImagePasteRoute = 'agent-tui' | 'agent-native-normalized' | 'shell-path';

export interface ClipboardImagePasteActions {
  forwardNativePaste: () => void;
  normalizeSystemClipboard: () => Promise<void>;
  insertSavedPngFallback: () => Promise<void>;
}

/**
 * Agent TUIs own clipboard-image import, so forward their Ctrl+V signal and let them read the
 * unchanged system clipboard. Plain terminals retain Workman's saved-file + escaped-path paste.
 */
export function clipboardImagePasteRoute(
  processKind: ProcessKind,
  agentToolType: string | null = null
): ClipboardImagePasteRoute {
  if (processKind !== 'agent') return 'shell-path';
  switch (normalizeAgentToolType(agentToolType)) {
    case 'claude':
    case 'claude_code':
      // Claude reads PNGf through AppleScript. Normalize the pasteboard before forwarding Ctrl+V
      // so TIFF and file-URL sources do not depend on an implicit AppKit conversion.
      return 'agent-native-normalized';
    case 'codex':
    default:
      // Codex owns image import and keeps the pre-existing immediate Ctrl+V behavior. Unknown
      // agent TUIs retain that same compatible fallback.
      return 'agent-tui';
  }
}

/** Execute the selected image-paste strategy without coupling tests to DOM or Tauri globals. */
export async function performClipboardImagePaste(
  route: ClipboardImagePasteRoute,
  actions: ClipboardImagePasteActions
): Promise<void> {
  if (route === 'agent-tui') {
    actions.forwardNativePaste();
    return;
  }
  if (route === 'shell-path') {
    await actions.insertSavedPngFallback();
    return;
  }
  try {
    await actions.normalizeSystemClipboard();
  } catch {
    await actions.insertSavedPngFallback();
    return;
  }
  actions.forwardNativePaste();
}

/** Ctrl+V as a PTY byte; Claude Code and Codex use it to import an image from the clipboard. */
export const AGENT_TUI_CLIPBOARD_IMAGE_PASTE = '\x16';

/** Keep replay-generated terminal replies gated without dropping physical user input. */
export function shouldForwardTerminalInput(
  inputEnabled: boolean,
  userInitiated: boolean
): boolean {
  return inputEnabled || userInitiated;
}

/** Match Terminal.app's unquoted, backslash-escaped file-path insertion. */
export function shellEscapePath(path: string): string {
  if (!path) throw new Error('Dropped file path is empty.');
  if (/[\0\r\n]/u.test(path)) {
    throw new Error('Dropped file paths cannot contain control characters.');
  }
  return Array.from(path, (character) =>
    safePathCharacter.test(character) ? character : `\\${character}`
  ).join('');
}

export function shellEscapePaths(paths: string[]): string {
  if (paths.length === 0) throw new Error('No dropped file paths were provided.');
  return paths.map(shellEscapePath).join(' ');
}

export function localPathsFromUriList(value: string): string[] {
  const paths: string[] = [];
  for (const line of value.split(/\r?\n/)) {
    const candidate = line.trim();
    if (!candidate || candidate.startsWith('#')) continue;
    try {
      const url = new URL(candidate);
      if (url.protocol !== 'file:' || (url.hostname && url.hostname !== 'localhost')) continue;
      const path = decodeURIComponent(url.pathname);
      if (path.startsWith('/')) paths.push(path);
    } catch {
      // Ignore malformed or non-local URI-list entries.
    }
  }
  return paths;
}

export function pointIsInsideRect(
  point: { x: number; y: number },
  rect: { left: number; top: number; right: number; bottom: number },
  physicalScale: number
): boolean {
  if (!Number.isFinite(physicalScale) || physicalScale <= 0) return false;
  const x = point.x / physicalScale;
  const y = point.y / physicalScale;
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}
