<script lang="ts">
  import CoffeeIcon from '@lucide/svelte/icons/coffee';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, untrack } from 'svelte';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Popover from '$lib/components/ui/popover';
  import * as Select from '$lib/components/ui/select';
  import type { ConnectionStatus, ProcessView, Project } from './daemon';
  import {
    armKeepAwake,
    disarmKeepAwake,
    evaluateKeepAwakeAtCurrentTime,
    evaluateKeepAwakeConnection,
    initialKeepAwakeConnectionState,
    initialKeepAwakeState,
    runningAgents,
    type KeepAwakeMode
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
    supported?: boolean;
  }

  interface NativeKeepAwakeStatus {
    supported: boolean;
    armed: boolean;
    active: boolean;
    assertion_pid: number | null;
    warning: string | null;
    notice: string | null;
    respawn_count: number;
    last_loss_reason: string | null;
    retry_in_ms: number | null;
  }

  type ReleaseReason = 'idle' | 'user' | null;

  let {
    processes,
    projects,
    connectionStatus,
    visible,
    open = $bindable(false),
    armed = $bindable(false),
    supported = $bindable(false)
  }: Props = $props();

  let busy = $state(false);
  let warning = $state<string | null>(null);
  let mode = $state<KeepAwakeMode>('all');
  let specificAgentId = $state('');
  let machine = $state(initialKeepAwakeState());
  let connectionMachine = $state(initialKeepAwakeConnectionState());
  let machineGeneration = $state(0);
  let autoReleasePending = false;
  let componentActive = false;
  let nativeSyncPending = false;
  let notifiedRespawnCount = 0;
  let daemonUnreachableNotified = false;
  let clockTick = $state(monotonicNow());
  let nativeStatus = $state<NativeKeepAwakeStatus>({
    supported: false,
    armed: false,
    active: false,
    assertion_pid: null,
    warning: null,
    notice: null,
    respawn_count: 0,
    last_loss_reason: null,
    retry_in_ms: null
  });
  let lastReleaseReason = $state<ReleaseReason>(null);
  let availableAgents = $derived(runningAgents(processes));
  let evaluation = $derived(evaluateKeepAwakeAtCurrentTime(
    machine,
    processes,
    clockTick,
    { connected: connectionStatus === 'connected' }
  ));
  let connectionEvaluation = $derived(evaluateKeepAwakeConnection(
    connectionMachine,
    machine.armed ? connectionStatus : 'connected',
    clockTick
  ));
  let selectedAgent = $derived(
    availableAgents.find((process) => String(process.id) === specificAgentId) ?? null
  );
  let triggerLabel = $derived.by(() => {
    if (warning) return warning;
    if (!machine.armed) return 'Keep Mac awake until agents idle';
    if (!nativeStatus.active) return 'Restoring the macOS idle-sleep assertion';
    if (connectionEvaluation.daemonUnreachable) {
      return 'Daemon unreachable — Mac is still being kept awake';
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
      if (lastReleaseReason === 'idle') return 'Released because all watched agents became idle.';
      if (lastReleaseReason === 'user') return 'Released by you.';
      return 'Ready to prevent system idle sleep.';
    }
    if (!nativeStatus.active) return 'Restoring the macOS idle-sleep assertion…';
    if (connectionEvaluation.daemonUnreachable) {
      return `Daemon unreachable — still keeping Mac awake. ${assertion}`;
    }
    if (connectionStatus !== 'connected') {
      return `Daemon reconnecting — still keeping Mac awake. ${assertion}`;
    }
    if (evaluation.releaseInSeconds !== null) {
      const subject = machine.mode === 'all' ? 'All agents' : 'Watched agent';
      return `${subject} idle — releasing in ${evaluation.releaseInSeconds}s. ${assertion}`;
    }
    const names = evaluation.waitingAgentIds
      .map(processName)
      .filter((name): name is string => name !== null);
    const count = evaluation.waitingAgentIds.length;
    return `Keeping Mac awake. ${assertion} Waiting on ${count} ${count === 1 ? 'agent' : 'agents'}: ${names.join(', ')}`;
  });
  let assertionHistoryLine = $derived(
    machine.armed && nativeStatus.notice ? nativeStatus.notice : null
  );

  $effect(() => {
    armed = machine.armed;
  });

  $effect(() => {
    if (machine.armed || availableAgents.some((process) => String(process.id) === specificAgentId)) return;
    specificAgentId = String(availableAgents[0]?.id ?? '');
  });

  $effect(() => {
    const next = evaluation;
    if (next.shouldRelease) {
      if (!autoReleasePending) void releaseAutomatically();
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
    let pollCount = 0;
    void syncNativeStatus();
    const timer = window.setInterval(() => {
      clockTick = monotonicNow();
      pollCount += 1;
      const shouldPoll = machine.armed ? pollCount % 3 === 0 : pollCount % 15 === 0;
      if (!busy && shouldPoll) void syncNativeStatus();
    }, 1_000);

    return () => {
      componentActive = false;
      window.clearInterval(timer);
      if (machine.armed) void invoke('keep_awake_stop').catch(() => undefined);
    };
  });

  async function syncNativeStatus(): Promise<void> {
    if (nativeSyncPending) return;
    nativeSyncPending = true;
    const statusGeneration = machineGeneration;
    try {
      const status = await invoke<NativeKeepAwakeStatus>('keep_awake_status');
      if (!componentActive || machineGeneration !== statusGeneration) return;
      applyNativeStatus(status);
      if (!machine.armed && status.armed) {
        mode = 'all';
        machine = armKeepAwake('all', null);
        machineGeneration += 1;
        lastReleaseReason = null;
        clockTick = monotonicNow();
      }
    } catch (cause) {
      if (componentActive && machineGeneration === statusGeneration) warning = message(cause);
    } finally {
      nativeSyncPending = false;
    }
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
        mode === 'specific' ? Number(specificAgentId) : null
      );
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

  async function disarm(): Promise<void> {
    if (busy) return;
    busy = true;
    const generation = machineGeneration;
    try {
      const status = await invoke<NativeKeepAwakeStatus>('keep_awake_stop');
      const disarmed = machine.armed && machineGeneration === generation;
      if (disarmed) {
        machine = disarmKeepAwake(machine);
        machineGeneration += 1;
        lastReleaseReason = 'user';
      }
      applyNativeStatus(status);
      warning = null;
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

  function applyNativeStatus(status: NativeKeepAwakeStatus): void {
    supported = status.supported;
    nativeStatus = status;
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
          data-armed={machine.armed}
          data-warning={statusIsWarning}
          label={triggerLabel}
        >
          {#snippet icon()}
            <CoffeeIcon size={15} strokeWidth={1.8} fill={machine.armed ? 'currentColor' : 'none'} />
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
          <input type="radio" bind:group={mode} value="all" />
          <span>Until all agents are idle</span>
        </label>
        <label class:chosen={mode === 'specific'}>
          <input type="radio" bind:group={mode} value="specific" />
          <span>Until a specific agent is idle</span>
        </label>
      </fieldset>

      {#if mode === 'specific'}
        <div class="agent-select">
          <label for="keep-awake-agent">Running agent</label>
          <Select.Root
            type="single"
            value={specificAgentId}
            disabled={busy || machine.armed || availableAgents.length === 0}
            onValueChange={(value) => { if (value) specificAgentId = value; }}
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
        onclick={() => machine.armed ? void disarm() : void arm()}
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
  .agent-select { display: grid; }
  .agent-select :global([data-slot='select-trigger']) { width: 100%; }
  p { min-height: 31px; margin: 0; border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space-2); background: var(--card); color: var(--text-soft); font: var(--font-size-xs) var(--font-mono); line-height: 1.45; }
  p.warning { border-color: color-mix(in srgb, var(--destructive) 40%, var(--border)); color: var(--destructive); }
  p.assertion-history { min-height: 0; border-style: dashed; color: var(--muted-foreground); }
  :global(.keep-awake-popover > button:last-child) { width: 100%; }
</style>
