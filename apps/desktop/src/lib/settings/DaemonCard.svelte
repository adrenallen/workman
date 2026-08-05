<script lang="ts">
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import { onMount } from 'svelte';

  import { Button } from '$lib/components/ui/button';
  import { Switch } from '$lib/components/ui/switch';
  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import type { ConnectionStatus } from '../daemon';
  import type { DaemonSettingsInfo, UpdateChannel } from '../settings';
  import CopyField from './CopyField.svelte';

  interface Props {
    info: DaemonSettingsInfo;
    connection: ConnectionStatus;
    restarting: boolean;
    onRestart: () => void;
    updateBusy: 'check' | 'apply' | 'preference' | null;
    updateMessage: string | null;
    onCheckUpdate: () => void;
    onUpdateNow: () => void;
    onAutomaticChecks: (enabled: boolean) => void;
    onUpdateChannel: (channel: UpdateChannel) => void;
  }

  let {
    info,
    connection,
    restarting,
    updateBusy,
    updateMessage,
    onRestart,
    onCheckUpdate,
    onUpdateNow,
    onAutomaticChecks,
    onUpdateChannel
  }: Props = $props();
  let observedAt = $state(Date.now());
  let now = $state(Date.now());
  let uptime = $derived(info.uptime_ms + Math.max(0, now - observedAt));

  $effect(() => {
    info.uptime_ms;
    observedAt = Date.now();
    now = observedAt;
  });

  onMount(() => {
    const ticker = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(ticker);
  });

  function formatUptime(milliseconds: number): string {
    const totalSeconds = Math.floor(milliseconds / 1000);
    const days = Math.floor(totalSeconds / 86_400);
    const hours = Math.floor((totalSeconds % 86_400) / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    if (days > 0) return `${days}d ${hours}h ${minutes}m`;
    if (hours > 0) return `${hours}h ${minutes}m ${seconds}s`;
    if (minutes > 0) return `${minutes}m ${seconds}s`;
    return `${seconds}s`;
  }

  function formatChecked(timestamp: number | null): string {
    if (!timestamp) return 'Never';
    return new Date(timestamp * 1000).toLocaleString();
  }
</script>

<section class="card daemon-card" aria-labelledby="daemon-card-title">
  <header>
    <div>
      <span class="eyebrow">Local runtime</span>
      <h2 id="daemon-card-title">Daemon</h2>
      <p>Owns process state, terminal sessions, and coordination data on this machine.</p>
    </div>
    <span class="status">
      <StatusIndicator
        tone={connection.status === 'connected' ? 'success' : 'danger'}
        label={connection.status === 'connected' ? `Daemon connected · port ${info.port}` : `Daemon ${connection.status}`}
      />
      {connection.status}
    </span>
  </header>

  <div class="runtime-stats">
    <div><span>Port</span><strong>{info.port}</strong></div>
    <div><span>App</span><strong>v{connection.app_version}</strong></div>
    <div><span>Daemon</span><strong>v{info.version}</strong></div>
    <div><span>Uptime</span><strong>{formatUptime(uptime)}</strong></div>
    <div><span>PID</span><strong>{info.pid}</strong></div>
  </div>

  <div class="data-path">
    <CopyField label="Data directory" value={info.data_dir} />
    <p>Projects, process metadata, todos, and scratchpads are stored here.</p>
  </div>

  <section class="updates" aria-labelledby="updates-title">
    <div class="update-heading">
      <div>
        <strong id="updates-title">About &amp; updates</strong>
        <span>Current {info.update.check.current} · Latest {info.update.check.latest}</span>
      </div>
      <div class="update-actions">
        <Button
          variant="outline"
          size="sm"
          disabled={connection.status !== 'connected' || updateBusy !== null}
          onclick={onCheckUpdate}
        >{updateBusy === 'check' ? 'Checking…' : 'Check for updates'}</Button>
        {#if info.update.check.available}
          <Button
            size="sm"
            disabled={connection.status !== 'connected' || updateBusy !== null}
            onclick={onUpdateNow}
          >{updateBusy === 'apply' ? 'Updating…' : `Update to ${info.update.check.latest}`}</Button>
        {/if}
      </div>
    </div>
    {#if info.update.check.available && info.update.check.notes}
      <p class="release-notes">{info.update.check.notes}</p>
    {/if}
    {#if updateMessage}<p class="update-message" aria-live="polite">{updateMessage}</p>{/if}
    <div class="update-preference">
      <label>
        <Switch
          size="sm"
          checked={info.update.automatic_checks}
          disabled={updateBusy !== null}
          onCheckedChange={(checked) => onAutomaticChecks(checked === true)}
        />
        Check weekly when awm starts
      </label>
      <span>Last checked: {formatChecked(info.update.last_checked_at)}</span>
    </div>
    <div class="update-channel">
      <label for="update-channel">Release channel</label>
      <select
        id="update-channel"
        value={info.update.channel}
        disabled={updateBusy !== null}
        onchange={(event) => onUpdateChannel(event.currentTarget.value as UpdateChannel)}
      >
        <option value="stable">Stable (recommended)</option>
        <option value="latest">Latest (includes prereleases)</option>
      </select>
      <span>{info.update.channel === 'stable' ? 'Only promoted releases.' : 'Newest release, including prereleases.'}</span>
    </div>
  </section>

  <footer>
    <div>
      <strong>Restart the control plane</strong>
      <span>The desktop reconnects automatically after the local service comes back.</span>
    </div>
    <Button
      variant="outline"
      size="sm"
      class="shrink-0"
      disabled={connection.status !== 'connected' || restarting}
      onclick={onRestart}
    >
      <RefreshCwIcon size={14} aria-hidden="true" />
      {restarting ? 'Restarting…' : 'Restart daemon'}
    </Button>
  </footer>
</section>

<style>
  .card {
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
  }

  header,
  footer,
  .status {
    display: flex;
    align-items: center;
  }

  header {
    justify-content: space-between;
    gap: 15px;
    padding: 11px 12px 10px;
  }

  .eyebrow,
  .status,
  .runtime-stats,
  footer span {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  .eyebrow {
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    font-weight: 650;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  h2 { margin: 2px 0 0; color: var(--foreground); font-size: 16px; }
  header p { margin: 3px 0 0; color: var(--text-soft); font-size: var(--font-size-sm); line-height: 1.4; }

  .status {
    flex: none;
    gap: 7px;
    border: 1px solid #304a55;
    border-radius: 999px;
    padding: 6px 8px;
    color: #738b95;
    font-size: var(--font-size-xs);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }


  .runtime-stats {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    border-block: 1px solid var(--border);
  }

  .runtime-stats > div { padding: 8px 10px; border-right: 1px solid var(--border); }
  .runtime-stats > div:last-child { border-right: 0; }
  .runtime-stats span, .runtime-stats strong { display: block; }
  .runtime-stats span { color: #59727d; font-size: var(--font-size-xs); text-transform: uppercase; }
  .runtime-stats strong { margin-top: 4px; overflow: hidden; color: var(--text-soft); font-size: var(--font-size-sm); text-overflow: ellipsis; }

  .data-path { padding: 10px 12px; }
  .data-path p { margin: 6px 0 0; color: #627b85; font-size: var(--font-size-sm); }

  .updates { border-top: 1px solid var(--border); padding: 10px 12px; }
  .update-heading, .update-actions, .update-preference, .update-channel { display: flex; align-items: center; }
  .update-heading, .update-preference { justify-content: space-between; gap: 12px; }
  .update-heading strong, .update-heading span { display: block; }
  .update-heading strong { color: var(--text); font-size: var(--font-size-sm); }
  .update-heading span, .update-preference, .update-message, .release-notes {
    color: var(--muted);
    font: var(--font-size-xs)/1.45 'JetBrains Mono Variable', monospace;
  }
  .update-heading span { margin-top: 2px; }
  .update-actions { flex-wrap: wrap; justify-content: flex-end; gap: 6px; }
  .release-notes { max-height: 74px; margin: 8px 0 0; overflow: auto; white-space: pre-wrap; }
  .update-message { margin: 7px 0 0; color: var(--text-soft); }
  .update-preference { margin-top: 8px; }
  .update-preference label { display: flex; align-items: center; gap: 6px; color: var(--text-soft); }
  .update-channel { gap: 7px; margin-top: 8px; color: var(--text-soft); font: var(--font-size-xs)/1.45 'JetBrains Mono Variable', monospace; }
  .update-channel label { flex: none; }
  .update-channel select {
    height: 26px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 7px;
    background: var(--surface-raised); color: var(--text-soft); font: inherit;
  }
  .update-channel select:disabled { opacity: .45; }
  .update-channel span { color: var(--muted); }

  footer {
    justify-content: space-between;
    gap: 16px;
    border-top: 1px solid var(--border);
    padding: 9px 12px;
    background: var(--card);
  }

  footer strong, footer span { display: block; }
  footer strong { color: #9fb3ba; font-size: var(--font-size-sm); }
  footer span { margin-top: 3px; color: #536c77; font-size: var(--font-size-xs); line-height: 1.4; }

  @media (max-width: 700px) {
    .runtime-stats { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .runtime-stats > div:nth-child(even) { border-right: 0; }
    .runtime-stats > div:not(:last-child) { border-bottom: 1px solid #243e49; }
    footer { align-items: flex-start; flex-direction: column; }
  }
</style>
