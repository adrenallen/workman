<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import NetworkIcon from '@lucide/svelte/icons/network';
  import OctagonXIcon from '@lucide/svelte/icons/octagon-x';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';

  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import { Button } from '$lib/components/ui/button';
  import type { AgentCascadeAction } from './agentCascade';
  import type { ProcessView } from './daemon';

  interface Props {
    processes: ProcessView[];
    descendants: ProcessView[];
    action: AgentCascadeAction;
    busy?: boolean;
    error?: string | null;
    onConfirm: () => void;
    onClose: () => void;
  }

  let {
    processes,
    descendants,
    action,
    busy = false,
    error = null,
    onConfirm,
    onClose
  }: Props = $props();

  let actionLabel = $derived(action === 'kill' ? 'Kill' : action === 'close' ? 'Close' : 'Stop');
  let kind = $derived(processes.every((process) => process.kind === 'terminal') ? 'terminal' : 'agent');
  let subject = $derived(
    processes.length === 1
      ? processes[0]?.name ?? kind
      : `${processes.length} ${kind}s`
  );
  let affectedCount = $derived(processes.length + descendants.length);
  let description = $derived(
    action === 'kill'
      ? `${processes.length === 1 ? 'The selected process' : 'The selected processes'} will be killed immediately. Unsaved terminal state may be lost.`
      : action === 'close'
        ? `${processes.length === 1 ? 'The selected entry' : 'The selected entries'} will be removed.${descendants.length > 0 ? ' Descendant entries will also be removed, stopping any that are still running.' : ''}`
        : `${processes.length === 1 ? 'The selected process' : 'The selected processes'} will stop gracefully.${descendants.length > 0 ? ' Their child processes will stop first.' : ''}`
  );
</script>

<AlertDialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <AlertDialog.Content class="w-[min(500px,calc(100vw-32px))] max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 rounded-lg border border-border bg-popover p-0">
    <AlertDialog.Header class="gap-2 border-b border-border px-4 py-4 text-left">
      <span class="flex items-center gap-2 text-destructive">
        <OctagonXIcon size={16} />
        <AlertDialog.Title>{actionLabel} {subject}?</AlertDialog.Title>
      </span>
      <AlertDialog.Description class="text-sm leading-relaxed">{description}</AlertDialog.Description>
    </AlertDialog.Header>

    <section class="grid min-h-0 content-start gap-3 overflow-y-auto overscroll-contain px-4 py-4">
      {#if descendants.length > 0}
        <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded border border-border bg-card px-3 py-3">
          <NetworkIcon class="mt-0.5 text-muted-foreground" size={15} />
          <strong class="text-sm">Includes {descendants.length} additional descendant {descendants.length === 1 ? 'agent' : 'agents'}</strong>
          <span></span>
          <small class="text-xs leading-relaxed text-muted-foreground">
            {action === 'close'
              ? 'Descendant entries are removed before their selected parent so no child agent is left running.'
              : 'Descendants always stop before their selected parent so no child agent is left running.'}
          </small>
        </div>

        <div class="grid gap-1.5" aria-label="Additional descendant agents">
          <span class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground"><NetworkIcon size={13} />Additional descendants</span>
          <ul class="grid gap-1 rounded border border-border bg-muted/25 p-1.5">
            {#each descendants as descendant (descendant.id)}
              <li class="flex min-w-0 items-center gap-2 rounded px-2 py-1.5 text-sm">
                <BotIcon class="shrink-0 text-muted-foreground" size={14} />
                <span class="min-w-0 flex-1 truncate">{descendant.name}</span>
                <code class="shrink-0 text-xs text-muted-foreground">#{descendant.id}</code>
              </li>
            {/each}
          </ul>
        </div>
      {:else if processes.length > 1}
        <div class="grid gap-1.5" aria-label={`Selected ${kind}s`}>
          <span class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            {#if kind === 'agent'}<BotIcon size={13} />{:else}<SquareTerminalIcon size={13} />{/if}
            Selected {kind}s
          </span>
          <ul class="grid gap-1 rounded border border-border bg-muted/25 p-1.5">
            {#each processes as process (process.id)}
              <li class="flex min-w-0 items-center gap-2 rounded px-2 py-1.5 text-sm">
                {#if kind === 'agent'}<BotIcon class="shrink-0 text-muted-foreground" size={14} />{:else}<SquareTerminalIcon class="shrink-0 text-muted-foreground" size={14} />{/if}
                <span class="min-w-0 flex-1 truncate">{process.name}</span>
                <code class="shrink-0 text-xs text-muted-foreground">#{process.id}</code>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      {#if error}<p class="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">{error}</p>{/if}
    </section>

    <AlertDialog.Footer class="border-t border-border px-4 py-3">
      <Button variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
      <Button variant="destructive" disabled={busy} onclick={onConfirm}>
        {#if busy}<LoaderCircleIcon class="spin" size={14} />{/if}{actionLabel} {affectedCount} {kind}{affectedCount === 1 ? '' : 's'}
      </Button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

<style>
  code { font-family: 'JetBrains Mono Variable', monospace; }
  :global(.spin) { animation: agent-cascade-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes agent-cascade-spin { to { transform: rotate(360deg); } }
</style>
