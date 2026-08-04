<script lang="ts">
  import type { SettingsPanelProps } from './workspace';
  import { loadDaemonSettings, restartDaemon, type DaemonSettingsInfo } from './settings';
  import AgentToolsCard from './settings/AgentToolsCard.svelte';
  import AppearanceCard from './settings/AppearanceCard.svelte';
  import DaemonCard from './settings/DaemonCard.svelte';
  import McpConnectionCard from './settings/McpConnectionCard.svelte';

  let { client, connection, onError }: SettingsPanelProps = $props();
  let info = $state<DaemonSettingsInfo | null>(null);
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
    try {
      const next = await loadDaemonSettings(client);
      if (current === request) info = next;
    } catch (cause) {
      if (current === request) onError(message(cause));
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
  {:else if info}
    <div class="settings-grid">
      <div class="mcp"><McpConnectionCard connection={info.mcp} /></div>
      <div class="daemon"><DaemonCard {info} {connection} {restarting} onRestart={() => void restart()} /></div>
      <div class="appearance"><AppearanceCard /></div>
      <div class="agents"><AgentToolsCard {client} connected={connection.status === 'connected'} {onError} /></div>
    </div>
  {/if}
</section>

<style>
  .settings-panel { min-width: 0; padding: 22px clamp(20px, 3.6vw, 52px) 48px; }
  .unavailable, .loading { display: flex; align-items: center; }
  .loading { font-family: 'JetBrains Mono Variable', monospace; }

  .settings-grid { display: grid; grid-template-columns: minmax(0, 1.35fr) minmax(280px, 0.65fr); gap: 14px; align-items: start; }
  .mcp, .agents { grid-column: 1 / -1; }
  .unavailable, .loading { min-height: 250px; justify-content: center; gap: 13px; border: 1px dashed #2c4651; border-radius: 5px; color: #68808a; }
  .unavailable > span { color: #45636e; font-size: 30px; }
  .unavailable h2 { margin: 0; color: #a5b7bd; font-size: 14px; }
  .unavailable p { max-width: 430px; margin: 5px 0 0; font-size: 10px; line-height: 1.5; }
  .loading { font-size: 8px; }
  .loading i { width: 14px; height: 14px; border: 1px solid #3c5660; border-top-color: var(--signal); border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 900px) { .settings-grid { grid-template-columns: 1fr; } .mcp, .agents { grid-column: auto; } }
</style>
