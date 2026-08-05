<script lang="ts">
  import AlertTriangleIcon from '@lucide/svelte/icons/triangle-alert';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import WrenchIcon from '@lucide/svelte/icons/wrench';

  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Separator } from '$lib/components/ui/separator';
  import type { DaemonClient, WorktreeHealth, WorktreeHealthCheck } from '../daemon';

  interface Props {
    client: DaemonClient;
    connected: boolean;
    onError: (message: string) => void;
  }

  let { client, connected, onError }: Props = $props();
  let health = $state<WorktreeHealth | null>(null);
  let loading = $state(false);
  let loadedWhileConnected = $state(false);

  $effect(() => {
    if (!connected) {
      loadedWhileConnected = false;
    } else if (!loadedWhileConnected && !loading) {
      loadedWhileConnected = true;
      void refresh();
    }
  });

  async function refresh(): Promise<void> {
    if (!connected || loading) return;
    loading = true;
    try {
      health = await client.worktreeHealth();
    } catch (cause) {
      loadedWhileConnected = false;
      onError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      loading = false;
    }
  }

  function tone(check: WorktreeHealthCheck): 'success' | 'warning' | 'danger' {
    if (check.status === 'ready') return 'success';
    if (check.required || check.status === 'missing') return 'danger';
    return 'warning';
  }

  function statusLabel(check: WorktreeHealthCheck): string {
    if (check.status === 'ready') return `${check.label} ready · ${check.detail}`;
    return `${check.label} ${check.status} · ${check.detail}`;
  }
</script>

<section class="overflow-hidden rounded-md border bg-card text-card-foreground" aria-labelledby="worktree-health-title">
  <header class="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
    <div class="flex min-w-0 gap-3">
      <span class="grid size-8 shrink-0 place-items-center rounded-md border bg-muted text-muted-foreground">
        <GitBranchIcon class="size-4" aria-hidden="true" />
      </span>
      <div class="min-w-0">
        <p class="font-mono text-xs font-semibold tracking-[0.08em] text-muted-foreground uppercase">Runtime Doctor</p>
        <h2 id="worktree-health-title" class="mt-1 text-base font-semibold tracking-tight">Worktrees</h2>
        <p class="mt-1 text-sm leading-5 text-muted-foreground">
          Git is required. GitHub CLI and Laravel Herd add PR status and local .test sites.
        </p>
      </div>
    </div>
    <div class="flex items-center gap-2">
      {#if health}
        <Badge variant="outline" class="gap-1.5 font-mono">
          <StatusIndicator
            tone={health.all_required_ready ? 'success' : 'danger'}
            label={health.summary}
          />
          {health.summary}
        </Badge>
      {/if}
      <Button variant="outline" size="sm" disabled={!connected || loading} onclick={() => void refresh()}>
        <RefreshCwIcon class={loading ? 'animate-spin' : undefined} aria-hidden="true" />
        {loading ? 'Checking…' : 'Refresh'}
      </Button>
    </div>
  </header>

  <Separator />

  {#if health}
    <div class="divide-y divide-border">
      {#each health.checks as check (check.id)}
        <article class="grid gap-3 px-4 py-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start">
          <div class="flex min-w-0 gap-3">
            <StatusIndicator tone={tone(check)} label={statusLabel(check)} class="mt-0.5" />
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2">
                <strong class="text-sm">{check.label}</strong>
                <Badge variant={check.required ? 'secondary' : 'outline'}>
                  {check.required ? 'Required' : 'Optional'}
                </Badge>
                {#if check.version}
                  <code class="truncate font-mono text-xs text-muted-foreground">{check.version}</code>
                {/if}
              </div>
              <p class="mt-1 text-xs leading-5 text-muted-foreground">{check.detail}</p>
              {#if check.fix_hint}
                <p class="mt-2 flex gap-2 rounded-md border border-warning/40 bg-warning/5 px-2.5 py-2 text-xs leading-5 text-muted-foreground">
                  <WrenchIcon class="mt-0.5 size-3.5 shrink-0 text-warning" aria-hidden="true" />
                  <span><strong class="text-foreground">Fix hint:</strong> {check.fix_hint}</span>
                </p>
              {/if}
            </div>
          </div>
          <span class="font-mono text-xs capitalize text-muted-foreground">{check.status}</span>
        </article>
      {/each}
    </div>
  {:else if loading}
    <div class="flex min-h-24 items-center justify-center gap-2 p-4 text-sm text-muted-foreground">
      <RefreshCwIcon class="size-4 animate-spin" aria-hidden="true" />
      Checking worktree dependencies…
    </div>
  {:else}
    <div class="flex min-h-24 items-center justify-center gap-2 p-4 text-sm text-muted-foreground">
      <AlertTriangleIcon class="size-4" aria-hidden="true" />
      Connect the daemon to inspect worktree dependencies.
    </div>
  {/if}
</section>
