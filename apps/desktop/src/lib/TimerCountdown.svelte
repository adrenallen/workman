<script module lang="ts">
  export interface TimerCountdownProps {
    processId: number;
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  import { liveTimers, type TimerView } from './timerLifecycle';

  let { processId }: TimerCountdownProps = $props();
  let now = $state(Date.now());

  const timers = $derived(
    Object.values($liveTimers)
      .filter((timer) => timer.delivery_process_id === processId && !timer.fired)
      .sort((left, right) => left.due_at - right.due_at)
  );

  onMount(() => {
    const interval = window.setInterval(() => (now = Date.now()), 250);
    return () => window.clearInterval(interval);
  });

  function remainingMs(value: TimerView): number {
    const clock = value.paused ? (value.paused_at ?? now) : now;
    return Math.max(0, value.due_at - clock);
  }

  function formatRemaining(value: TimerView): string {
    const seconds = Math.ceil(remainingMs(value) / 1_000);
    if (seconds < 60) return `${seconds}s`;
    const minutes = Math.floor(seconds / 60);
    return `${minutes}:${(seconds % 60).toString().padStart(2, '0')}`;
  }

  function formatInterval(milliseconds: number | null): string {
    const seconds = Math.max(1, Math.ceil((milliseconds ?? 0) / 1_000));
    if (seconds < 60) return `${seconds}s`;
    const minutes = Math.floor(seconds / 60);
    return `${minutes}:${(seconds % 60).toString().padStart(2, '0')}`;
  }

  function kindName(value: TimerView): string {
    if (value.kind === 'idle_any') return 'Idle-any timer';
    if (value.kind === 'idle_all') return 'Idle-all timer';
    return value.repeating ? 'Repeating delay timer' : 'One-shot delay timer';
  }

  function scheduleLabel(value: TimerView): string {
    const remaining = formatRemaining(value);
    if (value.paused) return `paused · ${remaining} left`;
    if (value.kind !== 'delay') return `waiting for idle · ${remaining} max`;
    if (value.repeating) return `next fire in ${remaining}`;
    return `fires in ${remaining}`;
  }

  function detail(value: TimerView): string {
    const watchList = value.watch_process_ids.length
      ? value.watch_process_ids.map((id) => `process #${id}`).join(', ')
      : 'none';
    const schedule = value.kind === 'delay' ? 'Next fire' : 'Maximum wait deadline';
    const lines = [
      `${kindName(value)} #${value.id}`,
      `Status: ${scheduleLabel(value)}`,
      `${schedule}: ${new Date(value.due_at).toLocaleString()}`,
      `Watch list: ${watchList}`,
      `Delivery target: process #${value.delivery_process_id}`,
      `Created by: ${value.owner_actor}`,
      `Created: ${new Date(value.created_at).toLocaleString()}`,
      `Message: ${value.body}`
    ];
    if (value.repeating) lines.splice(3, 0, `Repeat interval: ${formatInterval(value.interval_ms)}`);
    return lines.join('\n');
  }
</script>

{#each timers as timer (timer.id)}
  <span
    class:idle={timer.kind !== 'delay'}
    class:paused={timer.paused}
    class="countdown"
    title={detail(timer)}
    aria-label={scheduleLabel(timer)}
  >
    <span class="direction" aria-hidden="true">↓</span>
    <span class="copy">{scheduleLabel(timer)}</span>
  </span>
{/each}

<style>
  .countdown {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 5px;
    border-left: 1px solid #262a2f;
    padding: 0 9px;
    color: var(--text-soft);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .direction {
    color: var(--signal);
    font-size: 11px;
    font-weight: 700;
    line-height: 1;
  }

  .copy {
    color: #d6dae0;
    font-weight: 600;
  }

  .idle .direction {
    color: var(--warning);
  }

  .paused .direction {
    color: var(--muted);
  }

  .paused .copy {
    color: var(--muted);
  }
</style>
