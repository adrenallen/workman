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
    serverForceRequired?: boolean;
    onConfirm: (deleteFromDisk: boolean, forceDirty: boolean, confirmBranch?: string) => void;
    onClose: () => void;
  }

  let {
    repository,
    entry,
    busy = false,
    error = null,
    serverForceRequired = false,
    onConfirm,
    onClose
  }: Props = $props();
  let deleteFromDisk = $state(false);
  let forceDirty = $state(false);
  let confirmBranch = $state('');
  let safety = $derived(entry.delete_safety);
  let forceRequired = $derived(
    deleteFromDisk && (serverForceRequired || safety?.requires_force === true)
  );
  let canRemove = $derived(
    !busy && (!forceRequired || (forceDirty && confirmBranch === entry.branch))
  );
</script>

<AlertDialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <AlertDialog.Content class="w-[min(500px,calc(100vw-32px))] max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 rounded-lg border border-border bg-popover p-0">
    <AlertDialog.Header class="gap-2 border-b border-border px-4 py-4 text-left">
      <span class="flex items-center gap-2 text-destructive"><Trash2Icon size={16} /><AlertDialog.Title>Remove {entry.branch}?</AlertDialog.Title></span>
      <AlertDialog.Description class="text-sm leading-relaxed">
        Workman will unregister this project. Its folder and Git branch remain on your computer unless you explicitly choose local deletion below.
      </AlertDialog.Description>
    </AlertDialog.Header>

    <section class="grid min-h-0 content-start gap-3 overflow-y-auto overscroll-contain px-4 py-4">
      <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded border border-border bg-card px-3 py-2">
        <GitBranchIcon class="mt-0.5 text-muted-foreground" size={15} />
        <strong class="truncate font-mono text-sm">{entry.branch}</strong>
        <span></span><small class="break-all font-mono text-xs text-muted-foreground">{entry.path}</small>
      </div>

      <p class="text-sm text-muted-foreground">Any running processes in this worktree will stop before removal.</p>

      <label class="flex items-start gap-2 rounded border border-destructive/40 bg-destructive/5 px-3 py-3 text-sm">
        <Checkbox bind:checked={deleteFromDisk} aria-label="Also delete this worktree from my computer" />
        <span class="grid gap-1">
          <strong>Also delete from my computer</strong>
          <span class="text-xs leading-relaxed text-muted-foreground">Runs Git worktree removal and pruning. The branch is kept in {repository.name}.</span>
        </span>
      </label>

      {#if deleteFromDisk}
        <div class="grid gap-1 rounded border border-destructive/50 bg-destructive/10 px-3 py-3 text-sm">
          <strong class="text-destructive">This exact folder will be permanently deleted:</strong>
          <code class="break-all text-xs">{entry.path}</code>
        </div>
      {/if}

      {#if forceRequired}
        <div class="grid gap-2 rounded border border-warning/40 bg-warning/10 px-3 py-3">
          <strong class="text-sm text-warning">This worktree is not safe to delete without force.</strong>
          {#if safety}
            <ul class="list-disc space-y-1 pl-5 text-xs text-muted-foreground">
              {#if safety.dirty_files > 0}<li>{safety.dirty_files} dirty file(s), including {safety.untracked_files} untracked: <code class="break-all">{safety.dirty_paths.slice(0, 5).join(', ')}</code>{safety.dirty_paths.length > 5 ? `, and ${safety.dirty_paths.length - 5} more` : ''}</li>{/if}
              {#if safety.ignored_files > 0}<li>{safety.ignored_files} ignored local path(s) Git would delete: <code class="break-all">{safety.ignored_paths.slice(0, 5).join(', ')}</code>{safety.ignored_paths.length > 5 ? `, and ${safety.ignored_paths.length - 5} more` : ''}</li>{/if}
              {#if safety.unpushed_commits > 0}<li>{safety.unpushed_commits} commit(s) {safety.push_target ? `not pushed to ${safety.push_target}` : `have no branch upstream and are not present in ${safety.merge_target}`}</li>{/if}
              {#if safety.unmerged_commits > 0}<li>{safety.unmerged_commits} commit(s) not merged into {safety.merge_target}</li>{/if}
            </ul>
          {/if}
          <label class="flex items-center gap-2 text-sm">
            <Checkbox bind:checked={forceDirty} aria-label="Allow forced deletion of local files and commits" />
            Permanently delete these local files and commits
          </label>
          <label class="grid gap-1">
            <span class="text-xs text-muted-foreground">Type <code>{entry.branch}</code> to confirm</span>
            <Input bind:value={confirmBranch} class="font-mono" disabled={!forceDirty} autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck={false} />
          </label>
        </div>
      {/if}

      {#if error}<p class="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">{error}</p>{/if}
    </section>

    <AlertDialog.Footer class="border-t border-border px-4 py-3">
      <Button variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
      <Button variant={deleteFromDisk ? 'destructive' : 'default'} disabled={!canRemove} onclick={() => onConfirm(deleteFromDisk, forceDirty, forceRequired ? confirmBranch : undefined)}>
        {#if busy}<LoaderCircleIcon class="spin" size={14} />{/if}{deleteFromDisk ? 'Delete worktree' : 'Remove from Workman'}
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
