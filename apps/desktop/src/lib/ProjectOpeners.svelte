<script lang="ts">
  import { onMount } from 'svelte';

  import {
    customActionLabel,
    editorActionLabel,
    ensureOpenersLoaded,
    openerSettings,
    openProjectCustom,
    openProjectEditor,
    openProjectFinder
  } from './openers';

  interface Props {
    path: string;
    projectName: string;
    collapsed: boolean;
    onError: (message: string) => void;
  }

  let { path, projectName, collapsed, onError }: Props = $props();
  let busy = $state<'editor' | 'finder' | 'custom' | null>(null);

  let editorLabel = $derived(
    editorActionLabel($openerSettings.config, $openerSettings.editors)
  );
  let customLabel = $derived(customActionLabel($openerSettings.config));
  let visibleCount = $derived([
    $openerSettings.config.sidebar.editorEnabled,
    $openerSettings.config.sidebar.finderEnabled,
    $openerSettings.config.sidebar.customEnabled
  ].filter(Boolean).length);

  onMount(() => {
    void ensureOpenersLoaded();
  });

  async function launch(action: 'editor' | 'finder' | 'custom'): Promise<void> {
    if (busy) return;
    busy = action;
    try {
      if (action === 'editor') await openProjectEditor(path);
      else if (action === 'finder') await openProjectFinder(path);
      else await openProjectCustom(path);
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      busy = null;
    }
  }
</script>

{#if visibleCount > 0}
  <div
    class="project-openers"
    class:collapsed
    class:triple={visibleCount === 3}
    aria-label={`Open ${projectName}`}
  >
    {#if $openerSettings.config.sidebar.editorEnabled}
      <button
        type="button"
        aria-label={`${editorLabel}: ${projectName}`}
        title={editorLabel}
        disabled={busy !== null}
        onclick={(event) => { event.stopPropagation(); void launch('editor'); }}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path d="m6 3-4 5 4 5M10 3l4 5-4 5M9 2 7 14"></path>
        </svg>
      </button>
    {/if}
    {#if $openerSettings.config.sidebar.finderEnabled}
      <button
        type="button"
        aria-label={`Show ${projectName} in Finder`}
        title="Show in Finder"
        disabled={busy !== null}
        onclick={(event) => { event.stopPropagation(); void launch('finder'); }}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path d="M1.5 4.5h5l1.2-2h2.8l1.2 2h2.8v8.8H1.5zM1.5 6h13"></path>
        </svg>
      </button>
    {/if}
    {#if $openerSettings.config.sidebar.customEnabled}
      <button
        type="button"
        aria-label={`${customLabel}: ${projectName}`}
        title={customLabel}
        disabled={busy !== null}
        onclick={(event) => { event.stopPropagation(); void launch('custom'); }}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <circle cx="3" cy="8" r="1"></circle>
          <circle cx="8" cy="8" r="1"></circle>
          <circle cx="13" cy="8" r="1"></circle>
        </svg>
      </button>
    {/if}
  </div>
{/if}

<style>
  .project-openers {
    position: absolute;
    z-index: 5;
    top: 50%;
    right: 27px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px 2px 3px 14px;
    background: linear-gradient(90deg, transparent, #202328 15px);
    opacity: 0;
    pointer-events: none;
    transform: translate(3px, -50%);
    transition: opacity 90ms ease-out, transform 90ms ease-out;
  }

  :global(.project-row:hover) .project-openers,
  :global(.project-row:focus-within) .project-openers {
    opacity: 1;
    pointer-events: auto;
    transform: translate(0, -50%);
  }

  button {
    display: grid;
    width: 23px;
    height: 23px;
    place-items: center;
    border: 1px solid #454b53;
    border-radius: 3px;
    padding: 0;
    background: #272b30;
    color: #aeb5bd;
    cursor: pointer;
  }

  button:hover:not(:disabled),
  button:focus-visible {
    border-color: #77808a;
    background: #30353b;
    color: #f0f2f4;
    outline: none;
  }

  button:disabled { cursor: wait; opacity: .48; }
  svg { width: 12px; height: 12px; overflow: visible; fill: currentColor; }
  path { fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.25; }

  .collapsed {
    right: 2px;
    gap: 1px;
    padding: 3px 2px;
    background: #202328;
  }

  .collapsed button { width: 20px; height: 22px; }
  .collapsed.triple button { width: 15px; }
  .collapsed.triple svg { width: 10px; }

  @media (prefers-reduced-motion: reduce) {
    .project-openers { transition: none; }
  }
</style>
