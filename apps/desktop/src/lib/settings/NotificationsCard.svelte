<script lang="ts">
  import BellIcon from '@lucide/svelte/icons/bell';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';

  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Separator } from '$lib/components/ui/separator';
  import { Switch } from '$lib/components/ui/switch';
  import {
    nativeNotificationPreferences,
    nativeNotificationRuntime,
    openNativeNotificationSettings,
    refreshNativeNotificationPermission,
    requestNativeNotificationPermission,
    setNativeNotificationsEnabled,
    setNeedsInputNotificationsEnabled,
    setTopLevelNotificationsOnly,
    type NativeNotificationPermissionState
  } from '../nativeNotifications';

  let openingSettings = $state(false);
  let showSettingsShortcut = $derived(
    $nativeNotificationPreferences.enabled && (
      ['denied', 'unavailable', 'unknown'].includes($nativeNotificationRuntime.permission.state)
      || $nativeNotificationRuntime.permission.platform === 'linux'
    )
  );

  async function openSettings(): Promise<void> {
    if (openingSettings) return;
    openingSettings = true;
    try { await openNativeNotificationSettings(); }
    catch { /* The notification runtime displays the launch error below. */ }
    finally { openingSettings = false; }
  }

  let permissionLabel = $derived.by(() => {
    switch ($nativeNotificationRuntime.permission.state) {
      case 'granted': return 'Allowed by the operating system';
      case 'denied': return 'Denied in system settings';
      case 'not_determined': return 'Permission not requested';
      case 'checking': return 'Checking permission';
      case 'unavailable': return 'Permission state unavailable';
      default: return 'Unknown permission state';
    }
  });

  let permissionTone = $derived.by(() => {
    const state: NativeNotificationPermissionState = $nativeNotificationRuntime.permission.state;
    if (state === 'granted') return 'success' as const;
    if (state === 'denied' || state === 'unavailable') return 'danger' as const;
    if (state === 'not_determined' || state === 'unknown') return 'warning' as const;
    return 'neutral' as const;
  });

  $effect(() => {
    if ($nativeNotificationPreferences.enabled) void refreshNativeNotificationPermission();
  });
</script>

