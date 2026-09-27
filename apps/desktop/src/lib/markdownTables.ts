export type MarkdownTableAlignment = 'left' | 'center' | 'right' | null;

export interface MarkdownTableCell {
  from: number;
  to: number;
  text: string;
}

export interface MarkdownTableRow {
  from: number;
  to: number;
  line: number;
  cells: MarkdownTableCell[];
}

export interface MarkdownTable {
  from: number;
  to: number;
  startLine: number;
  endLine: number;
  columnCount: number;
  alignments: MarkdownTableAlignment[];
  header: MarkdownTableRow;
  delimiter: MarkdownTableRow;
  rows: MarkdownTableRow[];
}

export interface MarkdownTableChange {
  from: number;
  to: number;
  insert: string;
}

export type MarkdownInlineToken =
  | { kind: 'text'; text: string }
  | { kind: 'strong'; text: string }
  | { kind: 'emphasis'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'link'; text: string; href: string; source: string };

interface SourceLine {
  from: number;
  to: number;
  text: string;
  number: number;
  fenced: boolean;
}

interface SplitRow {
  cells: MarkdownTableCell[];
  separatorCount: number;
}

function backtickRun(text: string, start: number): number {
  let end = start;
  while (text[end] === '`') end += 1;
  return end - start;
}

function hasClosingBackticks(text: string, start: number, length: number): boolean {
  for (let index = start; index < text.length;) {
    if (text[index] !== '`') {
      index += 1;
      continue;
    }
    const run = backtickRun(text, index);
    if (run === length) return true;
    index += run;
  }
  return false;
}

function escaped(text: string, index: number): boolean {
  let slashes = 0;
  for (let cursor = index - 1; cursor >= 0 && text[cursor] === '\\'; cursor -= 1) slashes += 1;
  return slashes % 2 === 1;
}

function cell(source: string, lineFrom: number, from: number, to: number): MarkdownTableCell {
  while (from < to && /\s/.test(source[from])) from += 1;
  while (to > from && /\s/.test(source[to - 1])) to -= 1;
  return { from: lineFrom + from, to: lineFrom + to, text: source.slice(from, to) };
}

function splitRow(source: string, lineFrom: number): SplitRow {
  const boundaries: number[] = [];
  let codeRun = 0;
  for (let index = 0; index < source.length;) {
    if (source[index] === '`') {
      const run = backtickRun(source, index);
      if (codeRun === run) codeRun = 0;
      else if (codeRun === 0 && hasClosingBackticks(source, index + run, run)) codeRun = run;
      index += run;
      continue;
    }
    if (source[index] === '|' && codeRun === 0 && !escaped(source, index)) boundaries.push(index);
    index += 1;
  }

  const cells: MarkdownTableCell[] = [];
  let start = 0;
  for (const boundary of boundaries) {
    cells.push(cell(source, lineFrom, start, boundary));
    start = boundary + 1;
  }
  cells.push(cell(source, lineFrom, start, source.length));
  if (boundaries[0] !== undefined && source.slice(0, boundaries[0]).trim() === '') cells.shift();
  const lastBoundary = boundaries.at(-1);
  if (lastBoundary !== undefined && source.slice(lastBoundary + 1).trim() === '') cells.pop();
  return { cells, separatorCount: boundaries.length };
}

function delimiterAlignment(text: string): MarkdownTableAlignment | undefined {
  const match = /^(:)?(-+)(:)?$/.exec(text.trim());
  if (!match) return undefined;
  if (match[1] && match[3]) return 'center';
  if (match[1]) return 'left';
  if (match[3]) return 'right';
  return null;
}

function sourceLines(markdown: string): SourceLine[] {
  const lines: SourceLine[] = [];
  let from = 0;
  let fence: { marker: '`' | '~'; length: number } | null = null;
  const rawLines = markdown.replaceAll('\r\n', '\n').replaceAll('\r', '\n').split('\n');
  for (let index = 0; index < rawLines.length; index += 1) {
    const text = rawLines[index];
    const marker = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(text);
    const fenced = fence !== null || marker !== null;
    lines.push({ from, to: from + text.length, text, number: index, fenced });
    if (marker) {
      const candidate = marker[1];
      if (!fence) {
        fence = { marker: candidate[0] as '`' | '~', length: candidate.length };
      } else if (
        candidate[0] === fence.marker &&
        candidate.length >= fence.length &&
        marker[2].trim() === ''
      ) {
        fence = null;
      }
    }
    from += text.length + (index < rawLines.length - 1 ? 1 : 0);
  }
  return lines;
}

function padRow(row: MarkdownTableRow, columnCount: number): MarkdownTableRow {
  if (row.cells.length >= columnCount) return row;
  return {
    ...row,
    cells: [
      ...row.cells,
      ...Array.from({ length: columnCount - row.cells.length }, () => ({
        from: row.to,
        to: row.to,
        text: ''
      }))
    ]
  };
}

