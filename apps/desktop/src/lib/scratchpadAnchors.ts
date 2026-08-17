export type ScratchpadAnchorState = 'anchored' | 'orphaned' | 'unanchored';

export interface ScratchpadAnchorInput {
  quote: string | null;
  anchor_start: number | null;
  anchor_end: number | null;
  anchor_prefix: string | null;
  anchor_suffix: string | null;
}

export interface ScratchpadAnchorResolution {
  anchor_state: ScratchpadAnchorState;
  current_start: number | null;
  current_end: number | null;
  current_start_line: number | null;
  current_end_line: number | null;
}

export interface ScratchpadSelectionAnchor {
  quote: string;
  anchor_start: number;
  anchor_end: number;
  anchor_prefix: string;
  anchor_suffix: string;
}

export interface PositionMapper {
  mapPos(position: number, association?: -1 | 1): number;
}

const CONTEXT_CHARACTERS = 64;

function anchored(content: string, quote: string, start: number, end: number): ScratchpadAnchorResolution {
  const startLine = content.slice(0, start).split('\n').length;
  return {
    anchor_state: 'anchored',
    current_start: start,
    current_end: end,
    current_start_line: startLine,
    current_end_line: startLine + quote.split('\n').length - 1
  };
}

export function resolveScratchpadAnchor(
  content: string,
  anchor: ScratchpadAnchorInput
): ScratchpadAnchorResolution {
  const quote = anchor.quote;
  if (quote === null) {
    return {
      anchor_state: 'unanchored',
      current_start: null,
      current_end: null,
      current_start_line: null,
      current_end_line: null
    };
  }
  if (
    anchor.anchor_start !== null &&
    anchor.anchor_end !== null &&
    content.slice(anchor.anchor_start, anchor.anchor_end) === quote
  ) {
    return anchored(content, quote, anchor.anchor_start, anchor.anchor_end);
  }

  const candidates: Array<[number, number]> = [];
  for (let start = content.indexOf(quote); start >= 0; start = content.indexOf(quote, start + 1)) {
    candidates.push([start, start + quote.length]);
  }
  const contextual = candidates.filter(([start, end]) =>
    (!anchor.anchor_prefix || content.slice(0, start).endsWith(anchor.anchor_prefix)) &&
    (!anchor.anchor_suffix || content.slice(end).startsWith(anchor.anchor_suffix))
  );
  const match = contextual.length === 1
    ? contextual[0]
    : contextual.length === 0 && candidates.length === 1
      ? candidates[0]
      : null;
  if (match) return anchored(content, quote, match[0], match[1]);
  return {
    anchor_state: 'orphaned',
    current_start: null,
    current_end: null,
    current_start_line: null,
    current_end_line: null
  };
}

export function selectionAnchor(content: string, start: number, end: number): ScratchpadSelectionAnchor {
  return {
    quote: content.slice(start, end),
    anchor_start: start,
    anchor_end: end,
    anchor_prefix: Array.from(content.slice(0, start)).slice(-CONTEXT_CHARACTERS).join(''),
    anchor_suffix: Array.from(content.slice(end)).slice(0, CONTEXT_CHARACTERS).join('')
  };
}

/** Map a live selection anchor through a CodeMirror ChangeSet-compatible mapper. */
export function mapScratchpadSelectionAnchor(
  anchor: ScratchpadSelectionAnchor,
  nextContent: string,
  changes: PositionMapper
): ScratchpadSelectionAnchor {
  const start = changes.mapPos(anchor.anchor_start, -1);
  const end = changes.mapPos(anchor.anchor_end, 1);
  return selectionAnchor(nextContent, start, end);
}
