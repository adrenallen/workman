import { pointIsInsideRect } from './terminalInput.ts';

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
