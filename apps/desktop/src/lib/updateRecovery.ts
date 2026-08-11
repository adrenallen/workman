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
      dialogDescription: `The wrk and workmand launchers are missing or no longer point to a complete install. Workman will download and verify ${check.latest}, reinstall the command-line tools in the durable versioned layout, repair the launchers in ~/.local/bin, update the desktop app, and restart the daemon. Running project processes will stop.`,
      confirmLabel: 'Repair and update',
      bannerTitle: `Workman ${check.latest} is available and the CLI needs repair`,
      bannerDescription: 'The verified release can restore wrk and workmand before the update restarts Workman.'
    };
  }
  if (recovery) {
    return {
      buttonLabel: 'Repair command-line tools',
      busyLabel: 'Repairing…',
      dialogTitle: 'Repair the Workman command-line tools?',
      dialogDescription: `The wrk and workmand launchers are missing or no longer point to a complete install. Workman will download and verify ${check.current}, reinstall the command-line tools in the durable versioned layout, repair the launchers in ~/.local/bin, and restart the daemon. Running project processes will stop.`,
      confirmLabel: 'Repair CLI',
      bannerTitle: 'Workman command-line tools need repair',
      bannerDescription: 'The desktop app can restore wrk and workmand from the verified release.'
    };
  }
  return {
    buttonLabel: 'Update now',
    busyLabel: 'Updating…',
    dialogTitle: `Update to Workman ${check.latest}?`,
    dialogDescription: 'Workman will download and verify the release, replace the CLI and daemon in the configured install directory, then restart the daemon. Running project processes will stop.',
    confirmLabel: 'Update and restart',
    bannerTitle: `Workman ${check.latest} is available`,
    bannerDescription: 'The release is downloaded and SHA256 verified before workman and workmand are replaced.'
  };
}
