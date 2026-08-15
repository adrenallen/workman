import type { QuickPrompt } from './quickPrompts';

export type QuickPromptPaletteAction = 'insert' | 'insert-and-send' | 'new' | null;

interface PaletteKeyEvent {
  key: string;
  metaKey: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
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
        fuzzyScore(needle, prompt.name.toLocaleLowerCase()) ?? Number.NEGATIVE_INFINITY,
        fuzzyScore(needle, prompt.body.toLocaleLowerCase()) ?? Number.NEGATIVE_INFINITY
      )
    }))
    .filter((candidate) => Number.isFinite(candidate.score))
    .sort((left, right) => right.score - left.score || left.index - right.index)
    .map((candidate) => candidate.prompt);
}

export function quickPromptPreview(body: string): string {
  return body.replace(/\s+/g, ' ').trim();
}

function fuzzyScore(needle: string, haystack: string): number | null {
  const substring = haystack.indexOf(needle);
  if (substring >= 0) return 1_000 - substring - Math.max(0, haystack.length - needle.length) / 100;

  let cursor = 0;
  let first = -1;
  let previous = -2;
  let contiguous = 0;
  for (const character of needle) {
    const index = haystack.indexOf(character, cursor);
    if (index < 0) return null;
    if (first < 0) first = index;
    if (index === previous + 1) contiguous += 1;
    previous = index;
    cursor = index + 1;
  }
  return 100 + contiguous * 8 - first - (previous - first - needle.length);
}
