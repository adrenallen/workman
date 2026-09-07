export interface MarkdownCodeBlock {
  from: number;
  openingTo: number;
  contentFrom: number;
  contentTo: number;
  closingFrom: number | null;
  to: number;
  language: string;
}

/** Fences span viewports; scan the document on edits, never infer them from visible lines. */
export function markdownCodeBlocks(source: string): MarkdownCodeBlock[] {
  const blocks: MarkdownCodeBlock[] = [];
  let open: { block: MarkdownCodeBlock; marker: string } | null = null;
  let from = 0;
  for (const line of source.split('\n')) {
    const to = from + line.length;
    if (open) {
      const closing = /^ {0,3}(`{3,}|~{3,})[\t ]*$/.exec(line);
      if (closing && closing[1][0] === open.marker[0] && closing[1].length >= open.marker.length) {
        blocks.push({ ...open.block, contentTo: from, closingFrom: from, to });
        open = null;
      }
    } else {
      const opening = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line);
      if (opening && !(opening[1][0] === '`' && opening[2].includes('`'))) {
        open = { marker: opening[1], block: {
          from, openingTo: to, contentFrom: Math.min(source.length, to + 1),
          contentTo: source.length, closingFrom: null, to: source.length,
          language: opening[2].trim().split(/\s+/)[0] || 'Code'
        }};
      }
    }
    from = to + 1;
  }
  if (open) blocks.push(open.block);
  return blocks;
}
