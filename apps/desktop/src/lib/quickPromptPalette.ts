import type { QuickPrompt } from './quickPrompts';
import { fuzzySubsequenceScore } from './navigation.ts';

export type QuickPromptPaletteAction = 'insert' | 'insert-and-send' | 'new' | null;

interface PaletteKeyEvent {
  key: string;
  metaKey: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
}

export function isQuickPromptPaletteShortcut(event: PaletteKeyEvent): boolean {
  return event.metaKey && Boolean(event.shiftKey) && !event.ctrlKey && !event.altKey
    && event.key.toLowerCase() === 'p';
}

/** Interpret only palette-owned actions; arrow navigation remains owned by Command. */
export function quickPromptPaletteAction(event: PaletteKeyEvent): QuickPromptPaletteAction {
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
    .map((prompt, index) => ({
      prompt,
      index,
      score: Math.max(
        fuzzySubsequenceScore(needle, prompt.name) ?? Number.NEGATIVE_INFINITY,
        fuzzySubsequenceScore(needle, prompt.body) ?? Number.NEGATIVE_INFINITY
      )
    }))
    .filter((candidate) => Number.isFinite(candidate.score))
    .sort((left, right) => right.score - left.score || left.index - right.index)
    .map((candidate) => candidate.prompt);
}

export function quickPromptPreview(body: string): string {
  return body.replace(/\s+/g, ' ').trim();
}
