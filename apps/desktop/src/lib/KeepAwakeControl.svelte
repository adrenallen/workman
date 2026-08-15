<script lang="ts">
  import CoffeeIcon from '@lucide/svelte/icons/coffee';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Popover from '$lib/components/ui/popover';
  import * as Select from '$lib/components/ui/select';
  import type { ConnectionStatus, ProcessView, Project } from './daemon';
  import {
    armKeepAwake,
    disarmKeepAwake,
    evaluateKeepAwake,
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
    open?: boolean;
  }

  interface NativeKeepAwakeStatus {
    supported: boolean;
    active: boolean;
    warning: string | null;
  }

  let {
    processes,
    projects,
    connectionStatus,
    open = $bindable(false)
  }: Props = $props();

  let supported = $state<boolean | null>(null);
  let busy = $state(false);
  let warning = $state<string | null>(null);
  let mode = $state<KeepAwakeMode>('all');
  let specificAgentId = $state('');
  let machine = $state(initialKeepAwakeState());
  let autoReleasePending = false;
  let now = $state(Date.now());
  let availableAgents = $derived(runningAgents(processes));
  let evaluation = $derived(evaluateKeepAwake(machine, processes, now));
  let selectedAgent = $derived(
    availableAgents.find((process) => String(process.id) === specificAgentId) ?? null
  );
  let triggerLabel = $derived.by(() => {
    if (warning) return warning;
    if (!machine.armed) return 'Keep Mac awake until agents idle';
    if (mode === 'specific') {
      const name = processName(machine.watchedAgentIds[0]);
      return `Keeping Mac awake until ${name ?? 'the watched agent'} is idle`;
    }
    return 'Keeping Mac awake until all watched agents are idle';
  });
  let statusLine = $derived.by(() => {
    if (warning) return warning;
    if (!machine.armed) return 'Ready to prevent system idle sleep.';
    if (evaluation.releaseInSeconds !== null) {
      return `All watched agents idle — releasing in ${evaluation.releaseInSeconds}s`;
    }
    const names = evaluation.waitingAgentIds
      .map(processName)
      .filter((name): name is string => name !== null);
    const count = evaluation.waitingAgentIds.length;
    return `Waiting on ${count} ${count === 1 ? 'agent' : 'agents'}: ${names.join(', ')}`;
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
    if (!machine.armed || connectionStatus === 'connected') return;
    const timeout = window.setTimeout(() => {
      if (machine.armed) void disarm('Keep awake stopped because the daemon disconnected.');
    }, 10_000);
    return () => window.clearTimeout(timeout);
  });

  onMount(() => {
    let active = true;
    let pollCount = 0;
    void syncNativeStatus();
    const timer = window.setInterval(() => {
      now = Date.now();
      pollCount += 1;
      if (machine.armed && pollCount % 3 === 0) void syncNativeStatus();
    }, 1_000);

    return () => {
      active = false;
      window.clearInterval(timer);
      if (machine.armed) void invoke('keep_awake_stop').catch(() => undefined);
    };

    async function syncNativeStatus(): Promise<void> {
      try {
        const status = await invoke<NativeKeepAwakeStatus>('keep_awake_status');
        if (!active) return;
        supported = status.supported;
        if (status.warning) warning = status.warning;
        if (machine.armed && !status.active) machine = disarmKeepAwake(machine);
      } catch (cause) {
        if (active) warning = message(cause);
      }
    }
  });

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
      supported = status.supported;
      if (!status.supported || !status.active) {
        warning = status.warning ?? 'macOS keep awake is unavailable.';
        return;
      }
      machine = armKeepAwake(
        mode,
        mode === 'specific' ? Number(specificAgentId) : null,
        processes
      );
      now = Date.now();
    } catch (cause) {
      warning = message(cause);
    } finally {
      busy = false;
    }
  }

  async function disarm(reason: string | null = null): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await invoke<NativeKeepAwakeStatus>('keep_awake_stop');
      machine = disarmKeepAwake(machine);
      warning = reason;
    } catch (cause) {
      warning = message(cause);
    } finally {
      busy = false;
    }
  }

  async function releaseAutomatically(): Promise<void> {
    autoReleasePending = true;
    try {
      await invoke<NativeKeepAwakeStatus>('keep_awake_stop');
    } catch (cause) {
      warning = message(cause);
      machine = { ...machine, idleSince: Date.now() };
      autoReleasePending = false;
      return;
    }
    machine = disarmKeepAwake(machine);
    autoReleasePending = false;
    await deliverNativeSystemNotification(
      'Keep awake released — all watched agents idle',
      'Your Mac may sleep normally again.'
    );
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
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
          data-warning={warning !== null}
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
        <Popover.Description>Prevent system idle sleep until watched agents are fully idle.</Popover.Description>
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

      <p class:warning role={warning ? 'alert' : 'status'} aria-live="polite">{statusLine}</p>

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
  p { min-height: 31px; margin: 0; border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space-2); background: var(--card); color: var(--text-soft); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; line-height: 1.45; }
  p.warning { border-color: color-mix(in srgb, var(--destructive) 40%, var(--border)); color: var(--destructive); }
  :global(.keep-awake-popover > button:last-child) { width: 100%; }
</style>
