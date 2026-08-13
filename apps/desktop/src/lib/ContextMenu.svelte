<script lang="ts">
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import BotIcon from '@lucide/svelte/icons/bot';
  import CheckIcon from '@lucide/svelte/icons/check';
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
  import ClipboardPasteIcon from '@lucide/svelte/icons/clipboard-paste';
  import ClipboardIcon from '@lucide/svelte/icons/clipboard';
  import CopyIcon from '@lucide/svelte/icons/copy';
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
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlayIcon from '@lucide/svelte/icons/play';
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
    type ContextMenuItem
  } from './contextMenu';

  interface Props {
    x: number;
    y: number;
    title: string;
    subtitle: string;
    items: ContextMenuItem[];
    onSelect: (id: ContextActionId) => void;
    onClose: () => void;
  }

  let { x, y, title, subtitle, items, onSelect, onClose }: Props = $props();

  function choose(item: ContextMenuItem): void {
    if (item.disabled) return;
    onSelect(item.id);
  }

  const ICONS: Record<ContextActionIcon, Component> = {
    archive: ArchiveIcon,
    bot: BotIcon,
    check: CheckIcon,
    'circle-check': CircleCheckIcon,
    'clipboard-paste': ClipboardPasteIcon,
    clipboard: ClipboardIcon,
    copy: CopyIcon,
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
    pencil: PencilIcon,
    play: PlayIcon,
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
  <DropdownMenu.Content class="w-64 rounded-md border border-border p-1 shadow-xl" sideOffset={0}>
    <DropdownMenu.Label class="grid gap-0.5 px-2 py-1.5">
      <span class="font-mono text-xs tracking-wide text-muted-foreground">{subtitle}</span>
      <strong class="truncate text-sm font-semibold text-popover-foreground">{title}</strong>
    </DropdownMenu.Label>
    <DropdownMenu.Separator />
    {#each items as item (item.id)}
      {#if item.separatorBefore}<DropdownMenu.Separator />{/if}
      {@const Icon = ICONS[contextActionIcon(item.id)]}
      <DropdownMenu.Item
        variant={item.destructive ? 'destructive' : 'default'}
        disabled={item.disabled}
        onclick={() => choose(item)}
        class="min-h-8 gap-2 px-2 py-1.5"
      >
        {#if item.pullRequestState}
          <PullRequestStateIcon state={item.pullRequestState} size={16} strokeWidth={1.9} />
        {:else}
          <Icon class="size-4" aria-hidden="true" />
        {/if}
        <span class="grid min-w-0 flex-1">
          <span class="truncate text-sm">{item.label}</span>
          {#if item.detail}<span class="truncate font-mono text-xs text-muted-foreground">{item.detail}</span>{/if}
        </span>
        {#if item.shortcut}<DropdownMenu.Shortcut>{item.shortcut}</DropdownMenu.Shortcut>{/if}
      </DropdownMenu.Item>
    {/each}
  </DropdownMenu.Content>
</DropdownMenu.Root>

<style>
  .menu-anchor { position: fixed; z-index: 80; width: 1px; height: 1px; }
</style>
