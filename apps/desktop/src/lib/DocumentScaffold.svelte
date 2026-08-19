<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import Maximize2Icon from '@lucide/svelte/icons/maximize-2';
  import Minimize2Icon from '@lucide/svelte/icons/minimize-2';
  import type { Snippet } from 'svelte';

  import IconButton from '$lib/components/ds/IconButton.svelte';

  interface Props {
    ariaLabel: string;
    breadcrumbRoot: string;
    breadcrumbCurrent: string;
    reference?: string;
    previousDisabled?: boolean;
    nextDisabled?: boolean;
    onBack?: () => void;
    onPrevious?: () => void;
    onNext?: () => void;
    onCopyReference?: () => void;
    actions?: Snippet;
    rail?: Snippet;
    children: Snippet;
  }

  let {
    ariaLabel,
    breadcrumbRoot,
    breadcrumbCurrent,
    reference,
    previousDisabled = true,
    nextDisabled = true,
    onBack,
    onPrevious,
    onNext,
    onCopyReference,
    actions,
    rail,
    children
  }: Props = $props();

  let expanded = $state(false);

  function handleKeydown(event: KeyboardEvent): void {
    if (!expanded || event.key !== 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    expanded = false;
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<section class="document-shell" class:expanded aria-label={ariaLabel}>
  <header class="document-bar">
    <nav aria-label="Breadcrumb">
      {#if onBack}
        <button type="button" onclick={onBack}>{breadcrumbRoot}</button>
      {:else}
        <span>{breadcrumbRoot}</span>
      {/if}
      <ChevronRightIcon size={13} strokeWidth={1.8} aria-hidden="true" />
      <strong title={breadcrumbCurrent}>{breadcrumbCurrent}</strong>
    </nav>

    <div class="document-actions">
      {#if onPrevious}
        <IconButton label="Previous item" shortcut="⌘←" disabled={previousDisabled} onclick={onPrevious}>
          {#snippet icon()}<ChevronLeftIcon size={15} strokeWidth={1.8} />{/snippet}
        </IconButton>
      {/if}
      {#if onNext}
        <IconButton label="Next item" shortcut="⌘→" disabled={nextDisabled} onclick={onNext}>
          {#snippet icon()}<ChevronRightIcon size={15} strokeWidth={1.8} />{/snippet}
        </IconButton>
      {/if}
      {#if reference}
        <button class="reference" type="button" title={`Copy ${reference}`} onclick={onCopyReference}>
          <CopyIcon size={13} strokeWidth={1.8} aria-hidden="true" />
          {reference}
        </button>
      {/if}
      <IconButton
        label={expanded ? 'Exit full screen' : 'Expand document'}
        shortcut="Esc"
        aria-pressed={expanded}
        onclick={() => (expanded = !expanded)}
      >
        {#snippet icon()}
          {#if expanded}<Minimize2Icon size={14} strokeWidth={1.8} />{:else}<Maximize2Icon size={14} strokeWidth={1.8} />{/if}
        {/snippet}
      </IconButton>
      {#if actions}{@render actions()}{/if}
    </div>
  </header>

  <div class="document-viewport">
    <div class:with-rail={rail} class="document-layout">
      <main class="document-column">{@render children()}</main>
      {#if rail}<aside class="document-rail">{@render rail()}</aside>{/if}
    </div>
  </div>
</section>

<style>
  .document-shell { container-type: inline-size; display: grid; width: 100%; height: 100%; min-width: 0; min-height: 0; grid-template-rows: auto minmax(0, 1fr); background: var(--background); }
  .document-shell.expanded { position: fixed; z-index: 70; inset: 0; }
  .document-bar { display: flex; min-width: 0; min-height: 38px; align-items: center; justify-content: space-between; gap: var(--space-2); border-bottom: 1px solid var(--border); padding: 4px 8px 4px 12px; background: var(--card); }
  nav { display: flex; min-width: 0; align-items: center; gap: 5px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  nav button { overflow: hidden; max-width: 180px; border: 0; padding: 2px 0; background: transparent; color: var(--muted-foreground); text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  nav button:hover { color: var(--foreground); }
  nav strong { overflow: hidden; max-width: min(42vw, 420px); color: var(--text-soft); font-weight: 590; text-overflow: ellipsis; white-space: nowrap; }
  .document-actions { display: flex; flex: none; align-items: center; gap: 2px; }
  .reference { display: inline-flex; min-height: 28px; align-items: center; gap: 5px; border: 1px solid var(--border); border-radius: var(--radius); padding: 0 7px; background: transparent; color: var(--muted-foreground); font: 600 var(--font-size-xs)/1 var(--terminal-font-family); cursor: pointer; }
  .reference:hover { border-color: var(--input); color: var(--foreground); }
  .document-viewport { container-type: size; min-height: 0; overflow: auto; overscroll-behavior: contain; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .document-layout { display: grid; width: 100%; min-height: 100%; grid-template-columns: minmax(0, 780px); justify-content: center; }
  .document-layout.with-rail { grid-template-columns: minmax(0, 780px) minmax(168px, 220px); gap: 28px; }
  .document-column { min-width: 0; min-height: 100%; padding: 28px 28px 56px; }
  .document-rail { min-width: 0; padding: 30px 18px 48px 0; }

  @container (max-width: 880px) {
    .document-layout.with-rail { grid-template-columns: minmax(0, 720px); }
    .document-rail { display: none; }
  }

  @container (max-width: 620px) {
    .document-bar { padding-left: 8px; }
    nav strong { max-width: 30vw; }
    .document-column { padding: 20px 14px 56px; }
    .reference :global(svg) { display: none; }
  }
</style>
