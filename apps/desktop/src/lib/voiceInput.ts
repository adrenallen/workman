/** Insert speech at the saved selection; append if the user edited while transcription ran. */
export function insertDictation(
  current: string,
  original: string,
  selection: { start: number; end: number },
  transcript: string
): { text: string; caret: number } {
  const speech = transcript.trim();
  if (!speech) return { text: current, caret: current.length };
  const start = current === original ? Math.max(0, Math.min(selection.start, current.length)) : current.length;
  const end = current === original ? Math.max(start, Math.min(selection.end, current.length)) : current.length;
  const before = current.slice(0, start);
  const after = current.slice(end);
  const inserted = `${before && !/\s$/u.test(before) ? ' ' : ''}${speech}${after && !/^\s/u.test(after) ? ' ' : ''}`;
  return { text: before + inserted + after, caret: before.length + inserted.length };
}
