<script lang="ts">
  import type { SettingsPanelProps } from './workspace';
  import { ScrollArea } from '$lib/components/ui/scroll-area';
  import {
    applyUpdate,
    checkForUpdates,
    loadDaemonSettings,
    restartDaemon,
    setAutomaticUpdateChecks,
    setUserShell,
    setUpdateChannel,
    type DaemonSettingsInfo,
    type UpdateInstallReport,
    type UpdateChannel
  } from './settings';
  import {
    consumeNativeUpdateCheckRequest,
    nativeUpdateCheckRequest
  } from './nativeMenu';
  import { settingsSection, settingsSections } from './settingsSections';
  import AgentToolsCard from './settings/AgentToolsCard.svelte';
  import AboutUpdatesCard from './settings/AboutUpdatesCard.svelte';
  import AppearanceCard from './settings/AppearanceCard.svelte';
  import DaemonCard from './settings/DaemonCard.svelte';
  import HotkeysCard from './settings/HotkeysCard.svelte';
  import McpConnectionCard from './settings/McpConnectionCard.svelte';
  import NotificationsCard from './settings/NotificationsCard.svelte';
  import OpenersCard from './settings/OpenersCard.svelte';
  import RuntimeDoctor from './settings/RuntimeDoctor.svelte';
  import WorktreeHealthCard from './settings/WorktreeHealthCard.svelte';
  import SettingsConnectionCard from './settings/SettingsConnectionCard.svelte';
  import SettingsSectionNav from './settings/SettingsSectionNav.svelte';
  import SettingsStatusStrip from './settings/SettingsStatusStrip.svelte';
  import SidebarCard from './settings/SidebarCard.svelte';
  import TerminalAppearanceCard from './settings/TerminalAppearanceCard.svelte';
  import ProfilesCard from './settings/ProfilesCard.svelte';
  import QuickPromptsCard from './settings/QuickPromptsCard.svelte';

  let { client, project, connection, onError, onProfileSwitched }: SettingsPanelProps = $props();
  let info = $state<DaemonSettingsInfo | null>(null);
  let loadError = $state<string | null>(null);
  let loading = $state(false);
  let restarting = $state(false);
  let sawRestartDisconnect = $state(false);
  let updateBusy = $state<'check' | 'apply' | 'preference' | null>(null);
  let updateMessage = $state<string | null>(null);
  let loadedConnection = $state<string | null>(null);
  let viewport = $state<HTMLElement | null>(null);
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

  $effect(() => {
    // Keep the native menu and the About button on the same update-check implementation.
    const nativeRequest = $nativeUpdateCheckRequest;
    if (
      nativeRequest > 0 &&
      info &&
      updateBusy === null &&
      connection.status === 'connected'
    ) {
      consumeNativeUpdateCheckRequest();
      void checkUpdate();
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

  async function saveUserShell(shell: string | null): Promise<void> {
    if (!info || connection.status !== 'connected') return;
    try {
      info = { ...info, user_environment: await setUserShell(client, shell) };
    } catch (cause) {
      onError(message(cause));
      throw cause;
    }
  }

  async function checkUpdate(): Promise<void> {
    if (!info || updateBusy) return;
    updateBusy = 'check';
    updateMessage = null;
    try {
      info = { ...info, update: await checkForUpdates(client, true) };
      updateMessage = info.update.cli_recovery_required
        ? info.update.check.available
          ? `The command-line tools need repair. Workman ${info.update.check.latest} is also available.`
          : 'The command-line tools need repair. The desktop app can reinstall them.'
        : info.update.check.available
          ? `Workman ${info.update.check.latest} is available.`
          : `Workman ${info.update.check.current} is current.`;
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

  async function chooseUpdateChannel(channel: UpdateChannel): Promise<void> {
    if (!info || updateBusy || channel === info.update.channel) return;
    updateBusy = 'preference';
    try {
      info = { ...info, update: await setUpdateChannel(client, channel) };
      updateMessage = channel === 'stable'
        ? 'Stable channel selected. Prereleases are ignored.'
        : 'Latest channel selected. Prereleases are included.';
    } catch (cause) {
      updateMessage = message(cause);
    } finally {
      updateBusy = null;
    }
  }

  async function updateNow(): Promise<void> {
    if (!info || updateBusy) return;
    updateBusy = 'apply';
    const recovery = info.update.cli_recovery_required;
    updateMessage = recovery
      ? 'Downloading and verifying the release before repairing the command-line tools…'
      : 'Downloading and verifying the update…';
    try {
      const report: UpdateInstallReport = await applyUpdate(client);
      updateMessage = report.desktop_instruction
        ?? (recovery
          ? `Repaired the command-line tools with Workman ${report.latest}. Reconnecting…`
          : `Updated to Workman ${report.latest}. Reconnecting…`);
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
  <SettingsSectionNav />

  <ScrollArea class="min-h-0 min-w-0 w-full overflow-hidden px-0.5 pb-3" bind:viewportRef={viewport}>
    <div
      class="section-panel"
      id={`settings-panel-${$settingsSection}`}
      role="tabpanel"
      aria-labelledby={`settings-tab-${$settingsSection}`}
      tabindex="0"
    >
      {#if $settingsSection === 'appearance'}
        <AppearanceCard />
      {:else if $settingsSection === 'profiles'}
        <ProfilesCard
          {client}
          connected={connection.status === 'connected'}
          {onError}
          onSwitched={onProfileSwitched}
        />
      {:else if $settingsSection === 'terminal'}
        <TerminalAppearanceCard
          {client}
          environment={info?.user_environment ?? null}
          connected={connection.status === 'connected'}
          onShellChange={saveUserShell}
        />
      {:else if $settingsSection === 'sidebar'}
        <SidebarCard />
      {:else if $settingsSection === 'hotkeys'}
        <HotkeysCard />
      {:else if $settingsSection === 'notifications'}
        <NotificationsCard />
      {:else if $settingsSection === 'agents'}
        {#if project}
          <div class="section-stack">
            <AgentToolsCard {client} connected={connection.status === 'connected'} {onError} />
            <RuntimeDoctor {client} {project} connected={connection.status === 'connected'} {onError} />
            <WorktreeHealthCard {client} connected={connection.status === 'connected'} {onError} />
          </div>
        {:else}
          <SettingsConnectionCard
            title="Agent settings"
            connected={connection.status === 'connected'}
            loading={false}
            error="Load a project before checking agent runtime health."
            onRetry={() => {}}
          />
        {/if}
      {:else if $settingsSection === 'quick-prompts'}
        <QuickPromptsCard {client} connected={connection.status === 'connected'} {onError} />
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
            onRestart={() => void restart()}
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
      {:else if $settingsSection === 'about'}
        {#if info}
          <AboutUpdatesCard
            {info}
            {connection}
            {updateBusy}
            {updateMessage}
            onCheckUpdate={() => void checkUpdate()}
            onUpdateNow={() => void updateNow()}
            onAutomaticChecks={(enabled: boolean) => void toggleAutomaticChecks(enabled)}
            onUpdateChannel={(channel: UpdateChannel) => void chooseUpdateChannel(channel)}
          />
        {:else}
          <SettingsConnectionCard
            title="About & update settings"
            connected={connection.status === 'connected'}
            {loading}
            error={loadError}
            onRetry={() => void refresh()}
          />
        {/if}
      {/if}
    </div>
  </ScrollArea>
</section>

<style>
  .settings-panel { display: grid; width: 100%; height: 100%; min-width: 0; min-height: 0; grid-template-rows: auto auto auto minmax(0, 1fr); gap: 7px; overflow: hidden; padding: 9px 12px 12px; }
  .settings-header { display: flex; min-height: 45px; align-items: center; justify-content: space-between; gap: 18px; padding: 0 2px; }
  .settings-header .eyebrow { color: var(--muted); font: 700 var(--font-size-xs)/1.2 'JetBrains Mono Variable', monospace; letter-spacing: .08em; text-transform: uppercase; }
  .settings-header h2 { margin: 2px 0 0; color: var(--text); font-size: 18px; line-height: 1.05; }
  .settings-header p { display: grid; min-width: 150px; margin: 0; padding-left: 12px; border-left: 1px solid var(--border); text-align: right; }
  .settings-header p strong { color: var(--text-soft); font-size: var(--font-size-sm); }
  .settings-header p span { margin-top: 2px; color: var(--muted); font: var(--font-size-xs)/1.25 'JetBrains Mono Variable', monospace; }
  .section-panel { width: 100%; max-width: 1040px; min-width: 0; margin: 0 auto; outline: 0; }
  .section-panel:focus-visible { outline: 1px solid var(--signal); outline-offset: 2px; }
  .section-stack { display: grid; gap: 9px; }

  @media (max-width: 660px) {
    .settings-panel { padding-inline: 8px; }
    .settings-header p { display: none; }
  }
</style>
