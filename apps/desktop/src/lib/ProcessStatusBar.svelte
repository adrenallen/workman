<script module lang="ts">
  import type { DaemonClient, ProcessView, Project } from './daemon';

  export interface ProcessStatusBarProps {
    client: DaemonClient;
    project: Project;
    process: ProcessView;
    processes: ProcessView[];
    connected: boolean;
    onUnfocus: () => void;
    onSelectProcess: (processId: number) => void;
    onError: (message: string) => void;
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  import { liveStats, type DescendantProcessStats } from './liveStats';
  import { killSubprocess, listSubprocesses } from './subprocesses';
  import TimerCountdown from './TimerCountdown.svelte';

  let {
    client,
    project,
    process,
    processes,
    connected,
    onUnfocus,
    onSelectProcess,
    onError
  }: ProcessStatusBarProps = $props();

  let popoverRoot = $state<HTMLElement>();
  let popoverOpen = $state(false);
  let freshChildren = $state<DescendantProcessStats[]>([]);
  let loadingChildren = $state(false);
  let confirmingPid = $state<number | null>(null);
  let killingPid = $state<number | null>(null);

  const stats = $derived($liveStats.processes[process.id] ?? null);
  const childCount = $derived(popoverOpen ? freshChildren.length : (stats?.descendant_count ?? 0));
  const siblingIndex = $derived(processes.findIndex((candidate) => candidate.id === process.id));

  onMount(() => {
    const handlePointerDown = (event: PointerEvent) => {
      if (popoverOpen && !popoverRoot?.contains(event.target as Node)) closePopover();
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && popoverOpen) {
        event.preventDefault();
        closePopover();
      }
    };
    document.addEventListener('pointerdown', handlePointerDown);
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown);
      document.removeEventListener('keydown', handleKeyDown);
    };
  });

  $effect(() => {
    process.id;
    popoverOpen = false;
    freshChildren = [];
    confirmingPid = null;
  });

  $effect(() => {
    if (!popoverOpen || !connected) return;
    const processId = process.id;
    queueMicrotask(() => void refreshChildren(processId));
    const timer = window.setInterval(() => void refreshChildren(processId), 2_000);
    return () => window.clearInterval(timer);
  });

  function reportCause(cause: unknown): void {
    onError(cause instanceof Error ? cause.message : String(cause));
  }

  async function refreshChildren(processId: number): Promise<void> {
    if (loadingChildren) return;
    loadingChildren = true;
    try {
      const result = await listSubprocesses(client, processId);
      if (result.process_id === process.id) freshChildren = result.subprocesses;
    } catch (cause) {
      closePopover();
      reportCause(cause);
    } finally {
      loadingChildren = false;
    }
  }

  function togglePopover(): void {
    popoverOpen = !popoverOpen;
    confirmingPid = null;
    if (popoverOpen) {
      freshChildren = stats?.descendants ?? [];
    }
  }

  function closePopover(): void {
    popoverOpen = false;
    confirmingPid = null;
  }

  async function confirmKill(child: DescendantProcessStats): Promise<void> {
    if (killingPid !== null) return;
    killingPid = child.pid;
    try {
      await killSubprocess(client, process.id, child.pid);
      confirmingPid = null;
      await refreshChildren(process.id);
    } catch (cause) {
      reportCause(cause);
    } finally {
      killingPid = null;
    }
  }

  function cycle(direction: -1 | 1): void {
    if (processes.length < 2) return;
    const current = siblingIndex < 0 ? 0 : siblingIndex;
    const next = (current + direction + processes.length) % processes.length;
    onSelectProcess(processes[next].id);
  }

  function formatDuration(totalSeconds: number | undefined): string {
    const seconds = Math.max(0, Math.floor(totalSeconds ?? 0));
    const hours = Math.floor(seconds / 3_600);
    const minutes = Math.floor((seconds % 3_600) / 60);
    const remainder = seconds % 60;
    return hours > 0
      ? `${hours}:${minutes.toString().padStart(2, '0')}:${remainder.toString().padStart(2, '0')}`
      : `${minutes.toString().padStart(2, '0')}:${remainder.toString().padStart(2, '0')}`;
  }

  function formatMemory(bytes: number | undefined): string {
    if (!bytes) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1_024)), units.length - 1);
    const value = bytes / 1_024 ** index;
    return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
  }

  function attentionWord(): string {
    if (process.agent_state.needs_input) return 'Needs input';
    if (process.agent_state.working) return 'Working';
    if (process.agent_state.idle) return 'Idle';
    return 'Exited';
  }

  function childCommand(child: DescendantProcessStats): string {
    return child.command?.trim() || child.name;
  }
