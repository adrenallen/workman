import type { TodoPriority } from './coordination';

export type CreationDraftKind = 'agent' | 'command' | 'todo';

interface CreationDraftBase {
  id: number;
  projectId: number;
  kind: CreationDraftKind;
  createdAt: number;
  touched: boolean;
}

export interface AgentCreationDraft extends CreationDraftBase {
  kind: 'agent';
  agentToolId: number | null;
  templateId: number | null;
  name: string;
  prompt: string;
  attachments: string[];
  extraArgs: string;
}

export interface CommandCreationDraft extends CreationDraftBase {
  kind: 'command';
  name: string;
  command: string;
  workingDir: string;
  environment: string;
  restartWhenChanged: string;
  autoStart: boolean;
  autoRestart: boolean;
  saveMode: 'yml' | 'local';
}

export interface TodoCreationDraft extends CreationDraftBase {
  kind: 'todo';
  title: string;
  body: string;
  priority: TodoPriority;
  tags: string;
  blockerIds: number[];
}

export type CreationDraft =
  | AgentCreationDraft
  | CommandCreationDraft
  | TodoCreationDraft;

interface DraftStorage {
  version: 1;
  drafts: CreationDraft[];
}

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const storageKeyPrefix = 'workman.creation-drafts.v1';
const maxRestoredDrafts = 100;
const maxDraftTextLength = 256_000;
const maxRestoredDraftText = 1_000_000;
const maxRestoredBlockers = 1_000;
const maxAgentAttachments = 8;
const maxAttachmentPathLength = 4_096;
let lastAllocatedDraftId = 0;

export function creationDraftStorageKey(profileId: number): string {
  return `${storageKeyPrefix}.profile-${profileId}`;
}

export function createCreationDraft(
  kind: CreationDraftKind,
  projectId: number,
  id: number,
  createdAt = Date.now()
): CreationDraft {
  const base = { id, projectId, kind, createdAt, touched: false };
  if (kind === 'agent') {
    return {
      ...base,
      kind,
      agentToolId: null,
      templateId: null,
      name: '',
      prompt: '',
      attachments: [],
      extraArgs: ''
    };
  }
  if (kind === 'command') {
    return {
      ...base,
      kind,
      name: '',
      command: '',
      workingDir: '',
      environment: '',
      restartWhenChanged: '',
      autoStart: true,
      autoRestart: false,
      saveMode: 'yml'
    };
  }
  return {
    ...base,
    kind,
    title: '',
    body: '',
    priority: 'medium',
    tags: '',
    blockerIds: []
  };
}

export function isCreationDraftUntouched(draft: CreationDraft): boolean {
  return !draft.touched;
}

export function creationDraftHasContent(draft: CreationDraft): boolean {
  return !isCreationDraftUntouched(draft);
}

export function findUntouchedCreationDraft(
  drafts: readonly CreationDraft[],
  projectId: number,
  kind: CreationDraftKind
): CreationDraft | null {
  return drafts.find((draft) =>
    draft.projectId === projectId && draft.kind === kind && isCreationDraftUntouched(draft)
  ) ?? null;
}

export function creationDraftLabel(draft: CreationDraft): string {
  const value = draft.kind === 'todo' ? draft.title : draft.name;
  return value.trim() || `New ${draft.kind}`;
}

export function creationDraftsForCycle(
  drafts: readonly CreationDraft[],
  projectId: number,
  kind: 'agent' | 'command'
): CreationDraft[] {
  return drafts
    .filter((draft) => draft.projectId === projectId && draft.kind === kind)
    .sort((left, right) => left.createdAt - right.createdAt || left.id - right.id);
}

export function nextCreationDraftId(
  drafts: readonly CreationDraft[],
  now = Date.now()
): number {
  const timeBasedId = -Math.max(1, Math.floor(now));
  lastAllocatedDraftId = Math.min(
    timeBasedId,
    lastAllocatedDraftId,
    ...drafts.map((draft) => draft.id)
  ) - 1;
  return lastAllocatedDraftId;
}

export function pruneCreationDraftsToProjects(
  drafts: readonly CreationDraft[],
  projectIds: ReadonlySet<number>
): readonly CreationDraft[] {
  const next = drafts.filter((draft) => projectIds.has(draft.projectId));
  return next.length === drafts.length ? drafts : next;
}

export function loadCreationDrafts(
  profileId: number,
  storage: StorageLike | null = browserStorage()
): CreationDraft[] {
  if (!storage || !isPositiveInteger(profileId)) return [];
  try {
    const parsed = JSON.parse(storage.getItem(creationDraftStorageKey(profileId)) ?? 'null') as unknown;
    const candidates = Array.isArray(parsed)
      ? parsed
      : isRecord(parsed) && (parsed.version === 1 || parsed.version === undefined)
        ? parsed.drafts
        : null;
    if (!Array.isArray(candidates)) return [];
    const drafts: CreationDraft[] = [];
    const seenIds = new Set<number>();
    let restoredText = 0;
    for (const candidate of candidates) {
      const draft = parseCreationDraft(candidate);
      if (!draft || seenIds.has(draft.id)) continue;
      const textLength = creationDraftTextLength(draft);
      if (restoredText + textLength > maxRestoredDraftText) continue;
      seenIds.add(draft.id);
      restoredText += textLength;
      drafts.push(draft);
      if (drafts.length >= maxRestoredDrafts) break;
    }
    return drafts;
  } catch {
    return [];
  }
}

