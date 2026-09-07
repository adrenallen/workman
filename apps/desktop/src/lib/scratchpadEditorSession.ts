export interface ScratchpadEditorSession {
  path: string;
  baseHash: string;
  observedHash: string;
}

export function editorChangeAction(session: ScratchpadEditorSession, incomingHash: string, currentHash: string, editing: boolean) {
  if (incomingHash === session.observedHash) return 'unchanged';
  if (incomingHash === currentHash) return 'acknowledge';
  if (editing) return 'pending';
  return currentHash === session.baseHash ? 'import' : 'conflict';
}

export async function markdownHash(markdown: string): Promise<string> {
  const hash = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(markdown));
  return Array.from(new Uint8Array(hash), byte => byte.toString(16).padStart(2, '0')).join('');
}

export function loadEditorSession(key: string): ScratchpadEditorSession | null {
  try {
    const value = JSON.parse(localStorage.getItem(`workman.scratchpad-editor.${key}`) ?? 'null');
    return value && typeof value.path === 'string' && typeof value.baseHash === 'string'
      && typeof value.observedHash === 'string' ? value : null;
  } catch { return null; }
}

export function storeEditorSession(key: string, session: ScratchpadEditorSession): void {
  localStorage.setItem(`workman.scratchpad-editor.${key}`, JSON.stringify(session));
}
