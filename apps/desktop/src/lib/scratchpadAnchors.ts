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

/** Scratchpad storage, anchors, and CodeMirror all use LF line endings. */
export function normalizeScratchpadLineEndings(content: string): string {
  return content.replaceAll('\r\n', '\n').replaceAll('\r', '\n');
}

function anchored(content: string, start: number, end: number): ScratchpadAnchorResolution {
  const startLine = content.slice(0, start).split('\n').length;
  return {
    anchor_state: 'anchored',
    current_start: start,
    current_end: end,
    current_start_line: startLine,
    current_end_line: startLine + content.slice(start, end).split('\n').length - 1
  };
}

interface TolerantText {
  text: string;
  starts: number[];
  ends: number[];
}

/** Collapse formatting-only table differences while retaining real UTF-16 ranges. */
function tolerantText(source: string): TolerantText {
  let text = '';
  const starts: number[] = [];
  const ends: number[] = [];
  for (let index = 0; index < source.length;) {
    const point = source.codePointAt(index)!;
    const character = String.fromCodePoint(point);
    const length = character.length;
    const collapsed = /\s/u.test(character) ? ' ' : character === '-' ? '-' : null;
    if (collapsed !== null) {
      const start = index;
      index += length;
      while (index < source.length) {
        const nextPoint = source.codePointAt(index)!;
        const next = String.fromCodePoint(nextPoint);
        if (collapsed === ' ' ? !/\s/u.test(next) : next !== '-') break;
        index += next.length;
      }
      text += collapsed;
      starts.push(start);
      ends.push(index);
      continue;
    }
    text += character;
    for (let unit = 0; unit < length; unit += 1) starts.push(index);
    index += length;
    for (let unit = 0; unit < length; unit += 1) ends.push(index);
  }
  return { text, starts, ends };
}

function tolerantAnchorRange(
  content: string,
  quote: string,
  prefix: string | null,
  suffix: string | null
): [number, number] | null {
  const normalizedContent = tolerantText(content);
  const normalizedQuote = tolerantText(quote).text;
  if (!normalizedQuote) return null;
  const normalizedPrefix = prefix ? tolerantText(prefix).text : '';
  const normalizedSuffix = suffix ? tolerantText(suffix).text : '';
  const matches: Array<[number, number]> = [];
  for (
    let start = normalizedContent.text.indexOf(normalizedQuote);
    start >= 0;
    start = normalizedContent.text.indexOf(normalizedQuote, start + normalizedQuote.length)
  ) {
    const end = start + normalizedQuote.length;
    if (
      normalizedPrefix && (
        start < normalizedPrefix.length ||
        !normalizedContent.text.startsWith(normalizedPrefix, start - normalizedPrefix.length)
      ) ||
      normalizedSuffix && !normalizedContent.text.startsWith(normalizedSuffix, end)
    ) continue;
    const actualStart = normalizedContent.starts[start];
    const actualEnd = normalizedContent.ends[end - 1];
    if (actualStart !== undefined && actualEnd !== undefined) matches.push([actualStart, actualEnd]);
  }
  return matches.length === 1 ? matches[0] : null;
}

export function resolveScratchpadAnchor(
  content: string,
  anchor: ScratchpadAnchorInput
): ScratchpadAnchorResolution {
  content = normalizeScratchpadLineEndings(content);
  const quote = anchor.quote === null ? null : normalizeScratchpadLineEndings(anchor.quote);
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
    return anchored(content, anchor.anchor_start, anchor.anchor_end);
  }

  if (quote.length === 0) return {
    anchor_state: 'orphaned', current_start: null, current_end: null,
    current_start_line: null, current_end_line: null
  };
  const prefix = anchor.anchor_prefix === null
    ? null
    : normalizeScratchpadLineEndings(anchor.anchor_prefix);
  const suffix = anchor.anchor_suffix === null
    ? null
    : normalizeScratchpadLineEndings(anchor.anchor_suffix);
  const candidates: Array<[number, number]> = [];
  for (
    let start = content.indexOf(quote);
    start >= 0;
    start = content.indexOf(quote, start + quote.length)
  ) {
    candidates.push([start, start + quote.length]);
  }
  const contextual = candidates.filter(([start, end]) =>
    (!prefix || start >= prefix.length && content.startsWith(prefix, start - prefix.length)) &&
    (!suffix || content.startsWith(suffix, end))
  );
  const hasContext = Boolean(prefix || suffix);
  const match = hasContext
    ? contextual.length === 1 ? contextual[0] : null
    : candidates.length === 1 ? candidates[0] : null;
  if (match) return anchored(content, match[0], match[1]);
  const tolerant = tolerantAnchorRange(content, quote, prefix, suffix);
  if (tolerant) return anchored(content, tolerant[0], tolerant[1]);
  return {
    anchor_state: 'orphaned',
    current_start: null,
    current_end: null,
    current_start_line: null,
    current_end_line: null
  };
}

export function selectionAnchor(content: string, start: number, end: number): ScratchpadSelectionAnchor {
  content = normalizeScratchpadLineEndings(content);
  return {
    quote: content.slice(start, end),
    anchor_start: start,
    anchor_end: end,
    anchor_prefix: Array.from(content.slice(Math.max(0, start - CONTEXT_CHARACTERS * 2), start))
      .slice(-CONTEXT_CHARACTERS).join(''),
    anchor_suffix: Array.from(content.slice(end, end + CONTEXT_CHARACTERS * 2))
      .slice(0, CONTEXT_CHARACTERS).join('')
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
