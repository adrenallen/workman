import type { RecordedFeedback } from './recordedFeedback';
import {
  defaultRecordedFeedbackAgentPrompt,
  recordedFeedbackPromptFrame
} from './recordedFeedbackPrompt.ts';

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
export function feedbackAgentInputSteps(
  feedback: RecordedFeedback,
  promptTemplate = defaultRecordedFeedbackAgentPrompt,
  leadingPrompt = ''
): FeedbackAgentInputStep[] {
  const frame = recordedFeedbackPromptFrame(
    safeTerminalText(promptTemplate),
    safeTerminalText(feedback.title)
  );
  const steps: FeedbackAgentInputStep[] = [];
  appendTextStep(steps, leadingPrompt);
  appendTextStep(steps, frame.before);
  for (const block of feedback.blocks) {
    if (block.kind === 'text') {
      appendTextStep(steps, block.text);
      continue;
    }
    const snapshot = feedback.snapshots.find((candidate) => candidate.id === block.snapshot_id);
    if (!snapshot) continue;
    const caption = safeTerminalText(snapshot.caption).trim();
    appendTextStep(
      steps,
      `Screenshot #${snapshot.ordinal + 1}${caption ? ` — ${caption}` : ''}:`
    );
    steps.push({ kind: 'image', path: snapshot.image_path });
  }
  appendTextStep(steps, frame.after);
  return steps;
}

function appendTextStep(steps: FeedbackAgentInputStep[], value: string): void {
  const text = safeTerminalText(value).trim();
  if (!text) return;
  steps.push({ kind: 'text', text: `${steps.length > 0 ? '\n\n' : ''}${text}` });
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