export function saveCreationDrafts(
  profileId: number,
  drafts: readonly CreationDraft[],
  storage: StorageLike | null = browserStorage()
): void {
  if (!storage || !isPositiveInteger(profileId)) return;
  try {
    const payload: DraftStorage = { version: 1, drafts: drafts.map(cloneCreationDraft) };
    storage.setItem(creationDraftStorageKey(profileId), JSON.stringify(payload));
  } catch {
    // Drafts remain available for this session when webview storage is unavailable or full.
  }
}

export function cloneCreationDraft(draft: CreationDraft): CreationDraft {
  if (draft.kind === 'todo') return { ...draft, blockerIds: [...draft.blockerIds] };
  if (draft.kind === 'agent') return { ...draft, attachments: [...draft.attachments] };
  return { ...draft };
}

function parseCreationDraft(value: unknown): CreationDraft | null {
  if (
    !isRecord(value)
    || !isNegativeInteger(value.id)
    || !isPositiveInteger(value.projectId)
    || !isFiniteNumber(value.createdAt)
    || typeof value.touched !== 'boolean'
  ) return null;

  const base = {
    id: value.id,
    projectId: value.projectId,
    createdAt: value.createdAt,
    touched: value.touched
  };
  if (value.kind === 'agent') {
    const attachments = value.attachments ?? [];
    if (
      !isNullablePositiveInteger(value.agentToolId)
      || !isNullablePositiveInteger(value.templateId)
      || !isDraftText(value.name)
      || !isDraftText(value.prompt)
      || !isAgentAttachments(attachments)
      || !isDraftText(value.extraArgs)
    ) return null;
    return {
      ...base,
      kind: value.kind,
      agentToolId: value.agentToolId,
      templateId: value.templateId,
      name: value.name,
      prompt: value.prompt,
      attachments: [...attachments],
      extraArgs: value.extraArgs
    };
  }
  if (value.kind === 'command') {
    if (
      !isDraftText(value.name)
      || !isDraftText(value.command)
      || !isDraftText(value.workingDir)
      || !isDraftText(value.environment)
      || !isDraftText(value.restartWhenChanged)
      || typeof value.autoStart !== 'boolean'
      || typeof value.autoRestart !== 'boolean'
      || (value.saveMode !== 'yml' && value.saveMode !== 'local')
    ) return null;
    return {
      ...base,
      kind: value.kind,
      name: value.name,
      command: value.command,
      workingDir: value.workingDir,
      environment: value.environment,
      restartWhenChanged: value.restartWhenChanged,
      autoStart: value.autoStart,
      autoRestart: value.autoRestart,
      saveMode: value.saveMode
    };
  }
  if (value.kind !== 'todo') return null;
  if (
    !isDraftText(value.title)
    || !isDraftText(value.body)
    || !isTodoPriority(value.priority)
    || !isDraftText(value.tags)
    || !Array.isArray(value.blockerIds)
    || value.blockerIds.length > maxRestoredBlockers
    || !value.blockerIds.every(isPositiveInteger)
  ) return null;
  return {
    ...base,
    kind: value.kind,
    title: value.title,
    body: value.body,
    priority: value.priority,
    tags: value.tags,
    blockerIds: [...value.blockerIds]
  };
}

function isTodoPriority(value: unknown): value is TodoPriority {
  return value === 'high' || value === 'medium' || value === 'low';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isPositiveInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) > 0;
}

function isNegativeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) < 0;
}

function isNullablePositiveInteger(value: unknown): value is number | null {
  return value === null || isPositiveInteger(value);
}

function isDraftText(value: unknown): value is string {
  return typeof value === 'string' && value.length <= maxDraftTextLength;
}

function isAgentAttachments(value: unknown): value is string[] {
  return Array.isArray(value)
    && value.length <= maxAgentAttachments
    && value.every((path) =>
      typeof path === 'string'
      && path.startsWith('/')
      && path.length <= maxAttachmentPathLength
      && !/[\0\r\n]/u.test(path)
    );
}

function creationDraftTextLength(draft: CreationDraft): number {
  if (draft.kind === 'agent') {
    return draft.name.length
      + draft.prompt.length
      + draft.extraArgs.length
      + draft.attachments.reduce((total, path) => total + path.length, 0);
  }
  if (draft.kind === 'command') {
    return draft.name.length
      + draft.command.length
      + draft.workingDir.length
      + draft.environment.length
      + draft.restartWhenChanged.length;
  }
  return draft.title.length + draft.body.length + draft.tags.length;
}

function browserStorage(): StorageLike | null {
  return typeof localStorage === 'undefined' ? null : localStorage;
}
