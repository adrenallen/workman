<script lang="ts">
  import type { Snippet } from 'svelte';

  import * as Tooltip from '$lib/components/ui/tooltip';

  interface Props {
    label: string;
    children: Snippet;
    side?: 'top' | 'right' | 'bottom' | 'left';
  }

  let { label, children, side = 'top' }: Props = $props();
</script>

<Tooltip.Provider delayDuration={350}>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <span {...props} class="tooltip-anchor">{@render children()}</span>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content {side} sideOffset={6}>{label}</Tooltip.Content>
  </Tooltip.Root>
</Tooltip.Provider>

<style>
  .tooltip-anchor { display: inline-flex; min-width: 0; align-items: center; }
</style>
