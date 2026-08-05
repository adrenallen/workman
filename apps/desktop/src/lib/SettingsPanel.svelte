<script lang="ts">
  import type { SettingsPanelProps } from './workspace';
  import {
    applyUpdate,
    checkForUpdates,
    loadDaemonSettings,
    restartDaemon,
    setAutomaticUpdateChecks,
    type DaemonSettingsInfo,
    type UpdateInstallReport
  } from './settings';
  import { settingsSection, settingsSections } from './settingsSections';
  import AgentToolsCard from './settings/AgentToolsCard.svelte';
  import AppearanceCard from './settings/AppearanceCard.svelte';
  import DaemonCard from './settings/DaemonCard.svelte';
  import HotkeysCard from './settings/HotkeysCard.svelte';
  import McpConnectionCard from './settings/McpConnectionCard.svelte';
  import OpenersCard from './settings/OpenersCard.svelte';
  import RuntimeDoctor from './settings/RuntimeDoctor.svelte';
  import SettingsConnectionCard from './settings/SettingsConnectionCard.svelte';
  import SettingsSectionNav from './settings/SettingsSectionNav.svelte';
  import SettingsStatusStrip from './settings/SettingsStatusStrip.svelte';
  import SidebarCard from './settings/SidebarCard.svelte';
  import TerminalAppearanceCard from './settings/TerminalAppearanceCard.svelte';

  let { client, project, connection, onError }: SettingsPanelProps = $props();
  let info = $state<DaemonSettingsInfo | null>(null);
  let loadError = $state<string | null>(null);
  let loading = $state(false);
  let restarting = $state(false);
  let sawRestartDisconnect = $state(false);
  let updateBusy = $state<'check' | 'apply' | 'preference' | null>(null);
  let updateMessage = $state<string | null>(null);
  let loadedConnection = $state<string | null>(null);
  let viewport = $state<HTMLDivElement>();
  let request = 0;

  let activeDefinition = $derived(
    settingsSections.find((section) => section.id === $settingsSection) ?? settingsSections[0]
  );

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

  $effect(() => {
    $settingsSection;
    queueMicrotask(() => viewport?.scrollTo({ top: 0 }));
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

  async function checkUpdate(): Promise<void> {
    if (!info || updateBusy) return;
    updateBusy = 'check';
    updateMessage = null;
    try {
      info = { ...info, update: await checkForUpdates(client, true) };
      updateMessage = info.update.check.available
        ? `awm ${info.update.check.latest} is available.`
        : `awm ${info.update.check.current} is current.`;
    } catch (cause) {
      updateMessage = message(cause);
    } finally {
      updateBusy = null;
    }
  }

  async function toggleAutomaticChecks(enabled: boolean): Promise<void> {
    if (!info || updateBusy) return;
    updateBusy = 'preference';
    try {
      info = { ...info, update: await setAutomaticUpdateChecks(client, enabled) };
      updateMessage = enabled ? 'Weekly update checks enabled.' : 'Automatic checks disabled.';
    } catch (cause) {
      updateMessage = message(cause);
    } finally {
      updateBusy = null;
    }
  }

  async function updateNow(): Promise<void> {
    if (!info || updateBusy) return;
    if (!window.confirm('Update awm and restart the daemon? All running project processes will stop.')) return;
    updateBusy = 'apply';
    updateMessage = 'Downloading and verifying the update…';
    try {
      const report: UpdateInstallReport = await applyUpdate(client);
      updateMessage = report.desktop_instruction ?? `Updated to awm ${report.latest}. Reconnecting…`;
      restarting = true;
      sawRestartDisconnect = false;
    } catch (cause) {
      updateMessage = message(cause);
    } finally {
      updateBusy = null;
    }
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="settings-panel" aria-label="Settings controls">
  <header class="settings-header">
    <div>
      <span class="eyebrow">Workspace preferences</span>
      <h2>Settings</h2>
    </div>
    <p><strong>{activeDefinition?.label}</strong><span>{activeDefinition?.description}</span></p>
  </header>

  <SettingsStatusStrip {project} {connection} {info} />
  <SettingsSectionNav connected={connection.status === 'connected'} />

  <div class="section-viewport" bind:this={viewport}>
    <div
      class="section-panel"
      id={`settings-panel-${$settingsSection}`}
      role="tabpanel"
      aria-labelledby={`settings-tab-${$settingsSection}`}
      tabindex="0"
    >
      {#if $settingsSection === 'appearance'}
        <AppearanceCard />
      {:else if $settingsSection === 'terminal'}
        <TerminalAppearanceCard />
      {:else if $settingsSection === 'sidebar'}
        <SidebarCard />
      {:else if $settingsSection === 'hotkeys'}
        <HotkeysCard />
      {:else if $settingsSection === 'agents'}
        <div class="section-stack">
          <AgentToolsCard {client} connected={connection.status === 'connected'} {onError} />
          <RuntimeDoctor {client} {project} connected={connection.status === 'connected'} {onError} />
        </div>
      {:else if $settingsSection === 'tools'}
        <OpenersCard />
      {:else if $settingsSection === 'mcp'}
        {#if info}
          <McpConnectionCard connection={info.mcp} />
        {:else}
          <SettingsConnectionCard
            title="MCP settings"
            connected={connection.status === 'connected'}
            {loading}
            error={loadError}
            onRetry={() => void refresh()}
          />
        {/if}
      {:else if $settingsSection === 'daemon'}
        {#if info}
          <DaemonCard
            {info}
            {connection}
            {restarting}
            {updateBusy}
            {updateMessage}
            onRestart={() => void restart()}
            onCheckUpdate={() => void checkUpdate()}
            onUpdateNow={() => void updateNow()}
            onAutomaticChecks={(enabled) => void toggleAutomaticChecks(enabled)}
          />
        {:else}
          <SettingsConnectionCard
            title="Daemon settings"
            connected={connection.status === 'connected'}
            {loading}
            error={loadError}
            onRetry={() => void refresh()}
          />
        {/if}
      {/if}
    </div>
  </div>
</section>

<style>
  .settings-panel { display: grid; width: 100%; height: 100%; min-width: 0; min-height: 0; grid-template-rows: auto auto auto minmax(0, 1fr); gap: 7px; overflow: hidden; padding: 9px 12px 12px; }
  .settings-header { display: flex; min-height: 45px; align-items: center; justify-content: space-between; gap: 18px; padding: 0 2px; }
  .settings-header .eyebrow { color: var(--muted); font: 700 7px/1.2 'JetBrains Mono Variable', monospace; letter-spacing: .08em; text-transform: uppercase; }
  .settings-header h2 { margin: 2px 0 0; color: var(--text); font-size: 18px; line-height: 1.05; }
  .settings-header p { display: grid; min-width: 150px; margin: 0; padding-left: 12px; border-left: 1px solid var(--border); text-align: right; }
  .settings-header p strong { color: var(--text-soft); font-size: 9px; }
  .settings-header p span { margin-top: 2px; color: var(--muted); font: 7px/1.25 'JetBrains Mono Variable', monospace; }
  .section-viewport { min-width: 0; min-height: 0; overflow-y: auto; padding: 1px 2px 12px; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .section-panel { width: min(1040px, 100%); min-width: 0; margin: 0 auto; outline: 0; }
  .section-panel:focus-visible { outline: 1px solid var(--signal); outline-offset: 2px; }
  .section-stack { display: grid; gap: 9px; }

  @media (max-width: 660px) {
    .settings-panel { padding-inline: 8px; }
    .settings-header p { display: none; }
  }
</style>
