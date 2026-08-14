<script module lang="ts">
  import type { DaemonClient, ProcessView } from './daemon';

  export interface TimerPanelProps {
    client: DaemonClient;
    projectId: number;
    processes: ProcessView[];
    onError: (message: string) => void;
  }
</script>

<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import ChevronUpIcon from '@lucide/svelte/icons/chevron-up';
  import PauseIcon from '@lucide/svelte/icons/pause';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlayIcon from '@lucide/svelte/icons/play';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import { onMount } from 'svelte';

  import ConfirmationDialog from './ConfirmationDialog.svelte';
  import {
    deleteProjectTimer,
    listProjectTimers,
    timerKindLabel,
    timerStateLabel,
    updateProjectTimer
  } from './timerManagement';
  import { timerLifecycleRevision, type TimerView } from './timerLifecycle';

  let { client, projectId, processes, onError }: TimerPanelProps = $props();
  let timers = $state<TimerView[]>([]);
  let loading = $state(true);
  let busyTimerId = $state<number | null>(null);
  let deleteTimer = $state<TimerView | null>(null);
  let editingTimerId = $state<number | null>(null);
  let editDelaySeconds = $state('');
  let editIntervalSeconds = $state('');
  let editBody = $state('');
  let expandedBodies = $state<Set<number>>(new Set());
  let now = $state(Date.now());
  let refreshSequence = 0;
  let seenLifecycleRevision = -1;

  onMount(() => {
    void refresh();
    const interval = window.setInterval(() => (now = Date.now()), 1_000);
    return () => window.clearInterval(interval);
  });

  $effect(() => {
    const revision = $timerLifecycleRevision;
    if (seenLifecycleRevision < 0) {
      seenLifecycleRevision = revision;
      return;
    }
    if (revision === seenLifecycleRevision) return;
    seenLifecycleRevision = revision;
    queueMicrotask(() => void refresh(false));
  });

  function report(cause: unknown): void {
    onError(cause instanceof Error ? cause.message : String(cause));
  }

  async function refresh(showLoading = true): Promise<void> {
    const sequence = ++refreshSequence;
    if (showLoading) loading = true;
    try {
      const next = await listProjectTimers(client, projectId);
      if (sequence === refreshSequence) timers = next;
    } catch (cause) {
      report(cause);
    } finally {
      if (sequence === refreshSequence) loading = false;
    }
  }

  function processLabel(processId: number): string {
    const process = processes.find((candidate) => candidate.id === processId);
    return process ? `${process.name} · #${processId}` : `Process #${processId}`;
  }

  function ownerLabel(timer: TimerView): string {
    if (timer.owner_process_id !== null && timer.owner_process_name) {
      return `${timer.owner_process_name} · #${timer.owner_process_id}`;
    }
    if (timer.owner_process_id !== null) return processLabel(timer.owner_process_id);
    return timer.owner_label;
  }

  function remainingMs(timer: TimerView): number {
    const clock = timer.paused ? (timer.paused_at ?? now) : now;
    return Math.max(0, timer.due_at - clock);
  }

  function durationLabel(milliseconds: number): string {
    const seconds = Math.max(0, Math.ceil(milliseconds / 1_000));
    if (seconds < 60) return `${seconds}s`;
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
    const hours = Math.floor(minutes / 60);
    return `${hours}h ${minutes % 60}m`;
  }

  function dueLabel(timer: TimerView): string {
    if (timer.fired) return timer.fired_at ? new Date(timer.fired_at).toLocaleString() : 'Completed';
    const prefix = timer.paused ? 'Held with' : timer.kind === 'delay' ? 'Due in' : 'Max wait';
    return `${prefix} ${durationLabel(remainingMs(timer))}`;
  }

  function toggleBody(timerId: number): void {
    const next = new Set(expandedBodies);
    if (next.has(timerId)) next.delete(timerId);
    else next.add(timerId);
    expandedBodies = next;
  }

  function beginEdit(timer: TimerView): void {
    editingTimerId = timer.id;
    editDelaySeconds = Math.max(0, Math.ceil(remainingMs(timer) / 1_000)).toString();
    editIntervalSeconds = timer.interval_ms
      ? Math.max(1, Math.ceil(timer.interval_ms / 1_000)).toString()
      : '';
    editBody = timer.body;
  }

  async function saveEdit(timer: TimerView): Promise<void> {
    const delaySeconds = Number(editDelaySeconds);
    const intervalSeconds = Number(editIntervalSeconds);
    if (!Number.isFinite(delaySeconds) || delaySeconds < 0) {
      onError('Delay must be zero or a positive number of seconds.');
      return;
    }
    if (timer.repeating && (!Number.isFinite(intervalSeconds) || intervalSeconds <= 0)) {
      onError('Recurring interval must be greater than zero seconds.');
      return;
    }
    busyTimerId = timer.id;
    try {
      const updated = await updateProjectTimer(client, projectId, timer.id, {
        body: editBody,
        delay_ms: Math.round(delaySeconds * 1_000),
        ...(timer.repeating ? { interval_ms: Math.round(intervalSeconds * 1_000) } : {})
      });
      timers = timers.map((candidate) => (candidate.id === updated.id ? updated : candidate));
      editingTimerId = null;
    } catch (cause) {
      report(cause);
    } finally {
      busyTimerId = null;
    }
  }

  async function togglePaused(timer: TimerView): Promise<void> {
    busyTimerId = timer.id;
    try {
      const updated = await updateProjectTimer(client, projectId, timer.id, {
        paused: !timer.paused
      });
      timers = timers.map((candidate) => (candidate.id === updated.id ? updated : candidate));
    } catch (cause) {
      report(cause);
    } finally {
      busyTimerId = null;
    }
  }

  async function confirmDelete(): Promise<void> {
    if (!deleteTimer) return;
    const target = deleteTimer;
    busyTimerId = target.id;
    try {
      await deleteProjectTimer(client, projectId, target.id);
      timers = timers.filter((timer) => timer.id !== target.id);
      deleteTimer = null;
      if (editingTimerId === target.id) editingTimerId = null;
    } catch (cause) {
      report(cause);
    } finally {
      busyTimerId = null;
    }
  }
