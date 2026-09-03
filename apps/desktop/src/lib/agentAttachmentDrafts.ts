import { pointIsInsideRect } from './terminalInput.ts';
import { safeAgentTerminalText, type AgentInputStep } from './agentInputDelivery.ts';

export const maxAgentDraftAttachments = 8;

export interface NativeAttachmentDropPayload {
  type: 'enter' | 'over' | 'drop' | 'leave';
  paths?: readonly string[];
  position?: { x: number; y: number };
}

export interface AttachmentPathSelection {
  attachments: string[];
  added: string[];
  capReached: boolean;
}

export interface NativeAttachmentDropResult {
  dropActive: boolean;
  selection: AttachmentPathSelection | null;
}

export interface AgentDraftPromptInsertion {
  prompt: string;
  caret: number;
}

export interface AgentDraftAttachmentRemoval {
  prompt: string;
  attachments: string[];
}

const imageTokenPattern = /\[Image ([1-9][0-9]*)\]/gu;

export function agentDraftImageToken(index: number): string {
  return `[Image ${index + 1}]`;
}

export function insertAgentDraftImageTokens(
  prompt: string,
  selectionStart: number,
  selectionEnd: number,
  firstAttachmentIndex: number,
  count: number
): AgentDraftPromptInsertion {
  const start = Math.max(0, Math.min(prompt.length, selectionStart));
  const end = Math.max(start, Math.min(prompt.length, selectionEnd));
  const tokens = Array.from(
    { length: Math.max(0, count) },
    (_, offset) => agentDraftImageToken(firstAttachmentIndex + offset)
  ).join(' ');
  if (!tokens) return { prompt, caret: start };
  const before = prompt.slice(0, start);
  const after = prompt.slice(end);
  const prefix = before && !/\s$/u.test(before) ? ' ' : '';
  const suffix = after && !/^\s/u.test(after) ? ' ' : '';
  const insertion = `${prefix}${tokens}${suffix}`;
  return {
    prompt: `${before}${insertion}${after}`,
    caret: before.length + insertion.length
  };
}

export function removeAgentDraftAttachment(
  prompt: string,
  attachments: readonly string[],
  path: string
): AgentDraftAttachmentRemoval {
  const removedIndex = attachments.indexOf(path);
  if (removedIndex < 0) return { prompt, attachments: [...attachments] };
  const removedOrdinal = removedIndex + 1;
  let rewritten = '';
  let cursor = 0;
  for (const match of prompt.matchAll(imageTokenPattern)) {
    const matchIndex = match.index ?? 0;
    const ordinal = Number(match[1]);
    rewritten += prompt.slice(cursor, matchIndex);
    cursor = matchIndex + match[0].length;
    if (ordinal === removedOrdinal) {
      if (prompt[cursor] === ' ' && (rewritten.endsWith(' ') || matchIndex === 0)) cursor += 1;
      else if (cursor === prompt.length && rewritten.endsWith(' ')) rewritten = rewritten.slice(0, -1);
      continue;
    }
    rewritten += ordinal > removedOrdinal ? agentDraftImageToken(ordinal - 2) : match[0];
  }
  rewritten += prompt.slice(cursor);
  return {
    prompt: rewritten,
    attachments: attachments.filter((candidate) => candidate !== path)
  };
}

/** Convert visible image tokens into real inline image paste steps, preserving exact text order. */
export function agentDraftPromptInputSteps(
  prompt: string,
  attachments: readonly string[]
): AgentInputStep[] {
  const text = safeAgentTerminalText(prompt);
  const steps: AgentInputStep[] = [];
  const placed = new Set<number>();
  let cursor = 0;
  for (const match of text.matchAll(imageTokenPattern)) {
    const matchIndex = match.index ?? 0;
    const attachmentIndex = Number(match[1]) - 1;
    const path = attachments[attachmentIndex];
    if (!path) continue;
    if (matchIndex > cursor) steps.push({ kind: 'text', text: text.slice(cursor, matchIndex) });
    steps.push({ kind: 'image', path });
    placed.add(attachmentIndex);
    cursor = matchIndex + match[0].length;
  }
  if (cursor < text.length) steps.push({ kind: 'text', text: text.slice(cursor) });
  attachments.forEach((path, index) => {
    if (placed.has(index)) return;
    if (steps.length > 0) steps.push({ kind: 'text', text: '\n\n' });
    steps.push({ kind: 'image', path });
  });
  return steps;
}

export function isPlatformAbsolutePath(path: string): boolean {
  return /^(?:[A-Za-z]:[\\/]|[\\/])/u.test(path);
}

export function attachmentName(path: string): string {
  return path.split(/[\\/]/u).at(-1) || 'image';
}

export function isSupportedAttachmentPath(path: string): boolean {
  return isPlatformAbsolutePath(path)
    && /\.(?:png|jpe?g|gif|webp|bmp|tiff?)$/iu.test(path);
}

export function attachImagePaths(
  currentAttachments: readonly string[],
  candidatePaths: readonly string[],
  limit = maxAgentDraftAttachments
): AttachmentPathSelection {
  const attachments = currentAttachments.slice(0, limit);
  const existing = new Set(attachments);
  const added: string[] = [];
  for (const path of candidatePaths) {
    if (!isSupportedAttachmentPath(path) || existing.has(path)) continue;
    if (attachments.length >= limit) break;
    existing.add(path);
    attachments.push(path);
    added.push(path);
  }
  return {
    attachments,
    added,
    capReached: attachments.length >= limit
  };
}

export function handleNativePromptDrop(
  payload: NativeAttachmentDropPayload,
  rect: { left: number; top: number; right: number; bottom: number } | null,
  physicalScale: number,
  currentAttachments: readonly string[]
): NativeAttachmentDropResult {
  if (payload.type === 'leave') return { dropActive: false, selection: null };
  if (!rect || !payload.position) return { dropActive: false, selection: null };
  const inside = pointIsInsideRect(payload.position, rect, physicalScale);
  if (payload.type === 'enter' || payload.type === 'over') {
    return { dropActive: inside, selection: null };
  }
  return {
    dropActive: false,
    selection: inside ? attachImagePaths(currentAttachments, payload.paths ?? []) : null
  };
}
