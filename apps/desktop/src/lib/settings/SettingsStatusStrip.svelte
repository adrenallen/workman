<script lang="ts">
  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import type { ConnectionStatus, Project } from '../daemon';
  import type { DaemonSettingsInfo } from '../settings';

  interface Props {
    project: Project;
    connection: ConnectionStatus;
    info: DaemonSettingsInfo | null;
  }

  let { project, connection, info }: Props = $props();

  let projectName = $derived(project.display_name?.trim() || project.name || project.path.split('/').filter(Boolean).at(-1) || 'Project');
  let daemonLabel = $derived(
    connection.status === 'connected'
      ? `Daemon ${info?.version ?? connection.daemon_version ?? ''}`.trim()
      : connection.status === 'connecting' ? 'Daemon connecting' : 'Daemon offline'
  );

  function uptimeLabel(milliseconds: number | undefined): string {
    if (!milliseconds) return 'Starting';
    const minutes = Math.max(1, Math.floor(milliseconds / 60_000));
    if (minutes < 60) return `${minutes}m uptime`;
    const hours = Math.floor(minutes / 60);
    return hours < 24 ? `${hours}h uptime` : `${Math.floor(hours / 24)}d uptime`;
  }
</script>

<div class="status-strip" aria-label="Settings status">
  <span class="context"><strong>Settings</strong><i aria-hidden="true">/</i>{projectName}</span>
  <span class="saved">
    <StatusIndicator tone="success" label="Preferences saved locally on this Mac" />
    Preferences saved on this Mac
  </span>
  <span class="daemon" title={connection.message ?? daemonLabel}>
    <StatusIndicator
      tone={connection.status === 'connected' ? 'success' : connection.status === 'connecting' ? 'warning' : 'danger'}
      label={connection.status === 'connected' ? `Daemon connected · port ${info?.port ?? connection.port ?? 'unknown'}` : daemonLabel}
    />
    {daemonLabel}
    {#if info}<small>· {uptimeLabel(info.uptime_ms)} · :{info.port}</small>{/if}
  </span>
</div>

<style>
  .status-strip { display: flex; min-height: 31px; align-items: center; gap: 12px; border: 1px solid var(--border); border-radius: 4px; padding: 5px 9px; background: color-mix(in srgb, var(--night) 72%, var(--surface)); color: var(--muted); font: var(--font-size-xs)/1.2 'JetBrains Mono Variable', monospace; }
  .context, .saved, .daemon { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .context strong { color: var(--text-soft); font-weight: 700; }
  .context > i { color: var(--border-strong); font-style: normal; }
  .saved { margin-left: auto; }
  .daemon { overflow: hidden; padding-left: 11px; border-left: 1px solid var(--border); text-overflow: ellipsis; white-space: nowrap; }
  .daemon small { color: var(--muted); font: inherit; }

  @media (max-width: 760px) { .saved { display: none; } .daemon { margin-left: auto; } }
  @media (max-width: 520px) { .daemon small { display: none; } }
</style>
