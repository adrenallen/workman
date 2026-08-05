<script lang="ts">
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import GitForkIcon from '@lucide/svelte/icons/git-fork';
  import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import SearchIcon from '@lucide/svelte/icons/search';
  import CloudIcon from '@lucide/svelte/icons/cloud';
  import XIcon from '@lucide/svelte/icons/x';
  import { open } from '@tauri-apps/plugin-dialog';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Input } from '$lib/components/ui/input';
  import { ScrollArea } from '$lib/components/ui/scroll-area';
  import * as Tabs from '$lib/components/ui/tabs';
  import { fuzzySubsequenceScore } from './navigation';
  import type { Project } from './daemon';
  import type {
    EnvironmentPolicy,
    WorktreeDialogSubmission,
    WorktreeBranchOption,
    WorktreeEntry,
    WorktreeRepository
  } from './worktrees';

  interface Props {
    mode: 'create' | 'fork' | 'adopt';
    sourceProject: Project;
    repository: WorktreeRepository;
    sourceEntry?: WorktreeEntry | null;
    branchOptions?: WorktreeBranchOption[];
    originBranches?: string[];
    branchesLoading?: boolean;
    busy?: boolean;
    error?: string | null;
    onLoadBranches: () => void;
    onSubmit: (submission: WorktreeDialogSubmission) => void;
    onClose: () => void;
  }

  let {
    mode,
    sourceProject,
    repository,
    sourceEntry = null,
    branchOptions = [],
    originBranches = [],
    branchesLoading = false,
    busy = false,
    error = null,
    onLoadBranches,
    onSubmit,
    onClose
  }: Props = $props();

  let createKind = $state<'new' | 'origin'>('new');
  let branch = $state('');
  let existingBranch = $state('');
  let branchQuery = $state('');
  let branchOptionIndex = $state(0);
  let baseRef = $state('HEAD');
  let adoptPath = $state('');
  let envPolicy = $state<EnvironmentPolicy>('skip');
  let rememberEnvPolicy = $state(true);
  let branchInput = $state<HTMLInputElement | null>(null);
  let branchSearchInput = $state<HTMLInputElement | null>(null);
  let effectiveBranchOptions = $derived(branchOptions.length > 0
    ? branchOptions
    : originBranches.map((name) => ({ name, source: 'origin' as const })));
  let rankedBranches = $derived(rankBranches(effectiveBranchOptions, branchQuery));
  let activeBranch = $derived(rankedBranches[branchOptionIndex] ?? null);

  let title = $derived(mode === 'create'
    ? `New worktree in ${repository.name}`
    : mode === 'fork'
      ? `Fork ${sourceEntry?.branch ?? sourceProject.branch ?? sourceProject.name}`
      : `Adopt into ${repository.name}`);
  let actionLabel = $derived(busy
    ? mode === 'adopt' ? 'Adopting…' : mode === 'fork' ? 'Forking…' : 'Creating…'
    : mode === 'adopt' ? 'Adopt worktree' : mode === 'fork' ? 'Fork again' : 'Create worktree');
  let canSubmit = $derived(!busy && (
    mode === 'adopt'
      ? adoptPath.trim().length > 0
      : mode === 'create' && createKind === 'origin'
        ? existingBranch.length > 0
        : branch.trim().length > 0
  ));

  $effect(() => {
    const input = mode === 'create' && createKind === 'origin' ? branchSearchInput : branchInput;
    if (input) queueMicrotask(() => input?.focus());
  });

  function rankBranches(options: WorktreeBranchOption[], query: string): WorktreeBranchOption[] {
    return options
      .map((option) => ({ option, score: fuzzySubsequenceScore(query, option.name) }))
      .filter((entry): entry is { option: WorktreeBranchOption; score: number } => entry.score !== null)
      .sort((left, right) => right.score - left.score || left.option.name.localeCompare(right.option.name))
      .map((entry) => entry.option);
  }

  function changeCreateKind(value: string): void {
    if (value !== 'new' && value !== 'origin') return;
    createKind = value;
    if (value === 'origin' && effectiveBranchOptions.length === 0) onLoadBranches();
  }

  function submit(): void {
    if (!canSubmit) return;
    if (mode === 'adopt') {
      onSubmit({ mode, path: adoptPath.trim() });
      return;
    }
    const nextBranch = mode === 'create' && createKind === 'origin'
      ? existingBranch
      : branch.trim();
    if (!nextBranch) return;
    if (mode === 'fork') {
      onSubmit({ mode, branch: nextBranch, envPolicy, rememberEnvPolicy });
      return;
    }
    onSubmit({
      mode,
      branch: nextBranch,
      fromRef: createKind === 'new' && baseRef.trim() ? baseRef.trim() : undefined,
      envPolicy,
      rememberEnvPolicy
    });
  }

  async function chooseDirectory(): Promise<void> {
    const path = await open({ directory: true, multiple: false, title: 'Choose an existing Git worktree' });
    if (typeof path === 'string') adoptPath = path;
  }

  function chooseBranch(option: WorktreeBranchOption | null): void {
    if (!option) return;
    existingBranch = option.name;
    branchQuery = option.name;
    branchOptionIndex = 0;
  }

  function handleBranchKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (rankedBranches.length === 0) return;
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      branchOptionIndex = (branchOptionIndex + delta + rankedBranches.length) % rankedBranches.length;
      queueMicrotask(() => document.getElementById(`worktree-branch-option-${branchOptionIndex}`)?.scrollIntoView({ block: 'nearest' }));
      return;
    }
    if (event.key === 'Enter' && activeBranch) {
      event.preventDefault();
      chooseBranch(activeBranch);
    }
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="w-[min(560px,calc(100vw-32px))] max-w-none gap-0 overflow-hidden rounded-lg border border-border bg-popover p-0"
    showCloseButton={false}
    aria-describedby="worktree-dialog-description"
  >
    <form onsubmit={(event) => { event.preventDefault(); submit(); }}>
      <Dialog.Header class="flex-row items-start justify-between border-b border-border px-4 py-3 text-left">
        <span class="flex min-w-0 items-start gap-3">
          <span class="grid size-8 shrink-0 place-items-center rounded border border-border bg-card text-muted-foreground">
            {#if mode === 'adopt'}<FolderOpenIcon size={16} />{:else if mode === 'fork'}<GitForkIcon size={16} />{:else}<GitBranchIcon size={16} />{/if}
          </span>
          <span class="min-w-0">
            <Dialog.Title class="truncate text-base">{title}</Dialog.Title>
            <Dialog.Description id="worktree-dialog-description" class="mt-1 text-sm">
              {#if mode === 'fork'}Starts at exact HEAD <code>{sourceEntry?.head.slice(0, 10) ?? 'unknown'}</code>.{:else if mode === 'adopt'}Registers an existing worktree without moving or changing it.{:else}Creates a managed project under <code>{repository.managed_root}</code>.{/if}
            </Dialog.Description>
          </span>
        </span>
        <IconButton label="Close worktree dialog" disabled={busy} onclick={onClose}>{#snippet icon()}<XIcon size={14} />{/snippet}</IconButton>
      </Dialog.Header>

      <section class="grid gap-4 px-4 py-4">
        {#if mode === 'adopt'}
          <label class="grid gap-1.5">
            <span class="text-sm font-medium">Existing worktree folder</span>
            <span class="flex gap-2">
              <Input bind:value={adoptPath} class="min-w-0 flex-1 font-mono" placeholder="/path/to/existing-worktree" autocomplete="off" />
              <Button type="button" variant="outline" onclick={() => void chooseDirectory()}><FolderOpenIcon size={14} />Choose…</Button>
            </span>
            <small class="text-xs text-muted-foreground">The folder stays where it is and is marked adopted, not Workman-managed.</small>
          </label>
        {:else}
          {#if mode === 'create'}
            <Tabs.Root value={createKind} onValueChange={changeCreateKind}>
              <Tabs.List class="grid h-9 w-full grid-cols-2">
                <Tabs.Trigger value="new">New branch</Tabs.Trigger>
                <Tabs.Trigger value="origin">Existing branch</Tabs.Trigger>
              </Tabs.List>
            </Tabs.Root>
          {/if}

          {#if mode === 'create' && createKind === 'origin'}
            <section class="grid gap-1.5" aria-labelledby="worktree-branch-label">
              <span id="worktree-branch-label" class="text-sm font-medium">Local or origin branch</span>
              <span class="flex gap-2">
                <label class="relative min-w-0 flex-1">
                  <SearchIcon class="pointer-events-none absolute top-1/2 left-2.5 z-10 -translate-y-1/2 text-muted-foreground" size={14} aria-hidden="true" />
                  <Input
                    bind:ref={branchSearchInput}
                    bind:value={branchQuery}
                    class="pl-8 font-mono"
                    role="combobox"
                    aria-label="Filter local and origin branches"
                    aria-expanded="true"
                    aria-controls="worktree-branch-options"
                    aria-activedescendant={activeBranch ? `worktree-branch-option-${branchOptionIndex}` : undefined}
                    autocomplete="off"
                    spellcheck="false"
                    placeholder={branchesLoading ? 'Fetching branches…' : 'Type to fuzzy-filter branches'}
                    oninput={() => { existingBranch = ''; branchOptionIndex = 0; }}
                    onkeydown={handleBranchKeydown}
                  />
                </label>
                <Button type="button" variant="outline" disabled={branchesLoading} onclick={onLoadBranches}>
                  {#if branchesLoading}<LoaderCircleIcon class="spin" size={14} />{/if}Refresh
                </Button>
              </span>
              <div class="overflow-hidden rounded-lg border border-border bg-card">
                <div class="flex min-h-7 items-center justify-between border-b border-border px-2 text-xs text-muted-foreground" aria-live="polite">
                  <span>{branchQuery ? `${rankedBranches.length} matches` : 'Available branches'}</span>
                  <span>local + origin</span>
                </div>
                <ScrollArea id="worktree-branch-options" class="h-40" role="listbox" aria-label="Available branches">
                  <div class="grid gap-0.5 p-1">
                    {#each rankedBranches as option, index (option.name)}
                      <button
                        id={`worktree-branch-option-${index}`}
                        class="flex min-h-8 items-center gap-2 rounded px-2 text-left text-sm hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring data-[active=true]:bg-accent"
                        data-active={index === branchOptionIndex}
                        type="button"
                        role="option"
                        aria-selected={existingBranch === option.name}
                        onmouseenter={() => (branchOptionIndex = index)}
                        onclick={() => chooseBranch(option)}
                      >
                        {#if option.source === 'local'}<HardDriveIcon class="shrink-0 text-muted-foreground" size={14} aria-hidden="true" />{:else}<CloudIcon class="shrink-0 text-muted-foreground" size={14} aria-hidden="true" />{/if}
                        <span class="min-w-0 flex-1 truncate font-mono">{option.name}</span>
                        <Badge variant="outline" class="h-5 px-1.5 text-xs font-normal text-muted-foreground">{option.source}</Badge>
                      </button>
                    {:else}
                      <div class="grid min-h-20 place-content-center gap-1 px-3 text-center">
                        <strong class="text-sm font-medium">{branchesLoading ? 'Fetching branches…' : 'No matching branch'}</strong>
                        <span class="text-xs text-muted-foreground">{branchesLoading ? 'Local and origin refs load together.' : 'Try fewer letters; characters match in order.'}</span>
                      </div>
                    {/each}
                  </div>
                </ScrollArea>
              </div>
              <small class="text-xs text-muted-foreground">Unchecked-out branches only. Use ↑ ↓ and Enter to choose.</small>
            </section>
          {:else}
            <label class="grid gap-1.5">
              <span class="text-sm font-medium">Branch name</span>
              <Input bind:ref={branchInput} bind:value={branch} class="font-mono" placeholder={mode === 'fork' ? 'feature/follow-up' : 'feature/new-worktree'} autocomplete="off" spellcheck="false" />
            </label>
            {#if mode === 'create'}
              <label class="grid gap-1.5">
                <span class="text-sm font-medium">Base ref</span>
                <Input bind:value={baseRef} class="font-mono" placeholder="HEAD, main, origin/main, or SHA" autocomplete="off" spellcheck="false" />
                <small class="text-xs text-muted-foreground">The new branch starts here. Existing branches are never reset.</small>
              </label>
            {/if}
          {/if}

          <div class="grid gap-2 border-t border-border pt-3">
            <span class="text-sm font-medium">Environment file</span>
            <div class="grid grid-cols-2 gap-2">
              <Button type="button" variant={envPolicy === 'skip' ? 'secondary' : 'outline'} onclick={() => (envPolicy = 'skip')}>Skip .env</Button>
              <Button type="button" variant={envPolicy === 'copy' ? 'secondary' : 'outline'} onclick={() => (envPolicy = 'copy')}>Copy safe .env</Button>
            </div>
            <label class="flex items-center gap-2 text-sm text-muted-foreground">
              <Checkbox bind:checked={rememberEnvPolicy} aria-label="Remember environment choice for this repository" />
              Remember this choice for {repository.name}
            </label>
          </div>
        {/if}

        {#if error}<p class="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">{error}</p>{/if}
      </section>

      <Dialog.Footer class="flex-row justify-end border-t border-border px-4 py-3">
        <Button type="button" variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
        <Button type="submit" disabled={!canSubmit}>
          {#if busy}<LoaderCircleIcon class="spin" size={14} />{/if}{actionLabel}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<style>
  code { font-family: 'JetBrains Mono Variable', monospace; color: var(--foreground); }
  :global(.spin) { animation: worktree-dialog-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes worktree-dialog-spin { to { transform: rotate(360deg); } }
</style>
