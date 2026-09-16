import type { TerminalFrame } from './daemon';

/** Checkpoints carry screen geometry and consume no bytes in the raw PTY stream. */
export function terminalFrameContent(frame: TerminalFrame): {
  data: Uint8Array;
  endOffset: number;
  geometry?: { rows: number; columns: number };
} {
  const data = Uint8Array.from(frame.data);
  if (!frame.checkpoint) return { data, endOffset: frame.start_offset + data.length };
  const checkpoint = JSON.parse(new TextDecoder().decode(data));
  if (!Number.isInteger(checkpoint.rows) || checkpoint.rows < 1 || checkpoint.rows > 65535
    || !Number.isInteger(checkpoint.columns) || checkpoint.columns < 1 || checkpoint.columns > 65535
    || typeof checkpoint.ansi !== 'string') {
    throw new Error('Invalid terminal screen checkpoint');
  }
  return {
    data: new TextEncoder().encode(checkpoint.ansi),
    endOffset: frame.start_offset,
    geometry: { rows: checkpoint.rows, columns: checkpoint.columns }
  };
}
