import type { RecordedFeedback } from './recordedFeedback';

export type FeedbackAgentInputStep =
  | { kind: 'text'; text: string }
  | { kind: 'image'; path: string };

export interface FeedbackAgentInputActions {
  send: (data: Uint8Array) => Promise<void>;
  writeImageToClipboard: (path: string) => Promise<void>;
  waitForImageImport: () => Promise<void>;
}

const encoder = new TextEncoder();
const pasteStart = '\x1b[200~';
const pasteEnd = '\x1b[201~';
const maxTextChunkCharacters = 64 * 1024;

/** Build the same transcript/image order the user reviewed, without reducing it to file links. */
export function feedbackAgentInputSteps(feedback: RecordedFeedback): FeedbackAgentInputStep[] {
  const steps: FeedbackAgentInputStep[] = [{
    kind: 'text',
    text: `Review and act on this recorded feedback: “${safeTerminalText(feedback.title)}”.`
  }];
  for (const block of feedback.blocks) {
    if (block.kind === 'text') {
      const text = safeTerminalText(block.text).trim();
      if (text) steps.push({ kind: 'text', text: `\n\n${text}` });
      continue;
    }
    const snapshot = feedback.snapshots.find((candidate) => candidate.id === block.snapshot_id);
    if (!snapshot) continue;
    const caption = safeTerminalText(snapshot.caption).trim();
    steps.push({
      kind: 'text',
      text: `\n\nScreenshot #${snapshot.ordinal + 1}${caption ? ` — ${caption}` : ''}:\n`
    });
    steps.push({ kind: 'image', path: snapshot.image_path });
  }
  steps.push({
    kind: 'text',
    text: '\n\nUse the transcript and screenshots above in order, then make the requested changes.'
  });
  return steps;
}

/** Paste each image into the live agent composer, then submit the assembled turn once. */
export async function deliverFeedbackAgentInput(
  steps: FeedbackAgentInputStep[],
  actions: FeedbackAgentInputActions
): Promise<void> {
  for (const step of steps) {
    if (step.kind === 'image') {
      await actions.writeImageToClipboard(step.path);
      await actions.send(Uint8Array.of(0x16));
      await actions.waitForImageImport();
      continue;
    }
    for (let offset = 0; offset < step.text.length;) {
      let end = Math.min(step.text.length, offset + maxTextChunkCharacters);
      // Do not split a Unicode surrogate pair at the bounded PTY chunk boundary.
      if (
        end < step.text.length
        && isHighSurrogate(step.text.charCodeAt(end - 1))
        && isLowSurrogate(step.text.charCodeAt(end))
      ) end -= 1;
      const chunk = step.text.slice(offset, end);
      if (chunk) await actions.send(encoder.encode(`${pasteStart}${chunk}${pasteEnd}`));
      offset = end;
    }
  }
  await actions.send(Uint8Array.of(0x0d));
}

function isHighSurrogate(value: number): boolean {
  return value >= 0xd800 && value <= 0xdbff;
}

function isLowSurrogate(value: number): boolean {
  return value >= 0xdc00 && value <= 0xdfff;
}

function safeTerminalText(value: string): string {
  return value
    .replace(/\r\n?/g, '\n')
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, '');
}
