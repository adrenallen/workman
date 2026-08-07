import type { ProcessView } from './daemon';

export interface AgentTool {
  id: number;
  name: string;
  command: string;
  tool_type: string;
  enabled: boolean;
  source: 'local' | 'config';
  resume_args: string | null;
  continue_args: string | null;
}

export interface AgentToolInput {
  id?: number;
  name: string;
  command: string;
  tool_type: string;
  enabled: boolean;
}

export interface AgentToolHealth extends AgentTool {
  found_on_path: boolean;
  resolved_binary: string | null;
  version: string | null;
  version_error: string | null;
  config_path: string;
  config_exists: boolean;
  launch_ready: boolean;
  install_url: string | null;
  mcp_launch_supported: boolean;
  mcp_launch_mechanism: string;
  mcp_launch_note: string;
  configuration_mode: 'per_launch' | 'self_config' | 'unsupported';
  configuration_note: string;
}

export interface AgentToolsHealth {
  checked_at: number;
  ready_count: number;
  total_count: number;
  enabled_ready_count: number;
  enabled_count: number;
  all_enabled_ready: boolean;
  summary: string;
  tools: AgentToolHealth[];
}

export interface AgentToolConfigPreview {
  agent_tool_id: number;
  tool_type: string;
  automatic_wiring: boolean;
  can_write: boolean;
  requires_consent: boolean;
  path: string;
  preview: string | null;
  preview_sha256: string | null;
  already_configured: boolean;
  message: string;
}

export interface AgentToolConfigWrite {
  agent_tool_id: number;
  path: string;
  written: boolean;
  preview_sha256: string;
}

export interface AgentToolDeepCheck {
  agent_tool_id: number;
  process_id: number | null;
  success: boolean;
  elapsed_ms: number;
  message: string;
}

export interface SpawnAgentInput {
  project_id: number;
  agent_tool_id: number;
  name?: string;
  extra_args: string[];
}

export interface SpawnAgentResult {
  process_id: number;
  project_id: number;
  name: string;
  kind: 'agent';
  agent_instructions: string;
}

export interface DeleteAgentToolResult {
  agent_tool_id: number;
  deleted: boolean;
}

export interface AgentToolsClient {
  listAgentTools(): Promise<AgentTool[]>;
  agentToolsHealth(): Promise<AgentToolsHealth>;
  previewAgentToolConfig(agentToolId: number): Promise<AgentToolConfigPreview>;
  configureAgentTool(
    agentToolId: number,
    expectedPreviewSha256: string
  ): Promise<AgentToolConfigWrite>;
  deepCheckAgentTool(projectId: number, agentToolId: number): Promise<AgentToolDeepCheck>;
  saveAgentTool(tool: AgentToolInput): Promise<AgentTool>;
  deleteAgentTool(agentToolId: number): Promise<DeleteAgentToolResult>;
  reorderAgentTools(agentToolIds: number[]): Promise<AgentTool[]>;
  spawnAgent(input: SpawnAgentInput): Promise<SpawnAgentResult>;
  submitInput(processId: number, input: string): Promise<ProcessView>;
}

export interface AgentToolsSnapshot {
  tools: AgentTool[];
  loading: boolean;
  error: string | null;
}

type Subscriber = (snapshot: AgentToolsSnapshot) => void;

/** One cached registry store per daemon connection, shared by Agents and Settings. */
export class AgentToolsStore {
  private snapshot: AgentToolsSnapshot = { tools: [], loading: false, error: null };
  private subscribers = new Set<Subscriber>();
  private refreshPromise: Promise<AgentTool[]> | null = null;

  constructor(private readonly client: AgentToolsClient) {}

  current(): AgentToolsSnapshot {
    return this.snapshot;
  }

  subscribe(subscriber: Subscriber): () => void {
    this.subscribers.add(subscriber);
    subscriber(this.snapshot);
    return () => this.subscribers.delete(subscriber);
  }

  async refresh(force = false): Promise<AgentTool[]> {
    if (this.refreshPromise && !force) return this.refreshPromise;
    this.publish({ ...this.snapshot, loading: true, error: null });
    const request = this.client
      .listAgentTools()
      .then((tools) => {
        this.publish({ tools, loading: false, error: null });
        return tools;
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

  async save(input: AgentToolInput): Promise<AgentTool> {
    const saved = await this.client.saveAgentTool(input);
    const index = this.snapshot.tools.findIndex((tool) => tool.id === saved.id);
    const tools = [...this.snapshot.tools];
    if (index >= 0) tools[index] = saved;
    else tools.push(saved);
    this.publish({ tools, loading: false, error: null });
    return saved;
  }

  async remove(agentToolId: number): Promise<DeleteAgentToolResult> {
    const result = await this.client.deleteAgentTool(agentToolId);
    if (result.deleted) {
      this.publish({
        tools: this.snapshot.tools.filter((tool) => tool.id !== agentToolId),
        loading: false,
        error: null
      });
    }
    return result;
  }

  async reorder(agentToolIds: number[]): Promise<AgentTool[]> {
    const tools = await this.client.reorderAgentTools(agentToolIds);
    this.publish({ tools, loading: false, error: null });
    return tools;
  }

  private publish(snapshot: AgentToolsSnapshot): void {
    this.snapshot = snapshot;
    for (const subscriber of this.subscribers) subscriber(snapshot);
  }
}

const stores = new WeakMap<object, AgentToolsStore>();

export function getAgentToolsStore(client: AgentToolsClient): AgentToolsStore {
  const key = client as object;
  let store = stores.get(key);
  if (!store) {
    store = new AgentToolsStore(client);
    stores.set(key, store);
  }
  return store;
}

/** Parse a shell-like argument field into literal argv entries for safe server-side quoting. */
export function parseExtraArgs(source: string): string[] {
  const args: string[] = [];
  let current = '';
  let quote: 'single' | 'double' | null = null;
  let escaped = false;
  let started = false;
  for (const character of source) {
    if (escaped) {
      current += character;
      escaped = false;
      started = true;
    } else if (character === '\\' && quote !== 'single') {
      escaped = true;
      started = true;
    } else if (character === "'" && quote !== 'double') {
      quote = quote === 'single' ? null : 'single';
      started = true;
    } else if (character === '"' && quote !== 'single') {
      quote = quote === 'double' ? null : 'double';
      started = true;
    } else if (/\s/.test(character) && quote === null) {
      if (started) args.push(current);
      current = '';
      started = false;
    } else {
      current += character;
      started = true;
    }
  }
  if (escaped) current += '\\';
  if (quote !== null) throw new Error('Close the quoted launch argument before spawning');
  if (started) args.push(current);
  return args;
}
