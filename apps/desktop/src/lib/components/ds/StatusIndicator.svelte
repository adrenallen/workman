<script lang="ts">
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { cn } from '$lib/utils';

  type StatusTone = 'success' | 'danger' | 'warning' | 'needs-input' | 'waiting' | 'neutral';

  interface Props {
    label: string;
    tone?: StatusTone;
    class?: string;
  }

  let { label, tone = 'neutral', class: className }: Props = $props();
</script>

<Tooltip.Provider delayDuration={250}>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <span
          {...props}
          class={cn('status-indicator', className)}
          data-tone={tone}
          aria-label={label}
          role="status"
        ><span aria-hidden="true"></span></span>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content sideOffset={6}>{label}</Tooltip.Content>
  </Tooltip.Root>
</Tooltip.Provider>

<style>
  .status-indicator { display: inline-grid; width: 14px; height: 14px; flex: none; place-items: center; }
  .status-indicator > span { width: 7px; height: 7px; border-radius: 999px; background: var(--muted-foreground); }
  .status-indicator[data-tone='success'] > span { background: var(--success); }
  .status-indicator[data-tone='danger'] > span { background: var(--destructive); }
  .status-indicator[data-tone='warning'] > span { background: var(--warning-token); }
  .status-indicator[data-tone='needs-input'] > span { background: var(--agent-state-needs-input); }
  .status-indicator[data-tone='waiting'] > span { background: var(--agent-state-waiting); }
</style>
