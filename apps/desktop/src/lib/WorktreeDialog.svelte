<script lang="ts">
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import GitForkIcon from '@lucide/svelte/icons/git-fork';
  import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import SearchIcon from '@lucide/svelte/icons/search';
  import CloudIcon from '@lucide/svelte/icons/cloud';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
  import SlidersHorizontalIcon from '@lucide/svelte/icons/sliders-horizontal';
  import XIcon from '@lucide/svelte/icons/x';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onDestroy, onMount, untrack } from 'svelte';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import * as Collapsible from '$lib/components/ui/collapsible';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Input } from '$lib/components/ui/input';
  import { ScrollArea } from '$lib/components/ui/scroll-area';
  import * as Tabs from '$lib/components/ui/tabs';
  import { fuzzySubsequenceScore } from './navigation';
  import {
    defaultProjectTitleFromPath,
    defaultWorktreeTitle,
    resolvedProjectTitle,
    syncProjectTitleDefault
  } from './projectTitles';
  import type { Project } from './daemon';
  import type {
    EnvironmentPolicy,
    WorktreeDialogSubmission,
    WorktreeBranchOption,
    WorktreeCreateConflict,
    WorktreeCreateConflictAction,
    WorktreeCreateResolution,
    WorktreeEntry,
    WorktreeRefOption,
    WorktreeRefValidation,
    WorktreeRepository
  } from './worktrees';
  import { environmentPolicyFromPreferences } from './worktrees';

  interface Props {
    mode: 'create' | 'fork' | 'adopt';
    sourceProject: Project;
    repository: WorktreeRepository;
    sourceEntry?: WorktreeEntry | null;
    branchOptions?: WorktreeBranchOption[];
    originBranches?: string[];
    refOptions?: WorktreeRefOption[];
    defaultRef?: string | null;
    branchesLoading?: boolean;
    busy?: boolean;
    error?: string | null;
    conflict?: WorktreeCreateConflict | null;
    onLoadBranches: () => void;
    onValidateRef: (ref: string) => Promise<WorktreeRefValidation>;
    onSubmit: (submission: WorktreeDialogSubmission) => void;
    onOpenProject: (projectId: number) => void;
    onClearConflict: () => void;
    onClose: () => void;
  }

  let {
    mode,
    sourceProject,
    repository,
    sourceEntry = null,
    branchOptions = [],
    originBranches = [],
    refOptions = [],
    defaultRef = null,
    branchesLoading = false,
    busy = false,
    error = null,
    conflict = null,
    onLoadBranches,
    onValidateRef,
    onSubmit,
    onOpenProject,
    onClearConflict,
    onClose
  }: Props = $props();

  let createKind = $state<'new' | 'origin'>('new');
  let branch = $state('');
  let existingBranch = $state('');
  let branchQuery = $state('');
  let branchOptionIndex = $state(0);
  let baseRef = $state('HEAD');
  let baseRefTouched = $state(false);
  let baseRefOpen = $state(false);
  let baseRefOptionIndex = $state(0);
  let refValidation = $state<'idle' | 'checking' | 'valid' | 'invalid'>('idle');
  let refValidationError = $state<string | null>(null);
  let resolvedRef = $state<string | null>(null);
  let resolvedCommit = $state<string | null>(null);
  let adoptPath = $state('');
  let projectTitle = $state('');
  let projectTitleTouched = $state(false);
  let envPolicy = $state<EnvironmentPolicy>(untrack(
    () => environmentPolicyFromPreferences(repository.preferences)
  ));
  let rememberEnvPolicy = $state(true);
  let advancedOpen = $state(false);
  let branchInput = $state<HTMLInputElement | null>(null);
  let branchSearchInput = $state<HTMLInputElement | null>(null);
  let adoptPathInput = $state<HTMLInputElement | null>(null);
  let baseRefInput = $state<HTMLInputElement | null>(null);
  let projectTitleInput = $state<HTMLInputElement | null>(null);
  let baseRefPicker = $state<HTMLDivElement | null>(null);
  let conflictPanel = $state<HTMLElement | null>(null);
  let refValidationTimer: ReturnType<typeof setTimeout> | null = null;
  let refValidationSequence = 0;
  let appliedDefaultRef = false;
  let effectiveBranchOptions = $derived(branchOptions.length > 0
    ? branchOptions
    : originBranches.map((name) => ({ name, source: 'origin' as const })));
  let rankedBranches = $derived(rankBranches(effectiveBranchOptions, branchQuery));
  let activeBranch = $derived(rankedBranches[branchOptionIndex] ?? null);
  let rankedRefOptions = $derived(rankRefOptions(refOptions, baseRef));
  let activeRefOption = $derived(rankedRefOptions[baseRefOptionIndex] ?? null);
  let titleBranch = $derived(mode === 'create' && createKind === 'origin' ? existingBranch : branch);
  let defaultProjectTitle = $derived(mode === 'adopt'
    ? defaultProjectTitleFromPath(adoptPath, '')
    : defaultWorktreeTitle(titleBranch));
  let previewBranch = $derived(branch.trim() || 'branch-name');
  let previewRef = $derived(resolvedRef ?? (baseRef.trim() || 'starting-ref'));
  let previewCommit = $derived(resolvedCommit?.slice(0, 10) ?? null);

  let title = $derived(mode === 'create'
    ? 'New worktree'
    : mode === 'fork'
      ? `Fork ${sourceEntry?.branch ?? sourceProject.branch ?? sourceProject.name}`
      : `Adopt into ${repository.name}`);
  let actionLabel = $derived(busy
    ? mode === 'adopt' ? 'Adopting…' : mode === 'fork' ? 'Forking…' : 'Creating…'
    : mode === 'adopt' ? 'Adopt worktree' : mode === 'fork' ? 'Fork again' : 'Create worktree');
  let canSubmit = $derived(!busy && !conflict && (
    mode === 'adopt'
      ? adoptPath.trim().length > 0
      : mode === 'create' && createKind === 'origin'
        ? existingBranch.length > 0
      : branch.trim().length > 0
        && (mode !== 'create' || createKind !== 'new'
          || (baseRef.trim().length > 0 && refValidation === 'valid'))
  ));
  let environmentSummary = $derived(envPolicy === 'copy' ? 'Copy safe .env' : 'Skip .env');
  let herdSummary = $derived(repository.herd.parked
    ? `Herd · .${repository.herd.tld ?? 'test'}`
    : repository.herd.available ? 'Herd available' : 'No Herd');

  $effect(() => {
    const input = mode === 'adopt'
      ? adoptPathInput
      : mode === 'create' && createKind === 'origin' ? branchSearchInput : branchInput;
    if (input) queueMicrotask(() => input?.focus());
  });

  $effect(() => {
    projectTitle = syncProjectTitleDefault(projectTitle, defaultProjectTitle, projectTitleTouched);
  });

  $effect(() => {
    if (mode !== 'create' || !defaultRef || baseRefTouched || appliedDefaultRef) return;
    appliedDefaultRef = true;
    baseRef = defaultRef;
    queueMicrotask(() => scheduleRefValidation(0));
  });

  $effect(() => {
    if (!conflict || !conflictPanel) return;
    queueMicrotask(() => conflictPanel?.querySelector<HTMLButtonElement>('button')?.focus());
  });

  onMount(() => {
    if (mode === 'create') scheduleRefValidation(0);
  });
  onDestroy(() => {
    if (refValidationTimer) clearTimeout(refValidationTimer);
  });

  function rankBranches(options: WorktreeBranchOption[], query: string): WorktreeBranchOption[] {
    return options
      .map((option) => ({ option, score: fuzzySubsequenceScore(query, option.name) }))
      .filter((entry): entry is { option: WorktreeBranchOption; score: number } => entry.score !== null)
      .sort((left, right) => right.score - left.score || left.option.name.localeCompare(right.option.name))
      .map((entry) => entry.option);
  }

  function rankRefOptions(options: WorktreeRefOption[], query: string): WorktreeRefOption[] {
    const trimmed = query.trim();
    const exact = options.some((option) => option.name === trimmed);
    const filter = exact ? '' : trimmed;
    return options
      .map((option, index) => ({
        option,
        index,
        score: filter ? fuzzySubsequenceScore(filter, option.name) : 0
      }))
      .filter((entry): entry is { option: WorktreeRefOption; index: number; score: number } => entry.score !== null)
      .sort((left, right) => right.score - left.score || left.index - right.index)
      .map((entry) => entry.option);
  }

  function refSourceLabel(source: WorktreeRefOption['source']): string {
    if (source === 'current') return 'current';
    if (source === 'default') return 'origin default';
    return source;
  }

  function scheduleRefValidation(delay = 400): void {
    if (refValidationTimer) clearTimeout(refValidationTimer);
    resolvedRef = null;
    resolvedCommit = null;
    refValidationError = null;
    const value = baseRef.trim();
    if (!value) {
      refValidation = 'invalid';
      refValidationError = 'Enter a branch, tag, or commit.';
      return;
    }
    refValidation = 'idle';
    refValidationTimer = setTimeout(() => void validateBaseRef(value), delay);
  }

  async function validateBaseRef(value = baseRef.trim()): Promise<boolean> {
    if (refValidationTimer) clearTimeout(refValidationTimer);
    refValidationTimer = null;
    if (!value) {
      refValidation = 'invalid';
      refValidationError = 'Enter a branch, tag, or commit.';
      return false;
    }
    const sequence = ++refValidationSequence;
    refValidation = 'checking';
    refValidationError = null;
    try {
      const result = await onValidateRef(value);
      if (sequence !== refValidationSequence || baseRef.trim() !== value) return false;
      resolvedRef = result.resolved_ref;
      resolvedCommit = result.commit;
      refValidation = 'valid';
      return true;
    } catch (cause) {
      if (sequence !== refValidationSequence || baseRef.trim() !== value) return false;
      resolvedRef = null;
      resolvedCommit = null;
      refValidation = 'invalid';
      refValidationError = cause instanceof Error ? cause.message : String(cause);
      baseRefOpen = false;
      return false;
    }
  }

  function updateBaseRef(): void {
    baseRefTouched = true;
    baseRefOpen = true;
    baseRefOptionIndex = 0;
    scheduleRefValidation();
  }

  function chooseBaseRef(option: WorktreeRefOption | null): void {
    if (!option) return;
    baseRefTouched = true;
    baseRef = option.name;
    baseRefOpen = false;
    baseRefOptionIndex = 0;
    baseRefInput?.focus();
    scheduleRefValidation(0);
  }

  function handleBaseRefKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      baseRefOpen = true;
      if (rankedRefOptions.length === 0) return;
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      baseRefOptionIndex = (baseRefOptionIndex + delta + rankedRefOptions.length) % rankedRefOptions.length;
      queueMicrotask(() => document.getElementById(`worktree-ref-option-${baseRefOptionIndex}`)?.scrollIntoView({ block: 'nearest' }));
      return;
    }
    if (event.key === 'Enter' && baseRefOpen && activeRefOption) {
      event.preventDefault();
      chooseBaseRef(activeRefOption);
      return;
    }
    if (event.key === 'Escape' && baseRefOpen) {
      event.preventDefault();
      event.stopPropagation();
      baseRefOpen = false;
    }
  }

  function closeBaseRefPicker(event: FocusEvent): void {
    const next = event.relatedTarget;
    if (next instanceof Node && baseRefPicker?.contains(next)) return;
    baseRefOpen = false;
  }

  function changeCreateKind(value: string): void {
    if (value !== 'new' && value !== 'origin') return;
    createKind = value;
    if (value === 'origin' && effectiveBranchOptions.length === 0) onLoadBranches();
  }

  function updateProjectTitle(event: Event): void {
    const input = event.target;
    if (!(input instanceof HTMLInputElement)) return;
    projectTitleTouched = true;
    projectTitle = input.value;
  }

  async function submit(resolution?: WorktreeCreateResolution): Promise<void> {
    if (busy) return;
    const title = resolvedProjectTitle(projectTitle, defaultProjectTitle);
    if (mode === 'adopt') {
      onSubmit({ mode, path: adoptPath.trim(), title });
      return;
    }
    const nextBranch = mode === 'create' && createKind === 'origin'
      ? existingBranch
      : branch.trim();
    if (!nextBranch) return;
    if (mode === 'fork') {
      onSubmit({ mode, branch: nextBranch, title, resolution, envPolicy, rememberEnvPolicy });
      return;
    }
    if (createKind === 'new' && !(await validateBaseRef())) return;
    onSubmit({
      mode,
      branch: nextBranch,
      title,
      fromRef: resolution === undefined && createKind === 'new' && baseRef.trim()
        ? baseRef.trim()
        : undefined,
      resolution: resolution ?? (createKind === 'origin'
        ? effectiveBranchOptions.find((option) => option.name === nextBranch)?.source === 'local'
          ? 'use_existing_branch'
          : 'load_from_remote'
        : undefined),
      envPolicy,
      rememberEnvPolicy
    });
  }

  function conflictActionLabel(action: WorktreeCreateConflictAction): string {
    if (action === 'use_existing_branch') return 'Use existing branch';
    if (action === 'load_from_remote') return 'Load from remote';
    if (action === 'import_existing_worktree') return 'Import existing worktree';
    if (action === 'open_registered_project') return 'Open registered project';
    return 'Choose a different name';
  }

  function chooseConflictAction(action: WorktreeCreateConflictAction): void {
    if (!conflict || busy) return;
    if (action === 'use_existing_branch' || action === 'load_from_remote') {
      void submit(action);
      return;
    }
    if (action === 'import_existing_worktree') {
      onSubmit({
        mode: 'adopt',
        path: conflict.path,
        title: resolvedProjectTitle(projectTitle, defaultProjectTitleFromPath(conflict.path, conflict.branch))
      });
      return;
    }
    if (action === 'open_registered_project' && conflict.project_id !== null) {
      onOpenProject(conflict.project_id);
      return;
    }
    onClearConflict();
    branch = '';
    existingBranch = '';
    branchQuery = '';
    queueMicrotask(() => branchInput?.focus());
  }

  async function chooseDirectory(): Promise<void> {
    const path = await open({ directory: true, multiple: false, title: 'Choose an existing Git worktree' });
    if (typeof path === 'string') {
      const selectDerivedTitle = !projectTitleTouched;
      adoptPath = path;
      queueMicrotask(() => {
        projectTitleInput?.focus();
        if (selectDerivedTitle) projectTitleInput?.select();
      });
    }
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
    class="w-[min(520px,calc(100vw-32px))] max-w-none gap-0 rounded-lg border border-border bg-popover p-0"
    showCloseButton={false}
    aria-describedby="worktree-dialog-description"
    onOpenAutoFocus={(event) => {
      event.preventDefault();
      queueMicrotask(() => {
        const input = mode === 'adopt'
          ? adoptPathInput
          : mode === 'create' && createKind === 'origin' ? branchSearchInput : branchInput;
        input?.focus();
      });
    }}
  >
    <form class="grid min-h-0 max-h-[calc(100dvh-2rem)] grid-rows-[auto_minmax(0,1fr)_auto]" onsubmit={(event) => { event.preventDefault(); void submit(); }}>
      <Dialog.Header class="flex-row items-start justify-between border-b border-border px-4 py-3 text-left">
        <span class="flex min-w-0 items-start gap-3">
          <span class="grid size-8 shrink-0 place-items-center rounded border border-border bg-card text-muted-foreground">
            {#if mode === 'adopt'}<FolderOpenIcon size={16} />{:else if mode === 'fork'}<GitForkIcon size={16} />{:else}<GitBranchIcon size={16} />{/if}
          </span>
          <span class="min-w-0">
            <Dialog.Title class="truncate text-base">{title}</Dialog.Title>
            <Dialog.Description id="worktree-dialog-description" class="mt-1 text-sm">
              {#if mode === 'fork'}Creates a new branch from this worktree's exact HEAD commit <code>{sourceEntry?.head.slice(0, 10) ?? 'unknown'}</code>.{:else if mode === 'adopt'}Registers an existing worktree without moving or changing it.{:else}Creates a new branch at the starting ref you choose.{/if}
            </Dialog.Description>
          </span>
        </span>
        <IconButton label="Close worktree dialog" disabled={busy} onclick={onClose}>{#snippet icon()}<XIcon size={14} />{/snippet}</IconButton>
      </Dialog.Header>

      <section class="grid min-h-0 content-start gap-3 overflow-y-auto overscroll-contain px-4 py-3">
        {#if mode === 'adopt'}
          <label class="grid gap-1.5">
            <span class="text-sm font-medium">Existing worktree folder</span>
            <span class="flex gap-2">
              <Input bind:ref={adoptPathInput} bind:value={adoptPath} class="min-w-0 flex-1 font-mono" placeholder="/path/to/existing-worktree" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck={false} />
              <Button type="button" variant="outline" onclick={() => void chooseDirectory()}><FolderOpenIcon size={14} />Choose…</Button>
            </span>
            <small class="text-xs text-muted-foreground">The folder stays where it is and is marked adopted, not Workman-managed.</small>
          </label>
          <label class="grid gap-1.5" oninput={updateProjectTitle}>
            <span class="text-sm font-medium">Title</span>
            <Input
              bind:ref={projectTitleInput}
              value={projectTitle}
              autocomplete="off"
              aria-label="Worktree title"
              aria-describedby="worktree-title-help"
              placeholder="Follows the selected folder"
            />
            <small id="worktree-title-help" class="text-xs text-muted-foreground">Defaults to the existing folder name; an empty title keeps that default.</small>
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
                    autocapitalize="off"
                    autocorrect="off"
                    spellcheck={false}
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
                <ScrollArea id="worktree-branch-options" class="h-36" role="listbox" aria-label="Available branches">
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
              <Input bind:ref={branchInput} bind:value={branch} class="font-mono" placeholder={mode === 'fork' ? 'feature/follow-up' : 'feature/new-worktree'} autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck={false} />
            </label>
          {/if}

          <label class="grid gap-1.5" oninput={updateProjectTitle}>
            <span class="text-sm font-medium">Title</span>
            <Input
              bind:ref={projectTitleInput}
              value={projectTitle}
              autocomplete="off"
              aria-label="Worktree title"
              aria-describedby="worktree-title-help"
              placeholder="Follows the branch name"
            />
            <small id="worktree-title-help" class="text-xs text-muted-foreground">Defaults live to the full branch name; an empty title keeps that default.</small>
          </label>

          {#if mode === 'create' && createKind === 'new'}
            <section class="grid gap-1.5" aria-labelledby="worktree-base-ref-label">
              <span id="worktree-base-ref-label" class="text-sm font-medium">Start branch from…</span>
              <div
                bind:this={baseRefPicker}
                class="relative"
                onfocusout={closeBaseRefPicker}
              >
                <div class="relative">
                  <Input
                    bind:ref={baseRefInput}
                    bind:value={baseRef}
                    class="pr-9 font-mono"
                    role="combobox"
                    aria-autocomplete="list"
                    aria-expanded={baseRefOpen}
                    aria-controls="worktree-ref-options"
                    aria-activedescendant={baseRefOpen && activeRefOption ? `worktree-ref-option-${baseRefOptionIndex}` : undefined}
                    aria-invalid={refValidation === 'invalid'}
                    aria-describedby={refValidationError ? 'worktree-base-ref-help worktree-base-ref-error' : 'worktree-base-ref-help'}
                    placeholder={branchesLoading ? 'Detecting origin default…' : 'HEAD, origin/main, tag, or commit'}
                    autocomplete="off"
                    autocapitalize="off"
                    autocorrect="off"
                    spellcheck={false}
                    onfocus={() => (baseRefOpen = true)}
                    oninput={updateBaseRef}
                    onkeydown={handleBaseRefKeydown}
                  />
                  <button
                    class="absolute inset-y-0 right-0 grid w-8 place-items-center rounded-r-lg text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                    type="button"
                    aria-label="Show starting ref suggestions"
                    aria-expanded={baseRefOpen}
                    onclick={() => { baseRefOpen = !baseRefOpen; if (baseRefOpen) queueMicrotask(() => baseRefInput?.focus()); }}
                  >
                    <ChevronDownIcon class={`motion-safe:transition-transform ${baseRefOpen ? 'rotate-180' : ''}`} size={15} aria-hidden="true" />
                  </button>
                </div>
                {#if baseRefOpen}
                  <div id="worktree-ref-options" class="absolute top-full right-0 left-0 z-30 mt-1 overflow-hidden rounded-lg border border-border bg-popover shadow-lg" role="listbox" aria-label="Starting ref suggestions">
                    <div class="flex min-h-7 items-center justify-between border-b border-border px-2 text-xs text-muted-foreground">
                      <span>Suggested refs</span>
                      <span>or type any ref</span>
                    </div>
                    <ScrollArea class="max-h-44">
                      <div class="grid gap-0.5 p-1">
                        {#each rankedRefOptions as option, index (option.name)}
                          <button
                            id={`worktree-ref-option-${index}`}
                            class="flex min-h-8 items-center gap-2 rounded px-2 text-left text-sm hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring data-[active=true]:bg-accent"
                            data-active={index === baseRefOptionIndex}
                            type="button"
                            role="option"
                            aria-selected={baseRef === option.name}
                            onmouseenter={() => (baseRefOptionIndex = index)}
                            onmousedown={(event) => event.preventDefault()}
                            onclick={() => chooseBaseRef(option)}
                          >
                            {#if option.source === 'remote' || option.source === 'default'}<CloudIcon class="shrink-0 text-muted-foreground" size={14} aria-hidden="true" />{:else}<HardDriveIcon class="shrink-0 text-muted-foreground" size={14} aria-hidden="true" />{/if}
                            <span class="min-w-0 flex-1 truncate font-mono">{option.name}</span>
                            <Badge variant="outline" class="h-5 px-1.5 text-xs font-normal text-muted-foreground">{refSourceLabel(option.source)}</Badge>
                          </button>
                        {:else}
                          <div class="grid min-h-16 place-content-center gap-1 px-3 text-center">
                            <strong class="text-sm font-medium">No suggested match</strong>
                            <span class="text-xs text-muted-foreground">Keep typing to use a branch, tag, or commit directly.</span>
                          </div>
                        {/each}
                      </div>
                    </ScrollArea>
                  </div>
                {/if}
              </div>
              <small id="worktree-base-ref-help" class="text-xs text-muted-foreground">HEAD = your current checkout state; {defaultRef ? `${defaultRef} = the latest remote default branch.` : 'use origin/<branch> for the latest remote branch.'}</small>
              {#if refValidationError}
                <p id="worktree-base-ref-error" class="text-xs text-destructive" role="alert">{refValidationError}</p>
              {/if}
              <div class="flex min-h-9 flex-wrap items-center gap-x-1.5 gap-y-1 rounded border border-border bg-muted/30 px-2.5 py-1.5 text-xs" aria-live="polite">
                <span class="text-muted-foreground">Creates branch</span>
                <code class="font-medium">{previewBranch}</code>
                <span class="text-muted-foreground">at</span>
                <code class:text-destructive={refValidation === 'invalid'} class="font-medium">{previewRef}</code>
                {#if previewCommit}<span class="text-muted-foreground">· commit</span><code class="font-medium">{previewCommit}</code>{/if}
                {#if refValidation === 'checking'}<LoaderCircleIcon class="ml-auto spin text-muted-foreground" size={13} aria-label="Checking ref" />{:else if refValidation === 'valid'}<CircleCheckIcon class="ml-auto text-emerald-600 dark:text-emerald-400" size={13} aria-label="Ref found" />{/if}
              </div>
            </section>
          {:else if mode === 'fork'}
            <div class="flex min-h-9 flex-wrap items-center gap-x-1.5 gap-y-1 rounded border border-border bg-muted/30 px-2.5 py-1.5 text-xs">
              <span class="text-muted-foreground">Creates branch</span>
              <code class="font-medium">{previewBranch}</code>
              <span class="text-muted-foreground">at exact HEAD</span>
              <code class="font-medium">{sourceEntry?.head.slice(0, 10) ?? 'unknown'}</code>
            </div>
          {/if}

          <Collapsible.Root bind:open={advancedOpen} class="overflow-hidden rounded-lg border border-border bg-card">
            <Collapsible.Trigger class="flex min-h-10 w-full items-center gap-2 px-3 text-left hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
              <SlidersHorizontalIcon class="shrink-0 text-muted-foreground" size={14} aria-hidden="true" />
              <span class="min-w-0 flex-1">
                <strong class="block text-sm font-medium">Advanced settings</strong>
                <span class="block truncate text-xs text-muted-foreground">{environmentSummary} · {herdSummary}</span>
              </span>
              <ChevronDownIcon class={`shrink-0 text-muted-foreground motion-safe:transition-transform ${advancedOpen ? 'rotate-180' : ''}`} size={15} aria-hidden="true" />
            </Collapsible.Trigger>
            <Collapsible.Content>
              <div class="grid gap-3 border-t border-border px-3 py-3">
                <dl class="grid gap-1.5 text-xs">
                  <div class="grid grid-cols-[88px_minmax(0,1fr)] gap-2">
                    <dt class="text-muted-foreground">Destination</dt>
                    <dd class="truncate font-mono" title={repository.managed_root}>{repository.managed_root}</dd>
                  </div>
                  <div class="grid grid-cols-[88px_minmax(0,1fr)] gap-2">
                    <dt class="text-muted-foreground">Herd</dt>
                    <dd>{repository.herd.parked ? `Parked on .${repository.herd.tld ?? 'test'}` : repository.herd.available ? 'Available after creation' : 'Not detected'}</dd>
                  </div>
                </dl>

                <fieldset class="grid gap-2 border-t border-border pt-3">
                  <legend class="text-sm font-medium">Environment file</legend>
                  <div class="grid grid-cols-2 gap-2">
                    <Button type="button" variant={envPolicy === 'skip' ? 'secondary' : 'outline'} onclick={() => (envPolicy = 'skip')}>Skip .env</Button>
                    <Button type="button" variant={envPolicy === 'copy' ? 'secondary' : 'outline'} onclick={() => (envPolicy = 'copy')}>Copy safe .env</Button>
                  </div>
                  <label class="flex items-center gap-2 text-sm text-muted-foreground">
                    <Checkbox bind:checked={rememberEnvPolicy} aria-label="Remember environment choice for this repository" />
                    Remember for {repository.name}
                  </label>
                </fieldset>
              </div>
            </Collapsible.Content>
          </Collapsible.Root>
        {/if}

        {#if conflict}
          <section bind:this={conflictPanel} class="grid gap-2 rounded border border-warning/40 bg-warning/10 px-3 py-3" role="alert" aria-live="assertive">
            <div class="grid gap-1">
              <strong class="text-sm">This branch needs a choice</strong>
              <span class="text-xs leading-relaxed text-muted-foreground">{conflict.message}</span>
              <code class="break-all text-xs">{conflict.path}</code>
            </div>
            <div class="flex flex-wrap gap-2">
              {#each conflict.actions as action}
                <Button type="button" size="sm" variant={action === 'choose_different_name' ? 'ghost' : 'outline'} onclick={() => chooseConflictAction(action)}>
                  {conflictActionLabel(action)}
                </Button>
              {/each}
            </div>
          </section>
        {/if}
        {#if error}<p class="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">{error}</p>{/if}
      </section>

      <Dialog.Footer class="flex-row justify-end border-t border-border bg-card px-4 py-2.5">
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
