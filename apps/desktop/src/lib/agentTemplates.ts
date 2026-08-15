import type { AgentTool } from './agentTools';

export interface AgentTemplate {
  id: number;
  profile_id: number;
  name: string;
  agent_tool_id: number;
  extra_args: string[];
  prompt: string;
  sort_order: number;
  created_at: number;
  updated_at: number;
}

export interface AgentTemplateInput {
  id?: number;
  name: string;
  agent_tool_id: number;
  extra_args: string[];
  prompt: string;
}

export interface DeleteAgentTemplateResult {
  agent_template_id: number;
  deleted: boolean;
}

export interface AgentTemplatesClient {
  listAgentTemplates(): Promise<AgentTemplate[]>;
  saveAgentTemplate(template: AgentTemplateInput): Promise<AgentTemplate>;
  deleteAgentTemplate(agentTemplateId: number): Promise<DeleteAgentTemplateResult>;
  reorderAgentTemplates(agentTemplateIds: number[]): Promise<AgentTemplate[]>;
}

export interface AgentTemplatesSnapshot {
  templates: AgentTemplate[];
  loading: boolean;
  error: string | null;
}

type Subscriber = (snapshot: AgentTemplatesSnapshot) => void;

export class AgentTemplatesStore {
  private snapshot: AgentTemplatesSnapshot = { templates: [], loading: false, error: null };
  private subscribers = new Set<Subscriber>();
  private refreshPromise: Promise<AgentTemplate[]> | null = null;
  private readonly client: AgentTemplatesClient;

  constructor(client: AgentTemplatesClient) {
    this.client = client;
  }

  current(): AgentTemplatesSnapshot {
    return this.snapshot;
  }

  subscribe(subscriber: Subscriber): () => void {
    this.subscribers.add(subscriber);
    subscriber(this.snapshot);
    return () => this.subscribers.delete(subscriber);
  }

  async refresh(force = false): Promise<AgentTemplate[]> {
    if (this.refreshPromise && !force) return this.refreshPromise;
    this.publish({ ...this.snapshot, loading: true, error: null });
    const request = this.client
      .listAgentTemplates()
      .then((templates) => {
        this.publish({ templates, loading: false, error: null });
        return templates;
      })
      .catch((cause) => {
        const error = cause instanceof Error ? cause.message : String(cause);
        this.publish({ ...this.snapshot, loading: false, error });
        throw cause;
      })
      .finally(() => {
        if (this.refreshPromise === request) this.refreshPromise = null;
      });
    this.refreshPromise = request;
    return request;
  }

  async save(input: AgentTemplateInput): Promise<AgentTemplate> {
    const saved = await this.client.saveAgentTemplate(input);
    const templates = [...this.snapshot.templates];
    const index = templates.findIndex((template) => template.id === saved.id);
    if (index >= 0) templates[index] = saved;
    else templates.push(saved);
    templates.sort((a, b) => a.sort_order - b.sort_order || a.id - b.id);
    this.publish({ templates, loading: false, error: null });
    return saved;
  }

  async remove(agentTemplateId: number): Promise<DeleteAgentTemplateResult> {
    const result = await this.client.deleteAgentTemplate(agentTemplateId);
    if (result.deleted) {
      this.publish({
        templates: this.snapshot.templates.filter((template) => template.id !== agentTemplateId),
        loading: false,
        error: null
      });
    }
    return result;
  }

  async reorder(agentTemplateIds: number[]): Promise<AgentTemplate[]> {
    const templates = await this.client.reorderAgentTemplates(agentTemplateIds);
    this.publish({ templates, loading: false, error: null });
    return templates;
  }

  private publish(snapshot: AgentTemplatesSnapshot): void {
    this.snapshot = snapshot;
    for (const subscriber of this.subscribers) subscriber(snapshot);
  }
}

const stores = new WeakMap<object, AgentTemplatesStore>();

export function getAgentTemplatesStore(client: AgentTemplatesClient): AgentTemplatesStore {
  const key = client as object;
  let store = stores.get(key);
  if (!store) {
    store = new AgentTemplatesStore(client);
    stores.set(key, store);
  }
  return store;
}

export type AgentChoice =
  | { kind: 'template'; id: number }
  | { kind: 'tool'; id: number };

export const lastAgentChoiceStorageKey = 'workman.new-agent.last-choice.v1';

export function choiceValue(choice: AgentChoice): string {
  return `${choice.kind}:${choice.id}`;
}

export function parseChoiceValue(value: string): AgentChoice | null {
  const match = /^(template|tool):(\d+)$/.exec(value);
  if (!match) return null;
  return { kind: match[1] as AgentChoice['kind'], id: Number(match[2]) };
}

export function chooseInitialAgentChoice(
  templates: AgentTemplate[],
  tools: AgentTool[],
  storedValue: string | null
): AgentChoice | null {
  const stored = storedValue ? parseChoiceValue(storedValue) : null;
  if (
    stored?.kind === 'template'
    && templates.some((template) =>
      template.id === stored.id
      && tools.some((tool) => tool.enabled && tool.id === template.agent_tool_id)
    )
  ) return stored;
  if (stored?.kind === 'tool' && tools.some((tool) => tool.enabled && tool.id === stored.id)) {
    return stored;
  }
  const firstTool = tools.find((tool) => tool.enabled);
  return firstTool ? { kind: 'tool', id: firstTool.id } : null;
}
