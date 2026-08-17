import type { RenderedProcessOutput } from './daemon';

/** Distinguish a genuinely new PTY from one whose rendered screen must paint immediately. */
export function hasRetainedTerminalOutput(
  output: Pick<RenderedProcessOutput, 'text' | 'raw_end_offset'>
): boolean {
  return output.raw_end_offset > 0 || output.text.length > 0;
}

/** Plain text is a transition only; xterm owns the surface after the raw replay reaches its end. */
export function shouldShowRetainedPreview(
  output: Pick<RenderedProcessOutput, 'text'>,
  replayComplete: boolean
): boolean {
  return !replayComplete && output.text.length > 0;
}

/** Detect bytes that could not be replayed, since missing SGR state cannot be rendered honestly. */
export function rawReplayHasGap(requestedOffset: number, replayStartOffset: number): boolean {
  return replayStartOffset > requestedOffset;
}
