<script lang="ts">
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import BotIcon from '@lucide/svelte/icons/bot';
  import CheckIcon from '@lucide/svelte/icons/check';
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
  import ClipboardPasteIcon from '@lucide/svelte/icons/clipboard-paste';
  import ClipboardIcon from '@lucide/svelte/icons/clipboard';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
  import FileCodeIcon from '@lucide/svelte/icons/file-code';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import GitBranchPlusIcon from '@lucide/svelte/icons/git-branch-plus';
  import GitForkIcon from '@lucide/svelte/icons/git-fork';
  import ImportIcon from '@lucide/svelte/icons/import';
  import LinkIcon from '@lucide/svelte/icons/link';
  import MessageSquareIcon from '@lucide/svelte/icons/message-square';
  import NotebookTextIcon from '@lucide/svelte/icons/notebook-text';
  import OctagonXIcon from '@lucide/svelte/icons/octagon-x';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlayIcon from '@lucide/svelte/icons/play';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import SettingsIcon from '@lucide/svelte/icons/settings';
  import SquareIcon from '@lucide/svelte/icons/square';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import type { Component } from 'svelte';

  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import PullRequestStateIcon from './PullRequestStateIcon.svelte';

  import {
    contextActionIcon,
    type ContextActionIcon,
    type ContextActionId,
    type ContextMenuEntry,
    type ContextMenuItem
  } from './contextMenu';

  interface Props {
    x: number;
    y: number;
    title: string;
    subtitle: string;
    items: ContextMenuEntry[];
    onSelect: (id: ContextActionId) => void;
    onClose: () => void;
  }

  let { x, y, title, subtitle, items, onSelect, onClose }: Props = $props();

  function choose(item: ContextMenuItem): void {
    if (item.disabled) return;
    onSelect(item.id);
  }

  function isSubmenu(item: ContextMenuEntry): item is Extract<ContextMenuEntry, { kind: 'submenu' }> {
    return 'kind' in item && item.kind === 'submenu';
  }

  function entryKey(item: ContextMenuEntry): string {
    return isSubmenu(item) ? `submenu-${item.label}` : item.id;
  }

  function itemTone(item: ContextMenuItem): NonNullable<ContextMenuItem['tone']> {
    return item.tone ?? (item.destructive ? 'danger' : 'default');
  }

  const ICONS: Record<ContextActionIcon, Component> = {
    archive: ArchiveIcon,
    bot: BotIcon,
    check: CheckIcon,
    'circle-check': CircleCheckIcon,
    'clipboard-paste': ClipboardPasteIcon,
    clipboard: ClipboardIcon,
    copy: CopyIcon,
    ellipsis: EllipsisIcon,
    'external-link': ExternalLinkIcon,
    'file-code': FileCodeIcon,
    folder: FolderIcon,
    'git-branch': GitBranchIcon,
    'git-branch-plus': GitBranchPlusIcon,
    'git-fork': GitForkIcon,
    import: ImportIcon,
    link: LinkIcon,
    'message-square': MessageSquareIcon,
    'notebook-text': NotebookTextIcon,
    'octagon-x': OctagonXIcon,
    pencil: PencilIcon,
    play: PlayIcon,
    plus: PlusIcon,
    'refresh-cw': RefreshCwIcon,
    settings: SettingsIcon,
    square: SquareIcon,
    'square-terminal': SquareTerminalIcon,
    'trash-2': Trash2Icon
  };
</script>

