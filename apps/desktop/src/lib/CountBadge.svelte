<script module lang="ts">
  export interface CountBadgeProps {
    value: string | number;
    prefix?: string;
    tone?: 'neutral' | 'running' | 'attention';
    title?: string;
  }
</script>

<script lang="ts">
  let {
    value,
    prefix = '',
    tone = 'neutral',
    title
  }: CountBadgeProps = $props();
</script>

<span class="badge {tone}" {title}>{prefix}{value}</span>

<style>
  .badge {
    display: inline-flex;
    min-width: 17px;
    max-width: 68px;
    height: 18px;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    border: 1px solid var(--border, #30343a);
    border-radius: 3px;
    padding: 0 5px;
    background: var(--surface-raised, #1c1f23);
    color: var(--muted, #7d848e);
    font: 620 8px/1 'JetBrains Mono Variable', monospace;
    letter-spacing: 0.01em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .running { border-color: color-mix(in srgb, var(--signal, #55b989) 42%, var(--border, #30343a)); color: var(--signal, #55b989); }
  .attention { border-color: color-mix(in srgb, var(--warning, #d6a24f) 42%, var(--border, #30343a)); color: var(--warning, #d6a24f); }
</style>
