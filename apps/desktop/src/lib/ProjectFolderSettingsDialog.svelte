<script lang="ts">
  import FolderCogIcon from '@lucide/svelte/icons/folder-cog';
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import XIcon from '@lucide/svelte/icons/x';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Input } from '$lib/components/ui/input';
  import LucideIconLibrary from './LucideIconLibrary.svelte';
  import NameColorPicker from './NameColorPicker.svelte';
  import {
    normalizeProjectIcon,
    normalizeProjectIconColor,
    normalizeSidebarIdentityColor,
    sidebarIdentityColorValue,
    type SidebarIdentityColor
  } from './projectAppearance';
  import type {
    ProjectFolder,
    ProjectFolderSettingsInput
  } from './projectFolders';

  interface Props {
    folder: ProjectFolder;
    busy?: boolean;
    onSave: (settings: ProjectFolderSettingsInput) => void;
    onClose: () => void;
  }

  let { folder, busy = false, onSave, onClose }: Props = $props();

  function initialName(): string {
    return folder.name;
  }

  function initialIcon(): string | null {
    return normalizeProjectIcon(folder.icon);
  }

  function initialNameColor(): SidebarIdentityColor | null {
    return normalizeSidebarIdentityColor(folder.name_color);
  }

  let name = $state(initialName());
  let icon = $state<string | null>(initialIcon());
  let nameColor = $state<SidebarIdentityColor | null>(initialNameColor());
  let canSave = $derived(!busy && name.trim().length > 0);
  let previewColor = $derived(sidebarIdentityColorValue(nameColor));

  function submit(): void {
    if (!canSave) return;
    onSave({ name: name.trim(), icon, nameColor });
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="w-[min(720px,calc(100vw-32px))] max-w-none gap-0 overflow-hidden rounded-lg border border-border bg-popover p-0"
    showCloseButton={false}
    aria-describedby="folder-settings-description"
  >
    <form class="modal-form" onsubmit={(event) => { event.preventDefault(); submit(); }}>
      <Dialog.Header class="flex-row items-start justify-between border-b border-border px-4 py-3 text-left">
        <span class="flex min-w-0 items-start gap-3">
          <span class="grid size-8 shrink-0 place-items-center rounded border border-border bg-card text-muted-foreground">
            <FolderCogIcon size={16} />
          </span>
          <span class="min-w-0">
            <Dialog.Title class="truncate text-base">Folder settings</Dialog.Title>
            <Dialog.Description id="folder-settings-description" class="mt-1 text-sm">
              Choose how this folder appears in the project sidebar.
            </Dialog.Description>
          </span>
        </span>
        <IconButton label="Close folder settings" disabled={busy} onclick={onClose}>
          {#snippet icon()}<XIcon size={14} />{/snippet}
        </IconButton>
      </Dialog.Header>

      <div class="settings-body">
        <label class="name-field">
          <span>Folder name</span>
          <Input bind:value={name} autocomplete="off" aria-label="Folder name" />
        </label>

        <fieldset>
          <legend>Folder name color</legend>
          <NameColorPicker value={nameColor} disabled={busy} onChange={(value) => (nameColor = value)} />
        </fieldset>

        <fieldset>
          <legend>Folder icon</legend>
          <button
            class="default-icon"
            class:selected={icon === null}
            type="button"
            aria-pressed={icon === null}
            disabled={busy}
            onclick={() => (icon = null)}
          >
            <span class="default-preview" style:color={previewColor}>
              <FolderOpenIcon size={20} strokeWidth={1.8} />
            </span>
            <span>
              <strong>Folder</strong>
              <small>Use the standard open and closed folder icons</small>
            </span>
            <span class="default-tag">default</span>
          </button>
          <LucideIconLibrary
            value={icon}
            color={normalizeProjectIconColor(nameColor)}
            disabled={busy}
            title="Choose another icon"
            ariaLabel="Lucide folder icons"
            onChange={(value) => (icon = value)}
          />
        </fieldset>
      </div>

      <Dialog.Footer class="mx-0 mb-0 flex-row flex-wrap justify-end rounded-none rounded-b-lg border-t border-border bg-card px-4 py-3">
        <Button type="button" variant="outline" disabled={busy} onclick={onClose}>Cancel</Button>
        <Button type="submit" disabled={!canSave}>{busy ? 'Saving…' : 'Save changes'}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<style>
  .modal-form { display: grid; min-height: 0; max-height: calc(100dvh - 2rem); grid-template-rows: auto minmax(0, 1fr) auto; }
  .settings-body { display: grid; min-height: 0; align-content: start; gap: 14px; overflow-y: auto; overscroll-behavior: contain; padding: 14px 16px 16px; }
  .name-field { display: grid; gap: 6px; }
  .name-field > span, legend { color: var(--text-soft); font-size: var(--font-size-xs); font-weight: 700; letter-spacing: 0.045em; text-transform: uppercase; }
  fieldset { display: grid; min-width: 0; gap: 6px; margin: 0; border: 0; padding: 0; }
  legend { margin-bottom: 0; padding: 0; }
  .default-icon { display: grid; min-width: 0; min-height: 52px; grid-template-columns: 34px minmax(0, 1fr) auto; align-items: center; gap: 8px; border: 1px solid var(--border); border-radius: var(--radius); padding: 7px 8px; background: var(--card); color: var(--text-soft); text-align: left; cursor: pointer; }
  .default-icon:hover:not(:disabled) { border-color: var(--border-strong); background: var(--accent); }
  .default-icon.selected { border-color: var(--ring); background: color-mix(in srgb, var(--ring) 9%, var(--card)); color: var(--foreground); }
  .default-preview { display: grid; width: 32px; height: 32px; place-items: center; border: 1px solid var(--border); border-radius: 5px; background: var(--background); color: var(--muted-foreground); }
  .default-icon > span:nth-child(2) { min-width: 0; }
  .default-icon strong, .default-icon small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .default-icon strong { font-size: var(--font-size-sm); font-weight: 650; }
  .default-icon small { margin-top: 2px; color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .default-tag { border: 1px solid var(--border); border-radius: 999px; padding: 2px 5px; color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  button:disabled { cursor: default; opacity: 0.45; }
</style>
