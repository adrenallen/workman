<script module lang="ts">
  export interface TimerCountdownProps {
    processId: number;
    variant?: 'chips' | 'menu';
    density?: 'full' | 'compact' | 'hidden';
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  import { liveTimers, type TimerView } from './timerLifecycle';

  let { processId, variant = 'chips', density = 'full' }: TimerCountdownProps = $props();
  let now = $state(Date.now());

  const timers = $derived(
    Object.values($liveTimers)
      .filter((timer) => timer.delivery_process_id === processId && !timer.fired)
      .sort((left, right) => left.due_at - right.due_at)
  );
  const visibleTimers = $derived(timers.slice(0, 2));
  const hiddenTimerCount = $derived(Math.max(0, timers.length - visibleTimers.length));

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

  function lead(value: TimerView): string {
    if (value.paused) return 'paused';
    if (value.kind !== 'delay') return 'waiting for idle';
    return value.repeating ? 'next fire in' : 'fires in';
  }

  function suffix(value: TimerView): string {
    if (value.paused) return 'left';
    return value.kind === 'delay' ? '' : 'max';
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

{#if variant === 'menu'}
  <div class="timer-menu">
    {#if timers.length === 0}
      <span class="timer-empty">No active timers</span>
    {:else}
      {#each timers as timer (timer.id)}
        <article title={detail(timer)}>
          <div><strong>{kindName(timer)}</strong><span>{scheduleLabel(timer)}</span></div>
          <small>To process #{timer.delivery_process_id} · by {timer.owner_actor}</small>
        </article>
      {/each}
    {/if}
  </div>
{:else}
  {#each visibleTimers as timer (timer.id)}
    <span
      class:idle={timer.kind !== 'delay'}
      class:paused={timer.paused}
      class:compact={density === 'compact'}
      class:hidden={density === 'hidden'}
      class="countdown"
      title={detail(timer)}
      aria-label={scheduleLabel(timer)}
    >
      <span class="direction" aria-hidden="true">↓</span>
      <span class="timer-phrase">{lead(timer)}</span>
      {#if suffix(timer)}<span class="timer-divider">·</span>{/if}
      <span class="timer-value">{formatRemaining(timer)}</span>
      {#if suffix(timer)}<span class="timer-suffix">{suffix(timer)}</span>{/if}
    </span>
  {/each}
  {#if hiddenTimerCount > 0}
    <span
      class="timer-more"
      class:compact={density === 'compact'}
      class:hidden={density === 'hidden'}
      title={`${hiddenTimerCount} additional active timers`}
    >
      +{hiddenTimerCount}<span> timers</span>
    </span>
  {/if}
{/if}

<style>
  .countdown {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 4px;
    border-left: 1px solid var(--border);
    padding: 0 9px;
    color: var(--text-soft);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .timer-more {
    display: flex;
    align-items: center;
    border-left: 1px solid var(--border);
    padding: 0 7px;
    color: var(--muted);
    font-size: var(--font-size-xs);
    white-space: nowrap;
  }

  .direction {
    color: var(--signal);
    font-size: var(--font-size-sm);
    font-weight: 700;
    line-height: 1;
  }

  .timer-phrase,
  .timer-value,
  .timer-suffix {
    color: var(--foreground);
    font-weight: 600;
  }

  .timer-value {
    font-variant-numeric: tabular-nums;
  }

  .timer-divider,
  .timer-suffix {
    color: #89919b;
  }

  .idle .direction {
    color: var(--warning);
  }

  .paused .direction {
    color: var(--muted);
  }

  .paused .timer-phrase,
  .paused .timer-value,
  .paused .timer-divider,
  .paused .timer-suffix {
    color: var(--muted);
  }

  .timer-menu {
    display: grid;
    gap: 5px;
  }

  .timer-menu article {
    display: grid;
    gap: 3px;
    border: 1px solid #30353c;
    border-radius: 2px;
    padding: 7px 8px;
    background: #171a1e;
  }

  .timer-menu article div {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .timer-menu strong {
    color: #aeb5bd;
    font-size: var(--font-size-xs);
    text-transform: uppercase;
  }

  .timer-menu article span {
    color: var(--foreground);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }

  .timer-menu small,
  .timer-empty {
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
  }

  .countdown.compact {
    gap: 3px;
    padding: 0 7px;
  }

  .countdown.compact .timer-phrase,
  .countdown.compact .timer-divider,
  .countdown.compact .timer-suffix,
  .timer-more.compact span {
    display: none;
  }

  .countdown.hidden,
  .timer-more.hidden {
    display: none;
  }
</style>
