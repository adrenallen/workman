<script lang="ts">
  import FolderCogIcon from '@lucide/svelte/icons/folder-cog';
  import XIcon from '@lucide/svelte/icons/x';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Input } from '$lib/components/ui/input';
  import type { Project } from './daemon';
  import ProjectIcon from './ProjectIcon.svelte';
  import {
    PROJECT_ICON_CHOICES,
    PROJECT_ICON_COLOR_CHOICES,
    normalizeProjectIcon,
    normalizeProjectIconColor,
    projectIconColorValue,
    type ProjectIconColor,
    type ProjectIconName,
    type ProjectSettingsInput
  } from './projectAppearance';

  interface Props {
    project: Project;
    busy?: boolean;
    onSave: (settings: ProjectSettingsInput) => void;
    onClose: () => void;
  }

  let { project, busy = false, onSave, onClose }: Props = $props();

  function initialDisplayName(): string {
    return project.display_name ?? project.name;
  }

  function initialIcon(): ProjectIconName | null {
    return normalizeProjectIcon(project.icon);
  }

  function initialIconColor(): ProjectIconColor {
    return normalizeProjectIconColor(project.icon_color);
  }

  let displayName = $state(initialDisplayName());
  let icon = $state<ProjectIconName | null>(initialIcon());
  let iconColor = $state<ProjectIconColor>(initialIconColor());
  let canSave = $derived(!busy && displayName.trim().length > 0);
  let repositoryLabel = $derived(project.repository_root ?? 'Not linked to a Git repository');
  let branchLabel = $derived(project.branch ?? (project.repository_id === null ? 'Not available' : 'Primary checkout'));

  function submit(): void {
    if (!canSave) return;
    onSave({
      displayName: displayName.trim(),
      icon,
      iconColor: icon ? iconColor : null
    });
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="w-[min(540px,calc(100vw-32px))] max-w-none gap-0 overflow-hidden rounded-lg border border-border bg-popover p-0"
    showCloseButton={false}
    aria-describedby="project-settings-description"
  >
    <form class="modal-form" onsubmit={(event) => { event.preventDefault(); submit(); }}>
      <Dialog.Header class="flex-row items-start justify-between border-b border-border px-4 py-3 text-left">
        <span class="flex min-w-0 items-start gap-3">
          <span class="grid size-8 shrink-0 place-items-center rounded border border-border bg-card text-muted-foreground">
            <FolderCogIcon size={16} />
          </span>
          <span class="min-w-0">
            <Dialog.Title class="truncate text-base">Project settings</Dialog.Title>
            <Dialog.Description id="project-settings-description" class="mt-1 text-sm">
              Rename this project and choose how it appears across Workman.
            </Dialog.Description>
          </span>
        </span>
        <IconButton label="Close project settings" disabled={busy} onclick={onClose}>
          {#snippet icon()}<XIcon size={14} />{/snippet}
        </IconButton>
      </Dialog.Header>

      <div class="settings-body">
        <label class="name-field">
          <span>Project name</span>
          <Input bind:value={displayName} autocomplete="off" aria-label="Project name" />
        </label>

        <fieldset>
          <legend>Icon</legend>
          <div class="icon-grid">
            <button
              class:selected={icon === null}
              type="button"
              aria-pressed={icon === null}
              onclick={() => (icon = null)}
            >
              <ProjectIcon
                fallback={project.parent_project_id !== null ? 'worktree' : project.repository_id !== null ? 'repository' : 'project'}
                size={16}
              />
              <span>Automatic</span>
            </button>
            {#each PROJECT_ICON_CHOICES as choice (choice.id)}
              <button
                class:selected={icon === choice.id}
                type="button"
                aria-pressed={icon === choice.id}
                onclick={() => (icon = choice.id)}
              >
                <ProjectIcon icon={choice.id} color={iconColor} size={16} />
                <span>{choice.label}</span>
              </button>
            {/each}
          </div>
          <small>Automatic keeps the folder, repository, or worktree icon.</small>
        </fieldset>

        <fieldset class:disabled={icon === null} disabled={icon === null}>
          <legend>Icon color</legend>
          <div class="color-grid">
            {#each PROJECT_ICON_COLOR_CHOICES as choice (choice.id)}
              <button
                class:selected={iconColor === choice.id}
                type="button"
                aria-pressed={iconColor === choice.id}
                onclick={() => (iconColor = choice.id)}
              >
                <span class="color-swatch" style:background={projectIconColorValue(choice.id)}></span>
                <span>{choice.label}</span>
              </button>
            {/each}
          </div>
        </fieldset>

        <section class="project-info" aria-labelledby="project-info-title">
          <h3 id="project-info-title">Project information</h3>
          <dl>
            <div><dt>Path</dt><dd title={project.path}>{project.path}</dd></div>
            <div><dt>Repository</dt><dd title={repositoryLabel}>{repositoryLabel}</dd></div>
            <div><dt>Branch</dt><dd>{branchLabel}</dd></div>
          </dl>
        </section>
      </div>

      <Dialog.Footer class="border-t border-border bg-card px-4 py-2.5">
        <Button type="button" variant="outline" disabled={busy} onclick={onClose}>Cancel</Button>
        <Button type="submit" disabled={!canSave}>{busy ? 'Saving…' : 'Save changes'}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<style>
  .modal-form { display: grid; min-height: 0; max-height: calc(100dvh - 2rem); grid-template-rows: auto minmax(0, 1fr) auto; }
  .settings-body { display: grid; min-height: 0; align-content: start; gap: 14px; overflow-y: auto; overscroll-behavior: contain; padding: 14px 16px 16px; }
  .name-field { display: grid; gap: 6px; }
  .name-field > span, legend, h3 { color: var(--text-soft); font-size: var(--font-size-xs); font-weight: 700; letter-spacing: 0.045em; text-transform: uppercase; }
  fieldset { min-width: 0; margin: 0; border: 0; padding: 0; }
  fieldset.disabled { opacity: 0.5; }
  legend { margin-bottom: 6px; padding: 0; }
  fieldset > small { display: block; margin-top: 6px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .icon-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px; }
  .icon-grid button, .color-grid button { display: flex; min-width: 0; align-items: center; gap: 7px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--card); color: var(--text-soft); cursor: pointer; }
  .icon-grid button { min-height: 36px; padding: 6px 8px; }
  .color-grid button { min-height: 30px; padding: 4px 7px; }
  .icon-grid button:hover, .color-grid button:hover { border-color: var(--border-strong); background: var(--accent); }
  .icon-grid button.selected, .color-grid button.selected { border-color: var(--ring); background: color-mix(in srgb, var(--ring) 9%, var(--card)); color: var(--foreground); }
  .icon-grid button span, .color-grid button span { overflow: hidden; font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }
  .color-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px; }
  .color-swatch { width: 10px; height: 10px; flex: none; border: 1px solid color-mix(in srgb, currentColor 25%, transparent); border-radius: 999px; }
  .project-info { overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius); background: var(--card); }
  .project-info h3 { min-height: 29px; margin: 0; border-bottom: 1px solid var(--border); padding: 7px 9px 5px; }
  dl { margin: 0; }
  dl > div { display: grid; min-height: 30px; grid-template-columns: 84px minmax(0, 1fr); align-items: center; gap: 8px; border-bottom: 1px solid var(--border); padding: 5px 9px; }
  dl > div:last-child { border-bottom: 0; }
  dt { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  dd { overflow: hidden; margin: 0; color: var(--text-soft); font: var(--font-size-xs) var(--terminal-font-family); text-overflow: ellipsis; white-space: nowrap; }
  @media (max-width: 480px) { .icon-grid, .color-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
