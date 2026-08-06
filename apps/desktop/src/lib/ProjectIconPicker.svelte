<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import ImageIcon from '@lucide/svelte/icons/image';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import SearchIcon from '@lucide/svelte/icons/search';

  import type { Project, ProjectIconImage } from './daemon';
  import ProjectIcon from './ProjectIcon.svelte';
  import {
    PROJECT_ICON_CHOICES,
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

  const pageSize = 42;

  let query = $state('');
  let page = $state(0);
  let choosing = $state(false);
  let refreshing = $state(false);
  let automaticImage = $state<ProjectIconImage | null>(null);
  let customImage = $state<ProjectIconImage | null>(null);

  $effect(() => {
    if (project.icon_image?.source === 'auto') automaticImage = project.icon_image;
    if (project.icon_image?.source === 'custom') customImage = project.icon_image;
  });

  let filteredIcons = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return PROJECT_ICON_CHOICES;
    return PROJECT_ICON_CHOICES.filter((choice) => fuzzyMatch(choice.label.toLowerCase(), needle));
  });
  let pageCount = $derived(Math.max(1, Math.ceil(filteredIcons.length / pageSize)));
  let visibleIcons = $derived(filteredIcons.slice(page * pageSize, (page + 1) * pageSize));
  let automaticSelected = $derived(value === null);
  let customSelected = $derived(isProjectImageReference(value));
  let automaticPreview = $derived(automaticImage?.data_url ?? null);
  let customPreview = $derived(customImage?.data_url ?? (customSelected ? project.icon_image?.data_url ?? null : null));

  function updateQuery(value: string): void {
    query = value;
    page = 0;
  }

  function fuzzyMatch(label: string, needle: string): boolean {
    if (label.includes(needle)) return true;
    let cursor = 0;
    for (const character of label) {
      if (character === needle[cursor]) cursor += 1;
      if (cursor === needle.length) return true;
    }
    return false;
  }

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

<section class="library" aria-labelledby="project-icon-library-title">
  <header>
    <span>
      <strong id="project-icon-library-title">Lucide library</strong>
      <small>{filteredIcons.length.toLocaleString()} icons</small>
    </span>
    <label>
      <SearchIcon size={13} aria-hidden="true" />
      <input
        type="search"
        value={query}
        placeholder="Search icons"
        aria-label="Search Lucide icons"
        disabled={disabled}
        oninput={(event) => updateQuery(event.currentTarget.value)}
      />
    </label>
  </header>

  <div class="icon-grid" role="listbox" aria-label="Lucide project icons">
    {#each visibleIcons as choice (choice.id)}
      <button
        class:selected={value === choice.id}
        type="button"
        role="option"
        aria-selected={value === choice.id}
        aria-label={choice.label}
        title={choice.label}
        disabled={disabled}
        onclick={() => onChange(choice.id)}
      >
        <ProjectIcon icon={choice.id} {color} size={17} />
        <span>{choice.label}</span>
      </button>
    {:else}
      <p>No icons match “{query}”.</p>
    {/each}
  </div>

  <footer>
    <span>Page {Math.min(page + 1, pageCount)} of {pageCount}</span>
    <span class="page-actions">
      <button type="button" aria-label="Previous icon page" disabled={disabled || page === 0} onclick={() => (page -= 1)}><ChevronLeftIcon size={13} /></button>
      <button type="button" aria-label="Next icon page" disabled={disabled || page + 1 >= pageCount} onclick={() => (page += 1)}><ChevronRightIcon size={13} /></button>
    </span>
  </footer>
</section>

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

  .library { overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius); background: var(--card); }
  .library > header { display: flex; min-height: 42px; align-items: center; justify-content: space-between; gap: 10px; border-bottom: 1px solid var(--border); padding: 6px 8px; }
  .library > header > span { min-width: 0; }
  .library > header strong, .library > header small { display: block; }
  .library > header strong { color: var(--text-soft); font-size: var(--font-size-xs); letter-spacing: 0.045em; text-transform: uppercase; }
  .library > header small { margin-top: 1px; color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .library label { display: flex; width: min(210px, 48%); align-items: center; gap: 5px; border: 1px solid var(--input); border-radius: var(--radius); padding: 0 7px; background: var(--background); color: var(--muted-foreground); }
  .library input { width: 100%; min-width: 0; height: 27px; border: 0; outline: 0; background: transparent; color: var(--foreground); font-size: var(--font-size-xs); }
  .icon-grid { display: grid; min-height: 276px; grid-template-columns: repeat(7, minmax(0, 1fr)); align-content: start; gap: 3px; padding: 6px; }
  .icon-grid button { display: grid; min-width: 0; height: 42px; place-items: center; gap: 1px; border: 1px solid transparent; border-radius: 3px; padding: 3px 2px; background: transparent; color: var(--muted-foreground); cursor: pointer; }
  .icon-grid button:hover:not(:disabled) { border-color: var(--border-strong); background: var(--accent); color: var(--foreground); }
  .icon-grid button.selected { border-color: var(--ring); background: color-mix(in srgb, var(--ring) 10%, var(--background)); color: var(--foreground); }
  .icon-grid button span { width: 100%; overflow: hidden; font-size: 9px; line-height: 12px; text-align: center; text-overflow: ellipsis; white-space: nowrap; }
  .icon-grid > p { grid-column: 1 / -1; place-self: center; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .library > footer { display: flex; min-height: 29px; align-items: center; justify-content: space-between; border-top: 1px solid var(--border); padding: 4px 7px; color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .page-actions { display: inline-flex; gap: 3px; }
  .page-actions button { display: grid; width: 25px; height: 21px; place-items: center; border: 1px solid var(--border); border-radius: 3px; background: var(--background); color: var(--text-soft); cursor: pointer; }
  button:disabled { cursor: default; opacity: 0.45; }
  .spinning { animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 560px) { .source-row { grid-template-columns: 1fr; } .icon-grid { grid-template-columns: repeat(5, minmax(0, 1fr)); } }
  @media (prefers-reduced-motion: reduce) { .spinning { animation: none; } }
</style>