<section class="overflow-hidden rounded-md border bg-card text-card-foreground" aria-labelledby="notifications-card-title">
  <header class="flex flex-wrap items-start justify-between gap-4 px-4 py-3">
    <div class="flex min-w-0 gap-3">
      <span class="grid size-9 shrink-0 place-items-center rounded-md border bg-muted text-muted-foreground">
        <BellIcon class="size-4" aria-hidden="true" />
      </span>
      <div>
        <p class="font-mono text-xs font-semibold tracking-[0.08em] text-muted-foreground uppercase">Attention</p>
        <h2 id="notifications-card-title" class="mt-1 text-lg font-semibold tracking-tight">Notifications</h2>
        <p class="mt-1 max-w-2xl text-sm leading-5 text-muted-foreground">
          Keep notifications inside Workman or also show them on your computer.
        </p>
      </div>
    </div>
    {#if $nativeNotificationPreferences.enabled}
      <span class="flex items-center gap-1.5 rounded-full border px-2.5 py-1 font-mono text-xs text-muted-foreground">
        <StatusIndicator tone={permissionTone} label={permissionLabel} />
        {permissionLabel}
      </span>
    {/if}
  </header>

  <Separator />

  <div class="flex flex-wrap items-center justify-between gap-4 px-4 py-3">
    <div class="flex min-w-0 flex-1 items-center gap-3">
      <Switch
        id="native-notifications-enabled"
        size="sm"
        checked={$nativeNotificationPreferences.enabled}
        onCheckedChange={(checked) => setNativeNotificationsEnabled(checked === true)}
      />
      <label for="native-notifications-enabled" class="min-w-0">
        <span class="block text-sm font-medium">Computer notifications</span>
        <span class="mt-0.5 block text-xs leading-5 text-muted-foreground">Show background banners, system notification alerts, and Dock/taskbar badges. Turn off to keep new notifications inside Workman.</span>
      </label>
    </div>
    <span class="font-mono text-xs text-muted-foreground">
      {$nativeNotificationPreferences.enabled ? 'In-app + computer' : 'In-app only'}
    </span>
  </div>

  <Separator />

  <div class="flex flex-wrap items-center justify-between gap-4 px-4 py-3">
    <div class="flex min-w-0 flex-1 items-center gap-3">
      <Switch
        id="needs-input-notifications-enabled"
        size="sm"
        checked={$nativeNotificationPreferences.needsInput}
        disabled={!$nativeNotificationPreferences.enabled}
        onCheckedChange={(checked) => setNeedsInputNotificationsEnabled(checked === true)}
      />
      <label for="needs-input-notifications-enabled" class="min-w-0">
        <span class="block text-sm font-medium">Agent needs input</span>
        <span class="mt-0.5 block text-xs leading-5 text-muted-foreground">Send a banner when an unwatched agent reaches a new prompt that needs you.</span>
      </label>
    </div>
    <span class="font-mono text-xs text-muted-foreground">
      {$nativeNotificationPreferences.needsInput ? 'On' : 'Off'}
    </span>
  </div>

  <Separator />

  <div class="flex flex-wrap items-center justify-between gap-4 px-4 py-3">
    <div class="flex min-w-0 flex-1 items-center gap-3">
      <Switch
        id="top-level-notifications-only"
        size="sm"
        checked={$nativeNotificationPreferences.topLevelOnly}
        disabled={!$nativeNotificationPreferences.enabled}
        onCheckedChange={(checked) => setTopLevelNotificationsOnly(checked === true)}
      />
      <label for="top-level-notifications-only" class="min-w-0">
        <span class="block text-sm font-medium">Top-level agents only</span>
        <span class="mt-0.5 block text-xs leading-5 text-muted-foreground">Skip completion and input banners for agents spawned by another agent. Their activity remains in Workman.</span>
      </label>
    </div>
    <span class="font-mono text-xs text-muted-foreground">
      {$nativeNotificationPreferences.topLevelOnly ? 'On' : 'Off'}
    </span>
  </div>

  <Separator />

  <div class="grid gap-3 px-4 py-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
    <div>
      <strong class="block text-sm font-medium">Computer notification permission</strong>
      <p class="mt-1 text-xs leading-5 text-muted-foreground">
        {$nativeNotificationPreferences.enabled
          ? ($nativeNotificationRuntime.permission.detail ?? permissionLabel)
          : 'Turn on computer notifications to check system permission. In-app notifications do not need permission.'}
      </p>
      {#if $nativeNotificationPreferences.enabled && $nativeNotificationRuntime.error}
        <p class="mt-1 font-mono text-xs leading-5 text-destructive">{$nativeNotificationRuntime.error}</p>
      {/if}
      {#if showSettingsShortcut}
        <p class="mt-1 text-xs leading-5 text-muted-foreground">
          Enable notifications for Workman in system settings. Permission updates when you return to the app.
        </p>
      {/if}
    </div>
    <div class="flex flex-wrap gap-2">
      <Button
        variant="outline"
        size="sm"
        disabled={!$nativeNotificationPreferences.enabled || $nativeNotificationRuntime.busy}
        onclick={() => void refreshNativeNotificationPermission()}
      >
        <RefreshCwIcon class={$nativeNotificationRuntime.busy ? 'animate-spin' : undefined} aria-hidden="true" />
        Refresh
      </Button>
      {#if $nativeNotificationPreferences.enabled && $nativeNotificationRuntime.permission.state === 'not_determined'}
        <Button
          size="sm"
          disabled={$nativeNotificationRuntime.busy}
          onclick={() => void requestNativeNotificationPermission().catch(() => undefined)}
        >
          Allow notifications
        </Button>
      {/if}
      {#if showSettingsShortcut}
        <Button
          size="sm"
          disabled={openingSettings || $nativeNotificationRuntime.busy}
          onclick={() => void openSettings()}
        >
          <ExternalLinkIcon aria-hidden="true" />
          {openingSettings ? 'Opening settings…' : 'Open system settings'}
        </Button>
      {/if}
    </div>
  </div>

  <Separator />

  <footer class="bg-muted/40 px-4 py-3 text-xs leading-5 text-muted-foreground">
    In-app notifications stay available in either mode. Your choice is saved on this computer.
    Click a notification to open its agent; viewing the agent clears matching system alerts.
    Linux history, click actions, and launcher badges depend on your desktop environment.
  </footer>
</section>
