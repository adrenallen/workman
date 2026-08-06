<script lang="ts">
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import { Button } from '$lib/components/ui/button';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import { Input } from '$lib/components/ui/input';
  import type { WorktreeEntry, WorktreeRepository } from './worktrees';

  interface Props {
    repository: WorktreeRepository;
    entry: WorktreeEntry;
    busy?: boolean;
    error?: string | null;
    onConfirm: (forceDirty: boolean, confirmBranch?: string) => void;
    onClose: () => void;
  }

  let { repository, entry, busy = false, error = null, onConfirm, onClose }: Props = $props();
  let forceDirty = $state(false);
  let confirmBranch = $state('');
  let dirty = $derived(entry.status === 'dirty');
  let canRemove = $derived(!busy && (!dirty || (forceDirty && confirmBranch === entry.branch)));
</script>

<AlertDialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <AlertDialog.Content class="w-[min(500px,calc(100vw-32px))] max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 rounded-lg border border-border bg-popover p-0">
    <AlertDialog.Header class="gap-2 border-b border-border px-4 py-4 text-left">
      <span class="flex items-center gap-2 text-destructive"><Trash2Icon size={16} /><AlertDialog.Title>Remove {entry.branch}?</AlertDialog.Title></span>
      <AlertDialog.Description class="text-sm leading-relaxed">
        Workman will delete the managed worktree folder and unregister its project. The Git branch <strong>{entry.branch}</strong> is kept, so its commits remain available in {repository.name}.
      </AlertDialog.Description>
    </AlertDialog.Header>

    <section class="grid min-h-0 content-start gap-3 overflow-y-auto overscroll-contain px-4 py-4">
      <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded border border-border bg-card px-3 py-2">
        <GitBranchIcon class="mt-0.5 text-muted-foreground" size={15} />
        <strong class="truncate font-mono text-sm">{entry.branch}</strong>
        <span></span><small class="truncate font-mono text-xs text-muted-foreground">{entry.path}</small>
      </div>

      <p class="text-sm text-muted-foreground">Any running processes in this worktree will stop before removal.</p>

      {#if dirty}
        <div class="grid gap-2 rounded border border-warning/40 bg-warning/10 px-3 py-3">
          <strong class="text-sm text-warning">This worktree has local changes.</strong>
          <label class="flex items-center gap-2 text-sm">
            <Checkbox bind:checked={forceDirty} aria-label="Allow deletion of local changes" />
            Delete the local changes in this worktree
          </label>
          <label class="grid gap-1">
            <span class="text-xs text-muted-foreground">Type <code>{entry.branch}</code> to confirm</span>
            <Input bind:value={confirmBranch} class="font-mono" disabled={!forceDirty} autocomplete="off" spellcheck="false" />
          </label>
        </div>
      {/if}

      {#if error}<p class="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">{error}</p>{/if}
    </section>

    <AlertDialog.Footer class="border-t border-border px-4 py-3">
      <Button variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
      <Button variant="destructive" disabled={!canRemove} onclick={() => onConfirm(forceDirty, dirty ? confirmBranch : undefined)}>
        {#if busy}<LoaderCircleIcon class="spin" size={14} />{/if}Remove worktree
      </Button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

<style>
  code { color: var(--foreground); font-family: 'JetBrains Mono Variable', monospace; }
  :global(.spin) { animation: worktree-remove-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes worktree-remove-spin { to { transform: rotate(360deg); } }
</style>
