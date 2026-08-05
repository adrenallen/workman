<script lang="ts">
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import CheckIcon from '@lucide/svelte/icons/check';
  import ClipboardIcon from '@lucide/svelte/icons/clipboard';
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
  import FileCodeIcon from '@lucide/svelte/icons/file-code';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import GitBranchPlusIcon from '@lucide/svelte/icons/git-branch-plus';
  import GitForkIcon from '@lucide/svelte/icons/git-fork';
  import ImportIcon from '@lucide/svelte/icons/import';
  import MessageSquareIcon from '@lucide/svelte/icons/message-square';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlayIcon from '@lucide/svelte/icons/play';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import SquareIcon from '@lucide/svelte/icons/square';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';

  import type { ContextActionId, ContextMenuItem } from './contextMenu';

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

  function actionIcon(id: ContextActionId) {
    if (id.startsWith('copy-')) return ClipboardIcon;
    if (id === 'start' || id === 'start-all-commands' || id === 'reopen-todo') return PlayIcon;
    if (id === 'stop' || id === 'stop-all-commands') return SquareIcon;
    if (id === 'restart' || id === 'refresh-worktrees' || id === 'refresh-pull-request') return RefreshCwIcon;
    if (id === 'new-worktree') return GitBranchPlusIcon;
    if (id === 'adopt-worktree') return ImportIcon;
    if (id === 'fork-worktree') return GitForkIcon;
    if (id === 'rename') return PencilIcon;
    if (id === 'complete-todo' || id === 'select') return CheckIcon;
    if (id === 'send-prompt') return MessageSquareIcon;
    if (id === 'view-parent') return GitBranchIcon;
    if (id === 'reveal-config') return FileCodeIcon;
    if (id === 'open-in-editor' || id === 'open-custom' || id === 'open-pull-request' || id === 'open-herd-site') return ExternalLinkIcon;
    if (id === 'open-in-finder') return FolderIcon;
    if (id === 'archive-scratchpad') return ArchiveIcon;
    return Trash2Icon;
  }
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
      {@const Icon = actionIcon(item.id)}
      <DropdownMenu.Item
        variant={item.destructive ? 'destructive' : 'default'}
        disabled={item.disabled}
        onclick={() => choose(item)}
        class="min-h-8 gap-2 px-2 py-1.5"
      >
        <Icon class="size-4" aria-hidden="true" />
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
