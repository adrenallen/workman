<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import CircleXIcon from '@lucide/svelte/icons/circle-x';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';

  import {
    worktreeOperationStateLabel,
    type WorktreeOperation
  } from './worktreeProgress';

  interface Props {
    operation: WorktreeOperation;
    compact?: boolean;
  }

  let { operation, compact = false }: Props = $props();
  let label = $derived(worktreeOperationStateLabel(operation));
</script>

<span
  class="project-operation-status"
  class:compact
  data-status={operation.status}
  aria-label={label}
>
  <span class="status-icon" aria-hidden="true">
    {#if operation.status === 'failed'}<CircleXIcon size={12} />
    {:else if operation.status === 'completed'}<CheckIcon size={12} />
    {:else}<LoaderCircleIcon class="spinner" size={12} />{/if}
  </span>
  {#if !compact}<span>{label}</span>{/if}
</span>

<style>
  .project-operation-status { display: inline-flex; min-width: 0; height: 18px; align-items: center; gap: 5px; color: var(--agent-state-working); font: 600 var(--font-size-xs)/1 var(--terminal-font-family); }
  .project-operation-status > span:last-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status-icon { display: inline-flex; flex: none; }
  [data-status='failed'] { color: var(--destructive); }
  [data-status='completed'] { color: var(--success); }
  .compact { width: 14px; height: 14px; justify-content: center; border: 1px solid var(--card); border-radius: 999px; background: var(--card); }
  .compact .status-icon { align-items: center; justify-content: center; }
  .compact :global(svg) { width: 10px; height: 10px; }
  :global(.spinner) { animation: operation-status-spin 800ms linear infinite; }
  @keyframes operation-status-spin { to { transform: rotate(360deg); } }
</style>
