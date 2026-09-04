<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import DownloadIcon from '@lucide/svelte/icons/download';
  import MicIcon from '@lucide/svelte/icons/mic';
  import MonitorIcon from '@lucide/svelte/icons/monitor';
  import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
  import XIcon from '@lucide/svelte/icons/x';

  import * as Dialog from '$lib/components/ui/dialog';
  import { Button } from '$lib/components/ui/button';
  import { hotkeyDisplayLabel, hotkeyPreferences } from './hotkeys';
  import type { NativeFeedbackPreflight } from './recordedFeedback';

  interface Props {
    projectName: string;
    appendTitle?: string | null;
    preflight: NativeFeedbackPreflight | null;
    loading: boolean;
    installing: boolean;
    starting: boolean;
    progress: { downloaded: number; total: number } | null;
    error: string | null;
    onRefresh: () => void;
    onRequestScreen: () => void;
    onInstall: () => void;
    onStart: () => void;
    onClose: () => void;
  }

  let { projectName, appendTitle = null, preflight, loading, installing, starting, progress, error,
    onRefresh, onRequestScreen, onInstall, onStart, onClose }: Props = $props();

  let ready = $derived(Boolean(preflight?.supported
    && preflight.microphone_available
    && preflight.screen_capture_available
    && preflight.model_installed));

  function bytes(value: number): string {
    return `${Math.round(value / 1024 / 1024)} MB`;
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !installing && !starting) onClose(); }}>
  <Dialog.Content
    class="max-h-[calc(100dvh-24px)] w-[min(590px,calc(100vw-24px))] !max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden rounded-lg border border-border bg-popover p-0"
    showCloseButton={false}
  >
    <Dialog.Header class="border-b border-border px-5 py-4 text-left">
      <div class="eyebrow"><MicIcon size={13} /> Recorded Feedback</div>
      <Dialog.Title class="mt-1 min-w-0 break-words pr-5 text-lg">{appendTitle ? `Record more for ${appendTitle}` : `Record feedback for ${projectName}`}</Dialog.Title>
      <Dialog.Description class="mt-1 text-sm leading-relaxed">
        {#if appendTitle}New speech and snapshots will be added to the end. Your existing text, screenshots, and delivery history will stay intact.{:else}Workman records your microphone and saves a screenshot only when you click Snap. Audio, images, and transcription stay on this computer.{/if}
      </Dialog.Description>
    </Dialog.Header>

    <div class="requirements" aria-busy={loading || installing}>
      {#if !preflight && loading}
        <p class="loading">Checking this Mac…</p>
      {:else if preflight && !preflight.supported}
        <div class="unsupported"><strong>Not available on {preflight.platform}</strong><span>{preflight.message}</span></div>
      {:else if preflight}
        <div class:ready={preflight.microphone_available} class="requirement">
          <span class="icon"><MicIcon size={17} /></span>
          <div><strong>Microphone</strong><small>{preflight.microphone_available ? 'Available' : 'Connect or enable a microphone, then check again.'}</small></div>
          {#if preflight.microphone_available}<CheckIcon class="check" size={16} />{/if}
        </div>
        <div class:ready={preflight.screen_capture_available} class="requirement">
          <span class="icon"><MonitorIcon size={17} /></span>
          <div>
            <strong>Screen Recording</strong>
            <small>
              {#if preflight.screen_capture_available}
                Allowed
              {:else if preflight.screen_capture_authorized}
                Allowed, but Workman could not find an active display. Connect a display, then check again.
              {:else}
                macOS is still blocking this exact Workman app. If an older Workman entry is enabled, remove it, add the current app again, then fully quit and reopen Workman.
              {/if}
            </small>
          </div>
          {#if preflight.screen_capture_available}
            <CheckIcon class="check" size={16} />
          {:else if preflight.screen_capture_authorized}
            <Button variant="outline" size="sm" disabled={loading} onclick={onRefresh}>Check again</Button>
          {:else}
            <Button variant="outline" size="sm" disabled={loading} onclick={onRequestScreen}>Open settings</Button>
          {/if}
        </div>
        <div class:ready={preflight.model_installed} class="requirement">
          <span class="icon"><ShieldCheckIcon size={17} /></span>
          <div><strong>{preflight.model_name}</strong><small>{preflight.model_installed ? 'Installed for offline transcription' : `${bytes(preflight.model_size_bytes)} one-time download; inference stays offline.`}</small></div>
          {#if preflight.model_installed}
            <CheckIcon class="check" size={16} />
          {:else}
            <Button variant="outline" size="sm" disabled={installing} onclick={onInstall}><DownloadIcon size={13} />{installing ? 'Installing…' : 'Install'}</Button>
          {/if}
        </div>
        {#if installing && progress}
          <div class="progress" aria-label={`Downloaded ${bytes(progress.downloaded)} of ${bytes(progress.total)}`}>
            <span style={`width:${Math.min(100, progress.downloaded / Math.max(1, progress.total) * 100)}%`}></span>
          </div>
        {/if}
      {/if}
      {#if error}<button class="error" type="button" onclick={onRefresh}>{error}<span>Check again</span></button>{/if}
    </div>

    <Dialog.Footer class="mx-0 mb-0 flex-row flex-wrap items-center justify-between rounded-none border-t border-border bg-card px-5 py-3">
      <span class="privacy">No continuous screen video</span>
      <div class="actions">
        <Button variant="ghost" disabled={installing || starting} onclick={onClose}>Cancel</Button>
        <Button disabled={!ready || installing || starting} onclick={onStart}>
          <MicIcon size={14} />{starting ? 'Starting…' : 'Start recording'}
          {#if !starting && hotkeyDisplayLabel($hotkeyPreferences['start-feedback'])}
            <kbd>{hotkeyDisplayLabel($hotkeyPreferences['start-feedback'])}</kbd>
          {/if}
        </Button>
      </div>
    </Dialog.Footer>
    <Dialog.Close class="close" aria-label="Close" disabled={installing || starting}><XIcon size={15} /></Dialog.Close>
  </Dialog.Content>
</Dialog.Root>

<style>
  .eyebrow { display: flex; align-items: center; gap: 6px; color: var(--signal); font: 700 var(--font-size-xs) 'JetBrains Mono Variable', monospace; letter-spacing: .08em; text-transform: uppercase; }
  .requirements { display: grid; min-height: 0; overflow: auto; gap: 8px; padding: 14px 16px; }
  .requirement { display: grid; min-height: 58px; grid-template-columns: 34px minmax(0, 1fr) auto; align-items: center; gap: 10px; border: 1px solid var(--border); border-radius: 6px; padding: 8px 10px; background: var(--surface); }
  .requirement.ready { border-color: color-mix(in srgb, var(--success) 28%, var(--border)); }
  .icon { display: grid; width: 30px; height: 30px; place-items: center; border-radius: 5px; background: var(--muted); color: var(--muted-foreground); }
  .requirement.ready .icon, :global(.check) { color: var(--success); }
  .requirement strong, .requirement small { display: block; }
  .requirement strong { color: var(--foreground); font-size: var(--font-size-sm); }
  .requirement small { margin-top: 3px; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.35; }
  .progress { overflow: hidden; height: 4px; border-radius: 3px; background: var(--muted); }
  .progress span { display: block; height: 100%; background: var(--signal); transition: width .15s ease-out; }
  .loading, .unsupported { margin: 0; padding: 18px; color: var(--muted-foreground); text-align: center; }
  .unsupported strong, .unsupported span { display: block; }
  .unsupported strong { color: var(--foreground); }
  .unsupported span { margin-top: 6px; font-size: var(--font-size-sm); }
  .error { display: flex; justify-content: space-between; border: 1px solid color-mix(in srgb, var(--destructive) 45%, var(--border)); border-radius: 5px; padding: 8px 10px; background: color-mix(in srgb, var(--destructive) 7%, var(--surface)); color: var(--destructive); font-size: var(--font-size-xs); text-align: left; }
  .error span { font-weight: 700; }
  .privacy { color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .actions { display: flex; gap: 8px; }
  .actions kbd { border-left: 1px solid color-mix(in srgb, currentColor 28%, transparent); padding-left: 8px; font: 600 var(--font-size-xs) 'JetBrains Mono Variable', monospace; opacity: .72; }
  :global(.close) { position: absolute; top: 12px; right: 12px; display: grid; width: 28px; height: 28px; place-items: center; border: 0; border-radius: 4px; background: transparent; color: var(--muted-foreground); }
  :global(.close:hover) { background: var(--muted); color: var(--foreground); }
  @media (prefers-reduced-motion: reduce) { .progress span { transition: none; } }
</style>
