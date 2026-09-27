export interface MarkdownCodeBlock {
  from: number;
  openingTo: number;
  contentFrom: number;
  contentTo: number;
  closingFrom: number | null;
  to: number;
  language: string;
}

interface MarkdownFence {
  marker: string;
  rest: string;
  containerIndent: number;
}

function leadingSpaces(line: string): number {
  return /^ */.exec(line)?.[0].length ?? 0;
}

function fenceAt(line: string, listIndent: number | null): MarkdownFence | null {
  const listItem = /^ {0,3}(?:[-+*]|\d+[.)])[\t ]+/.exec(line);
  if (listItem) {
    const content = line.slice(listItem[0].length);
    const match = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(content);
    if (match) return {
      marker: match[1],
      rest: match[2],
      containerIndent: listItem[0].length
    };
  }

  const indent = leadingSpaces(line);
  if (listIndent !== null && indent < listIndent) return null;
  const containerIndent = listIndent !== null && indent >= listIndent ? listIndent : 0;
  const match = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line.slice(containerIndent));
  return match ? { marker: match[1], rest: match[2], containerIndent } : null;
}

/** Fences span viewports; scan the document on edits, never infer them from visible lines. */
export function markdownCodeBlocks(source: string): MarkdownCodeBlock[] {
  const blocks: MarkdownCodeBlock[] = [];
  let open: { block: MarkdownCodeBlock; marker: string; containerIndent: number } | null = null;
  let listIndent: number | null = null;
  let from = 0;
  for (const line of source.split('\n')) {
    const to = from + line.length;
    if (open) {
      const closing = fenceAt(line, open.containerIndent);
      if (
        closing &&
        closing.marker[0] === open.marker[0] &&
        closing.marker.length >= open.marker.length &&
        closing.rest.trim() === ''
      ) {
        blocks.push({ ...open.block, contentTo: from, closingFrom: from, to });
        open = null;
      }
    } else {
      const listItem = /^ {0,3}(?:[-+*]|\d+[.)])[\t ]+/.exec(line);
      if (listItem) listIndent = listItem[0].length;
      else if (line.trim() && listIndent !== null && leadingSpaces(line) < listIndent) listIndent = null;

      const opening = fenceAt(line, listIndent);
      if (opening && !(opening.marker[0] === '`' && opening.rest.includes('`'))) {
        open = { marker: opening.marker, containerIndent: opening.containerIndent, block: {
          from, openingTo: to, contentFrom: Math.min(source.length, to + 1),
          contentTo: source.length, closingFrom: null, to: source.length,
          language: opening.rest.trim().split(/\s+/)[0] || 'Code'
        }};
      }
    }
    from = to + 1;
  }
  if (open) blocks.push(open.block);
  return blocks;
}
