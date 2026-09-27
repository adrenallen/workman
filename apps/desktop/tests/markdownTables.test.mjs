import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';
import { ChangeSet, Text } from '@codemirror/state';

const sourcePath = new URL('../src/lib/markdownTables.ts', import.meta.url);
const source = await readFile(sourcePath, 'utf8');
const codeBlocks = await readFile(new URL('../src/lib/markdownCodeBlocks.ts', import.meta.url), 'utf8');
const combined = `${source.replace("import { markdownCodeBlocks, type MarkdownCodeBlock } from './markdownCodeBlocks';", '')}\n${codeBlocks}`;
const output = ts.transpileModule(combined, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 }
}).outputText;
const moduleUrl = `data:text/javascript;base64,${Buffer.from(output).toString('base64')}`;
const {
  formatMarkdownTable,
  mapMarkdownTableCellPosition,
  markdownTableFormattingChanges,
  markdownTableHeaderStartsAt,
  navigableTableCells,
  parseMarkdownInline,
  parseMarkdownTables,
  safeMarkdownInlineTokens,
  tableAtPosition
} = await import(moduleUrl);

test('parses optional pipes, alignments, source ranges, and ragged rows', () => {
  const source = [
    'before',
    'Name | Score | Notes',
    ':--- | :---: | ---:',
    '| Ada | 10 |',
    '| Grace | 9 | compiler | extra |',
    'after'
  ].join('\n');
  const tables = parseMarkdownTables(source);
  assert.equal(tables.length, 1);
  const [table] = tables;
  assert.deepEqual(table.alignments, ['left', 'center', 'right', null]);
  assert.equal(table.columnCount, 4);
  assert.deepEqual(table.header.cells.map((cell) => cell.text), ['Name', 'Score', 'Notes', '']);
  assert.deepEqual(table.rows[0].cells.map((cell) => cell.text), ['Ada', '10', '', '']);
  assert.deepEqual(table.rows[1].cells.map((cell) => cell.text), ['Grace', '9', 'compiler', 'extra']);
  const ada = table.rows[0].cells[0];
  assert.equal(source.slice(ada.from, ada.to), 'Ada');
  assert.equal(tableAtPosition(tables, ada.from), table);
  assert.equal(navigableTableCells(table).length, 12);
});

test('preserves escaped pipes and ignores pipes inside matched code spans', () => {
  const source = [
    '| Input | Output |',
    '| --- | --- |',
    '| a\\|b | `x | y` |',
    '| ``a ` b | c`` | done |'
  ].join('\n');
  const [table] = parseMarkdownTables(source);
  assert.deepEqual(table.rows[0].cells.map((cell) => cell.text), ['a\\|b', '`x | y`']);
  assert.deepEqual(table.rows[1].cells.map((cell) => cell.text), ['``a ` b | c``', 'done']);
  assert.deepEqual(parseMarkdownInline('a\\|b and `x | y`', { tableCell: true }).map((token) => token.text), [
    'a|b and ', 'x | y'
  ]);
});

test('excludes backtick and tilde fenced code while parsing tables outside fences', () => {
  const source = [
    '```md',
    '| fake | table |',
    '| --- | --- |',
    '```',
    '~~~',
    '| also | fake |',
    '| --- | --- |',
    '~~~',
    '| real | table |',
    '| --- | :---: |',
    '| yes | here |'
  ].join('\n');
  const tables = parseMarkdownTables(source);
  assert.equal(tables.length, 1);
  assert.deepEqual(tables[0].header.cells.map((cell) => cell.text), ['real', 'table']);
});

test('formats idempotently without dropping long rows or special cell content', () => {
  const source = [
    'name|value',
    ':--|--:',
    'short|a\\|b',
    'longer | `x | y` | extra'
  ].join('\n');
  const [table] = parseMarkdownTables(source);
  const formatted = formatMarkdownTable(table);
  assert.equal(formatted, [
    '| name   | value   |       |',
    '| :----- | ------: | ----- |',
    '| short  | a\\|b    |       |',
    '| longer | `x | y` | extra |'
  ].join('\n'));
  const [again] = parseMarkdownTables(formatted);
  assert.equal(formatMarkdownTable(again), formatted);
  const changes = markdownTableFormattingChanges(source, table);
  assert.equal(ChangeSet.of(changes, source.length).apply(Text.of(source.split('\n'))).toString(), formatted);
  assert.deepEqual(markdownTableFormattingChanges(formatted, again), []);
  assert.deepEqual(again.rows[1].cells.map((cell) => cell.text), ['longer', '`x | y`', 'extra']);
});

test('format changes preserve positions anchored inside cell content', () => {
  const source = 'Name|Score\n---|---:\nAda|10';
  const [table] = parseMarkdownTables(source);
  const changes = ChangeSet.of(markdownTableFormattingChanges(source, table), source.length);
  const from = source.indexOf('Ada');
  const to = from + 3;
  const formatted = changes.apply(Text.of(source.split('\n'))).toString();
  const formattedTable = parseMarkdownTables(formatted)[0];
  const mappedFrom = mapMarkdownTableCellPosition(table, formattedTable, from);
  const mappedTo = mapMarkdownTableCellPosition(table, formattedTable, to);
  assert.equal(formatted.slice(mappedFrom, mappedTo), 'Ada');
  assert.equal(formatted, formatMarkdownTable(table));
});