</script>

<section class="timer-panel" aria-label="Project timers">
  <header>
    <div>
      <span class="eyebrow">Project schedule</span>
      <h2>Timers <span>{timers.length}</span></h2>
    </div>
    <button class="icon-button" type="button" title="Refresh timers" onclick={() => void refresh()}>
      <span class:spinning={loading}><RefreshCwIcon size={14} aria-hidden="true" /></span>
      <span class="sr-only">Refresh timers</span>
    </button>
  </header>

  <div class="timer-list" aria-live="polite">
    {#if loading && timers.length === 0}
      <p class="empty">Loading project timers…</p>
    {:else if timers.length === 0}
      <p class="empty">No timers in this project.</p>
    {:else}
      {#each timers as timer (timer.id)}
        <article class:paused={timer.paused} class:fired={timer.fired}>
          <div class="time-rail" aria-hidden="true"><span></span></div>
          <div class="timer-main">
            <div class="timer-heading">
              <div class="timer-title">
                <span class="kind">{timerKindLabel(timer)}</span>
                <strong>#{timer.id}</strong>
                <span class="state">{timerStateLabel(timer)}</span>
              </div>
              <span class="due">{dueLabel(timer)}</span>
            </div>

            <dl>
              <div><dt>Owner</dt><dd>{ownerLabel(timer)}</dd></div>
              <div><dt>Delivery</dt><dd>{processLabel(timer.delivery_process_id)}</dd></div>
              {#if timer.watch_process_ids.length > 0}
                <div class="wide">
                  <dt>Watching</dt>
                  <dd>{timer.watch_process_ids.map(processLabel).join(', ')}</dd>
                </div>
              {/if}
              {#if timer.repeating && timer.interval_ms}
                <div><dt>Interval</dt><dd>{durationLabel(timer.interval_ms)}</dd></div>
              {/if}
            </dl>

            <button class="body-preview" class:expanded={expandedBodies.has(timer.id)} type="button" onclick={() => toggleBody(timer.id)}>
              <span>{timer.body || 'Empty timer body'}</span>
              {#if expandedBodies.has(timer.id)}
                <ChevronUpIcon size={13} aria-hidden="true" />
              {:else}
                <ChevronDownIcon size={13} aria-hidden="true" />
              {/if}
            </button>

            {#if editingTimerId === timer.id}
              <form class="timer-editor" onsubmit={(event) => { event.preventDefault(); void saveEdit(timer); }}>
                <label>
                  <span>Delay from now</span>
                  <div class="field-with-unit"><input type="number" min="0" step="1" bind:value={editDelaySeconds} /><em>sec</em></div>
                </label>
                {#if timer.repeating}
                  <label>
                    <span>Repeat every</span>
                    <div class="field-with-unit"><input type="number" min="1" step="1" bind:value={editIntervalSeconds} /><em>sec</em></div>
                  </label>
                {/if}
                <label class="body-field">
                  <span>Delivery body</span>
                  <textarea rows="3" bind:value={editBody}></textarea>
                </label>
                <div class="editor-actions">
                  <button type="button" onclick={() => (editingTimerId = null)}>Cancel</button>
                  <button class="primary" type="submit" disabled={busyTimerId === timer.id}>Save changes</button>
                </div>
              </form>
            {:else}
              <div class="timer-actions">
                <button type="button" disabled={timer.fired || busyTimerId === timer.id} onclick={() => void togglePaused(timer)}>
                  {#if timer.paused}<PlayIcon size={13} aria-hidden="true" /> Resume{:else}<PauseIcon size={13} aria-hidden="true" /> Pause{/if}
                </button>
                <button type="button" disabled={timer.fired || busyTimerId === timer.id} onclick={() => beginEdit(timer)}>
                  <PencilIcon size={13} aria-hidden="true" /> Edit
                </button>
                <button class="delete" type="button" disabled={busyTimerId === timer.id} onclick={() => (deleteTimer = timer)}>
                  <Trash2Icon size={13} aria-hidden="true" /> Delete
                </button>
              </div>
            {/if}
          </div>
        </article>
      {/each}
    {/if}
  </div>
</section>

{#if deleteTimer}
  <ConfirmationDialog
    title={`Delete timer #${deleteTimer.id}?`}
    description="This timer will be removed immediately and will never deliver its pending body."
    confirmLabel="Delete timer"
    busy={busyTimerId === deleteTimer.id}
    onConfirm={() => void confirmDelete()}
    onClose={() => (deleteTimer = null)}
  />
{/if}

<style>
  .timer-panel { width: min(500px, calc(100vw - 24px)); color: var(--foreground); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; border-bottom: 1px solid var(--border); padding: 2px 2px 10px; }
  header div { min-width: 0; }
  .eyebrow { color: var(--muted-foreground); font-size: 10px; font-weight: 700; letter-spacing: .1em; text-transform: uppercase; }
  h2 { margin: 2px 0 0; font-family: 'Archivo Variable', sans-serif; font-size: 17px; font-weight: 680; letter-spacing: -.02em; }
  h2 span { margin-left: 4px; color: var(--muted-foreground); font-family: 'JetBrains Mono Variable', monospace; font-size: 11px; font-weight: 550; }
  .icon-button { display: grid; width: 28px; height: 28px; place-items: center; border: 1px solid var(--border); border-radius: 5px; background: transparent; color: var(--muted-foreground); }
  .icon-button:hover { background: var(--accent); color: var(--foreground); }
  .icon-button > span { display: grid; place-items: center; }
  .spinning { animation: spin .7s linear infinite; }
  .timer-list { display: grid; max-height: min(580px, calc(100vh - 150px)); overflow-y: auto; padding: 3px 0 0; }
  article { position: relative; display: grid; grid-template-columns: 16px minmax(0, 1fr); border-bottom: 1px solid color-mix(in srgb, var(--border) 74%, transparent); padding: 11px 2px 11px 0; }
  article:last-child { border-bottom: 0; }
  .time-rail { display: flex; justify-content: center; padding-top: 5px; }
  .time-rail::before { position: absolute; top: 20px; bottom: -12px; width: 1px; background: var(--border); content: ''; }
  article:last-child .time-rail::before { display: none; }
  .time-rail span { z-index: 1; width: 7px; height: 7px; border: 2px solid var(--popover); border-radius: 50%; background: var(--signal); box-shadow: 0 0 0 1px var(--signal); }
  article.paused .time-rail span { background: var(--muted-foreground); box-shadow: 0 0 0 1px var(--muted-foreground); }
  article.fired .time-rail span { background: var(--border); box-shadow: 0 0 0 1px var(--muted-foreground); }
  .timer-main { min-width: 0; padding-left: 7px; }
  .timer-heading, .timer-title { display: flex; align-items: center; gap: 7px; }
  .timer-heading { justify-content: space-between; }
  .kind { font-size: 12px; font-weight: 690; }
  .timer-title strong, .due, dd { font-variant-numeric: tabular-nums; }
  .timer-title strong { color: var(--muted-foreground); font-family: 'JetBrains Mono Variable', monospace; font-size: 10px; font-weight: 500; }
  .state { border: 1px solid var(--border); border-radius: 999px; padding: 1px 6px; color: var(--muted-foreground); font-size: 9px; font-weight: 700; letter-spacing: .04em; text-transform: uppercase; }
  .due { color: var(--foreground); font-family: 'JetBrains Mono Variable', monospace; font-size: 10px; font-weight: 600; }
  dl { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 3px 15px; margin: 8px 0 0; }
  dl div { display: flex; min-width: 0; gap: 6px; }
  dl .wide { grid-column: 1 / -1; }
  dt { flex: 0 0 auto; color: var(--muted-foreground); font-size: 10px; }
  dd { min-width: 0; margin: 0; overflow: hidden; color: var(--foreground); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
  .body-preview { display: flex; width: 100%; align-items: flex-start; justify-content: space-between; gap: 8px; margin-top: 8px; border: 0; border-left: 2px solid var(--border); padding: 3px 6px; background: color-mix(in srgb, var(--muted) 50%, transparent); color: var(--text-soft); text-align: left; }
  .body-preview span { display: -webkit-box; overflow: hidden; font-family: 'JetBrains Mono Variable', monospace; font-size: 10px; line-height: 1.45; -webkit-box-orient: vertical; -webkit-line-clamp: 1; line-clamp: 1; white-space: pre-wrap; }
  .body-preview.expanded span { display: block; overflow: visible; -webkit-line-clamp: unset; line-clamp: unset; }
  .timer-actions, .editor-actions { display: flex; justify-content: flex-end; gap: 5px; margin-top: 8px; }
  .timer-actions button, .editor-actions button { display: inline-flex; align-items: center; gap: 4px; border: 1px solid var(--border); border-radius: 4px; padding: 4px 7px; background: transparent; color: var(--text-soft); font-size: 10px; font-weight: 620; }
  .timer-actions button:hover:not(:disabled), .editor-actions button:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .timer-actions button:disabled { opacity: .4; }
  .timer-actions .delete { color: var(--destructive); }
  .timer-editor { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin-top: 9px; border: 1px solid var(--border); border-radius: 5px; padding: 9px; background: color-mix(in srgb, var(--muted) 42%, transparent); }
  .timer-editor label { display: grid; gap: 4px; color: var(--muted-foreground); font-size: 10px; font-weight: 620; }
  .timer-editor .body-field, .editor-actions { grid-column: 1 / -1; }
  .field-with-unit { display: grid; grid-template-columns: 1fr auto; align-items: center; border: 1px solid var(--border); border-radius: 4px; background: var(--background); }
  input, textarea { min-width: 0; border: 1px solid var(--border); border-radius: 4px; padding: 6px 7px; background: var(--background); color: var(--foreground); font: 11px/1.35 'JetBrains Mono Variable', monospace; outline: none; }
  .field-with-unit input { width: 100%; border: 0; background: transparent; }
  .field-with-unit em { padding-right: 7px; color: var(--muted-foreground); font: 9px 'JetBrains Mono Variable', monospace; }
  textarea { resize: vertical; }
  input:focus, textarea:focus, .field-with-unit:focus-within { border-color: var(--ring); }
  .editor-actions { margin-top: 0; }
  .editor-actions .primary { border-color: var(--foreground); background: var(--foreground); color: var(--background); }
  .empty { margin: 0; padding: 26px 12px; color: var(--muted-foreground); font-size: 12px; text-align: center; }
  button:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spinning { animation: none; } }
</style>
