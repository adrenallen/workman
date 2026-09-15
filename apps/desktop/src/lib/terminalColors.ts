import type { TerminalPalette } from './appearance';

export function terminalDefaultColors(palette: Pick<TerminalPalette, 'foreground' | 'background' | 'cursor'>) {
  const rgb = (hex: string): [number, number, number] => {
    if (!/^#[0-9a-f]{6}$/i.test(hex)) throw new Error('Invalid terminal color');
    return [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16)) as [number, number, number];
  };
  return {
    foreground: rgb(palette.foreground),
    background: rgb(palette.background),
    cursor: rgb(palette.cursor)
  };
}
