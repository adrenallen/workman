<script lang="ts">
  import CoffeeIcon from '@lucide/svelte/icons/coffee';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { onMount, untrack } from 'svelte';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Popover from '$lib/components/ui/popover';
  import * as Select from '$lib/components/ui/select';
  import { Switch } from '$lib/components/ui/switch';
  import type { ConnectionStatus, ProcessView, Project } from './daemon';
  import {
    armKeepAwake,
    disarmKeepAwake,
    evaluateAutoKeepAwake,
    evaluateKeepAwakeAtCurrentTime,
    evaluateKeepAwakeConnection,
    initialAutoKeepAwakeState,
    initialKeepAwakeConnectionState,
    initialKeepAwakeState,
    loadAutoKeepAwakePreference,
    loadPersistedKeepAwakeState,
    reconcileKeepAwakeIntent,
    runningAgents,
    saveAutoKeepAwakePreference,
    savePersistedKeepAwakeState,
    suppressAutoKeepAwake,
    type KeepAwakeArmSource,
    type KeepAwakeMode,
    type PersistedKeepAwakeState
  } from './keepAwake';
  import { deliverNativeSystemNotification } from './nativeNotifications';
  import { projectDisplayName } from './worktrees';

  interface Props {
    processes: ProcessView[];
    projects: Project[];
    connectionStatus: ConnectionStatus['status'];
    visible: boolean;
    open?: boolean;
    armed?: boolean;
    autoEnabled?: boolean;
    supported?: boolean;
  }

  interface NativeKeepAwakeStatus {
    supported: boolean;
    armed: boolean;
    active: boolean;
    arm_source: Exclude<KeepAwakeArmSource, null> | null;
    assertion_pid: number | null;
    warning: string | null;
    notice: string | null;
    respawn_count: number;
    last_loss_reason: string | null;
    retry_in_ms: number | null;
    auto_enabled: boolean;
    auto_should_hold: boolean;
    auto_suppressed_until_activity_edge: boolean;
    auto_active_agent_ids: number[];
  }

  type ReleaseReason = 'idle' | 'toggle' | 'user' | null;

  const POWER_RESYNC_GAP_MS = 5_000;
  const KEEP_AWAKE_RESYNC_EVENT = 'keep-awake://resync';

  let {
    processes,
    projects,
    connectionStatus,
    visible,
    open = $bindable(false),
    armed = $bindable(false),
    autoEnabled = $bindable(false),
    supported = $bindable(false)
  }: Props = $props();

  let busy = $state(false);
  let warning = $state<string | null>(null);
  let mode = $state<KeepAwakeMode>('all');
  let specificAgentId = $state('');
  let preferredMode = $state<KeepAwakeMode>('all');
  let preferredSpecificAgentId = $state('');
  let machine = $state(initialKeepAwakeState());
  let autoMachine = $state(initialAutoKeepAwakeState());
  let connectionMachine = $state(initialKeepAwakeConnectionState());
  let machineGeneration = $state(0);
  let autoReleasePending = false;
  let componentActive = false;
  let nativeSyncPending = false;
  let nativeHydrated = $state(false);
  let preferencesHydrated = $state(false);
  let lastPersistedState = '';
  let restoredHold: PersistedKeepAwakeState['activeHold'] = null;
  let notifiedRespawnCount = 0;
  let daemonUnreachableNotified = false;
  let clockTick = $state(monotonicNow());
  let nativeStatus = $state<NativeKeepAwakeStatus>({
    supported: false,
    armed: false,
    active: false,
    arm_source: null,
    assertion_pid: null,
    warning: null,
    notice: null,
    respawn_count: 0,
    last_loss_reason: null,
    retry_in_ms: null,
    auto_enabled: false,
    auto_should_hold: false,
    auto_suppressed_until_activity_edge: false,
    auto_active_agent_ids: []
  });
  let lastReleaseReason = $state<ReleaseReason>(null);
  let availableAgents = $derived(runningAgents(processes));
  let autoEvaluation = $derived(evaluateAutoKeepAwake(
    autoMachine,
    processes,
    autoEnabled,
    clockTick,
    { connected: connectionStatus === 'connected' }
  ));
  let evaluation = $derived(evaluateKeepAwakeAtCurrentTime(
    machine,
    processes,
    clockTick,
    { connected: connectionStatus === 'connected' }
  ));
  let autoActiveAgentIds = $derived(
    nativeStatus.auto_enabled ? nativeStatus.auto_active_agent_ids : autoEvaluation.activeAgentIds
  );
  let connectionEvaluation = $derived(evaluateKeepAwakeConnection(
    connectionMachine,
    machine.armed ? connectionStatus : 'connected',
    clockTick
  ));
  let selectedAgent = $derived(
    availableAgents.find((process) => String(process.id) === specificAgentId) ?? null
  );
  let verifiedArmed = $derived(
    machine.armed && nativeStatus.armed && nativeStatus.active
  );
  let autoRetryInSeconds = $derived(
    nativeStatus.retry_in_ms === null ? null : Math.max(0, Math.ceil(nativeStatus.retry_in_ms / 1_000))
  );
  let triggerLabel = $derived.by(() => {
    if (warning) return warning;
    if (!machine.armed) {
      if (autoEnabled && autoMachine.suppressedUntilActivityEdge) {
        return 'Auto keep awake paused until fresh agent activity';
      }
      if (autoEnabled) {
        const count = autoActiveAgentIds.length;
        if (count > 0 && autoRetryInSeconds !== null && autoRetryInSeconds > 0) {
          return `Auto keep awake retrying in ${autoRetryInSeconds}s`;
        }
        return count > 0
          ? `Auto keep awake is arming for ${count} ${count === 1 ? 'agent' : 'agents'}`
          : 'Auto keep awake is on — waiting for agent activity';
      }
      return 'Keep Mac awake until agents idle';
    }
    if (!verifiedArmed) return 'Keep awake hold is being repaired';
    if (connectionEvaluation.daemonUnreachable) {
      return 'Daemon unreachable — Mac is still being kept awake';
    }
    if (machine.armSource === 'auto') {
      const count = autoActiveAgentIds.length;
      return count > 0
        ? `Keeping Mac awake — auto (${count} ${count === 1 ? 'agent' : 'agents'} running)`
        : 'Keeping Mac awake — auto (waiting for idle settle)';
    }
    if (machine.mode === 'specific') {
      const name = processName(machine.watchedAgentIds[0]);
      return `Keeping Mac awake until ${name ?? 'the watched agent'} is idle`;
    }
    return 'Keeping Mac awake until all agents are idle';
  });
  let statusIsWarning = $derived(
    warning !== null
      || (machine.armed && (!nativeStatus.active || connectionEvaluation.daemonUnreachable))
  );
  let statusLine = $derived.by(() => {
    const assertion = assertionStatus(nativeStatus);
    if (machine.armed && warning) return `${warning} ${assertion}`;
    if (!machine.armed && warning) return warning;
    if (!machine.armed) {
      if (autoEnabled && autoMachine.suppressedUntilActivityEdge) {
        return 'Auto keep awake paused by your manual disarm until fresh agent activity.';
      }
      if (autoEnabled) {
        const count = autoActiveAgentIds.length;
        if (count > 0 && autoRetryInSeconds !== null && autoRetryInSeconds > 0) {
          return `Auto keep awake could not arm — retrying in ${autoRetryInSeconds}s.`;
        }
        return count > 0
          ? `Auto keep awake is on — arming for ${count} ${count === 1 ? 'agent' : 'agents'}.`
          : 'Auto keep awake is on — waiting for agent activity.';
      }
      if (lastReleaseReason === 'idle') return 'Released because all watched agents became idle.';
      if (lastReleaseReason === 'toggle') return 'Released because auto keep awake was turned off.';
      if (lastReleaseReason === 'user') return 'Released by you.';
      return 'Ready to prevent system idle sleep.';
    }
    if (!verifiedArmed) return 'Keep awake hold lost — the native watchdog is repairing it…';
    if (connectionEvaluation.daemonUnreachable) {
      return `Daemon unreachable — still keeping Mac awake. ${assertion}`;
    }
    if (connectionStatus !== 'connected') {
      return `Daemon reconnecting — still keeping Mac awake. ${assertion}`;
    }
    if (evaluation.releaseInSeconds !== null) {
      const subject = machine.mode === 'all' ? 'All agents' : 'Watched agent';
      const prefix = machine.armSource === 'auto' ? 'Auto keep awake — ' : '';
      return `${prefix}${subject.toLocaleLowerCase()} idle — releasing in ${evaluation.releaseInSeconds}s. ${assertion}`;
    }
    const names = evaluation.waitingAgentIds
      .map(processName)
      .filter((name): name is string => name !== null);
    const count = evaluation.waitingAgentIds.length;
    if (machine.armSource === 'auto') {
      return `Keeping Mac awake — auto (${count} ${count === 1 ? 'agent' : 'agents'} running). ${assertion}`;
    }
    return `Keeping Mac awake. ${assertion} Waiting on ${count} ${count === 1 ? 'agent' : 'agents'}: ${names.join(', ')}`;
  });
  let assertionHistoryLine = $derived(
    machine.armed && nativeStatus.notice ? nativeStatus.notice : null
  );

  $effect(() => {
    armed = machine.armed;
  });

  $effect(() => {
    const next = autoEvaluation;
    if (next.state !== autoMachine) autoMachine = next.state;
  });

  $effect(() => {
    if (!preferencesHydrated || !nativeHydrated) return;
    const activeHold = machine.armed && machine.armSource !== null
      ? {
          mode: machine.mode,
          armSource: machine.armSource,
          watchedAgentIds: machine.watchedAgentIds
        }
      : null;
    const persistedState: PersistedKeepAwakeState = {
      autoState: {
        activeAgentIds: autoActiveAgentIds,
        suppressedUntilActivityEdge: autoMachine.suppressedUntilActivityEdge
      },
      preferredMode,
      preferredSpecificAgentId: positiveAgentId(preferredSpecificAgentId),
      activeHold
    };
    const serialized = JSON.stringify(persistedState);
    if (serialized === lastPersistedState) return;
    lastPersistedState = serialized;
    savePersistedKeepAwakeState(persistedState);
  });

  $effect(() => {
    if (machine.armed || availableAgents.some((process) => String(process.id) === specificAgentId)) return;
    const next = String(availableAgents[0]?.id ?? '');
    specificAgentId = next;
    preferredSpecificAgentId = next;
  });

  $effect(() => {
    const next = evaluation;
    if (next.shouldRelease) {
      if (machine.armSource !== 'auto' && !autoReleasePending) void releaseAutomatically();
      return;
    }
    if (next.state !== machine) machine = next.state;
  });

  $effect(() => {
    const next = connectionEvaluation;
    if (next.state !== connectionMachine) connectionMachine = next.state;
  });

  $effect(() => {
    const count = nativeStatus.respawn_count;
    if (count === 0) {
      notifiedRespawnCount = 0;
      return;
    }
    if (count <= notifiedRespawnCount) return;
    notifiedRespawnCount = count;
    void deliverNativeSystemNotification(
      'Keep awake assertion restored',
      `The macOS idle-sleep assertion has been restored ${count} ${count === 1 ? 'time' : 'times'} since arming.`
    );
  });

  $effect(() => {
    const unreachable = machine.armed && connectionEvaluation.daemonUnreachable;
    if (!unreachable) {
      daemonUnreachableNotified = false;
      return;
    }
    if (daemonUnreachableNotified) return;
    daemonUnreachableNotified = true;
    void deliverNativeSystemNotification(
      'Daemon unreachable — still keeping Mac awake',
      'Keep awake will not auto-release until the daemon reconnects.'
    );
  });

  $effect(() => {
    const status = connectionStatus;
    const documentVisible = visible;
    clockTick = monotonicNow();
    if (
      componentActive
      && documentVisible
      && untrack(() => machine.armed && !busy)
      && status === 'connected'
    ) void syncNativeStatus();
  });

  onMount(() => {
    componentActive = true;
    autoEnabled = loadAutoKeepAwakePreference();
    const persisted = loadPersistedKeepAwakeState();
    autoMachine = {
      ...initialAutoKeepAwakeState(),
      ...persisted.autoState
    };
    preferredMode = persisted.preferredMode;
    preferredSpecificAgentId = persisted.preferredSpecificAgentId === null
      ? ''
      : String(persisted.preferredSpecificAgentId);
    mode = preferredMode;
    specificAgentId = preferredSpecificAgentId;
    restoredHold = persisted.activeHold;
    preferencesHydrated = true;
    let pollCount = 0;
    let lastWallTick = Date.now();
    let lastMonotonicTick = monotonicNow();
    let unlistenPowerResume: UnlistenFn | null = null;
    void configureNativeAutoKeepAwake().finally(() => {
      if (componentActive) nativeHydrated = true;
    });
    const resync = () => {
      if (machine.armed || nativeStatus.armed) void syncNativeStatus();
    };
    const resyncWhenVisible = () => {
      if (document.visibilityState === 'visible') resync();
    };
    window.addEventListener('focus', resync);
    document.addEventListener('visibilitychange', resyncWhenVisible);
    void listen<NativeKeepAwakeStatus>(KEEP_AWAKE_RESYNC_EVENT, ({ payload }) => {
      if (!componentActive) return;
      reconcileNativeStatus(payload, machineGeneration);
    }).then((unlisten) => {
      if (componentActive) unlistenPowerResume = unlisten;
      else unlisten();
    }).catch(() => undefined);
    const timer = window.setInterval(() => {
      const nextMonotonicTick = monotonicNow();
      const nextWallTick = Date.now();
      const monotonicDelta = Math.max(0, nextMonotonicTick - lastMonotonicTick);
      const wallDelta = Math.max(0, nextWallTick - lastWallTick);
      const resumedFromPowerGap = wallDelta - monotonicDelta > POWER_RESYNC_GAP_MS
        || monotonicDelta > POWER_RESYNC_GAP_MS;
      lastMonotonicTick = nextMonotonicTick;
      lastWallTick = nextWallTick;
      clockTick = nextMonotonicTick;
      pollCount += 1;
      const shouldPoll = machine.armed ? pollCount % 3 === 0 : pollCount % 15 === 0;
      if (!busy && (resumedFromPowerGap || shouldPoll)) void syncNativeStatus();
    }, 1_000);

    return () => {
      componentActive = false;
      window.clearInterval(timer);
      window.removeEventListener('focus', resync);
      document.removeEventListener('visibilitychange', resyncWhenVisible);
      unlistenPowerResume?.();
      // Auto mode is owned by the native app process so a WebView reload/teardown
      // cannot create an assertion gap while the app itself is still running.
      if (machine.armed && machine.armSource !== 'auto') {
        void invoke('keep_awake_stop').catch(() => undefined);
      }
    };
  });

  async function syncNativeStatus(): Promise<void> {
    if (nativeSyncPending) return;
    nativeSyncPending = true;
    const statusGeneration = machineGeneration;
    try {
      const status = await invoke<NativeKeepAwakeStatus>('keep_awake_status');
      if (!componentActive || machineGeneration !== statusGeneration) return;
      reconcileNativeStatus(status, statusGeneration);
    } catch (cause) {
      if (componentActive && machineGeneration === statusGeneration) warning = message(cause);
    } finally {
      nativeSyncPending = false;
    }
  }

  async function configureNativeAutoKeepAwake(): Promise<void> {
    if (nativeSyncPending) return;
    nativeSyncPending = true;
    const statusGeneration = machineGeneration;
    try {
      const status = await invoke<NativeKeepAwakeStatus>('keep_awake_auto_configure', {
        enabled: autoEnabled,
        suppressedUntilActivityEdge: autoMachine.suppressedUntilActivityEdge,
        activeAgentIds: autoMachine.activeAgentIds
      });
      if (!componentActive || machineGeneration !== statusGeneration) return;
      reconcileNativeStatus(status, statusGeneration);
    } catch (cause) {
      if (componentActive && machineGeneration === statusGeneration) warning = message(cause);
    } finally {
      nativeSyncPending = false;
    }
  }

  function reconcileNativeStatus(status: NativeKeepAwakeStatus, statusGeneration: number): void {
    if (machineGeneration !== statusGeneration) return;
    applyNativeStatus(status);
    const reconciliation = reconcileKeepAwakeIntent(machine, status.armed);
    if (reconciliation.holdLost) {
      const lostSource = machine.armSource;
      machine = reconciliation.state;
      machineGeneration += 1;
      restoredHold = null;
      if (lostSource === 'auto') restoreManualSelection();
      if (lostSource === 'auto' && !status.auto_should_hold) {
        warning = status.warning;
        lastReleaseReason = status.auto_enabled ? 'idle' : 'toggle';
        return;
      }
      warning = status.warning ?? 'Keep awake hold lost — macOS no longer reports an armed assertion.';
      void deliverNativeSystemNotification(
        'Keep awake hold lost',
        'Workman no longer reports an armed macOS idle-sleep assertion.'
      );
      return;
    }
    if (machine.armed || !status.armed) return;

    const saved = restoredHold;
    const fallbackSource: Exclude<KeepAwakeArmSource, null> = autoEnabled
      && autoActiveAgentIds.length > 0
      && !autoMachine.suppressedUntilActivityEdge
      ? 'auto'
      : 'manual';
    const source = status.arm_source ?? saved?.armSource ?? fallbackSource;
    const adoptedMode = saved?.mode ?? (source === 'auto' ? 'all' : preferredMode);
    const watchedAgentId = adoptedMode === 'specific'
      ? saved?.watchedAgentIds[0] ?? positiveAgentId(preferredSpecificAgentId)
      : null;
    mode = adoptedMode;
    specificAgentId = watchedAgentId === null ? specificAgentId : String(watchedAgentId);
    machine = armKeepAwake(adoptedMode, watchedAgentId, source);
    machineGeneration += 1;
    lastReleaseReason = null;
    restoredHold = null;
    clockTick = monotonicNow();
  }

  function processName(processId: number | undefined): string | null {
    if (processId === undefined) return null;
    return processes.find((process) => process.id === processId)?.name ?? null;
  }

  function projectName(projectId: number): string {
    const project = projects.find((candidate) => candidate.id === projectId);
    return project ? projectDisplayName(project) : `Project ${projectId}`;
  }

  async function arm(): Promise<void> {
    if (busy || supported !== true) return;
    if (mode === 'specific' && !selectedAgent) {
      warning = 'Choose a running agent to watch.';
      return;
    }
    busy = true;
    warning = null;
    try {
      const status = await invoke<NativeKeepAwakeStatus>('keep_awake_start');
      applyNativeStatus(status);
      if (!status.supported || !status.armed) {
        warning = status.warning ?? 'macOS keep awake is unavailable.';
        return;
      }
      machine = armKeepAwake(
        mode,
        mode === 'specific' ? Number(specificAgentId) : null,
        'manual'
      );
      preferredMode = mode;
      preferredSpecificAgentId = specificAgentId;
      machineGeneration += 1;
      lastReleaseReason = null;
      clockTick = monotonicNow();
      if (!status.active) {
        warning = status.warning ?? 'Restoring the macOS idle-sleep assertion.';
      }
    } catch (cause) {
      warning = message(cause);
    } finally {
      busy = false;
    }
  }

  async function disarm(
    manualOverride = true,
    releaseReason: Exclude<ReleaseReason, 'idle' | null> = 'user'
  ): Promise<boolean> {
    if (busy) return false;
    busy = true;
    const generation = machineGeneration;
    const source = machine.armSource;
    try {
      const status = await invoke<NativeKeepAwakeStatus>('keep_awake_stop');
      const disarmed = machine.armed && machineGeneration === generation;
      if (disarmed) {
        machine = disarmKeepAwake(machine);
        if (manualOverride && autoEnabled) {
          autoMachine = suppressAutoKeepAwake(autoMachine, processes, clockTick);
        }
        machineGeneration += 1;
        lastReleaseReason = releaseReason;
        if (source === 'auto') restoreManualSelection();
      }
      applyNativeStatus(status);
      warning = null;
      return disarmed;
    } catch (cause) {
      warning = message(cause);
      return false;
    } finally {
      busy = false;
    }
  }

  async function changeAutoEnabled(next: boolean): Promise<void> {
    if (autoEnabled === next) return;
    autoEnabled = next;
    saveAutoKeepAwakePreference(next);
    if (!next) {
      autoMachine = initialAutoKeepAwakeState();
      if (!machine.armed) warning = null;
    }
    busy = true;
    const generation = machineGeneration;
    try {
      const status = await invoke<NativeKeepAwakeStatus>('keep_awake_auto_configure', {
        enabled: next,
        suppressedUntilActivityEdge: next && autoMachine.suppressedUntilActivityEdge,
        activeAgentIds: autoMachine.activeAgentIds
      });
      if (!componentActive || machineGeneration !== generation) return;
      reconcileNativeStatus(status, generation);
      if (!next && !status.armed) lastReleaseReason = 'toggle';
    } catch (cause) {
      warning = message(cause);
    } finally {
      busy = false;
    }
  }

  async function releaseAutomatically(): Promise<void> {
    if (busy || autoReleasePending || !machine.armed) return;
    busy = true;
    autoReleasePending = true;
    const generation = machineGeneration;
    try {
      const status = await invoke<NativeKeepAwakeStatus>('keep_awake_stop');
      applyNativeStatus(status);
    } catch (cause) {
      warning = message(cause);
      if (machine.armed && machineGeneration === generation) {
        machine = { ...machine, idleObservedMs: 0, lastIdleObservationAt: null };
      }
      autoReleasePending = false;
      busy = false;
      return;
    }
    const disarmed = machine.armed && machineGeneration === generation;
    if (disarmed) {
      machine = disarmKeepAwake(machine);
      machineGeneration += 1;
      lastReleaseReason = 'idle';
      restoreManualSelection();
    }
    autoReleasePending = false;
    busy = false;
    if (disarmed) {
      await deliverNativeSystemNotification(
        'Keep awake released — all watched agents idle',
        'Your Mac may sleep normally again.'
      );
    }
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }

  function selectManualMode(next: KeepAwakeMode): void {
    mode = next;
    preferredMode = next;
  }

  function selectSpecificAgent(value: string): void {
    specificAgentId = value;
    preferredSpecificAgentId = value;
  }

  function restoreManualSelection(): void {
    mode = preferredMode;
    specificAgentId = preferredSpecificAgentId;
  }

  function positiveAgentId(value: string): number | null {
    const parsed = Number(value);
    return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : null;
  }

  function applyNativeStatus(status: NativeKeepAwakeStatus): void {
    supported = status.supported;
    nativeStatus = status;
    autoMachine = {
      ...autoMachine,
      suppressedUntilActivityEdge: status.auto_suppressed_until_activity_edge
    };
    warning = status.warning;
  }

  function assertionStatus(status: NativeKeepAwakeStatus): string {
    if (!status.active) return 'No live idle-sleep assertion.';
    return status.assertion_pid === null
      ? 'macOS idle-sleep assertion held.'
      : `Assertion PID ${status.assertion_pid} held.`;
  }

  function monotonicNow(): number {
    return globalThis.performance?.now() ?? Date.now();
  }
