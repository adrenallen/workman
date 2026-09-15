import assert from 'node:assert/strict';
import test from 'node:test';
import { TERMINAL_THEME_PRESETS } from '../src/lib/appearance.ts';
import { terminalDefaultColors } from '../src/lib/terminalColors.ts';

test('terminal host colors match every rendered theme and imported RGB colors', () => {
  for (const { palette } of TERMINAL_THEME_PRESETS) {
    const colors = terminalDefaultColors(palette);
    for (const key of ['foreground', 'background', 'cursor']) {
      assert.equal(colors[key].reduce((value, channel) => value * 256 + channel, 0), parseInt(palette[key].slice(1), 16));
    }
  }
  assert.deepEqual(terminalDefaultColors({ foreground: '#ABCDef', background: '#010203', cursor: '#000000' }), {
    foreground: [171, 205, 239], background: [1, 2, 3], cursor: [0, 0, 0]
  });
});
