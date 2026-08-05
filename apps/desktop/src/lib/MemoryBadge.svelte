<script module lang="ts">
  export interface MemoryBadgeProps {
    bytes: number;
    title?: string;
  }
</script>

<script lang="ts">
  import TooltipLabel from '$lib/components/ds/TooltipLabel.svelte';

  let { bytes, title }: MemoryBadgeProps = $props();
  let formatted = $derived(formatBytes(bytes));

  function formatBytes(value: number): string {
    const bytes = Math.max(0, value);
    if (bytes < 1024) return `${Math.round(bytes)}B`;
    const units = ['KB', 'MB', 'GB', 'TB'];
    let amount = bytes / 1024;
    let unit = units[0];
    for (let index = 1; index < units.length && amount >= 1024; index += 1) {
      amount /= 1024;
      unit = units[index];
    }
    const digits = amount >= 100 ? 0 : 1;
    return `${amount.toFixed(digits).replace(/\.0$/, '')}${unit}`;
  }
</script>

<TooltipLabel label={title ?? `Process memory · ${formatted}`}>
  <span class="memory" aria-label={title ?? `Process memory · ${formatted}`}>{formatted}</span>
</TooltipLabel>

<style>
  .memory {
    display: inline-flex;
    max-width: 72px;
    height: 18px;
    align-items: center;
    overflow: hidden;
    border: 1px solid var(--border, var(--border));
    border-radius: 3px;
    padding: 0 5px;
    background: var(--surface-raised, var(--popover));
    color: var(--muted, var(--muted-foreground));
    font: 620 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace;
    font-variant-numeric: tabular-nums;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
