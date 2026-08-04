import type { DaemonClient } from './daemon';

export type McpClientId = 'claude' | 'codex' | 'gemini' | 'opencode' | 'generic';
export type McpSetupFormat = 'shell' | 'toml' | 'json' | 'text';

export interface McpSetupField {
  label: string;
  value: string;
  format: McpSetupFormat;
  sensitive: boolean;
}

export interface McpClientSetup {
  client: McpClientId;
  label: string;
  description: string;
  fields: McpSetupField[];
}

export interface McpConnectionInfo {
  endpoint: string;
  token: string;
  setups: McpClientSetup[];
}

export interface DaemonSettingsInfo {
  data_dir: string;
  port: number;
  pid: number;
  version: string;
  uptime_ms: number;
  mcp: McpConnectionInfo;
}

export interface DaemonRestartResult {
  restarting: boolean;
}

export function loadDaemonSettings(client: DaemonClient): Promise<DaemonSettingsInfo> {
  return client.control<DaemonSettingsInfo>('daemon.info');
}

export function restartDaemon(client: DaemonClient): Promise<DaemonRestartResult> {
  return client.control<DaemonRestartResult>('daemon.restart');
}
