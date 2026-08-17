<script lang="ts">
  import type { CreationDraft } from './creationDrafts';
  import { creationDraftLabel } from './creationDrafts';
  import {
    contextMenuRequest,
    keyboardContextMenuRequest,
    type ContextMenuRequest,
    type ContextMenuTarget
  } from './contextMenu';
  import { projectTreeSelection, type ProjectTreeSelection } from './projectTree';

  interface Props {
    draft: CreationDraft;
    selection: ProjectTreeSelection | null;
    onSelect: (selection: ProjectTreeSelection) => void;
    onContextMenu: (request: ContextMenuRequest) => void;
  }

  let { draft, selection, onSelect, onContextMenu }: Props = $props();
  const label = $derived(creationDraftLabel(draft));
  const draftSelection = $derived(
    projectTreeSelection('draft', draft.id, draft.projectId, label)
  );
  const target = $derived<ContextMenuTarget>({ kind: 'draft', draft, selection: draftSelection });

  function openKeyboardMenu(event: KeyboardEvent): void {
    const request = keyboardContextMenuRequest(event, target);
    if (request) onContextMenu(request);
  }
</script>

<button
  type="button"
  class="draft-row"
  class:selected={selection?.key === draftSelection.key}
  data-tree-row
  data-context-kind="draft"
  data-context-id={draft.id}
  onclick={() => onSelect(draftSelection)}
  oncontextmenu={(event) => onContextMenu(contextMenuRequest(event, target))}
  onkeydown={openKeyboardMenu}
>
  <span class="draft-rail" aria-hidden="true"></span>
  <span class="draft-copy"><strong>{label}</strong><small>{draft.kind} draft</small></span>
  <span class="draft-pill">draft</span>
</button>

<style>
  .draft-row { display: grid; width: 100%; min-height: 30px; grid-template-columns: 3px minmax(0, 1fr) auto; align-items: center; gap: 6px; border: 0; border-radius: 3px; padding: 3px 5px 3px 4px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .draft-row:hover { background: var(--popover); }
  .draft-row.selected { background: var(--accent); color: var(--foreground); box-shadow: inset 2px 0 var(--muted-foreground); }
  .draft-rail { align-self: stretch; border-left: 1px dashed var(--muted-foreground); opacity: .65; }
  .draft-copy { min-width: 0; }
  .draft-copy strong, .draft-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .draft-copy strong { font-size: var(--font-size-sm); font-weight: 570; }
  .draft-copy small { margin-top: 1px; color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .draft-pill { border: 1px dashed var(--border-strong); border-radius: 999px; padding: 1px 5px; color: var(--muted-foreground); font: 620 var(--font-size-xs)/1.25 var(--terminal-font-family); }
</style>
