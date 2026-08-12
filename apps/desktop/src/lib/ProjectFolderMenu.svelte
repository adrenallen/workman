<script lang="ts">
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import type { ProjectFolderMenuRequest } from './projectFolders';

  interface Props {
    request: ProjectFolderMenuRequest;
    onRename: () => void;
    onDelete: () => void;
    onClose: () => void;
  }

  let { request, onRename, onDelete, onClose }: Props = $props();
</script>

<DropdownMenu.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      <span
        {...props}
        class="menu-anchor"
        style:left={`${request.x}px`}
        style:top={`${request.y}px`}
        aria-label={`${request.folder.name} folder actions`}
      ></span>
    {/snippet}
  </DropdownMenu.Trigger>
  <DropdownMenu.Content class="w-64 rounded-md border border-border p-1 shadow-xl" sideOffset={0}>
    <DropdownMenu.Label class="grid gap-0.5 px-2 py-1.5">
      <span class="font-mono text-xs tracking-wide text-muted-foreground">
        FOLDER · {request.projectCount} {request.projectCount === 1 ? 'PROJECT' : 'PROJECTS'}
      </span>
      <strong class="truncate text-sm font-semibold text-popover-foreground">
        {request.folder.name}
      </strong>
    </DropdownMenu.Label>
    <DropdownMenu.Separator />
    <DropdownMenu.Item class="min-h-8 gap-2 px-2 py-1.5" onclick={onRename}>
      <PencilIcon class="size-4" aria-hidden="true" />
      <span class="text-sm">Rename folder</span>
    </DropdownMenu.Item>
    <DropdownMenu.Separator />
    <DropdownMenu.Item
      variant="destructive"
      class="min-h-8 gap-2 px-2 py-1.5"
      onclick={onDelete}
    >
      <Trash2Icon class="size-4" aria-hidden="true" />
      <span class="grid min-w-0 flex-1">
        <span class="truncate text-sm">Delete folder…</span>
        <span class="truncate font-mono text-xs text-muted-foreground">Projects return to top level</span>
      </span>
    </DropdownMenu.Item>
  </DropdownMenu.Content>
</DropdownMenu.Root>

<style>
  .menu-anchor { position: fixed; z-index: 80; width: 1px; height: 1px; }
</style>