<DropdownMenu.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      <span
        {...props}
        class="menu-anchor"
        style:left={`${x}px`}
        style:top={`${y}px`}
        aria-label={`${title} actions`}
      ></span>
    {/snippet}
  </DropdownMenu.Trigger>
  <DropdownMenu.Content
    class="w-72 rounded-md border border-border p-1 shadow-xl"
    sideOffset={0}
    avoidCollisions={true}
    collisionPadding={8}
  >
    <DropdownMenu.Label class="grid gap-0.5 px-2 py-1.5">
      <span class="font-mono text-xs tracking-wide text-muted-foreground">{subtitle}</span>
      <strong class="truncate text-sm font-semibold text-popover-foreground">{title}</strong>
    </DropdownMenu.Label>
    <DropdownMenu.Separator />
    {#each items as item (entryKey(item))}
      {#if item.separatorBefore}<DropdownMenu.Separator />{/if}
      {#if isSubmenu(item)}
        {@const SubmenuIcon = ICONS[item.icon]}
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger class="min-h-8 gap-2 px-2 py-1.5">
            <span class="action-row" data-tone="default">
              <span class="action-icon"><SubmenuIcon class="size-4" aria-hidden="true" /></span>
              <span class="grid min-w-0 flex-1">
                <span class="action-label truncate text-sm">{item.label}</span>
                {#if item.detail}<span class="truncate font-mono text-xs text-muted-foreground">{item.detail}</span>{/if}
              </span>
            </span>
          </DropdownMenu.SubTrigger>
          <DropdownMenu.SubContent class="w-72 rounded-md border border-border p-1 shadow-xl" sideOffset={4}>
            {#each item.items as child (child.id)}
              {#if child.separatorBefore}<DropdownMenu.Separator />{/if}
              {@const ChildIcon = ICONS[contextActionIcon(child.id)]}
              <DropdownMenu.Item
                variant={child.destructive ? 'destructive' : 'default'}
                disabled={child.disabled}
                onclick={() => choose(child)}
                class="min-h-8 gap-2 px-2 py-1.5"
              >
                <span class="action-row" data-tone={itemTone(child)}>
                  <span class="action-icon"><ChildIcon class="size-4" aria-hidden="true" /></span>
                  <span class="grid min-w-0 flex-1">
                    <span class="action-label truncate text-sm">{child.label}</span>
                    {#if child.detail}<span class="truncate font-mono text-xs text-muted-foreground">{child.detail}</span>{/if}
                  </span>
                  {#if child.shortcut}<DropdownMenu.Shortcut>{child.shortcut}</DropdownMenu.Shortcut>{/if}
                </span>
              </DropdownMenu.Item>
            {/each}
          </DropdownMenu.SubContent>
        </DropdownMenu.Sub>
      {:else}
        {@const Icon = ICONS[contextActionIcon(item.id)]}
        <DropdownMenu.Item
          variant={item.destructive ? 'destructive' : 'default'}
          disabled={item.disabled}
          onclick={() => choose(item)}
          class="min-h-8 gap-2 px-2 py-1.5"
        >
          <span class="action-row" data-tone={itemTone(item)}>
            <span class="action-icon">
              {#if item.pullRequestState}
                <PullRequestStateIcon state={item.pullRequestState} size={16} strokeWidth={1.9} />
              {:else}
                <Icon class="size-4" aria-hidden="true" />
              {/if}
            </span>
            <span class="grid min-w-0 flex-1">
              <span class="action-label truncate text-sm">{item.label}</span>
              {#if item.detail}<span class="truncate font-mono text-xs text-muted-foreground">{item.detail}</span>{/if}
            </span>
            {#if item.shortcut}<DropdownMenu.Shortcut>{item.shortcut}</DropdownMenu.Shortcut>{/if}
          </span>
        </DropdownMenu.Item>
      {/if}
    {/each}
  </DropdownMenu.Content>
</DropdownMenu.Root>

<style>
  .menu-anchor { position: fixed; z-index: 80; width: 1px; height: 1px; }
  .action-row { display: contents; }
  .action-icon {
    display: grid;
    width: 1.45rem;
    height: 1.45rem;
    flex: none;
    place-items: center;
    border: 1px solid transparent;
    border-radius: 0.35rem;
    color: var(--muted-foreground);
  }
  .action-row[data-tone='positive'] .action-icon {
    border-color: color-mix(in srgb, var(--success) 30%, transparent);
    background: color-mix(in srgb, var(--success) 12%, transparent);
    color: var(--success);
  }
  .action-row[data-tone='warning'] .action-icon {
    border-color: color-mix(in srgb, var(--warning) 32%, transparent);
    background: color-mix(in srgb, var(--warning) 12%, transparent);
    color: var(--warning);
  }
  .action-row[data-tone='info'] .action-icon {
    border-color: color-mix(in srgb, var(--information) 30%, transparent);
    background: color-mix(in srgb, var(--information) 12%, transparent);
    color: var(--information);
  }
  .action-row[data-tone='danger'] .action-icon {
    border-color: color-mix(in srgb, var(--destructive) 30%, transparent);
    background: color-mix(in srgb, var(--destructive) 10%, transparent);
    color: var(--destructive);
  }
  .action-row:not([data-tone='default']) .action-label { font-weight: 600; }
</style>
