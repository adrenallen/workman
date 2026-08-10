import type { RenderedProcessOutput } from './daemon';

/** Distinguish a genuinely new PTY from one whose rendered screen must paint immediately. */
export function hasRetainedTerminalOutput(
  output: Pick<RenderedProcessOutput, 'text' | 'raw_end_offset'>
): boolean {
  return output.raw_end_offset > 0 || output.text.length > 0;
}
