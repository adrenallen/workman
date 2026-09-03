export type AgentInputStep =
  | { kind: 'text'; text: string }
  | { kind: 'image'; path: string };

export interface AgentInputActions {
  send: (data: Uint8Array) => Promise<void>;
  writeImageToClipboard: (path: string) => Promise<void>;
  waitForImageImport: () => Promise<void>;
}

const encoder = new TextEncoder();
const pasteStart = '\x1b[200~';
const pasteEnd = '\x1b[201~';
const maxTextChunkCharacters = 64 * 1024;

/** Paste text and real clipboard images into an agent composer, then submit one ordered turn. */
export async function deliverAgentInput(
  steps: AgentInputStep[],
  actions: AgentInputActions
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

export function safeAgentTerminalText(value: string): string {
  return value
    .replace(/\r\n?/g, '\n')
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, '');
}

function isHighSurrogate(value: number): boolean {
  return value >= 0xd800 && value <= 0xdbff;
}

function isLowSurrogate(value: number): boolean {
  return value >= 0xdc00 && value <= 0xdfff;
}
