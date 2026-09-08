<script lang="ts">
  import type { SettingsPanelProps } from './workspace';
  import { ScrollArea } from '$lib/components/ui/scroll-area';
  import {
    checkForUpdates,
    loadDaemonSettings,
    restartDaemon,
    setAutomaticUpdateChecks,
    setUserShell,
    setUpdateChannel,
    type DaemonSettingsInfo,
    type UpdateChannel
  } from './settings';
  import {
    consumeNativeUpdateCheckRequest,
    nativeUpdateCheckRequest
  } from './nativeMenu';
  import { settingsSection, settingsSections } from './settingsSections';
  import AgentToolsCard from './settings/AgentToolsCard.svelte';
  import AgentTemplatesCard from './settings/AgentTemplatesCard.svelte';
  import AboutUpdatesCard from './settings/AboutUpdatesCard.svelte';
  import AppearanceCard from './settings/AppearanceCard.svelte';
  import DaemonCard from './settings/DaemonCard.svelte';
  import HotkeysCard from './settings/HotkeysCard.svelte';
  import McpConnectionCard from './settings/McpConnectionCard.svelte';
  import NotificationsCard from './settings/NotificationsCard.svelte';
  import RecordedFeedbackCard from './settings/RecordedFeedbackCard.svelte';
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

  let {
    client,
    project,
    connection,
    updateFlow,
    onApplyUpdate,
    onRestartUpdate,
    onDismissUpdate,
    onError,
    onProfileSwitched
  }: SettingsPanelProps = $props();
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
  let effectiveUpdateBusy = $derived(
    updateFlow.kind === 'running' || updateFlow.kind === 'restarting' ? 'apply' : updateBusy
  );

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
    if (!info || updateBusy || updateFlow.kind === 'running' || updateFlow.kind === 'restarting') return;
    updateMessage = null;
    try {
      await onApplyUpdate(info.update);
    } catch (cause) {
      updateMessage = message(cause);
    }
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="settings-panel" aria-label="Settings controls">
  <header class="settings-header">
    <h1>Settings</h1>
    <SettingsStatusStrip {connection} {info} />
  </header>
  <div class="settings-layout">
    <SettingsSectionNav />
    <ScrollArea class="settings-scroll min-h-0 min-w-0 w-full overflow-hidden" bind:viewportRef={viewport}>
    <div
      class="section-panel"
      id={`settings-panel-${$settingsSection}`}
      role="tabpanel"
      aria-label={activeDefinition?.label}
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
      {:else if $settingsSection === 'feedback'}
        <RecordedFeedbackCard />
      {:else if $settingsSection === 'templates'}
        <AgentTemplatesCard {client} connected={connection.status === 'connected'} {onError} />
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
            updateBusy={effectiveUpdateBusy}
            {updateMessage}
            {updateFlow}
            onCheckUpdate={() => void checkUpdate()}
            onUpdateNow={() => void updateNow()}
            onRestartUpdate={() => void onRestartUpdate()}
            {onDismissUpdate}
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
  </div>
</section>

<style>
  .settings-panel { container: settings / inline-size; display: grid; width: 100%; height: 100%; min-width: 0; min-height: 0; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; background: var(--background); }
  .settings-header { display: flex; min-height: 76px; align-items: center; justify-content: space-between; gap: 16px; padding: 20px 24px; border-top: 2px solid; border-image: var(--brand-gradient) 1; }
  .settings-header h1 { margin: 0; color: var(--foreground); font-size: var(--font-size-settings-title); font-weight: 650; letter-spacing: -.025em; line-height: 1.25; }
  .settings-layout { display: grid; min-width: 0; min-height: 0; grid-template-columns: 184px minmax(0, 1fr); padding: 0 24px 0 16px; }
  .section-panel { width: 100%; max-width: 1040px; min-width: 0; margin: 0 auto; padding: 0 2px 32px; outline: 0; }
  .section-panel:focus-visible { outline: 2px solid var(--ring); outline-offset: -2px; }
  .section-stack { display: grid; gap: 20px; }
  /* Cards share a reading rhythm while their controls retain their own layouts. */
  .section-panel :global(.card > header), .section-panel :global(section > header:first-child) { padding: 20px; gap: 16px; }
  .section-panel :global(h2) { font-size: 18px; line-height: 1.35; letter-spacing: -.015em; }
  .section-panel :global(p) { line-height: 1.6; }
  .section-panel :global(.eyebrow) { font-family: inherit; font-size: var(--font-size-xs); letter-spacing: 0; text-transform: none; }
  .section-panel :global(.setting-row) { padding-block: 16px; gap: 16px; }
  .section-panel :global(.setting-copy small) { font-size: var(--font-size-xs); line-height: 1.6; }
  @container settings (max-width: 720px) {
    .settings-header { min-height: 64px; padding: 16px; }
    .settings-layout { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); gap: 16px; padding: 0 14px; }
    .section-panel :global(.card > header), .section-panel :global(section > header:first-child) { padding: 16px; }
  }
</style>
