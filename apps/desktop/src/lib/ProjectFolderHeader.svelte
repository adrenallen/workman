<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import FolderClosedIcon from '@lucide/svelte/icons/folder-closed';
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import MoreHorizontalIcon from '@lucide/svelte/icons/more-horizontal';

  import { Button } from '$lib/components/ui/button';
  import IconButton from './components/ds/IconButton.svelte';
  import ProjectIcon from './ProjectIcon.svelte';
  import TooltipLabel from './components/ds/TooltipLabel.svelte';
  import { sidebarIdentityColorValue } from './projectAppearance';
  import { hotkeyPreferences, matchesHotkeyAction } from './hotkeys';
  import {
    folderDragId,
    type ProjectFolder,
    type ProjectFolderMenuRequest
  } from './projectFolders';
  import {
    reorderItem,
    type ReorderDirection,
    type ReorderDrop
  } from './reorder';

  interface Props {
    folder: ProjectFolder;
    projectCount: number;
    railCollapsed: boolean;
    busy: boolean;
    renaming: boolean;
    renameValue: string;
    onRenameValueChange: (value: string) => void;
    onRenameSubmit: () => void;
    onRenameCancel: () => void;
    onToggle: () => void;
    onDrop: (drop: ReorderDrop) => void;
    onKeyboardMove: (id: number, direction: ReorderDirection) => void;
    onContextMenu: (request: ProjectFolderMenuRequest) => void;
  }

  let {
    folder,
    projectCount,
    railCollapsed,
    busy,
    renaming,
    renameValue,
    onRenameValueChange,
    onRenameSubmit,
    onRenameCancel,
    onToggle,
    onDrop,
    onKeyboardMove,
    onContextMenu
  }: Props = $props();

  let dragId = $derived(folderDragId(folder.id));
  let folderLabel = $derived(
    `${folder.name} · ${projectCount} ${projectCount === 1 ? 'project' : 'projects'}`
  );

  function focusRename(node: HTMLInputElement): void {
    queueMicrotask(() => {
      node.focus();
      node.select();
    });
  }

  function showPointerMenu(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    onContextMenu({
      folder,
      projectCount,
      x: event.clientX,
      y: event.clientY,
      restoreFocus: event.currentTarget instanceof HTMLElement ? event.currentTarget : null
    });
  }

  function showKeyboardMenu(event: KeyboardEvent): void {
    if (!matchesHotkeyAction(event, 'open-context-menu', $hotkeyPreferences)) return;
    const anchor = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
    if (!anchor) return;
    const bounds = anchor.getBoundingClientRect();
    event.preventDefault();
    event.stopPropagation();
    onContextMenu({
      folder,
      projectCount,
      x: Math.min(bounds.left + 18, bounds.right - 8),
      y: Math.min(bounds.bottom - 3, window.innerHeight - 8),
      restoreFocus: anchor
    });
  }
</script>