</script>

{#if supported === true}
  <Popover.Root bind:open>
    <Popover.Trigger>
      {#snippet child({ props })}
        <IconButton
          {...props}
          class="keep-awake-trigger size-7 rounded border border-border bg-card"
          data-armed={verifiedArmed}
          data-warning={statusIsWarning}
          label={triggerLabel}
        >
          {#snippet icon()}
            <CoffeeIcon size={15} strokeWidth={1.8} fill={verifiedArmed ? 'currentColor' : 'none'} />
          {/snippet}
        </IconButton>
      {/snippet}
    </Popover.Trigger>
    <Popover.Content class="keep-awake-popover" align="start" side="bottom" sideOffset={7}>
      <Popover.Header>
        <Popover.Title>Keep Mac awake</Popover.Title>
        <Popover.Description>Prevents idle sleep on AC or battery. The hold ends when watched agents are idle, you disarm, or Workman quits; closing the lid still sleeps this Mac.</Popover.Description>
      </Popover.Header>

      <fieldset disabled={busy || machine.armed}>
        <legend>Release condition</legend>
        <label class:chosen={mode === 'all'}>
          <input
            type="radio"
            name="keep-awake-mode"
            checked={mode === 'all'}
            onchange={() => selectManualMode('all')}
          />
          <span>Until all agents are idle</span>
        </label>
        <label class:chosen={mode === 'specific'}>
          <input
            type="radio"
            name="keep-awake-mode"
            checked={mode === 'specific'}
            onchange={() => selectManualMode('specific')}
          />
          <span>Until a specific agent is idle</span>
        </label>
      </fieldset>

      <div class="auto-setting">
        <Switch
          id="auto-keep-awake"
          size="sm"
          checked={autoEnabled}
          aria-describedby="auto-keep-awake-description"
          onCheckedChange={(checked) => void changeAutoEnabled(checked === true)}
        />
        <label for="auto-keep-awake">
          <strong>Auto keep awake while agents are running</strong>
          <span id="auto-keep-awake-description">Uses working agents and live, unpaused waits. Manual disarm pauses auto mode until fresh agent activity.</span>
        </label>
      </div>

      {#if mode === 'specific'}
        <div class="agent-select">
          <label for="keep-awake-agent">Running agent</label>
          <Select.Root
            type="single"
            value={specificAgentId}
            disabled={busy || machine.armed || availableAgents.length === 0}
            onValueChange={(value) => { if (value) selectSpecificAgent(value); }}
          >
            <Select.Trigger id="keep-awake-agent" size="sm">
              {#if selectedAgent}
                {selectedAgent.name} · {projectName(selectedAgent.project_id)}
              {:else}
                No running agents
              {/if}
            </Select.Trigger>
            <Select.Content>
              {#each availableAgents as process (process.id)}
                <Select.Item
                  value={String(process.id)}
                  label={`${process.name} · ${projectName(process.project_id)}`}
                />
              {/each}
            </Select.Content>
          </Select.Root>
        </div>
      {/if}

      <p class:warning={statusIsWarning} role={statusIsWarning ? 'alert' : 'status'} aria-live="polite">{statusLine}</p>

      {#if assertionHistoryLine}
        <p class="assertion-history" role="status">{assertionHistoryLine}</p>
      {/if}

      <Button
        size="sm"
        variant={machine.armed ? 'outline' : 'default'}
        disabled={busy || (!machine.armed && mode === 'specific' && !selectedAgent)}
        onclick={() => machine.armed ? void disarm(true) : void arm()}
      >
        {busy ? 'Working…' : machine.armed ? 'Disarm' : 'Arm keep awake'}
      </Button>
    </Popover.Content>
  </Popover.Root>
{/if}

<style>
  :global(.keep-awake-trigger[data-armed='true']) {
    border-color: color-mix(in srgb, var(--agent-state-waiting) 48%, var(--border));
    background: color-mix(in srgb, var(--agent-state-waiting) 13%, var(--card));
    color: var(--agent-state-waiting);
  }
  :global(.keep-awake-trigger[data-warning='true']) { color: var(--destructive); }
  :global(.keep-awake-popover) { width: min(320px, calc(100vw - 24px)); gap: var(--space-2); }
  fieldset { display: grid; gap: var(--space-1); margin: 0; border: 0; padding: 0; }
  legend, .agent-select > label { margin-bottom: var(--space-1); color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 650; }
  fieldset > label { display: flex; min-height: 30px; align-items: center; gap: var(--space-2); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space-1) var(--space-2); color: var(--text-soft); font-size: var(--font-size-sm); }
  fieldset > label.chosen { border-color: var(--input); background: var(--accent); color: var(--foreground); }
  fieldset input { accent-color: var(--agent-state-waiting); }
  .auto-setting { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: start; gap: var(--space-2); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space-2); background: var(--card); }
  .auto-setting :global([data-slot='switch']) { margin-top: 2px; }
  .auto-setting label { min-width: 0; cursor: pointer; }
  .auto-setting strong, .auto-setting span { display: block; }
  .auto-setting strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 650; line-height: 1.3; }
  .auto-setting span { margin-top: 2px; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.4; }
  .agent-select { display: grid; }
  .agent-select :global([data-slot='select-trigger']) { width: 100%; }
  p { min-height: 31px; margin: 0; border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space-2); background: var(--card); color: var(--text-soft); font: var(--font-size-xs) var(--font-mono); line-height: 1.45; }
  p.warning { border-color: color-mix(in srgb, var(--destructive) 40%, var(--border)); color: var(--destructive); }
  p.assertion-history { min-height: 0; border-style: dashed; color: var(--muted-foreground); }
  :global(.keep-awake-popover > button:last-child) { width: 100%; }
</style>
