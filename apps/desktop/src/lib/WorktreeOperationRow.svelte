<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import CircleXIcon from '@lucide/svelte/icons/circle-x';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import type { WorktreeOperation } from './worktreeProgress';

  interface Props {
    operation: WorktreeOperation;
    collapsed: boolean;
    onSelect: () => void;
  }

  let { operation, collapsed, onSelect }: Props = $props();
  let stateLabel = $derived(
    operation.status === 'failed'
      ? 'Failed'
      : operation.status === 'completed'
        ? operation.mode === 'remove' ? 'Removed' : 'Ready'
        : operation.mode === 'remove' ? 'Removing' : 'Creating'
  );
</script>

<article class="operation-row" class:collapsed data-status={operation.status}>
  <button
    type="button"
    title={`${operation.label} · ${stateLabel}`}
    aria-label={`${operation.label} · ${stateLabel}`}
    onclick={onSelect}
  >
    <span class="state-icon" aria-hidden="true">
      {#if operation.status === 'failed'}<CircleXIcon size={13} />
      {:else if operation.status === 'completed'}<CheckIcon size={13} />
      {:else}<LoaderCircleIcon class="spinner" size={13} />{/if}
    </span>
    {#if operation.mode === 'remove'}<Trash2Icon size={15} strokeWidth={1.8} aria-hidden="true" />{:else}<GitBranchIcon size={15} strokeWidth={1.8} aria-hidden="true" />{/if}
    {#if !collapsed}
      <span class="copy"><strong>{operation.label}</strong><small>{stateLabel.toLowerCase()}{operation.status === 'running' ? '…' : ''}</small></span>
    {/if}
  </button>
</article>

<style>
  .operation-row { min-width: 0; }
  button { display: grid; width: 100%; min-height: 42px; grid-template-columns: 16px 17px minmax(0, 1fr); align-items: center; gap: 6px; border: 0; border-left: 2px solid var(--agent-state-working); padding: 4px 7px; background: color-mix(in srgb, var(--agent-state-working) 6%, transparent); color: var(--foreground); text-align: left; cursor: pointer; }
  button:active { transform: translateY(1px); }
  .state-icon { display: inline-flex; color: var(--agent-state-working); }
  .copy { min-width: 0; }
  .copy strong, .copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .copy strong { font-size: var(--font-size-sm); font-weight: 570; }
  .copy small { margin-top: 2px; color: var(--muted-foreground); font: var(--font-size-xs)/1 var(--terminal-font-family); }
  [data-status='failed'] button { border-left-color: var(--destructive); background: color-mix(in srgb, var(--destructive) 7%, transparent); }
  [data-status='failed'] .state-icon { color: var(--destructive); }
  [data-status='completed'] button { border-left-color: var(--success); background: color-mix(in srgb, var(--success) 5%, transparent); }
  [data-status='completed'] .state-icon { color: var(--success); }
  .operation-row.collapsed { width: 100%; height: 36px; flex: none; }
  .collapsed button { position: relative; height: 36px; min-height: 36px; grid-template-columns: 1fr; justify-items: center; border-left: 0; padding: 3px; }
  .collapsed .state-icon { position: absolute; z-index: 1; right: 3px; bottom: 3px; width: 12px; height: 12px; align-items: center; justify-content: center; border: 1px solid var(--card); border-radius: 999px; background: var(--card); }
  .collapsed .state-icon :global(svg) { width: 9px; height: 9px; }
  :global(.spinner) { animation: spin 800ms linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
