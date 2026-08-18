<script lang="ts">
  import type { Snippet } from 'svelte';

  import * as Tooltip from '$lib/components/ui/tooltip';

  interface Props {
    label: string;
    children: Snippet;
    content?: Snippet;
    side?: 'top' | 'right' | 'bottom' | 'left';
    sideOffset?: number;
    delayDuration?: number;
    disableHoverableContent?: boolean;
    skipDelayDuration?: number;
    contentClass?: string;
  }

  let {
    label,
    children,
    content,
    side = 'top',
    sideOffset = 6,
    delayDuration = 350,
    disableHoverableContent = false,
    skipDelayDuration = 300,
    contentClass
  }: Props = $props();
</script>

<Tooltip.Provider {delayDuration} {disableHoverableContent} {skipDelayDuration}>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <span {...props} class="tooltip-anchor">{@render children()}</span>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content {side} {sideOffset} class={contentClass}>
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
