<script lang="ts">
  import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
  import XIcon from '@lucide/svelte/icons/x';
  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import type { TrustFields, TrustReview } from './daemon';

  interface Props {
    review: TrustReview;
    busy: boolean;
    onApprove: () => void;
    onClose: () => void;
  }

  let { review, busy, onApprove, onClose }: Props = $props();
  let cancelButton: HTMLButtonElement | null = $state(null);
  const labels: Record<keyof TrustFields, string> = {
    command: 'Command', working_dir: 'Working directory', env: 'Environment',
    auto_start: 'Start automatically', auto_restart: 'Restart automatically',
    restart_when_changed: 'Restart when files change'
  };
  function formatValue(value: unknown): string {
    if (typeof value === 'boolean') return value ? 'On' : 'Off';
    if (typeof value === 'string') return value || 'None';
    if (value === null || (typeof value === 'object' && Object.keys(value).length === 0)) return 'None';
    return JSON.stringify(value, null, 2);
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="w-[min(680px,calc(100vw-32px))] max-w-none sm:max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 rounded-lg border border-border bg-popover p-0"
    showCloseButton={false}
    onOpenAutoFocus={(event) => { event.preventDefault(); requestAnimationFrame(() => cancelButton?.focus()); }}
    onEscapeKeydown={(event) => { if (busy) event.preventDefault(); }}
    onInteractOutside={(event) => { if (busy) event.preventDefault(); }}
  >
    <Dialog.Header class="flex-row items-start justify-between gap-3 border-b border-border px-4 py-3 text-left">
      <div class="min-w-0">
        <Dialog.Title class="flex items-center gap-2 text-base"><ShieldCheckIcon size={17} />Trust this command?</Dialog.Title>
        <Dialog.Description class="mt-2 text-sm">
          Review the command from this project's workman.yml before running it.
          Approval is remembered for this project when the command, directory, environment, and launch settings match.
        </Dialog.Description>
      </div>
      <IconButton label="Close trust review" disabled={busy} onclick={onClose}>
        {#snippet icon()}<XIcon size={14} />{/snippet}
      </IconButton>
    </Dialog.Header>

    <div class="review-body">
      <h3>{review.process_name}</h3>
      {#each Object.entries(labels) as [field, label] (field)}
        {@const value = review.fields[field as keyof TrustFields]}
        {@const change = review.changes.find(change => change.field === field)}
        <section aria-label={label}>
          <div class="field-heading">
            <strong>{label}</strong>
            {#if change && change.previous !== null}<span>Changed</span>{/if}
          </div>
          {#if change && change.previous !== null}
            <div class="value previous"><span>Previous</span><pre>{formatValue(change.previous)}</pre></div>
          {/if}
          <div class="value">
            {#if change && change.previous !== null}<span>Requested</span>{/if}
            <pre>{formatValue(value)}</pre>
          </div>
        </section>
      {/each}
    </div>

    <Dialog.Footer class="mx-0 mb-0 flex-row justify-end gap-2 bg-popover border-t border-border px-4 py-3">
      <Button bind:ref={cancelButton} variant="outline" disabled={busy} onclick={onClose}>Cancel</Button>
      <Button disabled={busy} onclick={onApprove}>{busy ? 'Approving…' : 'Trust and run'}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<style>
  .review-body { min-height: 0; overflow-y: auto; overscroll-behavior: contain; padding: 16px; }
  h3 { margin: 0 0 12px; overflow-wrap: anywhere; color: var(--foreground); font-size: var(--font-size-base); font-weight: 600; }
  section { overflow: hidden; margin-bottom: 10px; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--background); }
  section:last-child { margin-bottom: 0; }
  .field-heading { display: flex; justify-content: space-between; gap: 12px; padding: 9px 12px; border-bottom: 1px solid var(--border); background: var(--card); }
  .field-heading strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 550; }
  .field-heading span, .value > span { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .value { padding: 10px 12px; }
  .value > span { display: block; margin-bottom: 5px; }
  .value + .value { border-top: 1px solid var(--border); }
  pre { margin: 0; overflow-wrap: anywhere; white-space: pre-wrap; color: var(--foreground); font: 400 var(--font-size-sm)/1.6 'JetBrains Mono Variable', monospace; }
  .previous pre { color: var(--muted-foreground); }
</style>
