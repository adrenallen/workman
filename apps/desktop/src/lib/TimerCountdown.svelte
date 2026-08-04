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
  const timer = $derived(timers[0] ?? null);

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

  function label(value: TimerView): string {
    if (value.paused) return 'paused';
    if (value.kind !== 'delay') return 'idle';
    return value.repeating ? 'repeat' : 'timer';
  }

  function description(value: TimerView): string {
    const purpose = value.kind === 'delay' ? 'next delivery' : 'idle timer max-wait ceiling';
    const extra = timers.length > 1 ? `; ${timers.length - 1} more timer(s)` : '';
    return `${purpose} in ${formatRemaining(value)}${extra}`;
  }
</script>

{#if timer}
  <span class:paused={timer.paused} class="countdown" title={description(timer)}>
    <i aria-hidden="true"></i>
    <strong>{label(timer)}</strong>
    <span>{formatRemaining(timer)}</span>
    {#if timers.length > 1}<em>+{timers.length - 1}</em>{/if}
  </span>
{/if}

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

  i {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--signal);
  }

  strong {
    color: #aeb5bd;
    font-size: 8px;
    font-weight: 700;
    text-transform: uppercase;
  }

  span {
    color: #d6dae0;
  }

  em {
    color: var(--muted);
    font-size: 7px;
    font-style: normal;
  }

  .paused i {
    background: var(--muted);
  }

  .paused span,
  .paused strong {
    color: var(--muted);
  }
</style>
