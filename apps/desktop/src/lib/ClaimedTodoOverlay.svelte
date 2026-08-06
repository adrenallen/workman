<script lang="ts">
  import TodoStatusIndicator from '$lib/components/ds/TodoStatusIndicator.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Popover from '$lib/components/ui/popover';
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { claimedAtLabel, type ClaimedTodo } from './claimedTodos';

  interface Props {
    claims: ClaimedTodo[];
    onOpen: (claim: ClaimedTodo) => void;
  }

  let { claims, onOpen }: Props = $props();
  let open = $state(false);
  let primary = $derived(claims[0]);

  function openClaim(claim: ClaimedTodo): void {
    open = false;
    onOpen(claim);
  }
</script>

{#if primary}
  <aside
    class="pointer-events-none absolute right-3 top-3 z-30 flex max-w-[min(24rem,calc(100%-1.5rem))] items-center drop-shadow-sm"
    aria-label={claims.length === 1 ? 'Claimed todo' : `${claims.length} claimed todos`}
  >
    <Tooltip.Provider delayDuration={300}>
      <Tooltip.Root>
        <Tooltip.Trigger>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="secondary"
              size="sm"
              class="pointer-events-auto min-w-0 max-w-72 rounded-r-none border border-border bg-card pl-2 shadow-sm"
              onclick={() => openClaim(primary)}
            >
              <TodoStatusIndicator state="claimed" label={`Claimed todo · ${primary.title}`} />
              <span class="truncate">{primary.title}</span>
            </Button>
          {/snippet}
        </Tooltip.Trigger>
        <Tooltip.Content side="bottom" align="end" sideOffset={6} class="max-w-80">
          <strong class="block font-medium">{primary.title}</strong>
          <span class="mt-0.5 block text-muted-foreground">{claimedAtLabel(primary.claimed_at)}</span>
        </Tooltip.Content>
      </Tooltip.Root>
    </Tooltip.Provider>

    {#if claims.length > 1}
      <Popover.Root bind:open>
        <Popover.Trigger>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="outline"
              size="sm"
              class="pointer-events-auto rounded-l-none border-l-0 bg-card px-2 font-mono shadow-sm"
              aria-label={`${claims.length - 1} more claimed todos`}
              title={`${claims.length - 1} more claimed todos`}
            >
              +{claims.length - 1}
            </Button>
          {/snippet}
        </Popover.Trigger>
        <Popover.Content side="bottom" align="end" sideOffset={6} class="w-80 gap-1 p-1.5">
          <Popover.Header class="px-2 pb-1 pt-1">
            <Popover.Title>Claimed todos</Popover.Title>
            <Popover.Description>{claims.length} active leases for this process</Popover.Description>
          </Popover.Header>
          <div class="grid max-h-72 gap-0.5 overflow-y-auto" role="list">
            {#each claims as claim (claim.id)}
              <Button
                variant="ghost"
                class="h-auto min-w-0 justify-start px-2 py-1.5 text-left"
                onclick={() => openClaim(claim)}
              >
                <TodoStatusIndicator state="claimed" label={`Claimed todo · ${claim.title}`} />
                <span class="min-w-0 flex-1">
                  <strong class="block truncate text-sm font-medium">{claim.title}</strong>
                  <small class="block truncate text-xs font-normal text-muted-foreground">
                    {claimedAtLabel(claim.claimed_at)}
                  </small>
                </span>
              </Button>
            {/each}
          </div>
        </Popover.Content>
      </Popover.Root>
    {/if}
  </aside>
{/if}
