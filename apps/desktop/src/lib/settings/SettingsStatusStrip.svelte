<script lang="ts">
  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import type { ConnectionStatus } from '../daemon';
  import type { DaemonSettingsInfo } from '../settings';

  interface Props {
    connection: ConnectionStatus;
    info: DaemonSettingsInfo | null;
  }

  let { connection, info }: Props = $props();
  let daemonLabel = $derived(
    connection.status === 'connected'
      ? `Daemon ${info?.version ?? connection.daemon_version ?? ''}`.trim()
      : connection.status === 'connecting' ? 'Daemon connecting' : 'Daemon offline'
  );

</script>

<div class="status-strip" aria-label="Settings status">
  <span class="daemon" title={connection.message ?? daemonLabel}>
    <StatusIndicator
      tone={connection.status === 'connected' ? 'success' : connection.status === 'connecting' ? 'warning' : 'danger'}
      label={connection.status === 'connected' ? `Daemon connected · port ${info?.port ?? connection.port ?? 'unknown'}` : daemonLabel}
    />
    {daemonLabel}

  </span>
</div>

<style>
  .status-strip, .daemon { display: flex; min-width: 0; align-items: center; gap: 8px; }
  .status-strip { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .daemon { white-space: nowrap; }
</style>
