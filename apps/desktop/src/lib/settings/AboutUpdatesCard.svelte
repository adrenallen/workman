<script lang="ts">
  import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
  import CheckCircle2Icon from '@lucide/svelte/icons/circle-check';
  import DownloadIcon from '@lucide/svelte/icons/download';
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
  import PackageIcon from '@lucide/svelte/icons/package';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';

  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { ScrollArea } from '$lib/components/ui/scroll-area';
  import * as Select from '$lib/components/ui/select';
  import { Separator } from '$lib/components/ui/separator';
  import { Switch } from '$lib/components/ui/switch';
  import { cn } from '$lib/utils';
  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import MarkdownView from '../MarkdownView.svelte';
  import type { ConnectionStatus } from '../daemon';
  import type { DaemonSettingsInfo, UpdateChannel } from '../settings';
  import workmanLogo from '../../../../../assets/branding/workman-logo-wide.png';

  const repositoryUrl = 'https://github.com/adrenallen/workman';
  const releasesUrl = `${repositoryUrl}/releases`;
  const changelogUrl = `${repositoryUrl}/blob/main/CHANGELOG.md`;

  interface Props {
    info: DaemonSettingsInfo;
    connection: ConnectionStatus;
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
    updateBusy,
    updateMessage,
    onCheckUpdate,
    onUpdateNow,
    onAutomaticChecks,
    onUpdateChannel
  }: Props = $props();

  let updateDialogOpen = $state(false);
  let update = $derived(info.update);
  let check = $derived(update.check);
  let hasChecked = $derived(update.last_checked_at !== null && check.checked_at > 0);
  let connected = $derived(connection.status === 'connected');
  let currentReleaseUrl = $derived(`${releasesUrl}/tag/v${encodeURIComponent(check.current)}`);

  function formatChecked(timestamp: number | null): string {
    if (!timestamp) return 'Not checked yet';
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short'
    }).format(new Date(timestamp * 1000));
  }

  function chooseChannel(value: string): void {
    if (value === 'stable' || value === 'latest') onUpdateChannel(value);
  }

  function confirmUpdate(): void {
    updateDialogOpen = false;
    onUpdateNow();
  }
</script>