function tableRow(line: SourceLine, parsed: SplitRow): MarkdownTableRow {
  return { from: line.from, to: line.to, line: line.number, cells: parsed.cells };
}

function canStartTable(line: string): boolean {
  return !/^\s*(?:#{1,6}\s|>|[-+*]\s+|\d+\.\s+)/.test(line);
}

export function parseMarkdownTables(markdown: string): MarkdownTable[] {
  const lines = sourceLines(markdown);
  const tables: MarkdownTable[] = [];
  for (let index = 0; index + 1 < lines.length;) {
    const headerLine = lines[index];
    const delimiterLine = lines[index + 1];
    if (
      headerLine.fenced || delimiterLine.fenced || !headerLine.text.trim() ||
      !canStartTable(headerLine.text)
    ) {
      index += 1;
      continue;
    }
    const header = splitRow(headerLine.text, headerLine.from);
    const delimiter = splitRow(delimiterLine.text, delimiterLine.from);
    const alignments = delimiter.cells.map((candidate) => delimiterAlignment(candidate.text));
    if (
      (header.separatorCount === 0 && delimiter.separatorCount === 0) ||
      header.cells.length === 0 || delimiter.cells.length === 0 ||
      alignments.some((alignment) => alignment === undefined)
    ) {
      index += 1;
      continue;
    }

    const body: MarkdownTableRow[] = [];
    let cursor = index + 2;
    while (cursor < lines.length) {
      const line = lines[cursor];
      if (line.fenced || !line.text.trim()) break;
      const parsed = splitRow(line.text, line.from);
      if (parsed.separatorCount === 0) break;
      body.push(tableRow(line, parsed));
      cursor += 1;
    }
    const rawHeader = tableRow(headerLine, header);
    const rawDelimiter = tableRow(delimiterLine, delimiter);
    const columnCount = Math.max(
      rawHeader.cells.length,
      rawDelimiter.cells.length,
      ...body.map((row) => row.cells.length)
    );
    const paddedAlignments = Array.from(
      { length: columnCount },
      (_, column) => alignments[column] ?? null
    ) as MarkdownTableAlignment[];
    const paddedBody = body.map((row) => padRow(row, columnCount));
    const lastRow = paddedBody.at(-1) ?? rawDelimiter;
    tables.push({
      from: rawHeader.from,
      to: lastRow.to,
      startLine: index,
      endLine: lastRow.line,
      columnCount,
      alignments: paddedAlignments,
      header: padRow(rawHeader, columnCount),
      delimiter: padRow(rawDelimiter, columnCount),
      rows: paddedBody
    });
    index = cursor;
  }
  return tables;
}

function delimiterCell(alignment: MarkdownTableAlignment, width: number): string {
  width = Math.max(3, width);
  if (alignment === 'left') return `:${'-'.repeat(width - 1)}`;
  if (alignment === 'right') return `${'-'.repeat(width - 1)}:`;
  if (alignment === 'center') return `:${'-'.repeat(width - 2)}:`;
  return '-'.repeat(width);
}

interface FormattedRow {
  text: string;
  cells: Array<{ from: number; to: number }>;
}

function tableWidths(table: MarkdownTable): number[] {
  const rows = [table.header, ...table.rows];
  return Array.from({ length: table.columnCount }, (_, column) => Math.max(
    3,
    ...rows.map((row) => row.cells[column]?.text.length ?? 0)
  ));
}

function formattedRow(row: MarkdownTableRow, widths: number[]): FormattedRow {
  let text = '| ';
  const cells: Array<{ from: number; to: number }> = [];
  widths.forEach((width, column) => {
    const content = row.cells[column]?.text ?? '';
    const from = text.length;
    text += content;
    cells.push({ from, to: text.length });
    text += content.padEnd(width).slice(content.length);
    text += column === widths.length - 1 ? ' |' : ' | ';
  });
  return { text, cells };
}

function formattedDelimiter(table: MarkdownTable, widths: number[]): string {
  return `| ${widths
    .map((width, column) => delimiterCell(table.alignments[column] ?? null, width))
    .join(' | ')} |`;
}

export function formatMarkdownTable(table: MarkdownTable): string {
  const widths = tableWidths(table);
  return [
    formattedRow(table.header, widths).text,
    formattedDelimiter(table, widths),
    ...table.rows.map((row) => formattedRow(row, widths).text)
  ].join('\n');
}

function addChange(
  changes: MarkdownTableChange[],
  markdown: string,
  from: number,
  to: number,
  insert: string
): void {
  if (markdown.slice(from, to) !== insert) changes.push({ from, to, insert });
}

function rowFormattingChanges(
  markdown: string,
  row: MarkdownTableRow,
  formatted: FormattedRow,
  changes: MarkdownTableChange[]
): void {
  let sourceCursor = row.from;
  let formattedCursor = 0;
  row.cells.forEach((cell, column) => {
    if (cell.to <= cell.from) return;
    const target = formatted.cells[column];
    addChange(
      changes,
      markdown,
      sourceCursor,
      cell.from,
      formatted.text.slice(formattedCursor, target.from)
    );
    sourceCursor = cell.to;
    formattedCursor = target.to;
  });
  addChange(
    changes,
    markdown,
    sourceCursor,
    row.to,
    formatted.text.slice(formattedCursor)
  );
}

function delimiterFormattingChange(
  markdown: string,
  row: MarkdownTableRow,
  formatted: string,
  changes: MarkdownTableChange[]
): void {
  const source = markdown.slice(row.from, row.to);
  let prefix = 0;
  while (prefix < source.length && prefix < formatted.length && source[prefix] === formatted[prefix]) {
    prefix += 1;
  }
  let sourceEnd = source.length;
  let formattedEnd = formatted.length;
  while (
    sourceEnd > prefix && formattedEnd > prefix &&
    source[sourceEnd - 1] === formatted[formattedEnd - 1]
  ) {
    sourceEnd -= 1;
    formattedEnd -= 1;
  }
  addChange(
    changes,
    markdown,
    row.from + prefix,
    row.from + sourceEnd,
    formatted.slice(prefix, formattedEnd)
  );
}

export function markdownTableFormattingChanges(
  markdown: string,
  table: MarkdownTable
): MarkdownTableChange[] {
  const widths = tableWidths(table);
  const changes: MarkdownTableChange[] = [];
  rowFormattingChanges(markdown, table.header, formattedRow(table.header, widths), changes);
  delimiterFormattingChange(markdown, table.delimiter, formattedDelimiter(table, widths), changes);
  for (const row of table.rows) {
    rowFormattingChanges(markdown, row, formattedRow(row, widths), changes);
  }
  return changes.sort((left, right) => left.from - right.from || left.to - right.to);
}

export function tableAtPosition(
  tables: readonly MarkdownTable[],
  position: number
): MarkdownTable | null {
  return tables.find((table) => position >= table.from && position <= table.to) ?? null;
}

export function navigableTableCells(table: MarkdownTable): MarkdownTableCell[] {
  return [table.header, ...table.rows].flatMap((row) => row.cells);
}

export function mapMarkdownTableCellPosition(
  table: MarkdownTable,
  formatted: MarkdownTable,
  position: number
): number | null {
  const sourceRows = [table.header, ...table.rows];
  const formattedRows = [formatted.header, ...formatted.rows];
  for (let row = 0; row < sourceRows.length; row += 1) {
    for (let column = 0; column < table.columnCount; column += 1) {
      const source = sourceRows[row]?.cells[column];
      const target = formattedRows[row]?.cells[column];
      if (
        !source || !target || source.to <= source.from ||
        position < source.from || position > source.to
      ) continue;
      return table.from + target.from + Math.min(position - source.from, target.to - target.from);
    }
  }
  return null;
}

function unescapePipes(text: string): string {
  return text.replaceAll(/\\([|\\])/g, '$1');
}

export function parseMarkdownInline(text: string): MarkdownInlineToken[] {
  const tokens: MarkdownInlineToken[] = [];
  const pattern = /(`+)([^\n]*?)\1|\*\*([^*\n]+)\*\*|__([^_\n]+)__|(?<!\*)\*([^*\n]+)\*(?!\*)|(?<!_)_([^_\n]+)_(?!_)|\[([^\]\n]+)\]\(([^)\n]+)\)/g;
  let offset = 0;
  for (const match of text.matchAll(pattern)) {
    const start = match.index ?? 0;
    if (start > offset) tokens.push({ kind: 'text', text: unescapePipes(text.slice(offset, start)) });
    if (match[1] !== undefined) {
      tokens.push({ kind: 'code', text: unescapePipes(match[2]) });
    } else if (match[3] !== undefined || match[4] !== undefined) {
      tokens.push({ kind: 'strong', text: unescapePipes(match[3] ?? match[4]) });
    } else if (match[5] !== undefined || match[6] !== undefined) {
      tokens.push({ kind: 'emphasis', text: unescapePipes(match[5] ?? match[6]) });
    } else {
      tokens.push({
        kind: 'link',
        text: unescapePipes(match[7]),
        href: match[8],
        source: match[0]
      });
    }
    offset = start + match[0].length;
  }
  if (offset < text.length) tokens.push({ kind: 'text', text: unescapePipes(text.slice(offset)) });
  return tokens;
}
