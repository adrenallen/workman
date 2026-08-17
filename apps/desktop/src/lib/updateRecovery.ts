export interface RecoveryAwareUpdate {
  cli_recovery_required: boolean;
  check: {
    available: boolean;
    current: string;
    latest: string;
  };
}

export interface UpdateActionCopy {
  buttonLabel: string;
  busyLabel: string;
  dialogTitle: string;
  dialogDescription: string;
  confirmLabel: string;
  bannerTitle: string;
  bannerDescription: string;
}

export function updateActionAvailable(update: RecoveryAwareUpdate): boolean {
  return update.check.available || update.cli_recovery_required;
}

export function updateActionCopy(update: RecoveryAwareUpdate): UpdateActionCopy {
  const { check, cli_recovery_required: recovery } = update;
  if (recovery && check.available) {
    return {
      buttonLabel: 'Repair CLI and update',
      busyLabel: 'Repairing and updating…',
      dialogTitle: `Repair the CLI and update to Workman ${check.latest}?`,
      dialogDescription: `The wrk and workmand launchers are missing or no longer point to a complete install. Workman will download, verify, and install ${check.latest}, repair the launchers in ~/.local/bin, update the desktop app, then restart the app and daemon automatically. Running project processes will stop.`,
      confirmLabel: 'Repair and update',
      bannerTitle: `Workman ${check.latest} is available and the CLI needs repair`,
      bannerDescription: 'The verified release restores wrk and workmand, installs the update, then restarts Workman.'
    };
  }
  if (recovery) {
    return {
      buttonLabel: 'Repair command-line tools',
      busyLabel: 'Repairing…',
      dialogTitle: 'Repair the Workman command-line tools?',
      dialogDescription: `The wrk and workmand launchers are missing or no longer point to a complete install. Workman will download, verify, and install ${check.current}, repair the launchers in ~/.local/bin, then restart the app and daemon automatically. Running project processes will stop.`,
      confirmLabel: 'Repair CLI',
      bannerTitle: 'Workman command-line tools need repair',
      bannerDescription: 'The desktop app can restore wrk and workmand from the verified release.'
    };
  }
  return {
    buttonLabel: 'Update now',
    busyLabel: 'Updating…',
    dialogTitle: `Update to Workman ${check.latest}?`,
    dialogDescription: 'Workman will download, verify, and install the release, then restart the app and daemon automatically. Running project processes will stop.',
    confirmLabel: 'Update and restart',
    bannerTitle: `Workman ${check.latest} is available`,
    bannerDescription: 'The release is downloaded, SHA256 verified, and installed before Workman restarts.'
  };
}
