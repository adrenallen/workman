<script lang="ts">
  import ArrowRightLeftIcon from '@lucide/svelte/icons/arrow-right-left';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import OctagonXIcon from '@lucide/svelte/icons/octagon-x';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';

  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import { Button } from '$lib/components/ui/button';
  import type { Profile, ProfileRunningProcess } from './daemon';

  interface Props {
    profile: Profile;
    processes: ProfileRunningProcess[];
    busy?: boolean;
    error?: string | null;
    onConfirm: () => void;
    onClose: () => void;
  }

  let { profile, processes, busy = false, error = null, onConfirm, onClose }: Props = $props();
</script>

<AlertDialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <AlertDialog.Content class="w-[min(500px,calc(100vw-32px))] max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 rounded-lg border border-border bg-popover p-0">
    <AlertDialog.Header class="gap-2 border-b border-border px-4 py-4 text-left">
      <span class="flex items-center gap-2 text-destructive">
        <OctagonXIcon size={16} />
        <AlertDialog.Title>Stop {processes.length} {processes.length === 1 ? 'process' : 'processes'} and switch?</AlertDialog.Title>
      </span>
      <AlertDialog.Description class="text-sm leading-relaxed">
        Workman will stop the outgoing profile gracefully before loading <strong>{profile.name}</strong>. The daemon and connected MCP endpoint stay online.
      </AlertDialog.Description>
    </AlertDialog.Header>

    <section class="grid min-h-0 content-start gap-3 overflow-y-auto overscroll-contain px-4 py-4">
      <div class="grid gap-1.5" aria-label="Processes that will stop">
        <span class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          <SquareTerminalIcon size={13} />Outgoing work
        </span>
        <ul class="grid gap-1 rounded border border-border bg-muted/25 p-1.5">
          {#each processes as process (process.id)}
            <li class="flex min-w-0 items-center gap-2 rounded px-2 py-1.5 text-sm">
              <SquareTerminalIcon class="shrink-0 text-muted-foreground" size={14} />
              <span class="min-w-0 flex-1 truncate">{process.name}</span>
              <code class="shrink-0 text-xs text-muted-foreground">#{process.id}</code>
            </li>
          {/each}
        </ul>
      </div>
      {#if error}<p class="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">{error}</p>{/if}
    </section>

    <AlertDialog.Footer class="mx-0 mb-0 flex-row flex-wrap justify-end rounded-none rounded-b-lg border-t border-border bg-card px-4 py-3">
      <Button variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
      <Button variant="destructive" disabled={busy} onclick={onConfirm}>
        {#if busy}<LoaderCircleIcon class="animate-spin" size={14} />{/if}
        <ArrowRightLeftIcon size={14} />Stop {processes.length} & switch
      </Button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

<style>
  code { font-family: 'JetBrains Mono Variable', monospace; }
  @media (prefers-reduced-motion: reduce) { :global(.animate-spin) { animation: none; } }
</style>