<article class="folder-row group/folder" data-folder-id={folder.id}>
  {#if renaming && !railCollapsed}
    <form class="rename-form" onsubmit={(event) => { event.preventDefault(); onRenameSubmit(); }}>
      <input
        aria-label="Project folder name"
        value={renameValue}
        use:focusRename
        oninput={(event) => onRenameValueChange(event.currentTarget.value)}
        onkeydown={(event) => { if (event.key === 'Escape') onRenameCancel(); }}
      />
      <Button size="sm" type="submit">Save</Button>
    </form>
  {:else}
    <TooltipLabel label={folderLabel} side={railCollapsed ? 'right' : 'top'}>
      <button
        class="folder-select"
        type="button"
        aria-expanded={!folder.collapsed}
        aria-label={`${folder.collapsed ? 'Expand' : 'Collapse'} ${folderLabel}`}
        use:reorderItem={{
          id: dragId,
          group: 'projects',
          disabled: busy,
          label: folderLabel,
          canDropInside: (sourceId) => sourceId > 0,
          onDrop,
          onKeyboardMove
        }}
        onclick={onToggle}
        oncontextmenu={showPointerMenu}
        onkeydown={showKeyboardMenu}
      >
        {#if !railCollapsed}
          <span class="folder-chevron" aria-hidden="true">
            {#if folder.collapsed}<ChevronRightIcon size={14} />{:else}<ChevronDownIcon size={14} />{/if}
          </span>
        {/if}
        <span
          class="folder-icon"
          style:color={sidebarIdentityColorValue(folder.name_color)}
          aria-hidden="true"
        >
          {#if folder.icon}
            <ProjectIcon icon={folder.icon} color={folder.name_color} size={15} />
          {:else if folder.collapsed}
            <FolderClosedIcon size={15} strokeWidth={1.8} />
          {:else}
            <FolderOpenIcon size={15} strokeWidth={1.8} />
          {/if}
        </span>
        {#if !railCollapsed}
          <strong style:color={sidebarIdentityColorValue(folder.name_color)}>{folder.name}</strong>
          <small>{projectCount}</small>
        {/if}
      </button>
    </TooltipLabel>
    {#if !railCollapsed}
      <IconButton
        class="folder-actions size-7 opacity-0 group-hover/folder:opacity-100 focus-visible:opacity-100"
        label={`Actions for ${folder.name}`}
        onclick={(event) => {
          const bounds = event.currentTarget.getBoundingClientRect();
          onContextMenu({
            folder,
            projectCount,
            x: bounds.right,
            y: bounds.bottom,
            restoreFocus: event.currentTarget
          });
        }}
      >
        {#snippet icon()}<MoreHorizontalIcon size={14} />{/snippet}
      </IconButton>
    {/if}
  {/if}
</article>

<style>
  .folder-row { position: relative; display: flex; min-height: 32px; align-items: center; margin: 2px 0 0; border: 1px solid transparent; border-radius: var(--radius); color: var(--text-soft); }
  .folder-row:hover { background: var(--popover); }
  .folder-row > :global(.tooltip-anchor) { min-width: 0; flex: 1; align-self: stretch; }
  .folder-select { position: relative; display: flex; width: 100%; height: 100%; min-width: 0; align-items: center; gap: var(--space-1); border: 0; padding: 4px 6px; background: transparent; color: inherit; text-align: left; cursor: pointer; }
  .folder-select:focus-visible { outline: 1px solid var(--ring); outline-offset: -2px; background: var(--border); }
  .folder-chevron, .folder-icon { display: grid; width: 16px; height: 20px; flex: none; place-items: center; }
  .folder-icon { color: var(--muted-foreground); }
  strong { min-width: 0; flex: 1; overflow: hidden; color: var(--foreground); font-size: var(--font-size-sm); font-weight: 620; text-overflow: ellipsis; white-space: nowrap; }
  small { color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .rename-form { display: flex; width: 100%; align-items: center; gap: var(--space-1); padding: 3px; }
  .rename-form input { min-width: 0; flex: 1; border: 1px solid var(--border-strong); padding: 4px 5px; background: var(--background); color: var(--foreground); font-size: var(--font-size-sm); }
  .folder-row :global(.folder-actions) { flex: none; }
  :global(.folder-select[data-reorderable='true']) { cursor: grab; }
  :global(.folder-select[data-reorder-dragging='true']) { opacity: 0.42; cursor: grabbing; }
  :global(.folder-select[data-reorder-drop]::after) { position: absolute; z-index: 3; right: 5px; left: 5px; height: 2px; background: var(--ring); content: ''; pointer-events: none; }
  :global(.folder-select[data-reorder-drop='before']::after) { top: -2px; }
  :global(.folder-select[data-reorder-drop='after']::after) { bottom: -2px; }
  :global(.folder-select[data-reorder-drop='inside']) { outline: 1px solid var(--ring); outline-offset: -2px; background: var(--accent); }
  :global(.folder-select[data-reorder-drop='inside']::after) { display: none; }
</style>
