import type {
  UpdateInstallReport,
  UpdateProgress,
  UpdateStage,
  UpdateStatus
} from './settings';

export type UpdateCompletionAction = 'relaunch' | 'restart-daemon-only' | 'manual-restart';

export type UpdateFlow =
  | { kind: 'idle' }
  | { kind: 'running'; progress: UpdateProgress }
  | { kind: 'restarting'; version: string }
  | { kind: 'needs-restart'; version: string; instruction: string | null }
  | { kind: 'failed'; stage: UpdateStage; message: string };

export interface UpdateEnvironment {
  nativeRelaunchAvailable: boolean;
  appVersion: string;
}

export interface UpdateBannerState {
  visible: boolean;
  mode: 'available' | 'running' | 'restarting' | 'needs-restart' | 'failed';
  title: string;
  description: string;
  percent: number | null;
  indeterminate: boolean;
  retry: boolean;
  restart: boolean;
  dismiss: boolean;
}

export const idleUpdateFlow: UpdateFlow = { kind: 'idle' };

export function updateCompletionAction(
  report: UpdateInstallReport,
  environment: UpdateEnvironment
): UpdateCompletionAction {
  if (report.restart_plan.app && environment.nativeRelaunchAvailable) return 'relaunch';
  if (
    report.restart_plan.daemon
    && !report.restart_plan.app
    && report.latest === environment.appVersion
  ) {
    return 'restart-daemon-only';
  }
  return 'manual-restart';
}

export function updateBannerState(update: UpdateStatus | null, flow: UpdateFlow): UpdateBannerState {
  if (flow.kind === 'running') {
    const { progress } = flow;
    if (progress.failed) {
      return failedBanner(progress.stage, progress.message);
    }
    const detail = progress.stage === 'downloading'
      ? downloadDetail(progress)
      : progress.message;
    return {
      visible: true,
      mode: 'running',
      title: updateStageLabel(progress.stage),
      description: detail,
      percent: progress.percent,
      indeterminate: progress.percent === null,
      retry: false,
      restart: false,
      dismiss: false
    };
  }
  if (flow.kind === 'restarting') {
    return {
      visible: true,
      mode: 'restarting',
      title: `Installed Workman ${flow.version} — restarting…`,
      description: 'Stopping the old daemon and reopening the updated desktop app.',
      percent: 100,
      indeterminate: false,
      retry: false,
      restart: false,
      dismiss: false
    };
  }
  if (flow.kind === 'needs-restart') {
    return {
      visible: true,
      mode: 'needs-restart',
      title: `Installed Workman ${flow.version}. Restart Workman to finish`,
      description: flow.instruction ?? 'The update is installed, but this environment cannot relaunch automatically.',
      percent: 100,
      indeterminate: false,
      retry: false,
      restart: true,
      dismiss: true
    };
  }
  if (flow.kind === 'failed') return failedBanner(flow.stage, flow.message);

  const check = update?.check;
  const recovery = update?.cli_recovery_required === true;
  const available = check?.available === true;
  return {
    visible: available || recovery,
    mode: 'available',
    title: recovery
      ? available
        ? `Workman ${check?.latest} is available and the CLI needs repair`
        : 'Workman command-line tools need repair'
      : `Workman ${check?.latest} is available`,
    description: recovery
      ? 'The verified release can restore wrk and workmand before Workman restarts.'
      : 'The release is downloaded, SHA256 verified, installed, then Workman restarts automatically.',
    percent: null,
    indeterminate: false,
    retry: false,
    restart: false,
    dismiss: false
  };
}

export function updateStageLabel(stage: UpdateStage): string {
  switch (stage) {
    case 'checking': return 'Checking for updates…';
    case 'downloading': return 'Downloading update…';
    case 'verifying': return 'Verifying update…';
    case 'installing': return 'Installing update…';
    case 'restarting': return 'Restarting Workman…';
  }
}

function failedBanner(stage: UpdateStage, message: string): UpdateBannerState {
  return {
    visible: true,
    mode: 'failed',
    title: `${updateStageLabel(stage).replace(/…$/, '')} failed`,
    description: message,
    percent: null,
    indeterminate: false,
    retry: true,
    restart: false,
    dismiss: true
  };
}

function downloadDetail(progress: UpdateProgress): string {
  if (progress.bytes_done === null || progress.bytes_total === null) return progress.message;
  const done = formatBytes(progress.bytes_done);
  const total = formatBytes(progress.bytes_total);
  return `${done} of ${total}${progress.percent === null ? '' : ` · ${progress.percent}%`}`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
