<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  import { buttonVariants, type ButtonSize, type ButtonVariant } from '$lib/components/ui/button';
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { cn } from '$lib/utils';

  interface Props extends Omit<HTMLButtonAttributes, 'children'> {
    label: string;
    icon: Snippet;
    variant?: ButtonVariant;
    size?: ButtonSize;
    shortcut?: string;
    tooltip?: boolean;
    class?: string;
  }

  let {
    label,
    icon,
    variant = 'ghost',
    size = 'icon-sm',
    shortcut,
    tooltip = true,
    class: className,
    ...buttonProps
  }: Props = $props();
</script>

{#snippet button(props = {})}
  <button
    {...props}
    {...buttonProps}
    type="button"
    aria-label={label}
    class={cn(buttonVariants({ variant, size }), 'text-muted-foreground hover:text-foreground', className)}
  >
    {@render icon()}
  </button>
{/snippet}

{#if tooltip}
  <Tooltip.Provider delayDuration={350}>
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}{@render button(props)}{/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content sideOffset={6}>
        <span>{label}</span>
        {#if shortcut}<kbd>{shortcut}</kbd>{/if}
      </Tooltip.Content>
    </Tooltip.Root>
  </Tooltip.Provider>
{:else}
  {@render button()}
{/if}
