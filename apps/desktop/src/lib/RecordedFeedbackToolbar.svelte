<script lang="ts">
  import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
  import CameraIcon from '@lucide/svelte/icons/camera';
  import CircleIcon from '@lucide/svelte/icons/circle';
  import EraserIcon from '@lucide/svelte/icons/eraser';
  import GripVerticalIcon from '@lucide/svelte/icons/grip-vertical';
  import MousePointer2Icon from '@lucide/svelte/icons/mouse-pointer-2';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import SquareIcon from '@lucide/svelte/icons/square';
  import Undo2Icon from '@lucide/svelte/icons/undo-2';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount } from 'svelte';

  import type { NativeFeedbackSession } from './recordedFeedback';

  type Tool = 'pointer' | 'pen' | 'arrow' | 'rectangle' | 'ellipse';
  type SnapMode = 'region' | 'full';

  const tools: Array<{ id: Tool; label: string; icon: typeof MousePointer2Icon }> = [
    { id: 'pointer', label: 'Pointer', icon: MousePointer2Icon },
    { id: 'pen', label: 'Draw', icon: PencilIcon },
    { id: 'arrow', label: 'Arrow', icon: ArrowUpRightIcon },
    { id: 'rectangle', label: 'Rectangle', icon: SquareIcon },
    { id: 'ellipse', label: 'Ellipse', icon: CircleIcon }
  ];
  const colors = [
    { value: '#ff4d5e', label: 'Red' },
    { value: '#ffd84d', label: 'Yellow' },
    { value: '#35c9ff', label: 'Cyan' },
    { value: '#ffffff', label: 'White' }
  ];

  let session = $state<NativeFeedbackSession | null>(null);
  let tool = $state<Tool>('pointer');
  let color = $state('#ff4d5e');
  let width = $state(4);
  let snapMode = $state<SnapMode>('region');
  let selecting = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let ticker: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    try {
      const saved = localStorage.getItem('workman.feedback.snap-mode.v1');
      if (saved === 'region' || saved === 'full') snapMode = saved;
    } catch {
      // The split button still works when webview storage is unavailable.
    }
    void refreshStatus();
    ticker = setInterval(refreshStatus, 500);
    const unlisteners = Promise.all([
      listen<NativeFeedbackSession>('feedback://status', (event) => (session = event.payload)),
      listen<string>('feedback://shortcut', (event) => void handleShortcut(event.payload)),
      listen<{ selecting: boolean }>('feedback://region', (event) => (selecting = event.payload.selecting)),
      listen<{ message: string }>('feedback://error', (event) => (error = event.payload.message)),
      listen<{ message: string }>('feedback://ui-error', (event) => (error = event.payload.message))
    ]);
    return () => {
      if (ticker) clearInterval(ticker);
      void unlisteners.then((values) => values.forEach((unlisten) => unlisten()));
    };
  });

  async function refreshStatus(): Promise<void> {
    try {
      const next = await invoke<NativeFeedbackSession | null>('feedback_status');
      if (next) session = next;
    } catch {
      // A closing toolbar can race the final status request.
    }
  }

  async function selectTool(next: Tool): Promise<void> {
    error = null;
    try {
      session = await invoke<NativeFeedbackSession>('feedback_set_tool', {
        tool: next,
        color,
        width
      });
      tool = next;
    } catch (cause) {
      error = messageFor(cause);
    }
  }

  async function startToolbarDrag(event: PointerEvent): Promise<void> {
    if (event.button !== 0 || !event.isPrimary) return;
    event.preventDefault();
    try {
      await getCurrentWindow().startDragging();
    } catch (cause) {
      error = messageFor(cause);
    }
  }

  async function chooseColor(next: string): Promise<void> {
    color = next;
    await selectTool(tool);
  }

  async function cycleWidth(): Promise<void> {
    width = width === 2 ? 4 : width === 4 ? 8 : 2;
    await selectTool(tool);
  }

  async function undo(): Promise<void> {
    try { session = await invoke('feedback_undo'); } catch (cause) { error = messageFor(cause); }
  }

  async function clear(): Promise<void> {
    try { session = await invoke('feedback_clear'); } catch (cause) { error = messageFor(cause); }
  }

  async function snap(mode: SnapMode): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    snapMode = mode;
    try { localStorage.setItem('workman.feedback.snap-mode.v1', mode); } catch { /* optional */ }
    try {
      if (mode === 'region') await invoke('feedback_begin_region');
      else await invoke('feedback_capture_snapshot', { displayIndex: null, region: null });
      await refreshStatus();
    } catch (cause) {
      error = messageFor(cause);
    } finally {
      busy = false;
    }
  }

  async function cancelRegion(): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try {
      session = await invoke('feedback_cancel_region');
      selecting = false;
    } catch (cause) {
      error = messageFor(cause);
    } finally {
      busy = false;
    }
  }

  async function finish(): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try { await invoke('feedback_finish'); } catch (cause) {
      error = messageFor(cause);
      busy = false;
    }
  }

  async function handleShortcut(action: string): Promise<void> {
    if (action === 'snap') await snap(snapMode);
    else if (action === 'snapRegion') await snap('region');
    else if (action === 'snapFull') await snap('full');
    else if (action === 'toggleAnnotation') await selectTool(tool === 'pointer' ? 'pen' : 'pointer');
    else if (action === 'undo') await undo();
    else if (action === 'clear') await clear();
    else if (action === 'finish') await finish();
  }

  function formatElapsed(milliseconds = 0): string {
    const seconds = Math.max(0, Math.floor(milliseconds / 1_000));
    return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
  }

  function messageFor(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<div class="toolbar-shell" class:drawing={tool !== 'pointer'}>
  <button
    class="drag-handle"
    type="button"
    title="Move toolbar"
    aria-label="Move recording toolbar"
    onpointerdown={(event) => void startToolbarDrag(event)}
  ><GripVerticalIcon size={15} strokeWidth={1.8} /></button>

  <div class="recording" title="Microphone is recording" aria-label="Microphone recording">
    <span class="recording-dot"></span>
    <span class="recording-copy">
      <span class="timer">{formatElapsed(session?.elapsed_ms)}</span>
      <small aria-live="polite">{session?.snapshot_count ?? 0} snap{session?.snapshot_count === 1 ? '' : 's'}</small>
    </span>
  </div>

  <div class="divider"></div>
  <div class="tool-group" aria-label="Annotation tools">
    {#each tools as item}
      {@const Icon = item.icon}
      <button
        type="button"
        class:active={tool === item.id}
        class:exit-drawing={item.id === 'pointer' && tool !== 'pointer'}
        aria-label={item.id === 'pointer' && tool !== 'pointer' ? 'Exit drawing mode' : item.label}
        aria-pressed={tool === item.id}
        title={item.id === 'pointer' && tool !== 'pointer' ? 'Exit drawing mode and click through to your apps' : item.label}
        onclick={() => void selectTool(item.id)}
      >
        <Icon size={16} strokeWidth={1.9} />
        {#if item.id === 'pointer' && tool !== 'pointer'}<span>Exit draw</span>{/if}
      </button>
    {/each}
  </div>

  <div class="color-group" aria-label="Annotation color">
    {#each colors as swatch}
      <button
        type="button"
        class="swatch"
        class:active={color === swatch.value}
        style={`--swatch:${swatch.value}`}
        aria-label={swatch.label}
        aria-pressed={color === swatch.value}
        title={swatch.label}
        onclick={() => void chooseColor(swatch.value)}
      ></button>
    {/each}
    <button class="stroke-width" type="button" title={`Stroke width ${width}px · click to change`} aria-label={`Stroke width ${width} pixels; click to change`} onclick={() => void cycleWidth()}><span style={`height:${width}px`}></span></button>
  </div>

  <div class="tool-group" aria-label="Annotation history">
    <button type="button" title="Undo" aria-label="Undo last annotation" onclick={() => void undo()}><Undo2Icon size={16} /></button>
    <button type="button" title="Clear" aria-label="Clear annotations" onclick={() => void clear()}><EraserIcon size={16} /></button>
  </div>

  <div class="divider"></div>
  <div class="snap-control">
    {#if selecting}
      <button class="cancel-snap" type="button" disabled={busy} onclick={() => void cancelRegion()}>Cancel region</button>
    {:else}
      <button class="snap-main" type="button" disabled={busy} onclick={() => void snap('region')}>
        <CameraIcon size={15} /> Snap region
      </button>
      <button class="snap-display" type="button" disabled={busy} onclick={() => void snap('full')}>
        Snap display
      </button>
    {/if}
  </div>
  <button class="finish" type="button" disabled={busy} title="Stop recording and transcribe" onclick={() => void finish()}>Stop</button>
</div>

{#if error}
  <button class="error" type="button" onclick={() => (error = null)}>{error}</button>
{/if}

<style>
  :global(html), :global(body), :global(#app) { width: 100%; height: 100%; margin: 0; overflow: hidden; background: transparent !important; }
  .toolbar-shell { box-sizing: border-box; display: flex; width: 100%; height: 60px; align-items: center; gap: 7px; border: 1px solid color-mix(in srgb, var(--border) 82%, white 8%); border-radius: 10px; padding: 8px 9px 8px 5px; background: color-mix(in srgb, var(--popover) 93%, transparent); box-shadow: 0 12px 32px rgb(0 0 0 / 38%); color: var(--foreground); user-select: none; backdrop-filter: blur(18px) saturate(1.15); }
  .toolbar-shell.drawing { border-color: color-mix(in srgb, var(--ring) 48%, var(--border)); }
  button { display: inline-grid; box-sizing: border-box; min-width: 32px; height: 32px; place-items: center; border: 1px solid transparent; border-radius: 5px; background: transparent; color: var(--muted-foreground); cursor: pointer; }
  button:hover { border-color: var(--border); background: var(--muted); color: var(--foreground); }
  button:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
  button.active { border-color: color-mix(in srgb, var(--ring) 68%, var(--border)); background: color-mix(in srgb, var(--ring) 16%, var(--muted)); color: var(--foreground); }
  button:disabled { cursor: default; opacity: .52; }
  button.drag-handle { min-width: 22px; width: 22px; height: 38px; border: 0; color: var(--muted-foreground); cursor: grab; opacity: .68; touch-action: none; }
  button.drag-handle:hover { background: var(--muted); color: var(--foreground); opacity: 1; }
  button.drag-handle:active { cursor: grabbing; }
  .recording { display: flex; min-width: 76px; align-items: center; gap: 7px; padding: 0 2px; }
  .recording-dot { width: 9px; height: 9px; border-radius: 50%; background: #ff4d5e; box-shadow: 0 0 0 3px rgb(255 77 94 / 15%); }
  .recording-copy { display: grid; gap: 3px; }
  .timer { color: var(--foreground); font: 600 12px/1 'JetBrains Mono Variable', monospace; font-variant-numeric: tabular-nums; }
  .recording-copy small { color: var(--muted-foreground); font: 9px/1 'JetBrains Mono Variable', monospace; white-space: nowrap; }
  .divider { width: 1px; height: 24px; flex: none; background: var(--border); }
  .tool-group, .color-group { display: flex; align-items: center; gap: 2px; }
  button.exit-drawing { display: flex; min-width: 82px; grid-auto-flow: column; gap: 5px; border-color: color-mix(in srgb, var(--ring) 64%, var(--border)); padding-inline: 8px; background: color-mix(in srgb, var(--ring) 14%, var(--muted)); color: var(--foreground); font-size: 11px; font-weight: 700; white-space: nowrap; }
  button.swatch { min-width: 22px; width: 22px; height: 28px; }
  button.swatch::before { width: 11px; height: 11px; border: 1px solid rgb(0 0 0 / 45%); border-radius: 50%; background: var(--swatch); box-shadow: 0 0 0 1px rgb(255 255 255 / 14%); content: ''; }
  button.swatch.active::before { box-shadow: 0 0 0 2px var(--popover), 0 0 0 3px var(--ring); }
  button.stroke-width { min-width: 28px; width: 28px; }
  button.stroke-width span { display: block; width: 15px; max-height: 8px; border-radius: 999px; background: currentColor; }
  .snap-control { display: flex; min-width: 189px; margin-left: auto; }
  button.snap-main, button.snap-display, button.cancel-snap { display: flex; grid-auto-flow: column; gap: 7px; border-color: color-mix(in srgb, var(--ring) 70%, var(--border)); padding: 0 10px; background: color-mix(in srgb, var(--ring) 18%, var(--card)); color: var(--foreground); font-size: 12px; font-weight: 650; }
  button.snap-main { min-width: 100px; border-radius: 5px 0 0 5px; }
  button.snap-display { min-width: 89px; border-left: 0; border-radius: 0 5px 5px 0; }
  button.cancel-snap { width: 189px; border-color: color-mix(in srgb, var(--destructive) 55%, var(--border)); justify-content: center; background: color-mix(in srgb, var(--destructive) 12%, var(--card)); }
  button.finish { min-width: 58px; padding: 0 10px; background: #ff4d5e; color: white; font-size: 12px; font-weight: 700; }
  button.finish:hover { border-color: #ff8892; background: #e63e4f; }
  .error { position: fixed; right: 8px; bottom: 4px; left: 8px; display: block; overflow: hidden; width: calc(100% - 16px); height: 22px; border-color: color-mix(in srgb, var(--destructive) 65%, var(--border)); background: var(--popover); color: var(--destructive); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; } }
</style>
