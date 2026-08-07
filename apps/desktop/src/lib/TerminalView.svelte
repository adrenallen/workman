<script lang="ts">
  import { FitAddon } from '@xterm/addon-fit';
  import { WebLinksAddon } from '@xterm/addon-web-links';
  import { WebglAddon } from '@xterm/addon-webgl';
  import { Terminal } from '@xterm/xterm';
  import '@xterm/xterm/css/xterm.css';
  import { onMount } from 'svelte';

  import {
    appearance,
    currentAppearance,
    terminalFontCss,
    terminalXtermTheme
  } from './appearance';
  import { FOCUS_TERMINAL_EVENT } from './contextMenu';
  import type { DaemonClient, ProcessView, TerminalFrame } from './daemon';
  import { EXTERNAL_LINK_TOOLTIP, openExternalUrl } from './externalLinks';

  let {
    client,
    process,
    connected,
    onError,
    onUnfocus
  }: {
    client: DaemonClient;
    process: ProcessView;
    connected: boolean;
    onError: (message: string) => void;
    onUnfocus?: () => void;
  } = $props();

  let host: HTMLDivElement;
  let terminal = $state<Terminal | null>(null);
  let fitAddon: FitAddon | null = null;
  let hasOutput = $state(false);
  let linkHintVisible = $state(false);
  let resizeFrame = 0;
  let inputTimer: ReturnType<typeof setTimeout> | null = null;
  let inputProcessId: number | null = null;
  let inputBytes: number[] = [];
  let attachedProcessId: number | null = null;
  let attachedConnected = false;
  let attachmentGeneration = 0;
  let inputEnabled = false;
  let replayState: TerminalReplayState | null = null;
  let kittyKeyboardFlags = 0;
  let modifyOtherKeys = 0;
  let appliedThemeSignature = '';
  const encoder = new TextEncoder();
  const initialAppearance = currentAppearance();

  interface TerminalReplayState {
    generation: number;
    processId: number;
    replayEndOffset: number | null;
    parsedThrough: number;
    focusReporting: boolean;
    kittyKeyboardFlags: number;
    modifyOtherKeys: number;
    finishing: boolean;
    focusRequested: boolean;
  }

  onMount(() => {
    const initialPalette = initialAppearance.terminalTheme.palette;
    appliedThemeSignature = Object.values(initialPalette).join('|');
    const activateLink = (event: MouseEvent, uri: string) => {
      if (!event.metaKey || event.button !== 0) return;
      event.preventDefault();
      openExternalUrl(uri, onError);
    };
    const showLinkHint = () => { linkHintVisible = true; };
    const hideLinkHint = () => { linkHintVisible = false; };
    const instance = new Terminal({
      allowTransparency: false,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: 'bar',
      fontFamily: terminalFontCss(initialAppearance.terminalFont),
      fontSize: initialAppearance.terminalFontSize,
      fontWeight: 430,
      lineHeight: 1.18,
      linkHandler: {
        activate: activateLink,
        hover: showLinkHint,
        leave: hideLinkHint,
        allowNonHttpProtocols: false
      },
      scrollback: 10_000,
      smoothScrollDuration: 80,
      theme: terminalXtermTheme(initialPalette)
    });
    fitAddon = new FitAddon();
    instance.loadAddon(fitAddon);
    instance.loadAddon(
      new WebLinksAddon(activateLink, {
        hover: showLinkHint,
        leave: hideLinkHint
      })
    );
    instance.open(host);

    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => {
        webgl.dispose();
      });
      instance.loadAddon(webgl);
    } catch {
      // xterm automatically keeps its DOM renderer when WebGL is unavailable.
    }

    instance.attachCustomKeyEventHandler((event) => {
      if (
        event.metaKey && !event.altKey && !event.ctrlKey && !event.shiftKey
        && event.key.toLowerCase() === 'u'
      ) {
        if (event.type === 'keydown') onUnfocus?.();
        return false;
      }
      const modifiedKey = negotiatedModifiedKey(event);
      if (modifiedKey !== null) {
        if (event.type === 'keydown') queueInput(encoder.encode(modifiedKey));
        return false;
      }
      return true;
    });
    const dataDisposable = instance.onData((data) => queueInput(encoder.encode(data)));
    const binaryDisposable = instance.onBinary((data) => {
      queueInput(Uint8Array.from(data, (character) => character.charCodeAt(0) & 0xff));
    });
    const removeTerminalListener = client.onTerminal(handleTerminalFrame);
    const resizeObserver = new ResizeObserver(scheduleFit);
    resizeObserver.observe(host);
    terminal = instance;
    scheduleFit();
    const focusRequested = (event: Event) => {
      const detail = (event as CustomEvent<{ processId?: number }>).detail;
      if (detail?.processId !== process.id) return;
      if (inputEnabled) {
        instance.focus();
      } else if (replayState?.processId === process.id) {
        replayState.focusRequested = true;
      }
    };
    window.addEventListener(FOCUS_TERMINAL_EVENT, focusRequested);

    return () => {
      attachmentGeneration += 1;
      inputEnabled = false;
      replayState = null;
      setKeyboardProtocol(0, 0);
      flushInput();
      if (resizeFrame) cancelAnimationFrame(resizeFrame);
      resizeObserver.disconnect();
      removeTerminalListener();
      dataDisposable.dispose();
      binaryDisposable.dispose();
      window.removeEventListener(FOCUS_TERMINAL_EVENT, focusRequested);
      void client.detachTerminal().catch(() => undefined);
      instance.dispose();
      terminal = null;
    };
  });

  $effect(() => {
    const settings = $appearance;
    const instance = terminal;
    if (!instance) return;

    const family = terminalFontCss(settings.terminalFont);
    const typographyChanged = instance.options.fontFamily !== family
      || instance.options.fontSize !== settings.terminalFontSize;
    const themeSignature = Object.values(settings.terminalTheme.palette).join('|');
    const themeChanged = themeSignature !== appliedThemeSignature;
    if (typographyChanged) {
      instance.options.fontFamily = family;
      instance.options.fontSize = settings.terminalFontSize;
    }
    if (themeChanged) {
      appliedThemeSignature = themeSignature;
      instance.options.theme = terminalXtermTheme(settings.terminalTheme.palette);
    }
    if (typographyChanged || themeChanged) {
      // Refresh the currently rendered buffer. Offscreen scrollback picks up the same xterm
      // theme when it is next rendered, so old and new output never diverge.
      instance.refresh(0, Math.max(0, instance.rows - 1));
    }
    // UI zoom changes layout dimensions; terminal changes alter cell geometry.
    // In both cases FitAddon must resize xterm before the PTY receives rows/cols.
    scheduleFit();
    void document.fonts.ready.then(() => scheduleFit());
  });

  $effect(() => {
    const instance = terminal;
    const processId = process.id;
    const isConnected = connected;
    if (!instance) return;
    if (attachedProcessId === processId && attachedConnected === isConnected) return;

    attachedProcessId = processId;
    attachedConnected = isConnected;

    flushInput();
    inputEnabled = false;
    replayState = null;
    setKeyboardProtocol(0, 0);
    instance.reset();
    hasOutput = false;
    if (!isConnected) return;

    const generation = ++attachmentGeneration;
    const processRunning = process.status === 'running';
    const state: TerminalReplayState = {
      generation,
      processId,
      replayEndOffset: null,
      parsedThrough: 0,
      focusReporting: false,
      kittyKeyboardFlags: 0,
      modifyOtherKeys: 0,
      finishing: false,
      focusRequested: true
    };
    replayState = state;
    void (async () => {
      // Replay at the PTY's actual viewport dimensions. Starting at xterm's 80x24 default and
      // resizing afterward reflows the active zsh prompt differently from a native terminal.
      await document.fonts.ready;
      await nextAnimationFrame();
      if (replayState !== state) return;
      fitTerminal();
      if (processRunning) {
        await client.resizeTerminal(
          processId,
          instance.rows,
          instance.cols,
          Math.round(host.clientWidth),
          Math.round(host.clientHeight)
        );
      }
      if (replayState !== state) return;

      const attached = await client.attachTerminal(processId);
      if (replayState !== state) return;
      state.replayEndOffset = attached.replay_end_offset;
      state.parsedThrough = Math.max(state.parsedThrough, attached.replay_start_offset);
      state.focusReporting = attached.focus_reporting;
      state.kittyKeyboardFlags = attached.keyboard_protocol.kitty_flags;
      state.modifyOtherKeys = attached.keyboard_protocol.modify_other_keys;
      setKeyboardProtocol(state.kittyKeyboardFlags, state.modifyOtherKeys);
      finishReplayIfReady(state);
    })().catch((cause) => {
      if (replayState === state) onError(cause instanceof Error ? cause.message : String(cause));
    });
  });

  function handleTerminalFrame(frame: TerminalFrame): void {
    if (frame.process_id !== process.id || !terminal) return;
    if (frame.data.length > 0) hasOutput = true;
    const state = replayState;
    if (state) {
      state.kittyKeyboardFlags = frame.kitty_keyboard_flags;
      state.modifyOtherKeys = frame.modify_other_keys;
    }
    setKeyboardProtocol(frame.kitty_keyboard_flags, frame.modify_other_keys);
    terminal.write(Uint8Array.from(frame.data), () => {
      if (!state || replayState !== state || frame.process_id !== state.processId) return;
      state.parsedThrough = Math.max(
        state.parsedThrough,
        frame.start_offset + frame.data.length
      );
      finishReplayIfReady(state);
    });
  }

  function queueInput(bytes: Uint8Array): void {
    // xterm emits both physical keyboard data and terminal-protocol replies through onData.
    // Retained output is replayed into xterm on every attach, so its DA/DSR/XTVERSION/OSC and
    // focus replies must not be routed into the live shell until replay parsing is complete.
    if (!inputEnabled || process.status !== 'running') return;
    if (inputProcessId !== null && inputProcessId !== process.id) flushInput();
    inputProcessId = process.id;
    for (const byte of bytes) inputBytes.push(byte);
    if (!inputTimer) inputTimer = setTimeout(flushInput, 4);
  }

  function flushInput(): void {
    if (inputTimer) clearTimeout(inputTimer);
    inputTimer = null;
    const processId = inputProcessId;
    const bytes = Uint8Array.from(inputBytes);
    inputBytes = [];
    inputProcessId = null;
    if (processId === null || bytes.length === 0) return;
    void client
      .sendInput(processId, bytes)
      .catch((cause) => onError(cause instanceof Error ? cause.message : String(cause)));
  }

  function scheduleFit(): void {
    if (resizeFrame) cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(() => {
      resizeFrame = 0;
      const instance = terminal;
      if (!instance || !fitTerminal()) return;
      if (process.status !== 'running' || !connected) return;
      void client
        .resizeTerminal(
          process.id,
          instance.rows,
          instance.cols,
          Math.round(host.clientWidth),
          Math.round(host.clientHeight)
        )
        .catch((cause) => onError(cause instanceof Error ? cause.message : String(cause)));
    });
  }

  function fitTerminal(): Terminal | null {
    const instance = terminal;
    if (!instance || !fitAddon || host.clientWidth === 0 || host.clientHeight === 0) return null;
    fitAddon.fit();
    return instance;
  }

  function nextAnimationFrame(): Promise<void> {
    return new Promise((resolve) => requestAnimationFrame(() => resolve()));
  }

  function setKeyboardProtocol(kittyFlags: number, modifyOtherKeysLevel: number): void {
    kittyKeyboardFlags = kittyFlags & 1;
    modifyOtherKeys = modifyOtherKeysLevel === 1 || modifyOtherKeysLevel === 2
      ? modifyOtherKeysLevel
      : 0;
  }

  function negotiatedModifiedKey(event: KeyboardEvent): string | null {
    const codepoint = event.key === 'Enter' ? 13 : event.key === 'Tab' ? 9 : null;
    if (codepoint === null) return null;
    if (!event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey) return null;

    const modifier = 1
      + Number(event.shiftKey)
      + 2 * Number(event.altKey)
      + 4 * Number(event.ctrlKey)
      + 8 * Number(event.metaKey);
    if ((kittyKeyboardFlags & 1) !== 0) return `\x1b[${codepoint};${modifier}u`;

    const modifyOtherKeysApplies = modifyOtherKeys === 2
      || (modifyOtherKeys === 1 && (event.altKey || event.metaKey));
    return modifyOtherKeysApplies ? `\x1b[27;${modifier};${codepoint}~` : null;
  }

  function finishReplayIfReady(state: TerminalReplayState): void {
    if (
      replayState !== state
      || state.finishing
      || state.replayEndOffset === null
      || state.parsedThrough < state.replayEndOffset
      || !terminal
    ) {
      return;
    }
    state.finishing = true;

    const activate = () => {
      if (replayState !== state || state.generation !== attachmentGeneration) return;
      inputEnabled = true;
      if (state.focusRequested) terminal?.focus();
    };

    // Both emulators consume the same stream, but the daemon's Alacritty state is the durable
    // source of truth for mode 1004. Reconcile xterm before enabling its single reply route.
    if (terminal.modes.sendFocusMode !== state.focusReporting) {
      terminal.write(state.focusReporting ? '\x1b[?1004h' : '\x1b[?1004l', activate);
    } else {
      activate();
    }
  }

