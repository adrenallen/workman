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
  build_id: string;
  control_protocol_version: number;
  uptime_ms: number;
  mcp: McpConnectionInfo;
  update: UpdateStatus;
}

export interface UpdateCheckInfo {
  channel: UpdateChannel;
  prerelease: boolean;
  current: string;
  latest: string;
  url: string;
  notes: string;
  available: boolean;
  checked_at: number;
}

export interface UpdateStatus {
  automatic_checks: boolean;
  channel: UpdateChannel;
  last_checked_at: number | null;
  check: UpdateCheckInfo;
}

export type UpdateChannel = 'stable' | 'latest';

export interface UpdateInstallReport {
  current: string;
  latest: string;
  install_dir: string;
  updated_files: string[];
  desktop_instruction: string | null;
  quarantine_cleared: boolean;
}

export interface DaemonRestartResult {
  restarting: boolean;
}

export function loadDaemonSettings(client: DaemonClient): Promise<DaemonSettingsInfo> {
  return client.control<DaemonSettingsInfo>('daemon.info');
}

export function restartDaemon(client: DaemonClient): Promise<DaemonRestartResult> {
  return client.restartDaemon();
}

export function checkForUpdates(client: DaemonClient, force = true): Promise<UpdateStatus> {
  return client.control<UpdateStatus>('daemon.update_check', { force });
}

export function setAutomaticUpdateChecks(
  client: DaemonClient,
  automaticChecks: boolean
): Promise<UpdateStatus> {
  return client.control<UpdateStatus>('daemon.update_preferences', {
    automatic_checks: automaticChecks
  });
}

export function setUpdateChannel(
  client: DaemonClient,
  channel: UpdateChannel
): Promise<UpdateStatus> {
  return client.control<UpdateStatus>('daemon.update_preferences', { channel });
}

export function applyUpdate(client: DaemonClient): Promise<UpdateInstallReport> {
  return client.control<UpdateInstallReport>('daemon.update_apply');
}
