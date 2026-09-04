import type {
  RecordedFeedback,
  RecordedFeedbackBlock,
  RecordedFeedbackSnapshot,
  RecordedFeedbackTranscriptSegment
} from './recordedFeedback';

const PARAGRAPH_GAP_MS = 700;

/** Only compile the new segment; the daemon appends it to the saved, possibly edited document. */
export function compileFeedbackRecording(
  feedback: RecordedFeedback,
  segments: RecordedFeedbackTranscriptSegment[]
): RecordedFeedbackBlock[] {
  const checkpoint = feedback.append_state;
  const snapshots = checkpoint
    ? feedback.snapshots.filter((snapshot) => snapshot.ordinal >= checkpoint.next_ordinal)
      .map((snapshot) => ({ ...snapshot, anchor_ms: snapshot.anchor_ms - checkpoint.duration_ms }))
    : feedback.snapshots;
  return compileFeedbackTimeline(segments, snapshots);
}

/** Build an ordered review document from timed speech and sample-anchored screenshots. */
export function compileFeedbackTimeline(
  rawSegments: RecordedFeedbackTranscriptSegment[],
  rawSnapshots: RecordedFeedbackSnapshot[]
): RecordedFeedbackBlock[] {
  const segments = rawSegments
    .filter((segment) => Number.isFinite(segment.start_ms) && Number.isFinite(segment.end_ms))
    .map((segment) => ({
      start_ms: Math.max(0, Math.round(segment.start_ms)),
      end_ms: Math.max(0, Math.round(segment.end_ms)),
      text: normalizeSpeech(segment.text)
    }))
    .filter((segment) => segment.text.length > 0 && segment.end_ms >= segment.start_ms)
    .sort((left, right) => left.start_ms - right.start_ms || left.end_ms - right.end_ms);
  const snapshots = [...rawSnapshots].sort((left, right) =>
    left.anchor_samples - right.anchor_samples
      || left.anchor_ms - right.anchor_ms
      || left.ordinal - right.ordinal
      || left.id - right.id
  );

  const blocks: RecordedFeedbackBlock[] = [];
  let snapshotIndex = 0;
  let pendingText: Extract<RecordedFeedbackBlock, { kind: 'text' }> | null = null;

  const flushText = () => {
    if (!pendingText) return;
    blocks.push(pendingText);
    pendingText = null;
  };
  const appendImagesThrough = (anchorMs: number) => {
    while (snapshotIndex < snapshots.length && snapshots[snapshotIndex].anchor_ms <= anchorMs) {
      flushText();
      blocks.push({ kind: 'image', snapshot_id: snapshots[snapshotIndex].id });
      snapshotIndex += 1;
    }
  };

  for (const segment of segments) {
    appendImagesThrough(segment.start_ms - 1);
    if (pendingText && segment.start_ms - pendingText.end_ms > PARAGRAPH_GAP_MS) flushText();
    if (!pendingText) {
      pendingText = {
        kind: 'text',
        text: segment.text,
        start_ms: segment.start_ms,
        end_ms: segment.end_ms
      };
    } else {
      pendingText.text = joinSpeech(pendingText.text, segment.text);
      pendingText.end_ms = Math.max(pendingText.end_ms, segment.end_ms);
    }
    // Segment timestamps are the honest fallback when word timings are unavailable.
    appendImagesThrough(segment.end_ms);
  }
  flushText();
  while (snapshotIndex < snapshots.length) {
    blocks.push({ kind: 'image', snapshot_id: snapshots[snapshotIndex].id });
    snapshotIndex += 1;
  }
  return blocks;
}

export function moveFeedbackBlock(
  blocks: RecordedFeedbackBlock[],
  from: number,
  to: number
): RecordedFeedbackBlock[] {
  if (from < 0 || from >= blocks.length || to < 0 || to >= blocks.length || from === to) {
    return blocks;
  }
  const next = [...blocks];
  const [block] = next.splice(from, 1);
  next.splice(to, 0, block);
  return next;
}

export function replaceFeedbackText(
  blocks: RecordedFeedbackBlock[],
  index: number,
  text: string
): RecordedFeedbackBlock[] {
  const block = blocks[index];
  if (!block || block.kind !== 'text') return blocks;
  const next = [...blocks];
  next[index] = { ...block, text };
  return next;
}

export function removeFeedbackBlock(
  blocks: RecordedFeedbackBlock[],
  index: number
): RecordedFeedbackBlock[] {
  return index < 0 || index >= blocks.length
    ? blocks
    : blocks.filter((_, candidate) => candidate !== index);
}

function normalizeSpeech(value: string): string {
  return value.replace(/\s+/g, ' ').trim();
}

function joinSpeech(left: string, right: string): string {
  if (!left) return right;
  if (!right) return left;
  return `${left}${/\s$/.test(left) || /^[,.;:!?)]/.test(right) ? '' : ' '}${right}`;
}
