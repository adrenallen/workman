<script lang="ts">
  import type { SettingsPanelProps } from './workspace';
  import { loadDaemonSettings, restartDaemon, type DaemonSettingsInfo } from './settings';
  import AgentToolsCard from './settings/AgentToolsCard.svelte';
  import AppearanceCard from './settings/AppearanceCard.svelte';
  import DaemonCard from './settings/DaemonCard.svelte';
  import McpConnectionCard from './settings/McpConnectionCard.svelte';
  import OpenersCard from './settings/OpenersCard.svelte';
  import TerminalAppearanceCard from './settings/TerminalAppearanceCard.svelte';

  let { client, connection, onError }: SettingsPanelProps = $props();
  let info = $state<DaemonSettingsInfo | null>(null);
  let loadError = $state<string | null>(null);
  let loading = $state(false);
  let restarting = $state(false);
  let sawRestartDisconnect = $state(false);
  let loadedConnection = $state<string | null>(null);
  let request = 0;

  $effect(() => {
    const status = connection.status;
    const connectionKey = status === 'connected' ? String(connection.port ?? 'unknown') : null;
    if (status !== 'connected') {
      loadedConnection = null;
      if (restarting) sawRestartDisconnect = true;
    }
    if (status === 'connected') {
      if (loadedConnection !== connectionKey) {
        loadedConnection = connectionKey;
        void refresh();
      }
      if (restarting && sawRestartDisconnect) {
        restarting = false;
        sawRestartDisconnect = false;
      }
    }
  });

  async function refresh(): Promise<void> {
    const current = ++request;
    loading = info === null;
    loadError = null;
    try {
      const next = await loadDaemonSettings(client);
      if (current === request) {
        info = next;
        loadError = null;
      }
    } catch (cause) {
      if (current === request) loadError = message(cause);
    } finally {
      if (current === request) loading = false;
    }
  }

  async function restart(): Promise<void> {
    if (restarting) return;
    restarting = true;
    sawRestartDisconnect = false;
    try {
      await restartDaemon(client);
      setTimeout(() => {
        if (restarting && !sawRestartDisconnect) restarting = false;
      }, 10_000);
    } catch (cause) {
      restarting = false;
      onError(message(cause));
    }
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="settings-panel" aria-label="Settings controls">
  {#if connection.status !== 'connected' && info === null}
    <div class="unavailable">
      <span aria-hidden="true">⌁</span>
      <div><h2>Daemon connection required</h2><p>Settings load from the authenticated local daemon. Keep this view open while gbuild reconnects.</p></div>
    </div>
  {:else if loading && info === null}
    <div class="loading"><i aria-hidden="true"></i><span>Reading daemon settings…</span></div>
  {:else if loadError && info === null}
    <div class="settings-grid degraded">
      <section class="compatibility" aria-live="polite">
        <span class="compatibility-mark" aria-hidden="true">!</span>
        <div class="compatibility-copy">
          <span class="eyebrow">Daemon compatibility</span>
          <h2>Settings opened, but the daemon is out of date</h2>
          <p>The installed app is ready. Restart the local daemon to load connection, runtime, and agent-tool settings.</p>
          <code>{loadError}</code>
        </div>
        <button type="button" disabled={connection.status !== 'connected' || loading} onclick={() => void refresh()}>
          {loading ? 'Checking…' : 'Retry settings'}
        </button>
      </section>
      <div class="appearance"><AppearanceCard /></div>
      <div class="terminal"><TerminalAppearanceCard /></div>
      <div class="openers"><OpenersCard /></div>
    </div>
  {:else if info}
    <div class="settings-grid">
      <div class="mcp"><McpConnectionCard connection={info.mcp} /></div>
      <div class="daemon"><DaemonCard {info} {connection} {restarting} onRestart={() => void restart()} /></div>
      <div class="appearance"><AppearanceCard /></div>
      <div class="terminal"><TerminalAppearanceCard /></div>
      <div class="openers"><OpenersCard /></div>
      <div class="agents"><AgentToolsCard {client} connected={connection.status === 'connected'} {onError} /></div>
    </div>
  {/if}
</section>

<style>
  .settings-panel { min-width: 0; padding: 12px 16px 24px; }
  .unavailable, .loading { display: flex; align-items: center; }
  .loading { font-family: 'JetBrains Mono Variable', monospace; }

  .settings-grid { display: grid; grid-template-columns: minmax(0, 1.35fr) minmax(280px, 0.65fr); gap: 10px; align-items: start; }
  .mcp, .agents, .openers { grid-column: 1 / -1; }
  .compatibility { display: grid; min-height: 178px; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 12px; border: 1px solid color-mix(in srgb, var(--warning) 48%, var(--border)); border-radius: 4px; padding: 14px; background: var(--surface); }
  .compatibility-mark { display: grid; width: 34px; height: 34px; place-items: center; border: 1px solid color-mix(in srgb, var(--warning) 55%, var(--border)); background: color-mix(in srgb, var(--warning) 8%, var(--surface)); color: var(--warning); font: 700 14px/1 'JetBrains Mono Variable', monospace; }
  .compatibility-copy { min-width: 0; }
  .compatibility .eyebrow { color: var(--warning); font: 650 7px/1.2 'JetBrains Mono Variable', monospace; letter-spacing: 0.08em; text-transform: uppercase; }
  .compatibility h2 { margin: 4px 0 0; color: var(--text); font-size: 15px; }
  .compatibility p { max-width: 560px; margin: 5px 0 0; color: var(--muted); font-size: 10px; line-height: 1.5; }
  .compatibility code { display: block; overflow: hidden; margin-top: 8px; color: #a9afb7; font: 8px/1.4 'JetBrains Mono Variable', monospace; text-overflow: ellipsis; white-space: nowrap; }
  .compatibility button { min-height: 28px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 9px; background: var(--surface-raised); color: var(--text-soft); font: 650 8px 'JetBrains Mono Variable', monospace; cursor: pointer; }
  .compatibility button:hover:not(:disabled) { border-color: #707780; color: var(--text); }
  .compatibility button:disabled { cursor: default; opacity: 0.45; }
  .unavailable, .loading { min-height: 180px; justify-content: center; gap: 10px; border: 1px dashed var(--border-strong); border-radius: 4px; color: #9299a2; }
  .unavailable > span { color: #45636e; font-size: 30px; }
  .unavailable h2 { margin: 0; color: #a5b7bd; font-size: 14px; }
  .unavailable p { max-width: 430px; margin: 5px 0 0; font-size: 10px; line-height: 1.5; }
  .loading { font-size: 8px; }
  .loading i { width: 14px; height: 14px; border: 1px solid #3c5660; border-top-color: var(--signal); border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 900px) { .settings-grid { grid-template-columns: 1fr; } .mcp, .agents, .openers { grid-column: auto; } .compatibility { grid-template-columns: auto minmax(0, 1fr); } .compatibility button { grid-column: 2; justify-self: start; } }
</style>
