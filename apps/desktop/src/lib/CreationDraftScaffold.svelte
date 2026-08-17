<script lang="ts">
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import type { Snippet } from 'svelte';

  import { Button } from '$lib/components/ui/button';

  interface Props {
    projectName: string;
    kindLabel: string;
    title: string;
    createLabel: string;
    busy?: boolean;
    canCreate?: boolean;
    onCreate: () => void;
    onDiscard: () => void;
    secondaryAction?: Snippet;
    children: Snippet;
  }

  let {
    projectName,
    kindLabel,
    title,
    createLabel,
    busy = false,
    canCreate = true,
    onCreate,
    onDiscard,
    secondaryAction,
    children
  }: Props = $props();
</script>

<form
  class="draft-shell"
  data-creation-draft={kindLabel.toLowerCase()}
  onsubmit={(event) => {
    event.preventDefault();
    if (!busy && canCreate) onCreate();
  }}
>
  <header class="document-bar">
    <nav aria-label="Breadcrumb">
      <span>{projectName}</span>
      <ChevronRightIcon size={13} strokeWidth={1.8} aria-hidden="true" />
      <strong>{title}</strong>
    </nav>
    <span class="draft-status">Draft</span>
  </header>

  <div class="draft-viewport">
    <main class="draft-column">
      <div class="draft-heading">
        <span>{kindLabel} draft</span>
        <h1>{title}</h1>
        <p>This draft stays in the project tree until you create or discard it.</p>
      </div>
      {@render children()}
    </main>
  </div>

  <footer>
    <div>{#if secondaryAction}{@render secondaryAction()}{/if}</div>
    <div class="footer-actions">
      <Button type="button" variant="ghost" disabled={busy} onclick={onDiscard}>Discard</Button>
      <Button type="submit" disabled={busy || !canCreate}>
        {busy ? 'Creating…' : createLabel}
      </Button>
    </div>
  </footer>
</form>

<style>
  .draft-shell { display: grid; width: 100%; height: 100%; min-width: 0; min-height: 0; grid-template-rows: auto minmax(0, 1fr) auto; background: var(--background); color: var(--foreground); }
  .document-bar { display: flex; min-width: 0; min-height: 38px; align-items: center; justify-content: space-between; gap: var(--space-2); border-bottom: 1px solid var(--border); padding: 4px 10px 4px 12px; background: var(--card); }
  nav { display: flex; min-width: 0; align-items: center; gap: 5px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  nav span, nav strong { overflow: hidden; max-width: min(40vw, 360px); text-overflow: ellipsis; white-space: nowrap; }
  nav strong { color: var(--text-soft); font-weight: 590; }
  .draft-status { flex: none; border: 1px dashed var(--border-strong); border-radius: 999px; padding: 2px 7px; color: var(--muted-foreground); font: 650 var(--font-size-xs)/1.2 var(--terminal-font-family); text-transform: lowercase; }
  .draft-viewport { min-height: 0; overflow-y: auto; overscroll-behavior: contain; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .draft-column { width: min(100%, 780px); min-height: 100%; margin: 0 auto; padding: 26px 28px 44px; }
  .draft-heading { margin-bottom: 20px; padding-bottom: 16px; border-bottom: 1px solid var(--border); }
  .draft-heading > span { color: var(--muted-foreground); font: 680 var(--font-size-xs) var(--terminal-font-family); letter-spacing: .055em; text-transform: uppercase; }
  h1 { margin: 4px 0 0; color: var(--foreground); font-size: 21px; font-weight: 620; letter-spacing: -.015em; }
  .draft-heading p { margin: 6px 0 0; color: var(--muted-foreground); font-size: var(--font-size-sm); line-height: 1.5; }
  footer { display: flex; min-height: 52px; align-items: center; justify-content: space-between; gap: var(--space-2); border-top: 1px solid var(--border); padding: 8px 12px; background: var(--card); }
  .footer-actions { display: flex; align-items: center; gap: 6px; }

  @media (max-width: 620px) {
    .draft-column { padding: 20px 14px 36px; }
  }
</style>
