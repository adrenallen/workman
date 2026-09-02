import type { QuickPrompt } from './quickPrompts';
import { fuzzySubsequenceScoreNormalized } from './navigation.ts';
import { primaryModifier, secondaryModifier } from './primaryModifier.ts';

const QUICK_PROMPT_BODY_SEARCH_CHARS = 2_000;

interface SearchablePrompt {
  nameSource: string;
  bodySource: string;
  name: string;
  body: string;
}

const searchablePromptCache = new WeakMap<QuickPrompt, SearchablePrompt>();

export type QuickPromptNavigationAction = 'next' | 'previous' | 'first' | 'last';

export type QuickPromptPaletteAction =
  | 'next'
  | 'previous'
  | 'first'
  | 'last'
  | 'swallow'
  | 'insert'
  | null;

interface PaletteKeyEvent {
  key: string;
  metaKey: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
  isComposing?: boolean;
  keyCode?: number;
}

/** Interpret palette-owned navigation and selection actions. */
export function quickPromptPaletteAction(event: PaletteKeyEvent): QuickPromptPaletteAction {
  const key = event.key.toLowerCase();
  if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') return null;
  const vimNavigation = Boolean(event.ctrlKey) && ['n', 'j', 'p', 'k'].includes(key);
  const navigationKey = event.key === 'ArrowUp'
    || event.key === 'ArrowDown'
    || event.key === 'Home'
    || event.key === 'End'
    || event.key === 'PageUp'
    || event.key === 'PageDown'
    || vimNavigation;
  if (event.isComposing || event.keyCode === 229) return navigationKey ? 'swallow' : null;

  if (event.key === 'ArrowDown') return primaryModifier(event) ? 'last' : 'next';
  if (event.key === 'ArrowUp') return primaryModifier(event) ? 'first' : 'previous';
  if (event.key === 'Home') return 'first';
  if (event.key === 'End') return 'last';
  if (event.key === 'PageUp' || event.key === 'PageDown') return 'swallow';
  if (vimNavigation) {
    if (event.metaKey || event.altKey) return 'swallow';
    return key === 'n' || key === 'j' ? 'next' : 'previous';
  }
  if (event.key === 'Enter') {
    if (event.shiftKey || secondaryModifier(event) || event.altKey) return 'swallow';
    return primaryModifier(event) ? 'swallow' : 'insert';
  }
  return null;
}

/** Resolve palette navigation without relying on Command's internal selection state. */
export function moveQuickPromptSelection(
  selectedIndex: number,
  itemCount: number,
  action: QuickPromptNavigationAction
): number {
  if (itemCount <= 0) return 0;
  if (action === 'first') return 0;
  if (action === 'last') return itemCount - 1;
  const current = Math.min(Math.max(selectedIndex, 0), itemCount - 1);
  return action === 'next'
    ? (current + 1) % itemCount
    : (current - 1 + itemCount) % itemCount;
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
