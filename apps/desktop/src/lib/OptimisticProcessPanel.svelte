<script lang="ts">
  import CircleXIcon from '@lucide/svelte/icons/circle-x';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';

  import { Button } from '$lib/components/ui/button';
  import type { ProcessKind } from './daemon';

  interface Props {
    kind: ProcessKind;
    name: string;
    error?: string | null;
    onRetry?: () => void;
    onDismiss?: () => void;
  }

  let { kind, name, error = null, onRetry, onDismiss }: Props = $props();
</script>

<section class="process-placeholder" class:failed={error !== null} aria-live="polite" aria-busy={error === null}>
  <header>
    <span class="traffic"><i></i><i></i><i></i></span>
    <strong>{name}</strong>
    <small>{kind}</small>
  </header>
  <div class="terminal-copy">
    {#if error}
      <CircleXIcon size={18} aria-hidden="true" />
      <div><strong>Could not start {name}</strong><p>{error}</p></div>
    {:else}
      <LoaderCircleIcon class="spinner" size={18} aria-hidden="true" />
      <div><strong>Starting {name}…</strong><p>Registering the process and opening its PTY. Live output will replace this panel on the first connection.</p></div>
    {/if}
  </div>
  {#if error}
    <footer>
      {#if onDismiss}<Button size="sm" variant="ghost" onclick={onDismiss}>Dismiss</Button>{/if}
      {#if onRetry}<Button size="sm" variant="outline" onclick={onRetry}><RotateCcwIcon size={13} />Retry</Button>{/if}
    </footer>
  {/if}
</section>

<style>
  .process-placeholder { display: grid; width: 100%; height: 100%; min-height: 0; grid-template-rows: 34px 1fr auto; background: var(--background); color: var(--foreground); }
  header { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 9px; border-bottom: 1px solid var(--border); padding: 0 11px; background: var(--card); }
  header strong, header small { font-family: var(--terminal-font-family); font-size: var(--font-size-xs); }
  header strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  header small { color: var(--muted-foreground); text-transform: uppercase; }
  .traffic { display: flex; gap: 4px; }
  .traffic i { width: 7px; height: 7px; border-radius: 50%; background: var(--border-strong); }
  .traffic i:first-child { background: color-mix(in srgb, var(--destructive) 72%, var(--border)); }
  .traffic i:nth-child(2) { background: color-mix(in srgb, var(--warning) 72%, var(--border)); }
  .traffic i:last-child { background: color-mix(in srgb, var(--success) 72%, var(--border)); }
  .terminal-copy { display: flex; max-width: 620px; align-self: center; align-items: flex-start; gap: 10px; padding: 18px; color: var(--muted-foreground); }
  .terminal-copy strong { color: var(--text); font: 600 var(--font-size-sm)/1.35 var(--terminal-font-family); }
  .terminal-copy p { margin: 5px 0 0; color: var(--text-soft); font: var(--font-size-sm)/1.5 var(--terminal-font-family); }
  .failed .terminal-copy > :global(svg) { color: var(--destructive); }
  :global(.spinner) { color: var(--information); animation: spin 800ms linear infinite; }
  footer { display: flex; justify-content: flex-end; gap: 7px; border-top: 1px solid var(--border); padding: 8px; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
