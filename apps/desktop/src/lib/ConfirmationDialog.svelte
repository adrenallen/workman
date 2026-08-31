<script lang="ts">
  import AlertTriangleIcon from '@lucide/svelte/icons/triangle-alert';

  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import { Button } from '$lib/components/ui/button';

  interface Props {
    title: string;
    description: string;
    confirmLabel?: string;
    destructive?: boolean;
    busy?: boolean;
    onConfirm: () => void;
    onClose: () => void;
  }

  let {
    title,
    description,
    confirmLabel = 'Continue',
    destructive = true,
    busy = false,
    onConfirm,
    onClose
  }: Props = $props();
</script>

<AlertDialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <AlertDialog.Content class="w-[min(620px,calc(100vw-24px))] !max-w-none gap-0 rounded-lg border border-border bg-popover p-0">
    <AlertDialog.Header class="gap-2 border-b border-border px-4 py-4 text-left">
      <span class:danger={destructive} class="flex items-center gap-2">
        <AlertTriangleIcon size={16} />
        <AlertDialog.Title>{title}</AlertDialog.Title>
      </span>
      <AlertDialog.Description class="text-sm leading-relaxed">{description}</AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer class="mx-0 mb-0 flex-row flex-wrap justify-end rounded-none rounded-b-lg border-t border-border bg-card px-4 py-3">
      <Button variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
      <Button variant={destructive ? 'destructive' : 'default'} disabled={busy} onclick={onConfirm}>
        {confirmLabel}
      </Button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

<style>
  .danger { color: var(--destructive); }
</style>
