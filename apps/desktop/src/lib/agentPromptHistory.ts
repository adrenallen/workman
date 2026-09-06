import { configuredAgentLaunchOptions, splitAgentLaunchOptions } from './agentLaunchOptions.ts';
import type { AgentTool, SpawnAgentInput } from './agentTools';
import type { AgentTemplate } from './agentTemplates';
import { parseCreationDraft, type AgentCreationDraft } from './creationDrafts.ts';
import { formatExtraArgs } from './extraArgs.ts';

export interface AgentPromptHistoryEntry {
  id: string;
  createdAt: number;
  label: string;
  processId: number | null;
  draft: AgentCreationDraft;
}

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const maxAgentPromptHistory = 40;
const maxHistorySize = 1_500_000;

export function agentPromptHistoryKey(profileId: number): string {
  return `workman.agent-prompt-history.v1.profile-${profileId}`;
}

/** Freeze the combined instructions so editing/deleting a template cannot change recovery. */
export function snapshotAgentPrompt(
  draft: AgentCreationDraft,
  tool: AgentTool,
  template: AgentTemplate | null,
  input: SpawnAgentInput,
  id: string,
  createdAt = Date.now()
): AgentPromptHistoryEntry {
  const templateArgs = template?.agent_tool_id === tool.id ? template.extra_args : [];
  const launch = splitAgentLaunchOptions([...templateArgs, ...input.extra_args], tool.tool_type);
  const defaults = configuredAgentLaunchOptions(tool, templateArgs);
  return {
    id,
    createdAt,
    label: input.name || template?.name || tool.name,
    processId: null,
    draft: {
      ...draft,
      touched: true,
      name: input.name || template?.name || '',
      agentToolId: tool.id,
      templateId: null,
      prompt: [template?.prompt.trim(), input.prompt?.trim()].filter(Boolean).join('\n\n'),
      attachments: [...draft.attachments],
      model: input.model || launch.model || defaults.model || '',
      effort: launch.effort || defaults.effort || '',
      extraArgs: formatExtraArgs(launch.extraArgs)
    }
  };
}

export function boundAgentPromptHistory(entries: readonly AgentPromptHistoryEntry[]): AgentPromptHistoryEntry[] {
  let size = 0;
  return entries.slice(0, maxAgentPromptHistory).filter((entry) => {
    size += JSON.stringify(entry).length;
    return size <= maxHistorySize;
  });
}

export function loadAgentPromptHistory(
  profileId: number,
  storage?: StorageLike
): AgentPromptHistoryEntry[] {
  try {
    const value: unknown = JSON.parse((storage ?? localStorage).getItem(agentPromptHistoryKey(profileId)) ?? 'null');
    if (!Array.isArray(value)) return [];
    const entries: AgentPromptHistoryEntry[] = [];
    const seen = new Set<string>();
    for (const candidate of value.slice(0, maxAgentPromptHistory)) {
      if (!candidate || typeof candidate !== 'object') continue;
      const draft = parseCreationDraft(candidate.draft);
      if (draft?.kind !== 'agent'
        || typeof candidate.id !== 'string' || candidate.id.length > 100 || seen.has(candidate.id)
        || typeof candidate.label !== 'string' || candidate.label.length > 256_000
        || !Number.isFinite(candidate.createdAt) || candidate.createdAt < 0 || candidate.createdAt > 8_640_000_000_000_000
        || !(candidate.processId === null || (Number.isSafeInteger(candidate.processId) && candidate.processId > 0))) continue;
      seen.add(candidate.id);
      entries.push({ id: candidate.id, createdAt: candidate.createdAt, label: candidate.label, processId: candidate.processId, draft });
    }
    return boundAgentPromptHistory(entries);
  } catch {
    return [];
  }
}

export function saveAgentPromptHistory(
  profileId: number,
  entries: readonly AgentPromptHistoryEntry[],
  storage?: StorageLike
): boolean {
  try {
    (storage ?? localStorage).setItem(agentPromptHistoryKey(profileId), JSON.stringify(boundAgentPromptHistory(entries)));
    return true;
  } catch {
    return false;
  }
}
