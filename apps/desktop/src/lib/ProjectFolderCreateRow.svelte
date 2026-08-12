<script lang="ts">
  import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
  import XIcon from '@lucide/svelte/icons/x';

  import { Button } from '$lib/components/ui/button';
  import IconButton from './components/ds/IconButton.svelte';

  interface Props {
    value: string;
    busy: boolean;
    onValueChange: (value: string) => void;
    onSubmit: () => void;
    onCancel: () => void;
  }

  let { value, busy, onValueChange, onSubmit, onCancel }: Props = $props();

  function focusInput(node: HTMLInputElement): void {
    queueMicrotask(() => node.focus());
  }
</script>

<form class="folder-create" onsubmit={(event) => { event.preventDefault(); onSubmit(); }}>
  <FolderPlusIcon size={15} strokeWidth={1.8} aria-hidden="true" />
  <input
    aria-label="New project folder name"
    placeholder="Folder name"
    {value}
    use:focusInput
    disabled={busy}
    oninput={(event) => onValueChange(event.currentTarget.value)}
    onkeydown={(event) => { if (event.key === 'Escape') onCancel(); }}
  />
  <Button size="sm" type="submit" disabled={busy || value.trim().length === 0}>Create</Button>
  <IconButton class="size-7" label="Cancel new folder" disabled={busy} onclick={onCancel}>
    {#snippet icon()}<XIcon size={14} />{/snippet}
  </IconButton>
</form>

<style>
  .folder-create { display: flex; min-height: 36px; align-items: center; gap: var(--space-1); margin: 2px 0; border: 1px solid var(--border-strong); border-radius: var(--radius); padding: 3px; background: var(--popover); color: var(--muted-foreground); }
  input { min-width: 0; flex: 1; border: 0; border-bottom: 1px solid var(--border-strong); padding: 4px 2px; outline: 0; background: transparent; color: var(--foreground); font-size: var(--font-size-sm); }
  input:focus { border-bottom-color: var(--ring); }
</style>
