<script lang="ts">
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import { Button } from '$lib/components/ui/button';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import type { Project } from './daemon';
  import type { WorktreeEntry, WorktreeRepository } from './worktrees';
  import { projectDisplayName } from './worktrees';

  interface Props {
    project: Project;
    repository?: WorktreeRepository | null;
    entry?: WorktreeEntry | null;
    busy?: boolean;
    error?: string | null;
    serverForceRequired?: boolean;
    onConfirm: (deleteFromDisk: boolean, forceDirty: boolean) => void;
    onClose: () => void;
  }

  let {
    project,
    repository = null,
    entry = null,
    busy = false,
    error = null,
    serverForceRequired = false,
    onConfirm,
    onClose
  }: Props = $props();
  let deleteFromDisk = $state(false);
  let path = $derived(entry?.path ?? project.path);
  let confirmationText = $derived(entry?.branch ?? projectDisplayName(project));
  let safety = $derived(entry?.delete_safety ?? null);
  let forceRequired = $derived(
    deleteFromDisk && (serverForceRequired || safety?.requires_force === true)
  );
  let canRemove = $derived(!busy);
</script>

<AlertDialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <AlertDialog.Content class="max-h-[calc(100dvh-24px)] w-[min(760px,calc(100vw-24px))] !max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 rounded-lg border border-border bg-popover p-0">
    <AlertDialog.Header class="gap-2 border-b border-border px-4 py-4 text-left">
      <span class="flex items-center gap-2 text-destructive"><Trash2Icon size={16} /><AlertDialog.Title>Remove {projectDisplayName(project)}?</AlertDialog.Title></span>
      <AlertDialog.Description class="text-sm leading-relaxed">
        Workman will unregister this project. Its folder stays on your computer unless you explicitly choose local deletion below.
      </AlertDialog.Description>
    </AlertDialog.Header>

    <section class="grid min-h-0 content-start gap-3 overflow-y-auto overscroll-contain px-4 py-4">
      <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded border border-border bg-card px-3 py-2">
        {#if entry}<GitBranchIcon class="mt-0.5 text-muted-foreground" size={15} />{:else}<FolderIcon class="mt-0.5 text-muted-foreground" size={15} />{/if}
        <strong class="truncate font-mono text-sm">{confirmationText}</strong>
        <span></span><small class="break-all font-mono text-xs text-muted-foreground">{path}</small>
      </div>

      {#if error}<p class="rounded border border-destructive/60 bg-destructive/15 px-3 py-3 text-sm font-medium text-destructive" role="alert" aria-live="assertive">Project removal failed: {error}</p>{/if}

      <p class="text-sm text-muted-foreground">Any running processes in this project will stop before removal.</p>

      <label class="flex items-start gap-2 rounded border border-destructive/40 bg-destructive/5 px-3 py-3 text-sm">
        <Checkbox bind:checked={deleteFromDisk} aria-label="Also delete this project from my computer" />
        <span class="grid gap-1">
          <strong>Also delete from my computer</strong>
          <span class="text-xs leading-relaxed text-muted-foreground">
            {#if entry && entry.kind !== 'main'}Runs local Git worktree removal and pruning. The branch is kept in {repository?.name ?? 'the repository'}.{:else if entry}Deletes this primary checkout. Remote Git branches are never changed.{:else}Permanently deletes this local folder.{/if}
          </span>
        </span>
      </label>

      {#if deleteFromDisk}
        <div class="grid gap-1 rounded border border-destructive/50 bg-destructive/10 px-3 py-3 text-sm">
          <strong class="text-destructive">This exact folder will be permanently deleted:</strong>
          <code class="break-all text-xs">{path}</code>
        </div>
      {/if}

      {#if forceRequired}
        <div class="grid gap-3 rounded border border-warning/40 bg-warning/10 px-3 py-3">
          <div class="grid gap-1">
            <strong class="text-sm text-warning">Pending local work will be permanently deleted</strong>
            <span class="text-xs leading-relaxed text-muted-foreground">Review the exact changes below, then choose <strong class="text-foreground">Delete anyway</strong>.</span>
          </div>
          {#if safety}
            <ul class="change-groups">
              {#if safety.dirty_files > 0}
                <li>
                  <strong>{safety.dirty_files} changed file{safety.dirty_files === 1 ? '' : 's'} · {safety.untracked_files} untracked</strong>
                  <ul class="path-list">
                    {#each safety.dirty_paths.slice(0, 6) as dirtyPath}<li><code>{dirtyPath}</code></li>{/each}
                    {#if safety.dirty_paths.length > 6}<li class="more">+{safety.dirty_paths.length - 6} more</li>{/if}
                  </ul>
                </li>
              {/if}
              {#if safety.ignored_files > 0}
                <li>
                  <strong>{safety.ignored_files} ignored local path{safety.ignored_files === 1 ? '' : 's'}</strong>
                  <span class="summary">Build output, dependencies, and other ignored content are included in deletion.</span>
                  <ul class="path-list">
                    {#each safety.ignored_paths.slice(0, 4) as ignoredPath}<li><code>{ignoredPath}</code></li>{/each}
                    {#if safety.ignored_paths.length > 4}<li class="more">+{safety.ignored_paths.length - 4} more</li>{/if}
                  </ul>
                </li>
              {/if}
              {#if safety.unpushed_commits > 0}
                <li>
                  <strong>{safety.unpushed_commits} unpushed commit{safety.unpushed_commits === 1 ? '' : 's'}</strong>
                  <span class="summary">{safety.push_target ? `Not pushed to ${safety.push_target}.` : `No upstream; not present in ${safety.merge_target}.`}</span>
                  <ul class="subject-list">
                    {#each safety.unpushed_subjects as subject}<li>{subject}</li>{/each}
                    {#if safety.unpushed_commits > safety.unpushed_subjects.length}<li class="more">+{safety.unpushed_commits - safety.unpushed_subjects.length} more</li>{/if}
                  </ul>
                </li>
              {/if}
              {#if safety.unmerged_commits > 0}
                <li>
                  <strong>{safety.unmerged_commits} commit{safety.unmerged_commits === 1 ? '' : 's'} not merged into <code>{safety.merge_target}</code></strong>
                  <ul class="subject-list">
                    {#each safety.unmerged_subjects as subject}<li>{subject}</li>{/each}
                    {#if safety.unmerged_commits > safety.unmerged_subjects.length}<li class="more">+{safety.unmerged_commits - safety.unmerged_subjects.length} more</li>{/if}
                  </ul>
                </li>
              {/if}
              {#if safety.dependent_worktrees.length > 0}
                <li>
                  <strong>{safety.dependent_worktrees.length} linked worktree{safety.dependent_worktrees.length === 1 ? '' : 's'} depend on this primary checkout</strong>
                  <ul class="path-list">
                    {#each safety.dependent_worktrees.slice(0, 4) as dependentPath}<li><code>{dependentPath}</code></li>{/each}
                    {#if safety.dependent_worktrees.length > 4}<li class="more">+{safety.dependent_worktrees.length - 4} more</li>{/if}
                  </ul>
                </li>
              {/if}
            </ul>
          {:else}
            <p class="text-xs leading-relaxed text-muted-foreground">Workman found pending changes after this dialog opened. Confirm once more to delete the verified folder and its local-only contents.</p>
          {/if}
        </div>
      {/if}

    </section>

    <AlertDialog.Footer class="border-t border-border px-4 py-3">
      <Button variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
      <Button variant={deleteFromDisk ? 'destructive' : 'default'} disabled={!canRemove} onclick={() => onConfirm(deleteFromDisk, forceRequired)}>
        {#if busy}<LoaderCircleIcon class="spin" size={14} />{/if}{forceRequired ? 'Delete anyway' : deleteFromDisk ? 'Delete project' : 'Remove from Workman'}
      </Button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

<style>
  code { color: var(--foreground); font-family: 'JetBrains Mono Variable', monospace; }
  .change-groups { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .change-groups > li { display: grid; min-width: 0; gap: 4px; border-left: 2px solid color-mix(in srgb, var(--warning) 55%, var(--border)); padding: 2px 0 2px 9px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .change-groups strong { color: var(--foreground); font-size: var(--font-size-xs); }
  .summary { line-height: 1.4; }
  .path-list, .subject-list { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; }
  .path-list li, .subject-list li { min-width: 0; overflow-wrap: anywhere; line-height: 1.35; }
  .path-list li::before, .subject-list li::before { content: '—'; margin-right: 6px; color: var(--muted-foreground); }
  .more { color: var(--muted-foreground); font-family: 'JetBrains Mono Variable', monospace; }
  :global(.spin) { animation: worktree-remove-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes worktree-remove-spin { to { transform: rotate(360deg); } }
</style>
