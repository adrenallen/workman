import type { AgentTool } from './agentTools';

export interface AgentBrand {
  id: string;
  label: string;
  monogram: string;
}

interface AgentBrandDefinition extends AgentBrand {
  toolTypes: readonly string[];
  searchTerms: readonly string[];
}

/**
 * Keep brand recognition declarative. Search terms run before tool types because a preset can
 * launch a provider model through a generic host (for example DeepSeek through OpenCode).
 */
export const AGENT_BRANDS: readonly AgentBrandDefinition[] = [
  {
    id: 'grok',
    label: 'xAI Grok',
    monogram: 'GR',
    toolTypes: ['grok', 'grok_cli', 'grok_build', 'xai'],
    searchTerms: ['grok', 'xai', 'x.ai']
  },
  {
    id: 'deepseek',
    label: 'DeepSeek',
    monogram: 'DS',
    toolTypes: ['deepseek', 'deepseek_cli'],
    searchTerms: ['deepseek']
  },
  {
    id: 'anthropic',
    label: 'Anthropic',
    monogram: 'AN',
    toolTypes: ['claude', 'claude_code', 'anthropic'],
    searchTerms: ['claude', 'anthropic']
  },
  {
    id: 'openai',
    label: 'OpenAI',
    monogram: 'OA',
    toolTypes: ['codex', 'openai'],
    searchTerms: ['codex', 'openai']
  },
  {
    id: 'google',
    label: 'Google Gemini',
    monogram: 'GM',
    toolTypes: ['gemini', 'gemini_cli', 'google'],
    searchTerms: ['gemini', 'google']
  },
  {
    id: 'moonshot',
    label: 'Moonshot Kimi',
    monogram: 'KM',
    toolTypes: ['kimi', 'kimi_code', 'moonshot'],
    searchTerms: ['kimi', 'moonshot']
  },
  {
    id: 'opencode',
    label: 'OpenCode',
    monogram: 'OC',
    toolTypes: ['opencode', 'open_code'],
    searchTerms: ['opencode', 'open code']
  }
];

export function resolveAgentBrand(
  tool: Pick<AgentTool, 'name' | 'command' | 'tool_type'> | null | undefined,
  fallbackName = 'Agent',
  fallbackToolType?: string | null
): AgentBrand {
  const searchable = normalize(`${tool?.name ?? ''} ${tool?.command ?? ''}`);
  const searched = AGENT_BRANDS.find((brand) =>
    brand.searchTerms.some((term) => searchable.includes(normalize(term)))
  );
  if (searched) return searched;

  const toolType = normalize(tool?.tool_type ?? fallbackToolType ?? '');
  const typed = AGENT_BRANDS.find((brand) =>
    brand.toolTypes.some((candidate) => normalize(candidate) === toolType)
  );
  if (typed) return typed;

  const source = tool?.name || fallbackName || tool?.tool_type || fallbackToolType || 'Agent';
  return {
    id: 'custom',
    label: source,
    monogram: monogram(source)
  };
}

export function monogram(value: string): string {
  const words = value
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .split(/[^a-z0-9]+/i)
    .filter(Boolean);
  if (words.length >= 2) {
    return `${words[0][0]}${words[1][0]}`.toUpperCase();
  }
  const letters = (words[0] ?? 'AG').replace(/[^a-z0-9]/gi, '').slice(0, 2);
  return (letters || 'AG').toUpperCase().padEnd(2, 'G');
}

function normalize(value: string): string {
  return value.trim().toLowerCase().replace(/[_-]+/g, ' ');
}
