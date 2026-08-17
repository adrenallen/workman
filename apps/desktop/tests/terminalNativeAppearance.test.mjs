import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  DEFAULT_APPEARANCE,
  shouldAutoImportTerminalProfile,
  terminalAppearancePatchFromImport,
  terminalFontCss,
  terminalProfileLetterSpacing,
  terminalProfileXtermOptions
} from '../src/lib/appearance.ts';

const nativeStyle = {
  fontFamily: 'SFMonoTerminal-Regular',
  fontSize: 12,
  lineHeight: 1,
  characterWidthMultiplier: 1,
  letterSpacing: 0,
  cursorStyle: 'block',
  cursorBlink: false,
  drawBoldTextInBrightColors: false
};

test('xterm keeps its native metrics unless a terminal profile supplies them', () => {
  assert.deepEqual(terminalProfileXtermOptions(null), {
    cursorBlink: false,
    cursorStyle: 'block',
    drawBoldTextInBrightColors: true,
    letterSpacing: 0,
    lineHeight: 1
  });
  assert.deepEqual(terminalProfileXtermOptions(nativeStyle), {
    cursorBlink: false,
    cursorStyle: 'block',
    drawBoldTextInBrightColors: false,
    letterSpacing: 0,
    lineHeight: 1
  });
  assert.equal(
    terminalFontCss('profile', nativeStyle),
    '"SFMonoTerminal-Regular", monospace'
  );
  assert.equal(terminalProfileLetterSpacing({
    ...nativeStyle,
    characterWidthMultiplier: 1.5,
    letterSpacing: null
  }, 'monospace', 10), 3);
});

test('native import maps palette and typography without replacing explicit themes', () => {
  const report = {
    imported: true,
    source: 'Terminal.app',
    profile: 'Clear Dark',
    palette: DEFAULT_APPEARANCE.terminalTheme.palette,
    terminalStyle: nativeStyle,
    message: 'Imported.'
  };
  const patch = terminalAppearancePatchFromImport(report, { ...DEFAULT_APPEARANCE });
  assert.equal(patch?.terminalTheme?.id, 'imported');
  assert.equal(patch?.terminalFont, 'profile');
  assert.equal(patch?.terminalFontSize, 12);
  assert.deepEqual(patch?.terminalProfileStyle, nativeStyle);
  assert.equal(shouldAutoImportTerminalProfile({ ...DEFAULT_APPEARANCE }, false), true);
  assert.equal(shouldAutoImportTerminalProfile({ ...DEFAULT_APPEARANCE }, true), false);
  assert.equal(shouldAutoImportTerminalProfile({
    ...DEFAULT_APPEARANCE,
    terminalFont: 'menlo'
  }, false), false);
  assert.equal(shouldAutoImportTerminalProfile({
    ...DEFAULT_APPEARANCE,
    terminalFontSize: 16
  }, false), false);
  assert.equal(
    shouldAutoImportTerminalProfile({
      ...DEFAULT_APPEARANCE,
      terminalTheme: { ...DEFAULT_APPEARANCE.terminalTheme, id: 'custom' }
    }, false),
    false
  );
});

test('an unavailable profile font preserves the current family and size', () => {
  const originalDocument = globalThis.document;
  globalThis.document = { fonts: { check: () => false } };
  try {
    const report = {
      imported: true,
      source: 'Terminal.app',
      profile: 'Clear Dark',
      palette: DEFAULT_APPEARANCE.terminalTheme.palette,
      terminalStyle: { ...nativeStyle, fontSize: 22.5 }
    };
    const current = { ...DEFAULT_APPEARANCE, terminalFont: 'menlo', terminalFontSize: 16 };
    const patch = terminalAppearancePatchFromImport(report, current);
    assert.equal(patch?.terminalFont, 'menlo');
    assert.equal(patch?.terminalFontSize, 16);
  } finally {
    if (originalDocument === undefined) delete globalThis.document;
    else globalThis.document = originalDocument;
  }
});

test('TerminalView does not reintroduce opinionated xterm styling overrides', async () => {
  const source = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const constructor = source.slice(source.indexOf('new Terminal({'), source.indexOf('});', source.indexOf('new Terminal({')));
  assert.doesNotMatch(constructor, /fontWeight|fontWeightBold|minimumContrastRatio|smoothScrollDuration/);
  assert.match(source, /onContextLoss[\s\S]*scheduleWebglRecovery/);
});
