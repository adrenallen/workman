<script lang="ts">
  import CircleIcon from '@lucide/svelte/icons/circle';
  import CircleXIcon from '@lucide/svelte/icons/circle-x';
  import Clock3Icon from '@lucide/svelte/icons/clock-3';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';

  import * as Tooltip from '$lib/components/ui/tooltip';
  import { cn } from '$lib/utils';

  type StatusTone = 'success' | 'danger' | 'warning' | 'needs-input' | 'waiting' | 'neutral';
  type StatusState = 'working' | 'needs_input' | 'waiting' | 'idle' | 'stopped' | 'crashed';

  interface Props {
    label: string;
    tone?: StatusTone;
    state?: StatusState;
    class?: string;
  }

  let { label, tone = 'neutral', state, class: className }: Props = $props();
</script>

<Tooltip.Provider delayDuration={250}>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <span
          {...props}
          class={cn('status-indicator', className)}
          data-tone={tone}
          data-state={state}
          aria-label={label}
          role="status"
        >
          <span class:state-glyph={state !== undefined} aria-hidden="true">
            {#if state === 'working'}
              <LoaderCircleIcon />
            {:else if state === 'waiting'}
              <Clock3Icon />
            {:else if state === 'stopped' || state === 'crashed'}
              <CircleXIcon />
            {:else if state !== undefined}
              <CircleIcon />
            {/if}
          </span>
        </span>
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
  .status-indicator > .state-glyph { display: grid; width: 12px; height: 12px; place-items: center; border-radius: 0; background: transparent; color: var(--muted-foreground); }
  .status-indicator[data-tone='success'] > .state-glyph { color: var(--success); background: transparent; }
  .status-indicator[data-tone='danger'] > .state-glyph { color: var(--destructive); background: transparent; }
  .status-indicator[data-tone='needs-input'] > .state-glyph { color: var(--agent-state-needs-input); background: transparent; }
  .status-indicator[data-tone='waiting'] > .state-glyph { color: var(--agent-state-waiting); background: transparent; }
  .state-glyph :global(svg) { width: 10px; height: 10px; stroke-width: 2.1; }
  [data-state='needs_input'] .state-glyph :global(svg) { width: 7px; height: 7px; fill: currentColor; stroke: none; }
  @media (prefers-reduced-motion: no-preference) {
    [data-state='working'] .state-glyph :global(svg) { animation: status-indicator-spin 850ms linear infinite; }
  }
  @keyframes status-indicator-spin { to { transform: rotate(360deg); } }
</style>
