import type {
  UpdateInstallReport,
  UpdateProgress,
  UpdateStage,
  UpdateStatus
} from './settings';

export type UpdateCompletionAction = 'relaunch' | 'restart-daemon-only' | 'manual-restart';
export type UpdateRestartAction = 'app' | 'daemon' | null;

export type UpdateFlow =
  | { kind: 'idle' }
  | { kind: 'running'; progress: UpdateProgress }
  | { kind: 'restarting'; version: string; target: 'app' | 'daemon' }
  | {
      kind: 'needs-restart';
      version: string;
      title: string;
      instruction: string;
      restartAction: UpdateRestartAction;
    }
  | { kind: 'failed'; stage: UpdateStage; message: string };

export interface UpdateEnvironment {
  nativeRelaunchAvailable: boolean;
  appVersion: string;
  appBundle: string | null;
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
  restartLabel: string | null;
  dismiss: boolean;
}

export const idleUpdateFlow: UpdateFlow = { kind: 'idle' };

export function canPresentUpdateProgress(flow: UpdateFlow, installActive: boolean): boolean {
  return installActive && flow.kind === 'running';
}

export function updateCompletionAction(
  report: UpdateInstallReport,
  environment: UpdateEnvironment
): UpdateCompletionAction {
  const restartPlan = report.restart_plan ?? { daemon: true, app: false };
  if (
    restartPlan.app
    && environment.nativeRelaunchAvailable
    && report.installed_app_bundle
    && environment.appBundle
    && samePath(report.installed_app_bundle, environment.appBundle)
  ) return 'relaunch';
  if (
    restartPlan.daemon
    && !restartPlan.app
    && report.latest === environment.appVersion
  ) {
    return 'restart-daemon-only';
  }
  return 'manual-restart';
}

export function manualUpdateFlow(
  report: UpdateInstallReport,
  reason: string | null = null,
  restartAction: UpdateRestartAction = null
): Extract<UpdateFlow, { kind: 'needs-restart' }> {
  const restartPlan = report.restart_plan;
  const paths = report.updated_files.length > 0
    ? report.updated_files.map((path) => shortPath(path)).join(', ')
    : report.install_dir;
  if (restartPlan?.app && report.installed_app_bundle) {
    return {
      kind: 'needs-restart',
      version: report.latest,
      title: `Installed Workman ${report.latest}. Restart Workman to finish`,
      instruction: reason
        ?? report.desktop_instruction
        ?? `The app bundle at ${report.installed_app_bundle} was replaced, but it could not relaunch automatically.`,
      restartAction
    };
  }
  if (restartPlan && !restartPlan.app) {
    return {
      kind: 'needs-restart',
      version: report.latest,
      title: `Updated command-line tools and daemon to Workman ${report.latest}`,
      instruction: reason
        ?? report.desktop_instruction
        ?? `Updated wrk and workmand (${paths}). The desktop app bundle was not replaced.`,
      restartAction
    };
  }
  return {
    kind: 'needs-restart',
    version: report.latest,
    title: `Installed Workman ${report.latest}`,
    instruction: reason
      ?? report.desktop_instruction
      ?? 'The update completed through an older daemon. Close and reopen the affected Workman surfaces manually.',
    restartAction
  };
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
      restartLabel: null,
      dismiss: false
    };
  }
  if (flow.kind === 'restarting') {
    return {
      visible: true,
      mode: 'restarting',
      title: `Installed Workman ${flow.version} — restarting…`,
      description: flow.target === 'app'
        ? 'Stopping the old daemon and reopening the updated desktop app.'
        : 'Stopping the old daemon and reconnecting to the updated daemon.',
      percent: 100,
      indeterminate: false,
      retry: false,
      restart: false,
      restartLabel: null,
      dismiss: false
    };
  }
  if (flow.kind === 'needs-restart') {
    return {
      visible: true,
      mode: 'needs-restart',
      title: flow.title,
      description: flow.instruction,
      percent: 100,
      indeterminate: false,
      retry: false,
      restart: flow.restartAction !== null,
      restartLabel: flow.restartAction === 'daemon' ? 'Restart daemon' : flow.restartAction === 'app' ? 'Restart now' : null,
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
    restartLabel: null,
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
    restartLabel: null,
    dismiss: true
  };
}

function samePath(left: string, right: string): boolean {
  return left.replace(/\/+$/, '') === right.replace(/\/+$/, '');
}

function shortPath(path: string): string {
  const pieces = path.split('/').filter(Boolean);
  return pieces.at(-1) ?? path;
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
