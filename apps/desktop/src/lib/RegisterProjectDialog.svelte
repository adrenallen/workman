<script lang="ts">
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import XIcon from '@lucide/svelte/icons/x';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Input } from '$lib/components/ui/input';

  interface Props {
    path: string;
    defaultTitle: string;
    busy?: boolean;
    error?: string | null;
    onSubmit: (title: string) => void;
    onBack: () => void;
    onClose: () => void;
  }

  let {
    path,
    defaultTitle,
    busy = false,
    error = null,
    onSubmit,
    onBack,
    onClose
  }: Props = $props();

  function initialTitle(): string {
    return defaultTitle;
  }

  let title = $state(initialTitle());
  let titleInput = $state<HTMLInputElement | null>(null);

  function submit(value = title): void {
    if (busy) return;
    onSubmit(value);
  }

  function keepDefault(event: KeyboardEvent): void {
    event.preventDefault();
    submit(defaultTitle);
  }

  function handleTitleKeydown(event: KeyboardEvent): void {
    if (event.key !== 'Enter' || (!event.metaKey && !event.ctrlKey)) return;
    event.preventDefault();
    submit();
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="w-[min(420px,calc(100vw-32px))] max-w-none gap-0 rounded-lg border border-border bg-popover p-0"
    showCloseButton={false}
    aria-describedby="register-project-description"
    onOpenAutoFocus={(event) => {
      event.preventDefault();
      queueMicrotask(() => {
        titleInput?.focus();
        titleInput?.select();
      });
    }}
    onEscapeKeydown={keepDefault}
  >
    <form onsubmit={(event) => { event.preventDefault(); submit(); }}>
      <Dialog.Header class="flex-row items-start justify-between border-b border-border px-4 py-3 text-left">
        <span class="flex min-w-0 items-start gap-3">
          <span class="grid size-8 shrink-0 place-items-center rounded border border-border bg-card text-muted-foreground">
            <FolderPlusIcon size={16} />
          </span>
          <span class="min-w-0">
            <Dialog.Title class="text-base">Name this project</Dialog.Title>
            <Dialog.Description id="register-project-description" class="mt-1 truncate text-sm" title={path}>
              {path}
            </Dialog.Description>
          </span>
        </span>
        <IconButton label="Cancel project registration" disabled={busy} onclick={onClose}>
          {#snippet icon()}<XIcon size={14} />{/snippet}
        </IconButton>
      </Dialog.Header>

      <div class="grid gap-1.5 px-4 py-3">
        <label class="grid gap-1.5">
          <span class="text-sm font-medium">Title</span>
          <Input
            bind:ref={titleInput}
            bind:value={title}
            autocomplete="off"
            aria-label="Project title"
            aria-describedby={error ? 'register-project-title-help register-project-error' : 'register-project-title-help'}
            onkeydown={handleTitleKeydown}
          />
        </label>
        <small id="register-project-title-help" class="text-xs text-muted-foreground">Enter registers this title. Esc registers as “{defaultTitle}”. Cancel discards.</small>
        {#if error}<p id="register-project-error" class="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">{error}</p>{/if}
      </div>

      <Dialog.Footer class="mx-0 mb-0 flex-row flex-wrap justify-between rounded-none rounded-b-lg border-t border-border bg-card px-4 py-3">
        <Button type="button" variant="ghost" disabled={busy} onclick={onBack}><ArrowLeftIcon size={14} />Back</Button>
        <span class="flex items-center gap-2">
          <Button type="button" variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
          <Button type="submit" disabled={busy}>
            {#if busy}<LoaderCircleIcon class="spin" size={14} />{/if}{busy ? 'Registering…' : 'Register project'}
          </Button>
        </span>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<style>
  :global(.spin) { animation: register-project-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes register-project-spin { to { transform: rotate(360deg); } }
</style>