</script>

<section class="terminal-frame" class:is-stopped={process.status !== 'running'}>
  <div class="terminal-host" bind:this={host} aria-label={`${process.name} terminal`}></div>
  {#if process.status === 'running' && !hasOutput}
    <div class="terminal-starting" aria-live="polite">
      <span aria-hidden="true"></span>
      <strong>Waiting for first output…</strong>
    </div>
  {/if}
  {#if process.status !== 'running'}
    <div class="terminal-state">{process.status} · retained output</div>
  {/if}
  {#if linkHintVisible}
    <div class="terminal-link-hint" role="tooltip">{EXTERNAL_LINK_TOOLTIP}</div>
  {/if}
</section>

<style>
  .terminal-frame {
    position: relative;
    display: grid;
    grid-template-rows: minmax(0, 1fr);
    min-height: 0;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--terminal-background);
  }

  .terminal-starting {
    position: absolute;
    top: 12px;
    left: 12px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: color-mix(in srgb, var(--terminal-foreground) 68%, var(--terminal-background));
    font: 500 var(--font-size-sm)/1 var(--terminal-font-family);
    pointer-events: none;
  }

  .terminal-starting span {
    width: 10px;
    height: 10px;
    border: 1px solid color-mix(in srgb, var(--terminal-foreground) 34%, var(--terminal-background));
    border-top-color: var(--signal);
    border-radius: 50%;
    animation: terminal-waiting-spin 800ms linear infinite;
  }

  @keyframes terminal-waiting-spin { to { transform: rotate(360deg); } }

  .terminal-host {
    min-width: 0;
    min-height: 0;
    padding: 7px 6px 5px 8px;
  }

  .terminal-host :global(.xterm) {
    height: 100%;
  }

  .terminal-host :global(.xterm-viewport) {
    scrollbar-color: color-mix(in srgb, var(--terminal-foreground) 24%, var(--terminal-background)) transparent;
    scrollbar-width: thin;
  }

  .terminal-host :global(.xterm-screen canvas) {
    image-rendering: auto;
  }

  .terminal-state {
    position: absolute;
    right: 14px;
    bottom: 12px;
    border: 1px solid color-mix(in srgb, var(--terminal-foreground) 28%, var(--terminal-background));
    border-radius: 3px;
    padding: 5px 7px;
    color: color-mix(in srgb, var(--terminal-foreground) 68%, var(--terminal-background));
    background: color-mix(in srgb, var(--terminal-background) 88%, transparent);
    font: 500 var(--font-size-sm)/1 'JetBrains Mono Variable', monospace;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    pointer-events: none;
  }

  .terminal-link-hint {
    position: absolute;
    right: 10px;
    bottom: 9px;
    border: 1px solid color-mix(in srgb, var(--terminal-foreground) 24%, var(--terminal-background));
    border-radius: 3px;
    padding: 4px 6px;
    color: color-mix(in srgb, var(--terminal-foreground) 80%, var(--terminal-background));
    background: color-mix(in srgb, var(--terminal-background) 94%, transparent);
    font: 500 11px/1.2 var(--terminal-font-family);
    pointer-events: none;
  }

</style>
