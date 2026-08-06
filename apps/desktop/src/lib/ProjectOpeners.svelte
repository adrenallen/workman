<script lang="ts">
  import CodeXmlIcon from '@lucide/svelte/icons/code-xml';
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import WorkflowIcon from '@lucide/svelte/icons/workflow';
  import { onMount } from 'svelte';

  import IconButton from './components/ds/IconButton.svelte';
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
    siteUrl?: string | null;
    onError: (message: string) => void;
  }

  let { path, projectName, collapsed, siteUrl = null, onError }: Props = $props();
  let busy = $state<'editor' | 'finder' | 'custom' | 'site' | null>(null);

  let editorLabel = $derived(
    editorActionLabel($openerSettings.config, $openerSettings.editors)
  );
  let customLabel = $derived(customActionLabel($openerSettings.config));
  let visibleCount = $derived([
    $openerSettings.config.sidebar.editorEnabled,
    $openerSettings.config.sidebar.finderEnabled,
    $openerSettings.config.sidebar.customEnabled,
    Boolean(siteUrl)
  ].filter(Boolean).length);

  onMount(() => {
    void ensureOpenersLoaded();
  });

  async function launch(action: 'editor' | 'finder' | 'custom' | 'site'): Promise<void> {
    if (busy) return;
    busy = action;
    try {
      if (action === 'editor') await openProjectEditor(path);
      else if (action === 'finder') await openProjectFinder(path);
      else if (action === 'custom') await openProjectCustom(path);
      else if (siteUrl) window.open(siteUrl, '_blank', 'noopener,noreferrer');
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
    style={`--project-opener-count: ${visibleCount}`}
    aria-label={`Open ${projectName}`}
  >
    {#if $openerSettings.config.sidebar.editorEnabled}
      <IconButton
        class="size-6 shrink-0 border border-border bg-card"
        label={`${editorLabel}: ${projectName}`}
        disabled={busy !== null}
        onclick={(event) => { event.stopPropagation(); void launch('editor'); }}
      >
        {#snippet icon()}<CodeXmlIcon size={13} strokeWidth={1.8} />{/snippet}
      </IconButton>
    {/if}
    {#if $openerSettings.config.sidebar.finderEnabled}
      <IconButton
        class="size-6 shrink-0 border border-border bg-card"
        label={`Show ${projectName} in Finder`}
        disabled={busy !== null}
        onclick={(event) => { event.stopPropagation(); void launch('finder'); }}
      >
        {#snippet icon()}<FolderOpenIcon size={13} strokeWidth={1.8} />{/snippet}
      </IconButton>
    {/if}
    {#if $openerSettings.config.sidebar.customEnabled}
      <IconButton
        class="size-6 shrink-0 border border-border bg-card"
        label={`${customLabel}: ${projectName}`}
        disabled={busy !== null}
        onclick={(event) => { event.stopPropagation(); void launch('custom'); }}
      >
        {#snippet icon()}<WorkflowIcon size={13} strokeWidth={1.8} />{/snippet}
      </IconButton>
    {/if}
    {#if siteUrl}
      <IconButton
        class="size-6 shrink-0 border border-border bg-card"
        label={`Open ${siteUrl} in browser`}
        disabled={busy !== null}
        onclick={(event) => { event.stopPropagation(); void launch('site'); }}
      >
        {#snippet icon()}<ExternalLinkIcon size={13} strokeWidth={1.8} />{/snippet}
      </IconButton>
    {/if}
  </div>
{/if}

<style>
  .project-openers {
    display: flex;
    width: 0;
    max-width: 0;
    flex: none;
    align-items: center;
    gap: 2px;
    overflow: hidden;
    opacity: 0;
    pointer-events: none;
    transition: width 110ms ease-out, max-width 110ms ease-out, opacity 90ms ease-out;
  }

  :global(.project-row:hover) .project-openers,
  :global(.project-row:focus-within) .project-openers {
    width: calc(var(--project-opener-count) * 26px);
    max-width: calc(var(--project-opener-count) * 26px);
    opacity: 1;
    pointer-events: auto;
  }

  .collapsed {
    display: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .project-openers { transition: none; }
  }
</style>