test('parses bold, italic, inline code, and links for rendered cells', () => {
  assert.deepEqual(parseMarkdownInline(
    '**bold** *italics* _also_ `code` [link](https://example.com)',
    { tableCell: true }
  ), [
    { kind: 'strong', text: 'bold' },
    { kind: 'text', text: ' ' },
    { kind: 'emphasis', text: 'italics' },
    { kind: 'text', text: ' ' },
    { kind: 'emphasis', text: 'also' },
    { kind: 'text', text: ' ' },
    { kind: 'code', text: 'code' },
    { kind: 'text', text: ' ' },
    { kind: 'link', text: 'link', href: 'https://example.com', source: '[link](https://example.com)' }
  ]);
});

test('rejects pipe-shaped prose without a delimiter row', () => {
  assert.deepEqual(parseMarkdownTables('one | two\nnot a delimiter | still prose'), []);
  assert.deepEqual(parseMarkdownTables('# Heading | text\n--- | ---'), []);
  assert.deepEqual(parseMarkdownTables('one | two\n---'), []);
});

test('stops body rows at Markdown blocks while allowing ragged table rows', () => {
  for (const boundary of [
    '# Heading | text',
    '> Quote | text',
    '- List | text',
    '1. List | text',
    '---',
    '```md\ninside | fence\n--- | ---\n```'
  ]) {
    const [table] = parseMarkdownTables(`A | B\n--- | ---\nleft | right\n${boundary}`);
    assert.equal(table.rows.length, 1, boundary);
  }
  const [ragged] = parseMarkdownTables('A | B\n--- | ---\none |\nleft | right | extra');
  assert.equal(ragged.columnCount, 3);
});

test('preserves header indentation on every formatted row and shares list-aware fences', () => {
  const [table] = parseMarkdownTables('  A | B\n  --- | ---\none | two');
  assert.equal(formatMarkdownTable(table), [
    '  | A   | B   |',
    '  | --- | --- |',
    '  | one | two |'
  ].join('\n'));
  const fenced = '- item\n  ```md\n  fake | table\n  --- | ---\n  ```\n\nreal | table\n--- | ---';
  assert.deepEqual(parseMarkdownTables(fenced).map((candidate) => candidate.header.cells[0].text), ['real']);

  const lenientClose = '- Install\n  ```bash\n  npm test\n```\n\nNext | Value\n--- | ---';
  assert.deepEqual(parseMarkdownTables(lenientClose).map((candidate) => candidate.header.cells[0].text), ['Next']);
});

test('keeps non-table inline rendering compatible and unescapes only table-cell pipes', () => {
  for (const source of ['todo_lock then todo_get', 'CARGO_TARGET_DIR', '2 * 3 * 4']) {
    assert.deepEqual(parseMarkdownInline(source), [{ kind: 'text', text: source }]);
  }
  assert.deepEqual(parseMarkdownInline('`C:\\tmp\\work`'), [
    { kind: 'code', text: 'C:\\tmp\\work' }
  ]);
  assert.deepEqual(parseMarkdownInline('a\\|b'), [{ kind: 'text', text: 'a\\|b' }]);
  assert.deepEqual(parseMarkdownInline('a\\|b', { tableCell: true }), [{ kind: 'text', text: 'a|b' }]);
  assert.deepEqual(parseMarkdownInline('2 * 3 * 4', { tableCell: true }), [
    { kind: 'text', text: '2 * 3 * 4' }
  ]);
  assert.deepEqual(parseMarkdownInline('Python __init__ and __main__'), [
    { kind: 'text', text: 'Python __init__ and __main__' }
  ]);
  assert.deepEqual(parseMarkdownInline('`a\\|b`', { tableCell: true }), [
    { kind: 'code', text: 'a|b' }
  ]);
});

test('only Enter before the first header content inserts above the table', () => {
  const source = '| Name | Value |\n| --- | --- |\n| one | two |';
  const [table] = parseMarkdownTables(source);
  assert.equal(markdownTableHeaderStartsAt(source, table, table.header.from), true);
  assert.equal(markdownTableHeaderStartsAt(source, table, table.header.from + 1), true);
  assert.equal(markdownTableHeaderStartsAt(source, table, table.header.cells[0].to), false);
  assert.equal(markdownTableHeaderStartsAt(source, table, table.header.cells[1].from + 1), false);
});

test('downgrades unsafe rendered links to their literal Markdown source', () => {
  const tokens = parseMarkdownInline('[safe](https://example.com) [bad](javascript:alert(1))');
  assert.deepEqual(safeMarkdownInlineTokens(tokens, (url) => url.startsWith('https://')), [
    { kind: 'link', text: 'safe', href: 'https://example.com', source: '[safe](https://example.com)' },
    { kind: 'text', text: ' ' },
    { kind: 'text', text: '[bad](javascript:alert(1)' },
    { kind: 'text', text: ')' }
  ]);
});
