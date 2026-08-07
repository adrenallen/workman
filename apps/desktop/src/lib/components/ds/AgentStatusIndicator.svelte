<script module lang="ts">
  import type { ProcessView } from '$lib/daemon';

  export interface AgentStatusIndicatorProps {
    process: ProcessView;
    showLabel?: boolean;
    size?: 'sm' | 'lg';
    class?: string;
  }
</script>

<script lang="ts">
  import CircleIcon from '@lucide/svelte/icons/circle';
  import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
  import CircleXIcon from '@lucide/svelte/icons/circle-x';
  import Clock3Icon from '@lucide/svelte/icons/clock-3';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';

  import { agentStatusPresentation } from '$lib/agentStatus';
  import { cn } from '$lib/utils';
  import * as Tooltip from '$lib/components/ui/tooltip';

  let {
    process,
    showLabel = false,
    size = 'sm',
    class: className
  }: AgentStatusIndicatorProps = $props();

  const presentation = $derived(agentStatusPresentation(process));
</script>

<Tooltip.Provider delayDuration={250}>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <span
          {...props}
          class={cn('agent-status-indicator', className)}
          data-state={presentation.state}
          data-size={size}
          role="status"
          aria-label={presentation.label}
        >
          <span class="status-glyph" aria-hidden="true">
            {#if presentation.state === 'working'}
              <LoaderCircleIcon />
            {:else if presentation.state === 'needs_input'}
              <CircleAlertIcon />
            {:else if presentation.state === 'waiting'}
              <Clock3Icon />
            {:else if presentation.state === 'exited'}
              <CircleXIcon />
            {:else}
              <CircleIcon />
            {/if}
          </span>
          {#if showLabel}<span class="status-label">{presentation.shortLabel}</span>{/if}
        </span>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content sideOffset={6}>{presentation.label}</Tooltip.Content>
  </Tooltip.Root>
</Tooltip.Provider>

<style>
  .agent-status-indicator {
    display: inline-flex;
    min-width: 0;
    align-items: center;
    gap: 5px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    font-weight: 650;
    letter-spacing: 0.055em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .status-glyph {
    display: inline-grid;
    width: 16px;
    height: 16px;
    flex: none;
    place-items: center;
  }

  .status-glyph :global(svg) {
    width: 12px;
    height: 12px;
    stroke-width: 2;
  }

  [data-size='lg'] .status-glyph {
    width: 25px;
    height: 25px;
    border: 1px solid color-mix(in srgb, currentColor 36%, var(--border));
    border-radius: 999px;
  }

  [data-size='lg'] .status-glyph :global(svg) {
    width: 14px;
    height: 14px;
  }

  [data-state='working'] { color: var(--success); }
  [data-state='needs_input'] { color: var(--warning-token); }
  [data-state='waiting'] { color: var(--information); }
  [data-state='waiting'] .status-glyph {
    width: 14px;
    height: 14px;
    border: 0;
    border-radius: 999px;
    background: var(--information);
    color: var(--information-foreground);
  }
  [data-state='waiting'] .status-glyph :global(svg) {
    width: 9px;
    height: 9px;
    stroke-width: 2.2;
  }
  [data-state='waiting'][data-size='lg'] .status-glyph {
    width: 25px;
    height: 25px;
  }
  [data-state='waiting'][data-size='lg'] .status-glyph :global(svg) {
    width: 13px;
    height: 13px;
  }
  [data-state='exited'] { color: var(--destructive); }

  @media (prefers-reduced-motion: no-preference) {
    [data-state='working'] .status-glyph :global(svg) {
      animation: agent-status-spin 850ms linear infinite;
    }
  }

  @keyframes agent-status-spin {
    to { transform: rotate(360deg); }
  }
</style>
