<script lang="ts">
  import { onMount } from 'svelte';

  import type { ConnectionStatus } from '../daemon';
  import type { DaemonSettingsInfo } from '../settings';
  import CopyField from './CopyField.svelte';

  interface Props {
    info: DaemonSettingsInfo;
    connection: ConnectionStatus;
    restarting: boolean;
    onRestart: () => void;
  }

  let { info, connection, restarting, onRestart }: Props = $props();
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
</script>

<section class="card daemon-card" aria-labelledby="daemon-card-title">
  <header>
    <div>
      <span class="eyebrow">Local runtime</span>
      <h2 id="daemon-card-title">Daemon</h2>
      <p>Owns process state, terminal sessions, and coordination data on this machine.</p>
    </div>
    <span class:online={connection.status === 'connected'} class="status">
      <i aria-hidden="true"></i>{connection.status}
    </span>
  </header>

  <div class="runtime-stats">
    <div><span>Port</span><strong>{info.port}</strong></div>
    <div><span>Version</span><strong>v{info.version}</strong></div>
    <div><span>Uptime</span><strong>{formatUptime(uptime)}</strong></div>
    <div><span>PID</span><strong>{info.pid}</strong></div>
  </div>

  <div class="data-path">
    <CopyField label="Data directory" value={info.data_dir} />
    <p>Projects, process metadata, todos, and scratchpads are stored here.</p>
  </div>

  <footer>
    <div>
      <strong>Restart the control plane</strong>
      <span>The desktop reconnects automatically after the local service comes back.</span>
    </div>
    <button
      type="button"
      class="restart"
      disabled={connection.status !== 'connected' || restarting}
      onclick={onRestart}
    >
      <span aria-hidden="true">↻</span>
      {restarting ? 'Restarting…' : 'Restart daemon'}
    </button>
  </footer>
</section>

<style>
  .card {
    border: 1px solid #29444f;
    border-radius: 5px;
    background: rgb(10 28 36 / 91%);
  }

  header,
  footer,
  .status,
  .restart {
    display: flex;
    align-items: center;
  }

  header {
    justify-content: space-between;
    gap: 15px;
    padding: 17px 18px 14px;
  }

  .eyebrow,
  .status,
  .runtime-stats,
  footer span,
  .restart {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  .eyebrow {
    color: #6f8994;
    font-size: 7px;
    font-weight: 650;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  h2 { margin: 3px 0 0; color: #e1ebed; font-size: 17px; }
  header p { margin: 4px 0 0; color: #6e8690; font-size: 10px; line-height: 1.45; }

  .status {
    flex: none;
    gap: 7px;
    border: 1px solid #304a55;
    border-radius: 999px;
    padding: 6px 8px;
    color: #738b95;
    font-size: 7px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .status i { width: 6px; height: 6px; border-radius: 50%; background: #60737d; }
  .status.online i { background: var(--signal); box-shadow: 0 0 8px rgb(99 215 197 / 45%); }

  .runtime-stats {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border-block: 1px solid #243e49;
  }

  .runtime-stats > div { padding: 12px 14px; border-right: 1px solid #243e49; }
  .runtime-stats > div:last-child { border-right: 0; }
  .runtime-stats span, .runtime-stats strong { display: block; }
  .runtime-stats span { color: #59727d; font-size: 7px; text-transform: uppercase; }
  .runtime-stats strong { margin-top: 4px; overflow: hidden; color: #b9cbd0; font-size: 10px; text-overflow: ellipsis; }

  .data-path { padding: 15px 18px; }
  .data-path p { margin: 6px 0 0; color: #627b85; font-size: 9px; }

  footer {
    justify-content: space-between;
    gap: 16px;
    border-top: 1px solid #243e49;
    padding: 13px 18px;
    background: rgb(6 20 27 / 42%);
  }

  footer strong, footer span { display: block; }
  footer strong { color: #9fb3ba; font-size: 10px; }
  footer span { margin-top: 3px; color: #536c77; font-size: 7px; line-height: 1.4; }

  .restart {
    flex: none;
    gap: 7px;
    border: 1px solid #3a5c66;
    border-radius: 3px;
    padding: 8px 10px;
    background: #122d37;
    color: #c1d0d4;
    font-size: 8px;
    font-weight: 650;
    cursor: pointer;
  }

  .restart:hover:not(:disabled) { border-color: var(--signal); color: #eef5f5; }
  .restart:disabled { opacity: 0.45; cursor: default; }
  .restart span { margin: 0; color: var(--signal); font-size: 14px; }

  @media (max-width: 700px) {
    .runtime-stats { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .runtime-stats > div:nth-child(2) { border-right: 0; }
    .runtime-stats > div:nth-child(-n + 2) { border-bottom: 1px solid #243e49; }
    footer { align-items: flex-start; flex-direction: column; }
  }
</style>
