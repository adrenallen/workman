<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import CircleIcon from '@lucide/svelte/icons/circle';
  import CircleXIcon from '@lucide/svelte/icons/circle-x';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
  import XIcon from '@lucide/svelte/icons/x';

  import { Button } from '$lib/components/ui/button';
  import type { WorktreeOperation } from './worktreeProgress';

  interface Props {
    operation: WorktreeOperation;
    onRetry?: () => void;
    onDismiss?: () => void;
  }

  let { operation, onRetry, onDismiss }: Props = $props();
  let completed = $derived(operation.steps.filter((step) => step.status === 'completed' || step.status === 'skipped').length);
  let progress = $derived(Math.round((completed / operation.steps.length) * 100));
</script>

<section class="progress-panel" aria-live="polite" aria-busy={operation.status === 'running'}>
  <header>
    <div>
      <span>{operation.mode === 'adopt' ? 'Adopting worktree' : operation.mode === 'fork' ? 'Forking worktree' : 'Creating worktree'}</span>
      <h2>{operation.label}</h2>
    </div>
    <strong class:failed={operation.status === 'failed'} class:complete={operation.status === 'completed'}>
      {operation.status === 'failed' ? 'Needs attention' : operation.status === 'completed' ? 'Ready' : `${progress}%`}
    </strong>
  </header>

  <div class="progress-track" aria-label={`${progress}% complete`}><span style={`width: ${progress}%`}></span></div>

  <ol>
    {#each operation.steps as step}
      <li data-status={step.status}>
        <span class="step-icon" aria-hidden="true">
          {#if step.status === 'completed' || step.status === 'skipped'}<CheckIcon size={14} />
          {:else if step.status === 'running'}<LoaderCircleIcon class="spinner" size={14} />
          {:else if step.status === 'failed'}<CircleXIcon size={14} />
          {:else}<CircleIcon size={12} />{/if}
        </span>
        <div><strong>{step.label}</strong>{#if step.detail}<small>{step.detail}</small>{/if}</div>
      </li>
    {/each}
  </ol>

  {#if operation.error}
    <p class="operation-error" role="alert">{operation.error}</p>
  {/if}

  <footer>
    {#if onDismiss}<Button size="sm" variant="ghost" onclick={onDismiss}><XIcon size={13} />{operation.status === 'completed' ? 'Close' : 'Dismiss'}</Button>{/if}
    {#if operation.status === 'failed' && onRetry}<Button size="sm" variant="outline" onclick={onRetry}><RotateCcwIcon size={13} />Back to form</Button>{/if}
  </footer>
</section>

<style>
  .progress-panel { width: min(620px, calc(100% - 32px)); align-self: start; justify-self: center; margin-top: clamp(24px, 7vh, 72px); overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--card); color: var(--foreground); }
  header { display: flex; min-height: 70px; align-items: center; justify-content: space-between; gap: 18px; border-bottom: 1px solid var(--border); padding: 12px 14px; }
  header span { color: var(--muted-foreground); font: 650 var(--font-size-xs)/1 var(--terminal-font-family); letter-spacing: 0.08em; text-transform: uppercase; }
  h2 { margin: 5px 0 0; font-size: var(--font-size-lg); letter-spacing: -0.015em; }
  header > strong { color: var(--information); font: 650 var(--font-size-xs)/1 var(--terminal-font-family); }
  header > strong.failed { color: var(--destructive); }
  header > strong.complete { color: var(--success); }
  .progress-track { height: 2px; background: var(--muted); }
  .progress-track span { display: block; height: 100%; background: var(--information); transition: width 180ms ease; }
  ol { display: grid; gap: 0; margin: 0; padding: 5px 14px; list-style: none; }
  li { display: grid; min-height: 42px; grid-template-columns: 24px minmax(0, 1fr); align-items: center; border-bottom: 1px solid color-mix(in srgb, var(--border) 70%, transparent); color: var(--muted-foreground); }
  li:last-child { border-bottom: 0; }
  li strong, li small { display: block; }
  li strong { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 560; }
  li small { margin-top: 2px; color: var(--muted-foreground); font: var(--font-size-xs)/1.35 var(--terminal-font-family); }
  li[data-status='completed'], li[data-status='skipped'] { color: var(--success); }
  li[data-status='completed'] strong, li[data-status='skipped'] strong { color: var(--foreground); }
  li[data-status='running'] { color: var(--information); }
  li[data-status='running'] strong { color: var(--foreground); }
  li[data-status='failed'] { color: var(--destructive); }
  li[data-status='failed'] strong { color: var(--destructive); }
  .step-icon { display: inline-flex; align-items: center; }
  :global(.spinner) { animation: spin 800ms linear infinite; }
  .operation-error { margin: 0 14px 9px; border: 1px solid color-mix(in srgb, var(--destructive) 42%, var(--border)); border-radius: var(--radius); padding: 8px 9px; background: color-mix(in srgb, var(--destructive) 8%, var(--card)); color: var(--destructive); font-size: var(--font-size-sm); }
  footer { display: flex; justify-content: flex-end; gap: 7px; border-top: 1px solid var(--border); padding: 8px; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
