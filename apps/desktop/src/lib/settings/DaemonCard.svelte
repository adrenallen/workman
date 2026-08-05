<script lang="ts">
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import ServerIcon from '@lucide/svelte/icons/server';
  import { onMount } from 'svelte';

  import { Button } from '$lib/components/ui/button';
  import { Separator } from '$lib/components/ui/separator';
  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
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

<section class="overflow-hidden rounded-md border bg-card text-card-foreground" aria-labelledby="daemon-card-title">
  <header class="flex flex-wrap items-start justify-between gap-4 px-4 py-3">
    <div class="flex min-w-0 gap-3">
      <span class="grid size-9 shrink-0 place-items-center rounded-md border bg-muted text-muted-foreground">
        <ServerIcon class="size-4" aria-hidden="true" />
      </span>
      <div>
        <p class="font-mono text-xs font-semibold tracking-[0.08em] text-muted-foreground uppercase">Local runtime</p>
        <h2 id="daemon-card-title" class="mt-1 text-lg font-semibold tracking-tight">Daemon</h2>
        <p class="mt-1 max-w-2xl text-sm leading-5 text-muted-foreground">
          Owns process state, terminal sessions, and coordination data on this machine.
        </p>
      </div>
    </div>
    <span class="flex items-center gap-1.5 rounded-full border px-2.5 py-1 font-mono text-xs text-muted-foreground capitalize">
      <StatusIndicator
        tone={connection.status === 'connected' ? 'success' : 'danger'}
        label={connection.status === 'connected' ? `Daemon connected · port ${info.port}` : `Daemon ${connection.status}`}
      />
      {connection.status}
    </span>
  </header>

  <Separator />

  <dl class="grid grid-cols-1 divide-y divide-border font-mono sm:grid-cols-3 sm:divide-x sm:divide-y-0">
    <div class="px-4 py-3">
      <dt class="text-xs tracking-[0.06em] text-muted-foreground uppercase">Port</dt>
      <dd class="mt-1 truncate text-sm font-semibold">{info.port}</dd>
    </div>
    <div class="px-4 py-3">
      <dt class="text-xs tracking-[0.06em] text-muted-foreground uppercase">Uptime</dt>
      <dd class="mt-1 truncate text-sm font-semibold">{formatUptime(uptime)}</dd>
    </div>
    <div class="px-4 py-3">
      <dt class="text-xs tracking-[0.06em] text-muted-foreground uppercase">PID</dt>
      <dd class="mt-1 truncate text-sm font-semibold">{info.pid}</dd>
    </div>
  </dl>

  <Separator />

  <div class="px-4 py-3">
    <CopyField label="Data directory" value={info.data_dir} />
    <p class="mt-2 text-xs leading-5 text-muted-foreground">
      Projects, process metadata, todos, and scratchpads are stored here.
    </p>
  </div>

  <Separator />

  <footer class="flex flex-wrap items-center justify-between gap-4 bg-muted/40 px-4 py-3">
    <div>
      <strong class="block text-sm font-medium">Restart the control plane</strong>
      <span class="mt-1 block text-xs text-muted-foreground">The desktop reconnects after the local service comes back.</span>
    </div>
    <Button
      variant="outline"
      size="sm"
      disabled={connection.status !== 'connected' || restarting}
      onclick={onRestart}
    >
      <RefreshCwIcon class={restarting ? 'animate-spin' : undefined} aria-hidden="true" />
      {restarting ? 'Restarting…' : 'Restart daemon'}
    </Button>
  </footer>
</section>
