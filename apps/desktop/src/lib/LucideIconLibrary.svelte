<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import SearchIcon from '@lucide/svelte/icons/search';

  import ProjectIcon from './ProjectIcon.svelte';
  import {
    PROJECT_ICON_CHOICES,
    type ProjectIconColor
  } from './projectAppearance';

  interface Props {
    value: string | null;
    color: ProjectIconColor;
    disabled?: boolean;
    title?: string;
    ariaLabel?: string;
    onChange: (icon: string) => void;
  }

  let {
    value,
    color,
    disabled = false,
    title = 'Lucide library',
    ariaLabel = 'Lucide icons',
    onChange
  }: Props = $props();

  const pageSize = 42;
  let query = $state('');
  let page = $state(0);

  let filteredIcons = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return PROJECT_ICON_CHOICES;
    return PROJECT_ICON_CHOICES.filter((choice) => fuzzyMatch(choice.label.toLowerCase(), needle));
  });
  let pageCount = $derived(Math.max(1, Math.ceil(filteredIcons.length / pageSize)));
  let visibleIcons = $derived(filteredIcons.slice(page * pageSize, (page + 1) * pageSize));

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
</script>

<section class="library" aria-label={title}>
  <header>
    <span>
      <strong>{title}</strong>
      <small>{filteredIcons.length.toLocaleString()} icons</small>
    </span>
    <label>
      <SearchIcon size={13} aria-hidden="true" />
      <input
        type="search"
        value={query}
        placeholder="Search icons"
        aria-label={`Search ${ariaLabel.toLowerCase()}`}
        {disabled}
        oninput={(event) => updateQuery(event.currentTarget.value)}
      />
    </label>
  </header>

  <div class="icon-grid" role="listbox" aria-label={ariaLabel}>
    {#each visibleIcons as choice (choice.id)}
      <button
        class:selected={value === choice.id}
        type="button"
        role="option"
        aria-selected={value === choice.id}
        aria-label={choice.label}
        title={choice.label}
        {disabled}
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
  @media (max-width: 560px) { .icon-grid { grid-template-columns: repeat(5, minmax(0, 1fr)); } }
</style>
