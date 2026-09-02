<script lang="ts">
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import GitBranchPlusIcon from '@lucide/svelte/icons/git-branch-plus';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import XIcon from '@lucide/svelte/icons/x';
  import { tick } from 'svelte';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import type { Project } from './daemon';
  import ProjectIcon from './ProjectIcon.svelte';
  import { sidebarIdentityColorValue } from './projectAppearance';
  import { projectDisplayName } from './worktrees';

  interface Props {
    projects: Project[];
    folderBusy?: boolean;
    worktreeBusyProjectId?: number | null;
    onChooseFolder: () => void;
    onCreateWorktree: (project: Project) => void;
    onClose: () => void;
  }

  let {
    projects,
    folderBusy = false,
    worktreeBusyProjectId = null,
    onChooseFolder,
    onCreateWorktree,
    onClose
  }: Props = $props();

  let step = $state<'kind' | 'worktree-source'>('kind');
  let firstAction = $state<HTMLButtonElement | null>(null);
  let busy = $derived(folderBusy || worktreeBusyProjectId !== null);
  let worktreeSources = $derived(
    projects.filter((project) =>
      project.parent_project_id === null && project.repository_id !== null
    )
  );

  async function showWorktreeSources(): Promise<void> {
    if (worktreeSources.length === 0) return;
    step = 'worktree-source';
    await tick();
    const firstProjectId = worktreeSources[0]?.id;
    if (firstProjectId !== undefined) {
      document.getElementById(`add-project-source-${firstProjectId}`)?.focus();
    }
  }

  async function showKinds(): Promise<void> {
    step = 'kind';
    await tick();
    firstAction?.focus();
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="w-[min(520px,calc(100vw-32px))] max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 rounded-lg border border-border bg-popover p-0"
    showCloseButton={false}
    aria-describedby="add-project-description"
    onOpenAutoFocus={(event) => {
      event.preventDefault();
      queueMicrotask(() => firstAction?.focus());
    }}
  >
    <Dialog.Header class="flex-row items-start justify-between border-b border-border px-4 py-3 text-left">
      <span class="flex min-w-0 items-start gap-3">
        <span class:worktree-step={step === 'worktree-source'} class="dialog-icon grid size-8 shrink-0 place-items-center rounded border">
          {#if step === 'kind'}<FolderOpenIcon size={16} />{:else}<GitBranchPlusIcon size={16} />{/if}
        </span>
        <span class="min-w-0">
          <Dialog.Title class="text-base">{step === 'kind' ? 'Add project' : 'Create a worktree'}</Dialog.Title>
          <Dialog.Description id="add-project-description" class="mt-1 text-sm leading-snug">
            {step === 'kind'
              ? 'Choose how this project should be added to Workman.'
              : 'Choose the top-level project that owns the repository.'}
          </Dialog.Description>
        </span>
      </span>
      <IconButton label="Close add project" disabled={busy} onclick={onClose}>
        {#snippet icon()}<XIcon size={14} />{/snippet}
      </IconButton>
    </Dialog.Header>

    {#if step === 'kind'}
      <section class="choice-grid grid min-h-0 gap-2 overflow-y-auto p-4" aria-label="Ways to add a project">
        <button bind:this={firstAction} type="button" class="choice-card folder-choice" disabled={busy} onclick={onChooseFolder}>
          <span class="choice-icon">{#if folderBusy}<LoaderCircleIcon class="spin" size={19} />{:else}<FolderOpenIcon size={19} />{/if}</span>
          <span class="min-w-0 text-left">
            <strong>{folderBusy ? 'Opening folder picker…' : 'Choose a folder'}</strong>
            <small>Add an existing folder from this computer.</small>
          </span>
          <ChevronRightIcon class="choice-arrow" size={17} />
        </button>

        <button
          type="button"
          class="choice-card worktree-choice"
          disabled={busy || worktreeSources.length === 0}
          onclick={() => void showWorktreeSources()}
        >
          <span class="choice-icon"><GitBranchPlusIcon size={19} /></span>
          <span class="min-w-0 text-left">
            <strong>Create a worktree</strong>
            <small>
              {worktreeSources.length > 0
                ? `Start a new branch from ${worktreeSources.length === 1 ? 'your Git project' : `one of ${worktreeSources.length} Git projects`}.`
                : 'Add a Git repository first to create worktrees.'}
            </small>
          </span>
          <ChevronRightIcon class="choice-arrow" size={17} />
        </button>
      </section>
    {:else}
      <section class="grid min-h-0 content-start gap-1 overflow-y-auto overscroll-contain p-2" aria-label="Projects available for worktrees">
        {#each worktreeSources as project (project.id)}
          <button
            id={`add-project-source-${project.id}`}
            type="button"
            class="project-choice"
            disabled={busy}
            onclick={() => onCreateWorktree(project)}
          >
            <span class="project-glyph">
              <ProjectIcon
                icon={project.icon}
                image={project.icon_image?.data_url ?? null}
                color={project.icon_color}
                fallback="repository"
                size={17}
              />
            </span>
            <span class="min-w-0 flex-1 text-left">
              <strong class="block truncate" style:color={sidebarIdentityColorValue(project.name_color)}>{projectDisplayName(project)}</strong>
              <small class="block truncate" title={project.path}>{project.path}</small>
            </span>
            {#if worktreeBusyProjectId === project.id}
              <LoaderCircleIcon class="spin choice-arrow" size={16} />
            {:else}
              <ChevronRightIcon class="choice-arrow" size={16} />
            {/if}
          </button>
        {/each}
      </section>
    {/if}

    <Dialog.Footer class="mx-0 mb-0 flex-row flex-wrap justify-between rounded-none rounded-b-lg border-t border-border bg-card px-4 py-3">
      {#if step === 'worktree-source'}
        <Button type="button" variant="ghost" disabled={busy} onclick={() => void showKinds()}><ArrowLeftIcon size={14} />Back</Button>
      {:else}
        <span></span>
      {/if}
      <Button type="button" variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<style>
  .dialog-icon { border-color: color-mix(in srgb, var(--project-icon-blue) 36%, var(--border)); color: var(--project-icon-blue); background: color-mix(in srgb, var(--project-icon-blue) 10%, var(--card)); }
  .dialog-icon.worktree-step { border-color: color-mix(in srgb, var(--project-icon-violet) 36%, var(--border)); color: var(--project-icon-violet); background: color-mix(in srgb, var(--project-icon-violet) 10%, var(--card)); }
  .choice-card { display: grid; grid-template-columns: 38px minmax(0, 1fr) auto; align-items: center; gap: 12px; min-height: 74px; width: 100%; border: 1px solid var(--border); border-radius: 8px; padding: 12px; color: var(--foreground); background: var(--card); transition: border-color 120ms ease, background 120ms ease, transform 120ms ease; }
  .choice-card:hover:not(:disabled) { border-color: color-mix(in srgb, currentColor 26%, var(--border)); background: var(--accent); transform: translateY(-1px); }
  .choice-card:focus-visible, .project-choice:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
  .choice-card:disabled { cursor: not-allowed; opacity: .55; }
  .choice-icon, .project-glyph { display: grid; place-items: center; border: 1px solid currentColor; border-radius: 6px; }
  .choice-icon { width: 38px; height: 38px; }
  .folder-choice .choice-icon { color: var(--project-icon-blue); background: color-mix(in srgb, var(--project-icon-blue) 9%, transparent); }
  .worktree-choice .choice-icon { color: var(--project-icon-violet); background: color-mix(in srgb, var(--project-icon-violet) 9%, transparent); }
  .choice-card strong, .project-choice strong { font-size: 14px; font-weight: 650; }
  .choice-card small, .project-choice small { display: block; margin-top: 3px; color: var(--muted-foreground); font-size: 12px; line-height: 1.35; }
  :global(.choice-arrow) { flex: none; color: var(--muted-foreground); }
  .project-choice { display: flex; min-width: 0; align-items: center; gap: 11px; width: 100%; border: 1px solid transparent; border-radius: 7px; padding: 9px 10px; color: var(--foreground); }
  .project-choice:hover:not(:disabled) { border-color: var(--border); background: var(--accent); }
  .project-choice:disabled { cursor: wait; opacity: .65; }
  .project-glyph { width: 34px; height: 34px; border-color: var(--border); color: var(--muted-foreground); background: var(--card); }
  :global(.spin) { animation: add-project-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { .choice-card { transition: none; } :global(.spin) { animation: none; } }
  @keyframes add-project-spin { to { transform: rotate(360deg); } }
</style>
