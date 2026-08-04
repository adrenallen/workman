import type { DaemonClient } from './daemon';

export interface McpConnectionInfo {
  endpoint: string;
  token: string;
  claude_command: string;
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
