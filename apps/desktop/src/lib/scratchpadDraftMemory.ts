export interface ScratchpadDraft {
  markdown: string;
  baseMarkdown: string;
  baseRevision: number;
}

const drafts = new Map<string, ScratchpadDraft>();
const storageKey = (key: string) => `workman.scratchpad-draft.${key}`;

export function readScratchpadDraft(key: string): ScratchpadDraft | null {
  if (drafts.has(key)) return drafts.get(key)!;
  try {
    const value = JSON.parse(localStorage.getItem(storageKey(key)) ?? 'null');
    return value && typeof value.markdown === 'string' && typeof value.baseMarkdown === 'string'
      && Number.isSafeInteger(value.baseRevision) ? value : null;
  } catch { return null; }
}

export function storeScratchpadDraft(key: string, draft: ScratchpadDraft | null): void {
  if (draft) drafts.set(key, draft);
  else drafts.delete(key);
  try {
    if (draft) localStorage.setItem(storageKey(key), JSON.stringify(draft));
    else localStorage.removeItem(storageKey(key));
  } catch { /* The in-memory copy still survives pane navigation if storage is full. */ }
}
