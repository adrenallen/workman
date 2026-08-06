<script lang="ts">
  import FolderGit2Icon from '@lucide/svelte/icons/folder-git-2';
  import ImportIcon from '@lucide/svelte/icons/import';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import XIcon from '@lucide/svelte/icons/x';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { ScrollArea } from '$lib/components/ui/scroll-area';
  import type { WorktreeEntry, WorktreeRepository } from './worktrees';

  interface Props {
    repository: WorktreeRepository;
    entries: WorktreeEntry[];
    busyPath?: string | null;
    error?: string | null;
    onAdopt: (path: string) => void;
    onAdoptAll: () => void;
    onClose: () => void;
  }

  let { repository, entries, busyPath = null, error = null, onAdopt, onAdoptAll, onClose }: Props = $props();
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busyPath) onClose(); }}>
  <Dialog.Content class="grid w-[min(620px,calc(100vw-32px))] max-w-none grid-rows-[auto_minmax(0,1fr)_auto_auto] gap-0 rounded-lg border border-border bg-popover p-0" showCloseButton={false}>
    <Dialog.Header class="flex-row items-start justify-between border-b border-border px-4 py-3 text-left">
      <span class="flex items-start gap-3">
        <span class="grid size-8 place-items-center rounded border border-border bg-card text-muted-foreground"><FolderGit2Icon size={16} /></span>
        <span><Dialog.Title>Import existing worktrees</Dialog.Title><Dialog.Description class="mt-1">{repository.name} has linked worktrees that Workman does not know yet. Importing registers them in place.</Dialog.Description></span>
      </span>
      <IconButton label="Close import worktrees" disabled={busyPath !== null} onclick={onClose}>{#snippet icon()}<XIcon size={14} />{/snippet}</IconButton>
    </Dialog.Header>

    <ScrollArea class="min-h-0 max-h-[360px]">
      <div class="grid gap-1 p-3">
        {#each entries as entry (entry.path)}
          <article class="grid min-h-14 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 rounded border border-border bg-card px-3 py-2">
            <ImportIcon class="text-muted-foreground" size={15} aria-hidden="true" />
            <span class="min-w-0"><strong class="block truncate font-mono text-sm">{entry.branch}</strong><small class="mt-0.5 block truncate font-mono text-xs text-muted-foreground">{entry.path} · {entry.status}</small></span>
            <Button size="sm" variant="outline" disabled={busyPath !== null} onclick={() => onAdopt(entry.path)}>
              {#if busyPath === entry.path}<LoaderCircleIcon class="spin" size={14} />{/if}Import
            </Button>
          </article>
        {/each}
      </div>
    </ScrollArea>

    {#if error}<p class="mx-3 mb-3 rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">{error}</p>{/if}

    <Dialog.Footer class="flex-row justify-end border-t border-border bg-card px-4 py-2.5">
      <Button variant="ghost" disabled={busyPath !== null} onclick={onClose}>Later</Button>
      <Button disabled={busyPath !== null || entries.length === 0} onclick={onAdoptAll}>
        {#if busyPath === '*'}<LoaderCircleIcon class="spin" size={14} />{/if}Import all
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<style>
  :global(.spin) { animation: worktree-import-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes worktree-import-spin { to { transform: rotate(360deg); } }
</style>
