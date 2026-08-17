import type { AgentTool } from './agentTools';
import {
  chooseInitialAgentChoice,
  type AgentChoice,
  type AgentTemplate
} from './agentTemplates.ts';
import type { AgentCreationDraft } from './creationDrafts';

export interface AgentDraftChoiceResolution {
  enabledTools: AgentTool[];
  availableTemplates: AgentTemplate[];
  selectedTool: AgentTool | null;
  selectedTemplate: AgentTemplate | null;
  missingTool: boolean;
  missingTemplate: boolean;
  initialChoice: AgentChoice | null;
}

/** Resolve display state without mutating persisted IDs while metadata is absent or stale. */
export function resolveAgentDraftChoice(
  draft: AgentCreationDraft,
  tools: readonly AgentTool[],
  templates: readonly AgentTemplate[],
  metadataLoaded: boolean,
  rememberedChoice: string | null
): AgentDraftChoiceResolution {
  const enabledTools = tools.filter((tool) => tool.enabled);
  const availableTemplates = templates.filter((template) =>
    enabledTools.some((tool) => tool.id === template.agent_tool_id)
  );
  const selectedTool = enabledTools.find((tool) => tool.id === draft.agentToolId) ?? null;
  const selectedTemplate = availableTemplates.find(
    (template) => template.id === draft.templateId
  ) ?? null;
  const hasPersistedChoice = draft.agentToolId !== null || draft.templateId !== null;

  return {
    enabledTools,
    availableTemplates,
    selectedTool,
    selectedTemplate,
    missingTool: metadataLoaded && draft.agentToolId !== null && selectedTool === null,
    missingTemplate: metadataLoaded && draft.templateId !== null && selectedTemplate === null,
    initialChoice: metadataLoaded && !hasPersistedChoice
      ? chooseInitialAgentChoice(availableTemplates, enabledTools, rememberedChoice)
      : null
  };
}
