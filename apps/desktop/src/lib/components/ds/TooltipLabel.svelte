<script lang="ts">
  import type { Snippet } from 'svelte';

  import * as Tooltip from '$lib/components/ui/tooltip';

  interface Props {
    label: string;
    children: Snippet;
    content?: Snippet;
    side?: 'top' | 'right' | 'bottom' | 'left';
  }

  let { label, children, content, side = 'top' }: Props = $props();
</script>

<Tooltip.Provider delayDuration={350}>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <span {...props} class="tooltip-anchor">{@render children()}</span>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content {side} sideOffset={6}>
      {#if content}
        <span class="tooltip-detail">{@render content()}</span>
      {:else}
        {label}
      {/if}
    </Tooltip.Content>
  </Tooltip.Root>
</Tooltip.Provider>

<style>
  .tooltip-anchor { display: inline-flex; min-width: 0; align-items: center; }
  .tooltip-detail { display: grid; max-width: 320px; gap: 2px; text-align: left; }
</style>
