<script lang="ts">
  import CircleIcon from '@lucide/svelte/icons/circle';
  import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
  import CircleDotIcon from '@lucide/svelte/icons/circle-dot';

  import * as Tooltip from '$lib/components/ui/tooltip';
  import { cn } from '$lib/utils';
  import type { TodoClaimState } from '$lib/todoPresentation';

  interface Props {
    state: TodoClaimState;
    label: string;
    class?: string;
  }

  let { state, label, class: className }: Props = $props();
</script>

<Tooltip.Provider delayDuration={250}>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <span
          {...props}
          class={cn('todo-status-indicator', className)}
          data-state={state}
          aria-label={label}
          role="status"
        >
          {#if state === 'open'}
            <CircleIcon size={13} strokeWidth={1.8} aria-hidden="true" />
          {:else if state === 'claimed'}
            <CircleDotIcon size={13} strokeWidth={2} aria-hidden="true" />
          {:else if state === 'blocked'}
            <CircleAlertIcon size={13} strokeWidth={2} aria-hidden="true" />
          {:else}
            <CircleCheckIcon size={13} strokeWidth={1.8} aria-hidden="true" />
          {/if}
        </span>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content sideOffset={6}>{label}</Tooltip.Content>
  </Tooltip.Root>
</Tooltip.Provider>

<style>
  .todo-status-indicator {
    display: inline-grid;
    width: 15px;
    height: 15px;
    flex: none;
    place-items: center;
    color: var(--todo-state-open);
  }

  .todo-status-indicator[data-state='claimed'] { color: var(--todo-state-claimed); }
  .todo-status-indicator[data-state='blocked'] { color: var(--todo-state-blocked); }
  .todo-status-indicator[data-state='completed'] { color: var(--todo-state-completed); opacity: 0.72; }
</style>