</script>

<footer class="status-bar" aria-label={`${project.name} selected process status`}>
  <nav class="navigation" aria-label="Process selection">
    <button
      type="button"
      class="unfocus"
      title="Unfocus terminal (⌘U)"
      aria-keyshortcuts="Meta+U"
      onclick={onUnfocus}
    >
      <span aria-hidden="true">×</span> Unfocus
    </button>
    <span class="rule" aria-hidden="true"></span>
    <button
      type="button"
      title="Previous process"
      disabled={processes.length < 2}
      onclick={() => cycle(-1)}
    >
      <span aria-hidden="true">←</span> Prev
    </button>
    <button
      type="button"
      title="Next process"
      disabled={processes.length < 2}
      onclick={() => cycle(1)}
    >
      Next <span aria-hidden="true">→</span>
    </button>
  </nav>

  <div class="telemetry">
    <span class="metric uptime" title={`Process uptime: ${formatDuration(stats?.uptime_seconds)}`}>
      <span class="clock" aria-hidden="true">◷</span>
      <span>up {formatDuration(stats?.uptime_seconds)}</span>
    </span>
    <TimerCountdown processId={process.id} />
    <strong class="process-name" title={process.name}>{process.name}</strong>

    <div class="subprocess-control" bind:this={popoverRoot}>
      <button
        type="button"
        class="subprocess-trigger"
        class:active={popoverOpen}
        aria-haspopup="dialog"
        aria-expanded={popoverOpen}
        disabled={!connected || process.pid === null}
        onclick={togglePopover}
      >
        <span class="branch" aria-hidden="true">⑂</span>
        +{childCount} {childCount === 1 ? 'subprocess' : 'subprocesses'}
      </button>

      {#if popoverOpen}
        <dialog open class="subprocess-popover" aria-label={`${process.name} subprocesses`}>
          <header>
            <div>
              <span class="eyebrow">Live process tree</span>
              <h2>{process.name}</h2>
            </div>
            <span class="root-pid">ROOT {process.pid}</span>
          </header>

          {#if loadingChildren && freshChildren.length === 0}
            <div class="empty-tree"><span></span>Sampling descendants…</div>
          {:else if freshChildren.length === 0}
            <div class="empty-tree"><span></span>No live subprocesses</div>
          {:else}
            <div class="child-list">
              {#each freshChildren as child (child.pid)}
                <article class="child-row">
                  <span class="lineage" aria-hidden="true"><i></i></span>
                  <div class="child-copy">
                    <div class="child-heading">
                      <strong>{child.name}</strong>
                      <span>PID {child.pid}</span>
                    </div>
                    <code title={childCommand(child)}>{childCommand(child)}</code>
                  </div>
                  <div class="child-stats" aria-label={`PID ${child.pid} resource usage`}>
                    <span>{child.cpu_percent.toFixed(1)}%</span>
                    <span>{formatMemory(child.memory_bytes)}</span>
                  </div>
                  <div class="kill-action">
                    {#if confirmingPid === child.pid}
                      <button type="button" class="cancel" onclick={() => (confirmingPid = null)}>
                        Cancel
                      </button>
                      <button
                        type="button"
                        class="confirm"
                        disabled={killingPid !== null}
                        onclick={() => confirmKill(child)}
                      >
                        {killingPid === child.pid ? 'Stopping…' : 'Confirm'}
                      </button>
                    {:else}
                      <button
                        type="button"
                        class="kill"
                        disabled={killingPid !== null}
                        onclick={() => (confirmingPid = child.pid)}
                      >
                        Kill
                      </button>
                    {/if}
                  </div>
                </article>
              {/each}
            </div>
          {/if}

          <footer>
            <span>Only descendants of this live root can be signaled.</span>
            <button type="button" onclick={closePopover}>Close</button>
          </footer>
        </dialog>
      {/if}
    </div>

    <span class="metric" title="CPU usage">CPU {(stats?.cpu_percent ?? 0).toFixed(1)}%</span>
    <span class="metric" title="Memory usage">MEM {formatMemory(stats?.memory_bytes)}</span>
    <span
      class="attention"
      class:waiting={process.agent_state.needs_input}
      class:working={process.agent_state.working}
      >{attentionWord()}</span
    >
    <span class:active-run={process.status === 'running'} class:fault={process.status === 'crashed'} class="run-state">
      <i aria-hidden="true"></i>{process.status}
    </span>
  </div>
</footer>

<style>
  .status-bar {
    position: relative;
    z-index: 20;
    display: flex;
    min-width: 0;
    height: 30px;
    align-items: stretch;
    justify-content: space-between;
    border-top: 1px solid var(--border);
    background: #141619;
    color: var(--text-soft);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 9px;
    letter-spacing: 0.02em;
  }

  button {
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.36;
    cursor: default;
  }

  .navigation,
  .telemetry {
    display: flex;
    min-width: 0;
    align-items: stretch;
  }

  .navigation button {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 8px;
    color: #8d949e;
    font-size: 8px;
    font-weight: 600;
    text-transform: uppercase;
  }

  .navigation button:not(:disabled):hover,
  .navigation button:focus-visible {
    background: #202329;
    color: var(--fog);
  }

  .navigation .unfocus {
    color: #b8bdc4;
  }

  .navigation .unfocus span {
    color: var(--fault);
    font-size: 12px;
  }

  .rule {
    width: 1px;
    height: 14px;
    align-self: center;
    background: var(--border);
  }

  .telemetry {
    overflow: visible;
    justify-content: flex-end;
  }

  .telemetry > span,
  .process-name,
  .subprocess-trigger {
    display: flex;
    min-width: 0;
    align-items: center;
    border-left: 1px solid #262a2f;
    padding: 0 9px;
    white-space: nowrap;
  }

  .metric {
    color: #858c96;
    font-variant-numeric: tabular-nums;
  }

  .uptime {
    gap: 5px;
    color: #a7adb5;
  }

  .clock {
    color: #737b85;
    font-size: 11px;
  }

  .process-name {
    max-width: 220px;
    overflow: hidden;
    color: #e0e3e7;
    font-family: 'Archivo Variable', sans-serif;
    font-size: 10px;
    font-weight: 650;
    letter-spacing: 0;
    text-overflow: ellipsis;
  }

  .subprocess-control {
    position: relative;
    display: flex;
  }

  .subprocess-trigger {
    gap: 5px;
    color: #aab0b8;
    font-size: 8px;
    font-weight: 600;
    text-transform: uppercase;
  }

  .subprocess-trigger:hover,
  .subprocess-trigger.active {
    background: #20242a;
    color: #edf0f3;
  }

  .branch {
    color: var(--signal);
    font-size: 12px;
  }

  .attention {
    color: #979da6;
    font-weight: 650;
    text-transform: uppercase;
  }

  .attention.waiting {
    color: var(--warning);
  }

  .attention.working {
    color: var(--signal);
  }

  .run-state {
    gap: 6px;
    color: #878e98;
    font-weight: 650;
    text-transform: uppercase;
  }

  .run-state i {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
  }

  .run-state.active-run {
    color: var(--signal);
  }

  .run-state.fault {
    color: var(--fault);
  }

  .subprocess-popover {
    position: absolute;
    right: 0;
    bottom: calc(100% + 7px);
    width: min(620px, calc(100vw - 42px));
    margin: 0;
    padding: 0;
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    background: #191c20;
    box-shadow: 0 18px 48px rgb(0 0 0 / 48%);
    color: var(--text-soft);
    font: inherit;
  }

  .subprocess-popover::after {
    position: absolute;
    right: 32px;
    bottom: -5px;
    width: 8px;
    height: 8px;
    border-right: 1px solid var(--border-strong);
    border-bottom: 1px solid var(--border-strong);
    background: #191c20;
    content: '';
    transform: rotate(45deg);
  }

  .subprocess-popover > header,
  .subprocess-popover > footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .subprocess-popover > header {
    min-height: 48px;
    padding: 8px 11px 7px 15px;
    border-bottom: 1px solid var(--border);
    background: #1d2025;
  }

  .eyebrow {
    display: block;
    margin-bottom: 3px;
    color: var(--signal);
    font-size: 7px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  h2 {
    margin: 0;
    color: var(--fog);
    font-family: 'Archivo Variable', sans-serif;
    font-size: 13px;
    font-weight: 650;
  }

  .root-pid {
    border: 1px solid #3a3f46;
    border-radius: 2px;
    padding: 4px 6px;
    color: #8f969f;
    font-size: 7px;
  }

  .child-list {
    max-height: 300px;
    overflow-y: auto;
    padding: 5px 0;
  }

  .child-row {
    display: grid;
    min-height: 54px;
    grid-template-columns: 18px minmax(150px, 1fr) auto auto;
    align-items: center;
    padding: 4px 9px 4px 7px;
  }

  .child-row + .child-row {
    border-top: 1px solid #24282d;
  }

  .lineage {
    position: relative;
    width: 12px;
    height: 100%;
  }

  .lineage::before {
    position: absolute;
    top: -9px;
    bottom: -9px;
    left: 4px;
    width: 1px;
    background: #384049;
    content: '';
  }

  .lineage i {
    position: absolute;
    top: 50%;
    left: 4px;
    width: 8px;
    height: 1px;
    background: var(--signal);
  }

  .child-copy {
    min-width: 0;
    padding-right: 12px;
  }

  .child-heading {
    display: flex;
    min-width: 0;
    align-items: baseline;
    gap: 8px;
  }

  .child-heading strong {
    overflow: hidden;
    color: #dfe2e6;
    font-family: 'Archivo Variable', sans-serif;
    font-size: 10px;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .child-heading span {
    color: #707781;
    font-size: 7px;
    white-space: nowrap;
  }

  code {
    display: block;
    margin-top: 4px;
    overflow: hidden;
    color: #858c96;
    font-family: inherit;
    font-size: 8px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .child-stats {
    display: grid;
    min-width: 74px;
    grid-template-columns: repeat(2, minmax(35px, auto));
    gap: 8px;
    color: #9ba2ab;
    font-size: 8px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .kill-action {
    display: flex;
    min-width: 70px;
    justify-content: flex-end;
    gap: 4px;
    margin-left: 10px;
  }

  .kill-action button,
  .subprocess-popover > footer button {
    border: 1px solid #3b4047;
    border-radius: 2px;
    padding: 4px 6px;
    background: #22262b;
    color: #aeb4bc;
    font-size: 7px;
    font-weight: 650;
    text-transform: uppercase;
  }

  .kill-action .kill:hover,
  .kill-action .confirm {
    border-color: #714747;
    color: #e58b8b;
  }

  .empty-tree {
    display: flex;
    min-height: 76px;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: #777f89;
    font-size: 8px;
    text-transform: uppercase;
  }

  .empty-tree span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--signal);
    box-shadow: 0 0 0 3px rgb(85 185 137 / 12%);
  }

  .subprocess-popover > footer {
    min-height: 34px;
    padding: 5px 8px 5px 14px;
    border-top: 1px solid var(--border);
    color: #6e7580;
    font-size: 7px;
  }

  @media (max-width: 780px) {
    .metric,
    .attention,
    .process-name {
      display: none;
    }

    .subprocess-popover {
      right: -72px;
    }
  }

  @media (prefers-reduced-motion: no-preference) {
    .subprocess-popover {
      animation: reveal 120ms ease-out;
      transform-origin: right bottom;
    }

    @keyframes reveal {
      from {
        opacity: 0;
        transform: translateY(3px) scale(0.99);
      }
    }
  }
</style>
