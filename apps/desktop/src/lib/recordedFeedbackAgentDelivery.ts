import type { RecordedFeedback } from './recordedFeedback';
import {
  deliverAgentInput,
  safeAgentTerminalText,
  type AgentInputActions,
  type AgentInputStep
} from './agentInputDelivery.ts';
import {
  defaultRecordedFeedbackAgentPrompt,
  recordedFeedbackPromptFrame
} from './recordedFeedbackPrompt.ts';

export type FeedbackAgentInputStep = AgentInputStep;
export type FeedbackAgentInputActions = AgentInputActions;

/** Record the actual submission result, without confusing a failed receipt with a failed send. */
export async function trackFeedbackDelivery(
  send: () => Promise<void>,
  acknowledge: (error: string | null) => Promise<unknown>,
  onSent: () => Promise<void> = async () => {}
): Promise<void> {
  try {
    await send();
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : String(cause);
    try { await acknowledge(message); }
    catch { throw new Error(`${message} Delivery history could not be updated; this attempt is unconfirmed.`); }
    throw cause;
  }
  try {
    await onSent();
  } finally {
    try {
      await acknowledge(null);
    } catch (cause) {
      throw new Error(`Feedback was sent, but delivery history could not be updated: ${cause instanceof Error ? cause.message : String(cause)}`);
    }
  }
}

/** Build the same transcript/image order the user reviewed, without reducing it to file links. */
export function feedbackAgentInputSteps(
  feedback: RecordedFeedback,
  promptTemplate = defaultRecordedFeedbackAgentPrompt,
  leadingPrompt = ''
): FeedbackAgentInputStep[] {
  const frame = recordedFeedbackPromptFrame(
    safeAgentTerminalText(promptTemplate),
    safeAgentTerminalText(feedback.title)
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
    const caption = safeAgentTerminalText(snapshot.caption).trim();
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
  const text = safeAgentTerminalText(value).trim();
  if (!text) return;
  steps.push({ kind: 'text', text: `${steps.length > 0 ? '\n\n' : ''}${text}` });
}

/** Paste each image into the live agent composer, then submit the assembled turn once. */
export async function deliverFeedbackAgentInput(
  steps: FeedbackAgentInputStep[],
  actions: FeedbackAgentInputActions
): Promise<void> {
  await deliverAgentInput(steps, actions);
}
