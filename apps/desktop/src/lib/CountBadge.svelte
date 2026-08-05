<script module lang="ts">
  export interface CountBadgeProps {
    value: string | number;
    prefix?: string;
    tone?: 'neutral' | 'running' | 'attention';
    title?: string;
  }
</script>

<script lang="ts">
  import TooltipLabel from '$lib/components/ds/TooltipLabel.svelte';

  let {
    value,
    prefix = '',
    tone = 'neutral',
    title
  }: CountBadgeProps = $props();
</script>

<TooltipLabel label={title ?? `Count: ${prefix}${value}`}>
  <span class="badge {tone}" aria-label={title ?? `Count: ${prefix}${value}`}>{prefix}{value}</span>
</TooltipLabel>

<style>
  .badge {
    display: inline-flex;
    min-width: 17px;
    max-width: 68px;
    height: 18px;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    border: 1px solid var(--border, var(--border));
    border-radius: 3px;
    padding: 0 5px;
    background: var(--surface-raised, var(--popover));
    color: var(--muted, var(--muted-foreground));
    font: 620 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace;
    letter-spacing: 0.01em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .running { border-color: color-mix(in srgb, var(--signal, #55b989) 42%, var(--border, var(--border))); color: var(--signal, #55b989); }
  .attention { border-color: color-mix(in srgb, var(--warning, #d6a24f) 42%, var(--border, var(--border))); color: var(--warning, #d6a24f); }
</style>
