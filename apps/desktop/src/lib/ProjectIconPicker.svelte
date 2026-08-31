<script lang="ts">
  import ImageIcon from '@lucide/svelte/icons/image';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';

  import type { Project, ProjectIconImage } from './daemon';
  import LucideIconLibrary from './LucideIconLibrary.svelte';
  import ProjectIcon from './ProjectIcon.svelte';
  import {
    isProjectImageReference,
    type ProjectIconColor
  } from './projectAppearance';

  interface Props {
    project: Project;
    value: string | null;
    color: ProjectIconColor;
    disabled?: boolean;
    onChange: (icon: string | null) => void;
    onChooseImage: () => Promise<Project | null>;
    onRefreshAutomatic: () => Promise<ProjectIconImage | null>;
  }

  let {
    project,
    value,
    color,
    disabled = false,
    onChange,
    onChooseImage,
    onRefreshAutomatic
  }: Props = $props();

  let choosing = $state(false);
  let refreshing = $state(false);
  let automaticImage = $state<ProjectIconImage | null>(null);
  let customImage = $state<ProjectIconImage | null>(null);

  $effect(() => {
    if (project.icon_image?.source === 'auto') automaticImage = project.icon_image;
    if (project.icon_image?.source === 'custom') customImage = project.icon_image;
  });

  let automaticSelected = $derived(value === null);
  let customSelected = $derived(isProjectImageReference(value));
  let automaticPreview = $derived(automaticImage?.data_url ?? null);
  let customPreview = $derived(customImage?.data_url ?? (customSelected ? project.icon_image?.data_url ?? null : null));

  async function refreshAutomatic(): Promise<void> {
    refreshing = true;
    onChange(null);
    try {
      automaticImage = await onRefreshAutomatic();
    } finally {
      refreshing = false;
    }
  }

  async function chooseImage(): Promise<void> {
    choosing = true;
    try {
      const updated = await onChooseImage();
      if (!updated) return;
      customImage = updated.icon_image?.source === 'custom' ? updated.icon_image : null;
      onChange(updated.icon);
    } finally {
      choosing = false;
    }
  }
</script>

<div class="source-row">
  <button
    class="source-card"
    class:selected={automaticSelected}
    type="button"
    aria-pressed={automaticSelected}
    disabled={disabled}
    onclick={() => onChange(null)}
  >
    <span class="source-preview">
      <ProjectIcon
        image={automaticPreview}
        fallback={project.parent_project_id !== null ? 'worktree' : project.repository_id !== null ? 'repository' : 'project'}
        size={22}
      />
    </span>
    <span class="source-copy">
      <strong>Automatic</strong>
      <small>{automaticImage?.path ?? 'Scan common favicon locations'}</small>
    </span>
    <span class="source-tag">default</span>
  </button>

  <button
    class="source-card"
    class:selected={customSelected}
    type="button"
    aria-pressed={customSelected}
    disabled={disabled || choosing}
    onclick={() => void chooseImage()}
  >
    <span class="source-preview">
      {#if customPreview}
        <ProjectIcon image={customPreview} size={22} />
      {:else}
        <ImageIcon size={20} strokeWidth={1.7} />
      {/if}
    </span>
    <span class="source-copy">
      <strong>{choosing ? 'Opening Finder…' : 'Choose image…'}</strong>
      <small>{customImage?.path ?? (customSelected ? 'Image missing — choose a replacement' : 'Copy into .workman/')}</small>
    </span>
  </button>
</div>

<div class="automatic-actions">
  <span>{automaticImage ? 'Favicon found' : 'No cached favicon'}</span>
  <button type="button" disabled={disabled || refreshing} onclick={() => void refreshAutomatic()}>
    <span class:spinning={refreshing}><RefreshCwIcon size={12} /></span>
    {refreshing ? 'Scanning…' : 'Refresh favicon'}
  </button>
</div>

<LucideIconLibrary
  {value}
  {color}
  {disabled}
  ariaLabel="Lucide project icons"
  onChange={onChange}
/>

<style>
  .source-row { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px; }
  .source-card { display: grid; min-width: 0; min-height: 52px; grid-template-columns: 34px minmax(0, 1fr) auto; align-items: center; gap: 8px; border: 1px solid var(--border); border-radius: var(--radius); padding: 7px 8px; background: var(--card); color: var(--text-soft); text-align: left; cursor: pointer; }
  .source-card:hover:not(:disabled) { border-color: var(--border-strong); background: var(--accent); }
  .source-card.selected { border-color: var(--ring); background: color-mix(in srgb, var(--ring) 9%, var(--card)); color: var(--foreground); }
  .source-preview { display: grid; width: 32px; height: 32px; place-items: center; border: 1px solid var(--border); border-radius: 5px; background: var(--background); color: var(--muted-foreground); }
  .source-copy { min-width: 0; }
  .source-copy strong, .source-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .source-copy strong { font-size: var(--font-size-sm); font-weight: 650; }
  .source-copy small { margin-top: 2px; color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .source-tag { border: 1px solid var(--border); border-radius: 999px; padding: 2px 5px; color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .automatic-actions { display: flex; min-height: 27px; align-items: center; justify-content: space-between; color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .automatic-actions button { display: inline-flex; align-items: center; gap: 4px; border: 0; padding: 3px 0; background: transparent; color: var(--text-soft); font: inherit; cursor: pointer; }

  button:disabled { cursor: default; opacity: 0.45; }
  .spinning { animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 560px) { .source-row { grid-template-columns: 1fr; } }
  @media (prefers-reduced-motion: reduce) { .spinning { animation: none; } }
</style>
