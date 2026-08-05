<script lang="ts">
  interface Props {
    title: string;
    connected: boolean;
    loading: boolean;
    error: string | null;
    onRetry: () => void;
  }

  let { title, connected, loading, error, onRetry }: Props = $props();
</script>

<section class="connection-card" aria-live="polite">
  <span class="mark" class:loading aria-hidden="true">{loading ? '' : error ? '!' : '⌁'}</span>
  <div>
    <span class="eyebrow">{error ? 'Daemon compatibility' : 'Local connection'}</span>
    <h2>{loading ? `Loading ${title}` : error ? `${title} is temporarily unavailable` : 'Daemon connection required'}</h2>
    <p>
      {#if loading}
        Reading authenticated settings from the local daemon.
      {:else if error}
        The installed app is ready, but the daemon could not provide this section. Restart or retry after updating.
      {:else}
        Keep Settings open while awm reconnects. Local appearance, terminal, sidebar, hotkey, and opener settings remain available.
      {/if}
    </p>
    {#if error}<code>{error}</code>{/if}
  </div>
  {#if !loading && (connected || error)}
    <button type="button" disabled={!connected} onclick={onRetry}>Retry</button>
  {/if}
</section>

<style>
  .connection-card { display: grid; min-height: 178px; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 12px; border: 1px solid color-mix(in srgb, var(--warning) 45%, var(--border)); border-radius: 4px; padding: 14px; background: var(--surface); }
  .mark { display: grid; width: 34px; height: 34px; place-items: center; border: 1px solid color-mix(in srgb, var(--warning) 55%, var(--border)); border-radius: 3px; background: color-mix(in srgb, var(--warning) 8%, var(--surface)); color: var(--warning); font: 700 14px/1 'JetBrains Mono Variable', monospace; }
  .mark.loading { border-radius: 50%; border-color: var(--border); border-top-color: var(--signal); background: transparent; animation: spin .8s linear infinite; }
  .eyebrow { color: var(--warning); font: 650 var(--font-size-xs)/1.2 'JetBrains Mono Variable', monospace; letter-spacing: .08em; text-transform: uppercase; }
  h2 { margin: 4px 0 0; color: var(--text); font-size: 15px; }
  p { max-width: 590px; margin: 5px 0 0; color: var(--muted); font-size: var(--font-size-sm); line-height: 1.5; }
  code { display: block; overflow: hidden; max-width: 640px; margin-top: 8px; color: var(--text-soft); font: var(--font-size-xs)/1.4 'JetBrains Mono Variable', monospace; text-overflow: ellipsis; white-space: nowrap; }
  button { min-height: 28px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 9px; background: var(--surface-raised); color: var(--text-soft); font: 650 var(--font-size-xs) 'JetBrains Mono Variable', monospace; cursor: pointer; }
  button:disabled { cursor: default; opacity: .4; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 650px) { .connection-card { grid-template-columns: auto minmax(0, 1fr); } button { grid-column: 2; justify-self: start; } }
</style>
