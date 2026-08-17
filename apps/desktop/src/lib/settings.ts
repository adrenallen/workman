import type { DaemonClient } from './daemon';
import {
  currentAppearance,
  shouldAutoImportTerminalProfile,
  terminalAppearancePatchFromImport,
  updateAppearance,
  type ImportedTerminalAppearance
} from './appearance';

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

export interface UserEnvironmentInfo {
  active_shell: string;
  configured_shell: string | null;
  inferred_shell: string;
  inferred_from: string;
  using_override: boolean;
  warning: string | null;
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
  user_environment: UserEnvironmentInfo;
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
  cli_recovery_required: boolean;
}

export type UpdateChannel = 'stable' | 'latest';

export interface UpdateInstallReport {
  current: string;
  latest: string;
  install_dir: string;
  updated_files: string[];
  desktop_instruction: string | null;
  installed_app_bundle?: string | null;
  quarantine_cleared: boolean;
  restart_plan?: UpdateRestartPlan;
}

export interface UpdateRestartPlan {
  daemon: boolean;
  app: boolean;
}

export type UpdateStage = 'checking' | 'downloading' | 'verifying' | 'installing' | 'restarting';

export interface UpdateProgress {
  stage: UpdateStage;
  message: string;
  bytes_done: number | null;
  bytes_total: number | null;
  percent: number | null;
  failed: boolean;
}

export interface DaemonRestartResult {
  restarting: boolean;
}

export interface TerminalThemeImportReport extends ImportedTerminalAppearance {
  message: string;
}

export const TERMINAL_PROFILE_AUTO_IMPORT_KEY = 'workman.terminal-profile-auto.v1';

export function loadDaemonSettings(client: DaemonClient): Promise<DaemonSettingsInfo> {
  return client.control<DaemonSettingsInfo>('daemon.info');
}

export function restartDaemon(client: DaemonClient): Promise<DaemonRestartResult> {
  return client.restartDaemon();
}

export function setUserShell(
  client: DaemonClient,
  shell: string | null
): Promise<UserEnvironmentInfo> {
  return client.control<UserEnvironmentInfo>('settings.user_shell', { shell });
}

export function importTerminalTheme(client: DaemonClient): Promise<TerminalThemeImportReport> {
  return client.control<TerminalThemeImportReport>('settings.terminal_theme_import');
}

export interface TerminalThemeApplyResult {
  applied: boolean;
  profileFontSkipped: string | null;
}

export function applyTerminalThemeImport(report: TerminalThemeImportReport): TerminalThemeApplyResult {
  const patch = terminalAppearancePatchFromImport(report, currentAppearance());
  if (!patch) return { applied: false, profileFontSkipped: null };
  const profileFontSkipped = report.terminalStyle?.fontFamily && patch.terminalFont !== 'profile'
    ? report.terminalStyle.fontFamily
    : null;
  updateAppearance(patch);
  return { applied: true, profileFontSkipped };
}

/** Import the native profile once on a stock install without replacing an explicit user theme. */
export async function autoImportTerminalProfile(
  client: DaemonClient
): Promise<TerminalThemeImportReport | null> {
  let attempted = false;
  try {
    attempted = localStorage.getItem(TERMINAL_PROFILE_AUTO_IMPORT_KEY) !== null;
  } catch {
    // An unavailable webview store should not prevent a best-effort native profile import.
  }
  if (!shouldAutoImportTerminalProfile(currentAppearance(), attempted)) return null;

  const report = await importTerminalTheme(client);
  try {
    localStorage.setItem(
      TERMINAL_PROFILE_AUTO_IMPORT_KEY,
      report.imported ? (report.source ?? 'imported') : 'not-found'
    );
  } catch {
    // The applied in-memory appearance remains useful without persistence.
  }
  applyTerminalThemeImport(report);
  return report;
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

export function applyUpdate(
  client: DaemonClient,
  onProgress: (progress: UpdateProgress) => void
): Promise<UpdateInstallReport> {
  return client.applyUpdate(onProgress);
}
