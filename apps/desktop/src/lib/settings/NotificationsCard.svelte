<script lang="ts">
  import { onMount } from 'svelte';
  import UploadIcon from '@lucide/svelte/icons/upload';
  import Volume2Icon from '@lucide/svelte/icons/volume-2';
  import BellIcon from '@lucide/svelte/icons/bell';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';

  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import IconButton from '$lib/components/ds/IconButton.svelte';
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
    setNativeNotificationMode,
    setNotificationSoundEnabled,
    type NotificationSettingsTarget,
    type NativeNotificationMode,
    type NativeNotificationPermissionState
  } from '../nativeNotifications';

  import { notificationSound, refreshNotificationSound, chooseNotificationSound, selectNotificationSound, previewNotificationSound, setNotificationSoundVolume } from '../notificationSound';

  import { createNotificationTest } from '../notificationTest';

  let volume = $state(100);
  $effect(() => { volume = $notificationSound.info?.volume ?? 100; });
  let testPending = $state(false);
  let testMessage = $state('');
  const notificationTest = createNotificationTest((pending, message) => {
    testPending = pending;
    testMessage = message;
  });
  let previewing = $state(false);
  async function previewSound(): Promise<void> {
    if (previewing) return;
    previewing = true;
    try { await previewNotificationSound(); }
    finally { previewing = false; }
  }
  onMount(() => {
    void refreshNotificationSound();
    return notificationTest.watch();
  });


  let openingSettings = $state(false);
  let needsPermissionHelp = $derived(
    $nativeNotificationPreferences.enabled && (
      ['denied', 'unavailable', 'unknown'].includes($nativeNotificationRuntime.permission.state)
      || ($nativeNotificationPreferences.soundEnabled && $nativeNotificationRuntime.permission.sound_enabled === false)
    )
  );

  async function openSettings(target: NotificationSettingsTarget = 'notifications'): Promise<void> {
    if (openingSettings) return;
    openingSettings = true;
    try { await openNativeNotificationSettings(target); }
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

  <div class="grid gap-3 px-4 py-3">
    <label for="notification-mode" class="text-sm font-medium">Notification mode</label>
    <select
      id="notification-mode"
      class="h-10 w-full min-w-0 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
      value={$nativeNotificationPreferences.mode}
      disabled={!$nativeNotificationPreferences.enabled}
      aria-describedby="notification-mode-description"
      onchange={(event) => setNativeNotificationMode(event.currentTarget.value as NativeNotificationMode)}
    >
      <option value="all">All agents</option>
      <option value="top_level">Only top-level agents</option>
      <option value="project_ready">Only when all agents are ready</option>
    </select>
    <p id="notification-mode-description" class="text-xs leading-5 text-muted-foreground">
      {#if $nativeNotificationPreferences.mode === 'project_ready'}
        One alert per project after every agent, including children, is idle, waiting, or stopped.
        A short pause avoids alerts during handoffs.
      {:else if $nativeNotificationPreferences.mode === 'top_level'}
        Completion and input alerts for top-level agents. Child-agent activity stays in Workman.
      {:else}
        Completion and input alerts for every agent, including child agents.
      {/if}
      Crash, timer, and task alerts still notify you in every mode.
    </p>
  </div>

  <Separator />

  {#if $nativeNotificationPreferences.mode !== 'project_ready'}
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
          <span class="mt-0.5 block text-xs leading-5 text-muted-foreground">Also alert when an unwatched agent reaches a new prompt that needs you.</span>
        </label>
      </div>
      <span class="font-mono text-xs text-muted-foreground">
        {$nativeNotificationPreferences.needsInput ? 'On' : 'Off'}
      </span>
    </div>
    <Separator />
  {/if}

  <div class="grid gap-3 px-4 py-3">
    <div class="flex items-center gap-3">
      <Switch
        id="notification-sound-enabled"
        size="sm"
        checked={$nativeNotificationPreferences.soundEnabled}
        disabled={!$nativeNotificationPreferences.enabled}
        onCheckedChange={(checked) => setNotificationSoundEnabled(checked === true)}
      />
      <label for="notification-sound-enabled" class="min-w-0">
        <span class="block text-sm font-medium">Notification sound</span>
        <span class="mt-0.5 block text-xs leading-5 text-muted-foreground">Play a sound with computer alerts in any notification mode. System volume and Do Not Disturb settings still apply.</span>
      </label>
    </div>
    <div class="min-w-0 rounded-md border bg-muted/30 p-3">
      <label for="notification-sound-preset" class="mb-2 block text-sm font-medium">Sound</label>
      <div class="flex min-w-0 items-center gap-2">
        <select
          id="notification-sound-preset"
          class="h-10 w-full min-w-0 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
          value={$notificationSound.info?.preset ?? 'system'}
          disabled={!$nativeNotificationPreferences.enabled || !$nativeNotificationPreferences.soundEnabled || $notificationSound.busy || !$notificationSound.info}
          aria-describedby="notification-sound-description"
          onchange={(event) => {
            const preset = event.currentTarget.value;
            // Keep the displayed value tied to the saved choice while a native write is pending
            // or fails. A failed write must never look like a successfully selected sound.
            event.currentTarget.value = $notificationSound.info?.preset ?? 'system';
            if (preset === 'system' || preset === 'doom') void selectNotificationSound(preset);
          }}
        >
          <option value="system">System default</option>
          <option value="doom" disabled={!$notificationSound.info?.supported}>Doom</option>
          {#if $notificationSound.info?.preset === 'custom'}
            <option value="custom">Custom: {$notificationSound.info.name}</option>
          {/if}
        </select>
        <IconButton
          label={previewing ? 'Playing notification sound' : `Preview ${$notificationSound.info?.name ?? 'system default'} notification sound`}
          variant="outline"
          size="icon"
          class="size-10 shrink-0"
          disabled={$notificationSound.busy || !$notificationSound.info}
          aria-busy={previewing}
          onclick={() => void previewSound()}
        >
          {#snippet icon()}<Volume2Icon class={previewing ? 'size-4 animate-pulse' : 'size-4'} aria-hidden="true" />{/snippet}
        </IconButton>
      </div>
      <div class="mt-3">
        <div class="flex items-center justify-between gap-3 text-sm">
          <label for="notification-sound-volume" class="font-medium">Volume</label>
          <output for="notification-sound-volume" class="font-mono text-xs tabular-nums text-muted-foreground">
            {$notificationSound.info?.volume_supported ? `${volume}%` : 'System volume'}
          </output>
        </div>
        <input id="notification-sound-volume" type="range" min="0" max="100" step="1"
          class="mt-1 h-8 w-full cursor-pointer accent-primary disabled:cursor-not-allowed disabled:opacity-40"
          bind:value={volume}
          disabled={!$notificationSound.info?.volume_supported || $notificationSound.busy}
          aria-describedby="notification-volume-description"
          onchange={async () => {
            await setNotificationSoundVolume(volume);
            volume = $notificationSound.info?.volume ?? 100;
          }}
        />
        <p id="notification-volume-description" class="text-xs leading-5 text-muted-foreground">
          {$notificationSound.info?.volume_supported
            ? 'Applies to previews and notifications. Your system volume still applies.'
            : $notificationSound.info?.supported
              ? 'Your operating system controls the default sound volume. Choose Doom or a custom sound to adjust it here.'
              : 'Your operating system controls the notification sound volume.'}
        </p>
      </div>
      <p id="notification-sound-description" class="mt-2 text-xs leading-5 text-muted-foreground">
        {#if $notificationSound.info?.supported}
          Doom is the bundled item pickup sound. You can also choose your own WAV under 30 seconds (16-bit PCM, mono or stereo, 8–48 kHz, up to 6 MB).
          Workman saves a local copy.
        {:else if !$notificationSound.info}
          {$notificationSound.busy ? 'Checking sound support…' : 'Sound settings are unavailable.'}
        {/if}
        {$notificationSound.info?.detail ?? ''}
      </p>
      <div class="mt-3 flex flex-wrap gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={!$nativeNotificationPreferences.enabled || !$nativeNotificationPreferences.soundEnabled || $notificationSound.busy || !$notificationSound.info?.supported}
          onclick={() => void chooseNotificationSound()}
        >
          <UploadIcon aria-hidden="true" />
          {$notificationSound.busy ? 'Loading…' : 'Choose sound…'}
        </Button>
        <Button variant="outline" size="sm"
          disabled={!$nativeNotificationPreferences.enabled || testPending || $nativeNotificationRuntime.busy || $notificationSound.busy}
          onclick={() => void notificationTest.schedule()}
        >{testPending ? 'Test scheduled…' : 'Send test in 5s'}</Button>
        {#if !$notificationSound.info && !$notificationSound.busy}
          <Button variant="outline" size="sm" onclick={() => void refreshNotificationSound()}>Retry</Button>
        {/if}
      </div>
      {#if $notificationSound.error}
        <p class="mt-2 text-xs leading-5 text-destructive" role="alert">{$notificationSound.error}</p>
      {/if}
      {#if testMessage}<p class="mt-2 text-xs leading-5 text-muted-foreground" role="status">{testMessage}</p>{/if}
    </div>
  </div>

  <Separator />

  <div class="space-y-3 px-4 py-3">
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
      {#if needsPermissionHelp}
        <p class="mt-1 text-xs leading-5 text-muted-foreground">
          {#if $nativeNotificationRuntime.permission.state === 'granted' && $nativeNotificationRuntime.permission.sound_enabled === false}
            Enable notification sounds for Workman in system settings.
          {:else}
            Enable notifications for Workman in system settings.
          {/if}
          Permission updates when you return to the app.
        </p>
      {/if}
      {#if $nativeNotificationPreferences.enabled}
        <p class="mt-2 text-xs leading-5 text-muted-foreground">
          {#if $nativeNotificationRuntime.permission.platform === 'macos'}
            No banner or sound? In Control Center, turn off Focus, or allow Workman in that Focus.
            macOS can also silence notifications while sharing or mirroring your screen.
            In System Settings → Notifications, check “when mirroring or sharing the display”.
          {:else}
            No banner or sound? Check Do Not Disturb, notification sound permissions, and system volume in your desktop settings.
          {/if}
          The speaker button previews audio directly, so hearing it does not confirm that computer alerts are allowed through.
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
      {#if $nativeNotificationPreferences.enabled}
        <Button
          size="sm"
          disabled={openingSettings || $nativeNotificationRuntime.busy}
          onclick={() => void openSettings()}
        >
          <ExternalLinkIcon aria-hidden="true" />
          {openingSettings ? 'Opening settings…' : 'Open system settings'}
        </Button>
        {#if $nativeNotificationRuntime.permission.platform === 'macos'}
          <Button
            variant="outline"
            size="sm"
            disabled={openingSettings || $nativeNotificationRuntime.busy}
            onclick={() => void openSettings('focus')}
          >
            <ExternalLinkIcon aria-hidden="true" />
            Open Focus settings
          </Button>
        {/if}
      {/if}
    </div>
  </div>

  <Separator />

  <footer class="bg-muted/40 px-4 py-3 text-xs leading-5 text-muted-foreground">
    In-app notifications always stay available. Your choice is saved on this computer.
    Click a notification to open its agent or project; reading it clears matching system alerts.
    Linux history, click actions, and launcher badges depend on your desktop environment.
  </footer>
</section>
