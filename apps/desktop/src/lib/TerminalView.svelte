<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import PlayIcon from '@lucide/svelte/icons/play';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
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
  import { Button } from './components/ui/button';
  import { EXTERNAL_LINK_TOOLTIP, openExternalUrl } from './externalLinks';
  import { encodeTerminalKey } from './terminalKeys';
  import { installTerminalTransfers } from './terminalTransfers';

  let {
    client,
    process,
    connected,
    busy = false,
    onStart,
    onError,
    onUnfocus
  }: {
    client: DaemonClient;
    process: ProcessView;
    connected: boolean;
    busy?: boolean;
    onStart?: (process: ProcessView) => void;
    onError: (message: string) => void;
    onUnfocus?: () => void;
  } = $props();

  let host: HTMLDivElement;
  let frame: HTMLElement;
  let terminal = $state<Terminal | null>(null);
  let fitAddon: FitAddon | null = null;
  let hasOutput = $state(false);
  let linkHintVisible = $state(false);
  let transferDropActive = $state(false);
  let imagePasteSaving = $state(false);
  let resizeFrame = 0;
  let inputTimer: ReturnType<typeof setTimeout> | null = null;
  let inputProcessId: number | null = null;
  let inputBytes: number[] = [];
  let attachedProcessId: number | null = null;
  let attachedProcessPid: number | null = null;
  let attachedConnected = false;
  let attachedStatus = '';
  let attachmentGeneration = 0;
  let inputEnabled = false;
  let replayState: TerminalReplayState | null = null;
  let kittyKeyboardFlags = 0;
  let modifyOtherKeys = 0;
  let appliedThemeSignature = '';
  let retainedOutput = $state('');
  let retainedOutputLoading = $state(false);
  let retainedOutputGeneration = 0;
  const encoder = new TextEncoder();
  const initialAppearance = currentAppearance();
  let processDead = $derived(
    process.status === 'stopped' || process.status === 'exited' || process.status === 'crashed'
  );
  let processStarting = $derived(process.status === 'starting');
  let retainedTail = $derived(outputTail(retainedOutput));

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
    const removeTerminalTransfers = installTerminalTransfers({
      element: frame,
      canInsert: () => inputEnabled && process.status === 'running',
      insert: (text) => {
        queueInput(encoder.encode(text));
        flushInput();
      },
      focus: () => instance.focus(),
      reportError: onError,
      setDropActive: (active) => { transferDropActive = active; },
      setPasteSaving: (saving) => { imagePasteSaving = saving; }
    });

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
      const modifiedKey = encodeTerminalKey(event, { kittyFlags: kittyKeyboardFlags, modifyOtherKeys });
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
      removeTerminalTransfers();
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
    const processPid = process.pid;
    const isConnected = connected;
    const processStatus = process.status;
    if (!instance) return;
    if (
      attachedProcessId === processId
      && attachedProcessPid === processPid
      && attachedConnected === isConnected
      && attachedStatus === processStatus
    ) return;

    attachedProcessId = processId;
    attachedProcessPid = processPid;
    attachedConnected = isConnected;
    attachedStatus = processStatus;

    flushInput();
    inputEnabled = false;
    replayState = null;
    setKeyboardProtocol(0, 0);
    instance.reset();
    hasOutput = false;
    linkHintVisible = false;
    if (!isConnected || processStatus !== 'running') {
      attachmentGeneration += 1;
      void client.detachTerminal().catch(() => undefined);
      if (isConnected) scheduleFit();
      return;
    }

    const generation = ++attachmentGeneration;
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
      await client.resizeTerminal(
        processId,
        instance.rows,
        instance.cols,
        Math.round(host.clientWidth),
        Math.round(host.clientHeight)
      );
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

  $effect(() => {
    const processId = process.id;
    const shouldLoad = connected && processDead;
    const generation = ++retainedOutputGeneration;
    retainedOutput = '';
    retainedOutputLoading = shouldLoad;
    if (!shouldLoad) return;

    void client.renderedProcessOutput(processId).then((output) => {
      if (generation !== retainedOutputGeneration || process.id !== processId) return;
      retainedOutput = output.text;
    }).catch((cause) => {
      if (generation !== retainedOutputGeneration || process.id !== processId) return;
      onError(cause instanceof Error ? cause.message : String(cause));
    }).finally(() => {
      if (generation === retainedOutputGeneration && process.id === processId) {
        retainedOutputLoading = false;
      }
    });
  });

  function handleTerminalFrame(frame: TerminalFrame): void {
    if (frame.process_id !== process.id || process.status !== 'running' || !terminal) return;
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
      if (!connected) return;
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

  function outputTail(output: string): string {
    const rows = output.replaceAll('\r', '').split('\n');
    while (rows.length > 0 && rows.at(-1)?.trim() === '') rows.pop();
    return rows.slice(-24).join('\n');
  }

  function processNoun(): 'agent' | 'terminal' {
    return process.kind === 'agent' ? 'agent' : 'terminal';
  }

  function deadTitle(): string {
    if (process.status === 'crashed') return `${processNoun() === 'agent' ? 'Agent' : 'Terminal'} crashed`;
    if (process.status === 'stopped') return `${processNoun() === 'agent' ? 'Agent' : 'Terminal'} stopped`;
    return `${processNoun() === 'agent' ? 'Agent' : 'Terminal'} exited`;
  }

  function exitSummary(): string {
    if (process.exit_signal !== null) return `Terminated by signal ${process.exit_signal}`;
    if (process.exit_code !== null) {
      return process.exit_code === 0
        ? 'Exited cleanly · code 0'
        : `Exited with code ${process.exit_code}`;
    }
    if (process.status === 'stopped') return 'Stopped by you';
    if (process.status === 'crashed') return 'The process ended unexpectedly';
    return 'The process has ended';
  }

  function exitedAtLabel(): string | null {
    if (process.exited_at === null) return null;
    return new Intl.DateTimeFormat([], {
      dateStyle: 'medium',
      timeStyle: 'short'
    }).format(new Date(process.exited_at));
  }

</script>

<section
  bind:this={frame}
  class="terminal-frame"
  class:is-dead={processDead}
  class:is-drop-target={transferDropActive}
>
  <div
    class="terminal-host"
    class:is-hidden={process.status !== 'running'}
    bind:this={host}
    aria-hidden={process.status !== 'running'}
    aria-label={`${process.name} terminal`}
  ></div>
  {#if process.status === 'running' && !hasOutput}
    <div class="terminal-starting" aria-live="polite">
      <span aria-hidden="true"></span>
      <strong>Waiting for first output…</strong>
    </div>
  {/if}
  {#if processStarting}
    <div class="process-starting" aria-live="polite">
      <span aria-hidden="true"></span>
      <strong>Starting {processNoun()}…</strong>
      <small>The live terminal will appear when the process is ready.</small>
    </div>
  {:else if processDead}
    {@const ProcessIcon = process.kind === 'agent' ? BotIcon : SquareTerminalIcon}
    {@const stoppedAt = exitedAtLabel()}
    <div class="dead-pane" aria-label={`${process.name} ${process.status}`}>
      <div class="dead-document">
        <header class="dead-summary">
          <span class="dead-icon" class:is-crashed={process.status === 'crashed'} aria-hidden="true">
            <ProcessIcon size={20} strokeWidth={1.7} />
          </span>
          <div class="dead-copy">
            <span class="dead-kicker">Session ended</span>
            <h2>{deadTitle()}</h2>
            <p>{exitSummary()}{#if stoppedAt}<span> · {stoppedAt}</span>{/if}</p>
          </div>
          {#if onStart}
            <span class="dead-start">
              <Button
                class="w-full"
                size="sm"
                disabled={!connected || busy}
                aria-busy={busy}
                onclick={() => onStart?.(process)}
              >
                <PlayIcon size={14} strokeWidth={1.8} aria-hidden="true" />
                {busy ? 'Starting…' : `Start ${processNoun()}`}
              </Button>
            </span>
          {/if}
        </header>

        <section class="output-card" aria-label={`Last output from ${process.name}`}>
          <header>
            <strong>Last output</strong>
            <span>Read only</span>
          </header>
          {#if retainedOutputLoading}
            <p class="output-empty">Loading retained output…</p>
          {:else if retainedTail}
            <pre>{retainedTail}</pre>
          {:else}
            <p class="output-empty">No output was retained for this session.</p>
          {/if}
        </section>
      </div>
    </div>
  {/if}
  {#if linkHintVisible}
    <div class="terminal-link-hint" role="tooltip">{EXTERNAL_LINK_TOOLTIP}</div>
  {/if}
  {#if transferDropActive || imagePasteSaving}
    <div class="terminal-transfer-hint" aria-live="polite">
      {transferDropActive ? 'Drop to insert file path' : 'Saving pasted image…'}
    </div>
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

  .terminal-frame.is-dead {
    background: var(--background);
  }

  .terminal-frame.is-drop-target {
    border-color: var(--ring);
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
    background: var(--terminal-background);
  }

  .terminal-host.is-hidden {
    opacity: 0;
    pointer-events: none;
  }

  .terminal-host :global(.xterm) {
    box-sizing: border-box;
    height: 100%;
    padding: 7px 6px 5px 8px;
    background: var(--terminal-background);
  }

  .terminal-host :global(.xterm-viewport) {
    background-color: var(--terminal-background);
    scrollbar-color: color-mix(in srgb, var(--terminal-foreground) 24%, var(--terminal-background)) transparent;
    scrollbar-width: thin;
  }

  .terminal-host :global(.xterm-screen) {
    background: var(--terminal-background);
  }

  .terminal-host :global(.composition-view) {
    background: var(--terminal-background);
    color: var(--terminal-foreground);
  }

  .terminal-host :global(.xterm-screen canvas) {
    image-rendering: auto;
  }

  .process-starting {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: var(--space-2);
    padding: var(--space-4);
    color: var(--muted-foreground);
    background: var(--background);
    text-align: center;
  }

  .process-starting > span {
    width: 18px;
    height: 18px;
    border: 1.5px solid var(--border-strong);
    border-top-color: var(--agent-state-working);
    border-radius: 50%;
    animation: terminal-waiting-spin 800ms linear infinite;
  }

  .process-starting strong {
    color: var(--foreground);
    font-size: var(--font-size-base);
  }

  .process-starting small {
    font-size: var(--font-size-sm);
  }

  .dead-pane {
    position: absolute;
    inset: 0;
    overflow: auto;
    padding: clamp(var(--space-4), 4vw, 40px);
    color: var(--foreground);
    background: var(--background);
  }

  .dead-document {
    display: grid;
    width: min(760px, 100%);
    min-height: 100%;
    align-content: center;
    gap: var(--space-4);
    margin: 0 auto;
  }

  .dead-summary {
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-4);
    background: var(--card);
  }

  .dead-icon {
    display: grid;
    width: 40px;
    height: 40px;
    place-items: center;
    border: 1px solid color-mix(in srgb, var(--agent-state-exited) 34%, var(--border));
    border-radius: var(--radius);
    color: var(--agent-state-exited);
    background: color-mix(in srgb, var(--agent-state-exited) 6%, var(--card));
  }

  .dead-icon.is-crashed {
    border-color: color-mix(in srgb, var(--destructive) 44%, var(--border));
    color: var(--destructive);
    background: color-mix(in srgb, var(--destructive) 7%, var(--card));
  }

  .dead-copy {
    min-width: 0;
  }

  .dead-kicker {
    display: block;
    margin-bottom: var(--space-1);
    color: var(--muted-foreground);
    font: 650 var(--font-size-xs)/1 var(--terminal-font-family);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .dead-copy h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 650;
    line-height: 1.25;
  }

  .dead-copy p {
    margin: var(--space-1) 0 0;
    color: var(--muted-foreground);
    font-size: var(--font-size-sm);
  }

  .dead-start {
    min-width: 116px;
  }

  .output-card {
    min-height: 132px;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--terminal-background);
  }

  .output-card > header {
    display: flex;
    min-height: 32px;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid color-mix(in srgb, var(--terminal-foreground) 18%, var(--terminal-background));
    padding: 0 var(--space-3);
    color: color-mix(in srgb, var(--terminal-foreground) 70%, var(--terminal-background));
    font: var(--font-size-xs)/1 var(--terminal-font-family);
  }

  .output-card > header strong {
    color: color-mix(in srgb, var(--terminal-foreground) 86%, var(--terminal-background));
    font-weight: 600;
  }

  .output-card pre {
    max-height: min(44vh, 440px);
    overflow: auto;
    margin: 0;
    padding: var(--space-3);
    color: color-mix(in srgb, var(--terminal-foreground) 86%, var(--terminal-background));
    font: var(--font-size-sm)/1.45 var(--terminal-font-family);
    tab-size: 4;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .output-empty {
    margin: 0;
    padding: var(--space-4) var(--space-3);
    color: color-mix(in srgb, var(--terminal-foreground) 55%, var(--terminal-background));
    font: var(--font-size-sm)/1.4 var(--terminal-font-family);
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

  .terminal-transfer-hint {
    position: absolute;
    left: 10px;
    bottom: 9px;
    border: 1px solid color-mix(in srgb, var(--terminal-foreground) 28%, var(--terminal-background));
    border-radius: 3px;
    padding: 4px 6px;
    color: color-mix(in srgb, var(--terminal-foreground) 84%, var(--terminal-background));
    background: color-mix(in srgb, var(--terminal-background) 94%, transparent);
    font: 500 11px/1.2 var(--terminal-font-family);
    pointer-events: none;
  }

  @media (max-width: 720px) {
    .dead-summary {
      grid-template-columns: 40px minmax(0, 1fr);
    }

    .dead-start {
      grid-column: 1 / -1;
      justify-self: stretch;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .terminal-starting span,
    .process-starting > span {
      animation: none;
    }
  }

</style>
