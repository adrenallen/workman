<script lang="ts">
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
  import XIcon from '@lucide/svelte/icons/x';

  import IconButton from './components/ds/IconButton.svelte';

  export interface AgentDoneNotice {
    id: string;
    processId: number;
    projectId: number;
    name: string;
  }

  interface Props {
    notices: AgentDoneNotice[];
    onOpen: (notice: AgentDoneNotice) => void;
    onDismiss: (id: string) => void;
  }

  let { notices, onOpen, onDismiss }: Props = $props();
</script>

<section class="toast-stack" aria-label="Agent completion notifications" aria-live="polite">
  {#each notices as notice (notice.id)}
    <article class="done-toast">
      <button
        class="toast-open"
        type="button"
        aria-label={`Open ${notice.name}, which finished with unread output`}
        onclick={() => onOpen(notice)}
      >
        <span class="toast-icon" aria-hidden="true"><CircleCheckIcon size={18} strokeWidth={2} /></span>
        <span class="toast-copy">
          <strong>{notice.name} finished</strong>
          <small>Unread output · click to view</small>
        </span>
      </button>
      <IconButton label={`Dismiss ${notice.name} notification`} onclick={() => onDismiss(notice.id)}>
        {#snippet icon()}<XIcon size={14} />{/snippet}
      </IconButton>
    </article>
  {/each}
</section>

<style>
  .toast-stack {
    position: fixed;
    z-index: 90;
    top: 48px;
    right: 14px;
    display: grid;
    width: min(340px, calc(100vw - 28px));
    gap: 7px;
    pointer-events: none;
  }

  .done-toast {
    display: flex;
    min-height: 58px;
    align-items: center;
    gap: 4px;
    border: 1px solid color-mix(in srgb, #8fb8ff 48%, var(--border));
    border-radius: 7px;
    padding: 5px;
    background: color-mix(in srgb, var(--popover) 94%, #8fb8ff 6%);
    box-shadow: 0 14px 34px rgb(0 0 0 / 34%);
    pointer-events: auto;
  }

  .toast-open {
    display: grid;
    min-width: 0;
    flex: 1;
    grid-template-columns: 24px minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    border: 0;
    padding: 5px 6px;
    background: transparent;
    color: var(--foreground);
    text-align: left;
    cursor: pointer;
  }

  .toast-open:focus-visible { outline: 1px solid #8fb8ff; outline-offset: 1px; }
  .toast-icon { display: grid; color: #8fb8ff; place-items: center; }
  .toast-copy { min-width: 0; }
  .toast-copy strong, .toast-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .toast-copy strong { font-size: var(--font-size-sm); font-weight: 680; }
  .toast-copy small { margin-top: 2px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
</style>
