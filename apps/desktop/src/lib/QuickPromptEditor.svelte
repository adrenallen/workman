<script lang="ts">
  import XIcon from '@lucide/svelte/icons/x';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Input } from '$lib/components/ui/input';
  import { Textarea } from '$lib/components/ui/textarea';
  import IconButton from '$lib/components/ds/IconButton.svelte';
  import type { QuickPrompt, QuickPromptsStore } from './quickPrompts';

  interface Props {
    store: QuickPromptsStore;
    prompt?: QuickPrompt | null;
    onSaved?: (prompt: QuickPrompt) => void;
    onClose: () => void;
    onError: (message: string) => void;
  }

  let { store, prompt = null, onSaved, onClose, onError }: Props = $props();
  let name = $state('');
  let body = $state('');
  let saving = $state(false);

  $effect(() => {
    name = prompt?.name ?? '';
    body = prompt?.body ?? '';
  });

  async function save(): Promise<void> {
    const normalizedName = name.trim();
    if (!normalizedName || !body.trim() || saving) return;
    saving = true;
    try {
      const saved = await store.save({
        ...(prompt ? { id: prompt.id } : {}),
        name: normalizedName,
        body
      });
      onSaved?.(saved);
      onClose();
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      saving = false;
    }
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !saving) onClose(); }}>
  <Dialog.Content
    class="w-[min(620px,calc(100vw-24px))] !max-w-none gap-0 overflow-hidden rounded-lg border border-border bg-popover p-0 shadow-2xl"
    showCloseButton={false}
    aria-labelledby="quick-prompt-editor-title"
  >
    <form
      class="grid gap-0"
      onsubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <header class="flex min-h-12 items-center justify-between gap-3 border-b border-border px-3 py-2">
        <div class="min-w-0">
          <span class="block text-xs font-semibold uppercase tracking-wider text-muted-foreground">Active profile</span>
          <h2 id="quick-prompt-editor-title" class="mt-0.5 truncate text-sm font-semibold text-foreground">
            {prompt ? `Edit ${prompt.name}` : 'New quick prompt'}
          </h2>
        </div>
        <IconButton label="Close quick prompt editor" disabled={saving} onclick={onClose}>
          {#snippet icon()}<XIcon size={15} strokeWidth={1.8} />{/snippet}
        </IconButton>
      </header>

      <div class="grid gap-3 p-3">
        <label class="grid gap-1.5 text-xs font-semibold text-foreground">
          <span>Name</span>
          <Input bind:value={name} maxlength={120} placeholder="Review for regressions" autofocus />
        </label>
        <label class="grid gap-1.5 text-xs font-semibold text-foreground">
          <span>Prompt</span>
          <Textarea
            bind:value={body}
            class="min-h-40 resize-y font-mono text-xs leading-relaxed"
            placeholder="Review this change for regressions, missing tests, and unsafe edge cases."
            spellcheck={true}
          />
        </label>
      </div>

      <footer class="flex items-center justify-between gap-3 border-t border-border px-3 py-2">
        <p class="m-0 text-xs text-muted-foreground">Saved to the active workspace profile.</p>
        <div class="flex gap-2">
          <Button type="button" variant="outline" size="sm" disabled={saving} onclick={onClose}>Cancel</Button>
          <Button type="submit" size="sm" disabled={saving || !name.trim() || !body.trim()}>
            {saving ? 'Saving…' : 'Save prompt'}
          </Button>
        </div>
      </footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