<section class="overflow-hidden rounded-md border bg-card text-card-foreground" aria-labelledby="about-updates-title">
  <header class="flex flex-wrap items-start justify-between gap-4 px-4 py-3">
    <div class="min-w-0">
      <p class="font-mono text-xs font-semibold tracking-[0.08em] text-muted-foreground uppercase">Release desk</p>
      <h2 id="about-updates-title" class="mt-1 text-lg font-semibold tracking-tight">About Workman</h2>
      <p class="mt-1 max-w-2xl text-sm leading-5 text-muted-foreground">
        Keep the desktop and its local control plane on the release stream you choose.
      </p>
    </div>
    <Badge variant="outline" class="gap-1.5 font-mono">
      <StatusIndicator
        tone={connected ? 'success' : 'danger'}
        label={connected ? 'Desktop connected to daemon' : `Daemon ${connection.status}`}
      />
      {connected ? 'Connected' : connection.status}
    </Badge>
  </header>

  <Separator />

  <div class="grid overflow-hidden bg-black text-white sm:grid-cols-[minmax(0,1fr)_170px]">
    <img
      src={workmanLogo}
      alt="Workman"
      class="h-28 w-full object-cover object-center sm:h-32"
    />
    <div class="flex items-center justify-between gap-4 border-t border-white/10 px-4 py-3 sm:block sm:border-t-0 sm:border-l">
      <div>
        <span class="block text-xs font-medium text-zinc-400">Desktop app</span>
        <strong class="mt-1 block text-base font-semibold">Workman</strong>
      </div>
      <code class="text-xs text-zinc-300 sm:mt-3 sm:block">v{connection.app_version}</code>
    </div>
  </div>

  <Separator />

  <div class="grid grid-cols-1 divide-y divide-border sm:grid-cols-2 sm:divide-x sm:divide-y-0">
    <div class="flex items-center gap-3 px-4 py-3">
      <span class="grid size-8 shrink-0 place-items-center rounded-md border bg-muted text-muted-foreground">
        <PackageIcon class="size-4" aria-hidden="true" />
      </span>
      <div class="min-w-0">
        <span class="block text-xs font-medium text-muted-foreground">Desktop app</span>
        <strong class="mt-0.5 block truncate font-mono text-sm">Workman {connection.app_version}</strong>
      </div>
    </div>
    <div class="flex items-center gap-3 px-4 py-3">
      <span class="grid size-8 shrink-0 place-items-center rounded-md border bg-muted text-muted-foreground">
        <PackageIcon class="size-4" aria-hidden="true" />
      </span>
      <div class="min-w-0">
        <span class="block text-xs font-medium text-muted-foreground">Local daemon</span>
        <strong class="mt-0.5 block truncate font-mono text-sm">workmand {info.version}</strong>
      </div>
    </div>
  </div>

  <Separator />

  <div class="grid gap-3 p-4 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-center">
    <div>
      <label for="update-channel" class="text-sm font-medium">Release channel</label>
      <p class="mt-1 text-xs leading-5 text-muted-foreground">
        Stable follows promoted releases. Latest also sees prerelease builds.
      </p>
    </div>
    <Select.Root
      type="single"
      value={update.channel}
      disabled={!connected || updateBusy !== null}
      onValueChange={chooseChannel}
    >
      <Select.Trigger id="update-channel" size="default" class="w-full min-w-52 lg:w-60">
        {update.channel === 'stable' ? 'Stable · recommended' : 'Latest · prereleases'}
      </Select.Trigger>
      <Select.Content>
        <Select.Item value="stable" label="Stable · recommended" />
        <Select.Item value="latest" label="Latest · includes prereleases" />
      </Select.Content>
    </Select.Root>
  </div>

  <Separator />

  <div class="p-4">
    <div
      class={cn(
        'rounded-md border p-3',
        hasChecked && !check.available && 'border-success bg-success/5',
        check.available && 'border-warning bg-warning/5'
      )}
      aria-live="polite"
    >
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="flex min-w-0 gap-3">
          <span
            class={cn(
              'grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground',
              hasChecked && !check.available && 'bg-success/10 text-success',
              check.available && 'bg-warning/10 text-warning'
            )}
          >
            {#if check.available}
              <DownloadIcon class="size-4" aria-hidden="true" />
            {:else if hasChecked}
              <CheckCircle2Icon class="size-4" aria-hidden="true" />
            {:else}
              <CalendarClockIcon class="size-4" aria-hidden="true" />
            {/if}
          </span>
          <div class="min-w-0">
            {#if check.available}
              <div class="flex flex-wrap items-center gap-2">
                <strong class="text-sm">Workman {check.latest} is available</strong>
                {#if check.prerelease}<Badge variant="secondary">Prerelease</Badge>{/if}
              </div>
              <p class="mt-1 text-xs leading-5 text-muted-foreground">
                You are running {check.current} on the {update.channel} channel.
              </p>
            {:else if hasChecked}
              <strong class="text-sm">Workman {check.current} is up to date</strong>
              <p class="mt-1 text-xs leading-5 text-muted-foreground">
                No newer {update.channel} release was found.
              </p>
            {:else}
              <strong class="text-sm">Ready to check</strong>
              <p class="mt-1 text-xs leading-5 text-muted-foreground">
                Workman has not checked the {update.channel} channel yet.
              </p>
            {/if}
          </div>
        </div>
        <div class="flex flex-wrap gap-2">
          <Button
            variant={check.available ? 'outline' : 'default'}
            size="sm"
            disabled={!connected || updateBusy !== null}
            onclick={onCheckUpdate}
          >
            <RefreshCwIcon class={updateBusy === 'check' ? 'animate-spin' : undefined} aria-hidden="true" />
            {updateBusy === 'check' ? 'Checking…' : 'Check for updates'}
          </Button>
          {#if check.available}
            <Button
              size="sm"
              disabled={!connected || updateBusy !== null}
              onclick={() => (updateDialogOpen = true)}
            >
              <DownloadIcon aria-hidden="true" />
              {updateBusy === 'apply' ? 'Updating…' : 'Update now'}
            </Button>
          {/if}
        </div>
      </div>

      {#if check.available && check.notes.trim()}
        <Separator class="my-3" />
        <div>
          <div class="mb-2 flex items-center justify-between gap-3">
            <strong class="text-xs font-semibold tracking-[0.04em] text-muted-foreground uppercase">Release notes</strong>
            <Button variant="link" size="xs" href={check.url} target="_blank" rel="noreferrer">
              View release <ExternalLinkIcon aria-hidden="true" />
            </Button>
          </div>
          <ScrollArea class="max-h-52 rounded-md border bg-background p-3">
            <MarkdownView source={check.notes} />
          </ScrollArea>
        </div>
      {/if}

      {#if updateMessage}
        <p class="mt-3 border-t pt-3 font-mono text-xs leading-5 text-muted-foreground">{updateMessage}</p>
      {/if}
    </div>
  </div>

  <Separator />

  <div class="flex flex-wrap items-center justify-between gap-4 px-4 py-3">
    <div class="flex min-w-0 items-center gap-3">
      <Switch
        id="automatic-update-checks"
        size="sm"
        checked={update.automatic_checks}
        disabled={!connected || updateBusy !== null}
        onCheckedChange={(checked) => onAutomaticChecks(checked === true)}
      />
      <label for="automatic-update-checks" class="min-w-0">
        <span class="block text-sm font-medium">Check weekly when Workman starts</span>
        <span class="mt-0.5 block text-xs text-muted-foreground">A quiet background check; Workman never installs automatically.</span>
      </label>
    </div>
    <span class="flex items-center gap-1.5 font-mono text-xs text-muted-foreground">
      <CalendarClockIcon class="size-3.5" aria-hidden="true" />
      Last checked {formatChecked(update.last_checked_at)}
    </span>
  </div>

  <Separator />

  <footer class="flex flex-wrap items-center gap-x-1 gap-y-1 px-3 py-2">
    <Button variant="link" size="sm" href={repositoryUrl} target="_blank" rel="noreferrer">
      GitHub repository <ExternalLinkIcon aria-hidden="true" />
    </Button>
    <Button variant="link" size="sm" href={currentReleaseUrl} target="_blank" rel="noreferrer">
      Current release notes <ExternalLinkIcon aria-hidden="true" />
    </Button>
    <Button variant="link" size="sm" href={changelogUrl} target="_blank" rel="noreferrer">
      CHANGELOG <ExternalLinkIcon aria-hidden="true" />
    </Button>
  </footer>
</section>

<AlertDialog.Root bind:open={updateDialogOpen}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>Update to Workman {check.latest}?</AlertDialog.Title>
      <AlertDialog.Description>
        Workman will download and verify the release, replace the CLI and daemon in the configured install directory, then restart the daemon. Running project processes will stop.
      </AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
      <AlertDialog.Action onclick={confirmUpdate}>
        <DownloadIcon aria-hidden="true" />
        Update and restart
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
