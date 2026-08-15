import type { QuickPrompt } from './quickPrompts';
import { fuzzySubsequenceScoreNormalized } from './navigation.ts';

const QUICK_PROMPT_BODY_SEARCH_CHARS = 2_000;

interface SearchablePrompt {
  nameSource: string;
  bodySource: string;
  name: string;
  body: string;
}

const searchablePromptCache = new WeakMap<QuickPrompt, SearchablePrompt>();

export type QuickPromptPaletteAction = 'insert' | 'insert-and-send' | 'new' | null;

interface PaletteKeyEvent {
  key: string;
  metaKey: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
  isComposing?: boolean;
  keyCode?: number;
}

export function isQuickPromptPaletteShortcut(event: PaletteKeyEvent): boolean {
  return event.metaKey && Boolean(event.shiftKey) && !event.ctrlKey && !event.altKey
    && event.key.toLowerCase() === 'p';
}

/** Interpret only palette-owned actions; arrow navigation remains owned by Command. */
export function quickPromptPaletteAction(event: PaletteKeyEvent): QuickPromptPaletteAction {
  if (event.isComposing || event.keyCode === 229) return null;
  if (event.ctrlKey || event.altKey) return null;
  if (event.key === 'Enter' && !event.shiftKey) {
    return event.metaKey ? 'insert-and-send' : 'insert';
  }
  if (event.metaKey && !event.shiftKey && event.key.toLowerCase() === 'n') return 'new';
  return null;
}

/** Fuzzy subsequence search across both the saved name and body, with substring priority. */
export function filterQuickPrompts(prompts: QuickPrompt[], query: string): QuickPrompt[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return prompts;
  return prompts
    .map((prompt, index) => {
      const searchable = searchablePrompt(prompt);
      return {
        prompt,
        index,
        score: Math.max(
          fuzzySubsequenceScoreNormalized(needle, searchable.name)
            ?? Number.NEGATIVE_INFINITY,
          fuzzySubsequenceScoreNormalized(needle, searchable.body)
            ?? Number.NEGATIVE_INFINITY
        )
      };
    })
    .filter((candidate) => Number.isFinite(candidate.score))
    .sort((left, right) => right.score - left.score || left.index - right.index)
    .map((candidate) => candidate.prompt);
}

function searchablePrompt(prompt: QuickPrompt): SearchablePrompt {
  const cached = searchablePromptCache.get(prompt);
  if (cached?.nameSource === prompt.name && cached.bodySource === prompt.body) return cached;
  const searchable = {
    nameSource: prompt.name,
    bodySource: prompt.body,
    name: prompt.name.toLocaleLowerCase(),
    body: prompt.body.slice(0, QUICK_PROMPT_BODY_SEARCH_CHARS).toLocaleLowerCase()
  };
  searchablePromptCache.set(prompt, searchable);
  return searchable;
}

/** Remove terminal control bytes while preserving prompt line breaks and tabs. */
export function sanitizeQuickPromptBody(body: string): string {
  return body.replace(/[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/g, '');
}

export function quickPromptPreview(body: string): string {
  return body.replace(/\s+/g, ' ').trim();
}
