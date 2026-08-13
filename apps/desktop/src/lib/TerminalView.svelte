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
  import {
    contextMenuRequest,
    FOCUS_TERMINAL_EVENT,
    TERMINAL_CONTEXT_ACTION_EVENT,
    type ContextMenuRequest,
    type TerminalContextActionDetail
  } from './contextMenu';
  import type { DaemonClient, ProcessView, TerminalFrame } from './daemon';
  import { Button } from './components/ui/button';
  import { EXTERNAL_LINK_TOOLTIP, openExternalUrl } from './externalLinks';
  import { stoppedOutputSnapshotKey } from './stoppedOutput';
  import { hasRetainedTerminalOutput } from './terminalFirstPaint';
  import {
    AGENT_TUI_CLIPBOARD_IMAGE_PASTE,
    clipboardImagePasteRoute,
    shouldForwardTerminalInput
  } from './terminalInput';
  import { encodeTerminalKey } from './terminalKeys';
  import {
    installTerminalTransfers,
    type TerminalTransfers,
    writeTerminalClipboardText
  } from './terminalTransfers';

  let {
    client,
    process,
    connected,
    visible = true,
    busy = false,
    onStart,
    onError,
    onContextMenu,
    onUnfocus
  }: {
    client: DaemonClient;
    process: ProcessView;
    connected: boolean;
    visible?: boolean;
    busy?: boolean;
    onStart?: (process: ProcessView) => void;
    onError: (message: string) => void;
    onContextMenu?: (request: ContextMenuRequest) => void;
    onUnfocus?: () => void;
  } = $props();

  let host: HTMLDivElement;
  let frame: HTMLElement;
  let terminal = $state<Terminal | null>(null);
  let fitAddon: FitAddon | null = null;
  let terminalTransfers: TerminalTransfers | null = null;
  let hasOutput = $state(false);
  let liveOutputPreviewElement = $state<HTMLPreElement | null>(null);
  let liveOutputPreview = $state('');
  let liveOutputLoaded = $state(false);
  let liveOutputRetained = $state(false);
  let linkHintVisible = $state(false);
  let hoveredLinkUri: string | null = null;
  let transferDropActive = $state(false);
  let imagePasteSaving = $state(false);
  let resizeFrame = 0;
  let inputTimer: ReturnType<typeof setTimeout> | null = null;
  let inputProcessId: number | null = null;
  let inputBytes: number[] = [];
  let nextUserKeyToken = 0;
  let pendingUserKeyTokens: number[] = [];
  let attachedProcessId: number | null = null;
  let attachedProcessPid: number | null = null;
  let attachedConnected = false;
  let attachedStatus = '';
  let attachedVisible = true;
  let attachmentGeneration = 0;
  let terminalOffset = 0;
  let inputEnabled = false;
  let replayState: TerminalReplayState | null = null;
  let kittyKeyboardFlags = 0;
  let modifyOtherKeys = 0;
  let appliedThemeSignature = '';
  let retainedOutput = $state('');
  let retainedOutputLoading = $state(false);
  let retainedOutputSnapshotKey: string | null = null;
  let retainedOutputGeneration = 0;
  const encoder = new TextEncoder();
  const initialAppearance = currentAppearance();
  let processDead = $derived(
    process.status === 'stopped' || process.status === 'exited' || process.status === 'crashed'
  );
  let processNeverRun = $derived(
    process.kind === 'command'
      && process.status === 'stopped'
      && process.exited_at === null
      && process.exit_code === null
      && process.exit_signal === null
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
    const showLinkHint = (_event: MouseEvent, uri: string) => {
      hoveredLinkUri = uri;
      linkHintVisible = true;
    };
    const hideLinkHint = (_event: MouseEvent, uri: string) => {
      if (hoveredLinkUri === uri) hoveredLinkUri = null;
      linkHintVisible = false;
    };
    const instance = new Terminal({
      allowTransparency: false,
      convertEol: false,
      cursorBlink: false,
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
    terminalTransfers = installTerminalTransfers({
      element: frame,
      canInsert: () => process.status === 'running',
      insert: (text) => {
        queueInput(encoder.encode(text), true);
        flushInput();
      },
      pasteText: (text) => {
        // Programmatic xterm paste emits onData without a preceding key event. Mark the next
        // emission as user input so it remains lossless while retained output is replaying.
        pendingUserKeyTokens.push(++nextUserKeyToken);
        instance.paste(text);
      },
      imagePasteRoute: () => clipboardImagePasteRoute(process.kind),
      forwardAgentImagePaste: () => {
        queueInput(encoder.encode(AGENT_TUI_CLIPBOARD_IMAGE_PASTE), true);
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
      let userKeyToken: number | null = null;
      if (event.type === 'keydown') {
        userKeyToken = ++nextUserKeyToken;
        pendingUserKeyTokens.push(userKeyToken);
        setTimeout(() => removePendingUserKeyToken(userKeyToken), 0);
      }
      if (
        event.metaKey && !event.altKey && !event.ctrlKey && !event.shiftKey
        && event.key.toLowerCase() === 'u'
      ) {
        if (event.type === 'keydown') onUnfocus?.();
        return false;
      }
      const modifiedKey = encodeTerminalKey(event, { kittyFlags: kittyKeyboardFlags, modifyOtherKeys });
      if (modifiedKey !== null) {
        if (event.type === 'keydown') {
          removePendingUserKeyToken(userKeyToken);
          queueInput(encoder.encode(modifiedKey), true);
        }
        return false;
      }
      return true;
    });
    const dataDisposable = instance.onData((data) => {
      queueInput(encoder.encode(data), consumePendingUserKeyToken());
    });
    const binaryDisposable = instance.onBinary((data) => {
      queueInput(
        Uint8Array.from(data, (character) => character.charCodeAt(0) & 0xff),
        consumePendingUserKeyToken()
      );
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
    const runContextAction = (event: Event) => {
      const detail = (event as CustomEvent<TerminalContextActionDetail>).detail;
      if (detail?.processId !== process.id) return;
      void runTerminalContextAction(detail);
    };
    window.addEventListener(TERMINAL_CONTEXT_ACTION_EVENT, runContextAction);

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
      window.removeEventListener(TERMINAL_CONTEXT_ACTION_EVENT, runContextAction);
      terminalTransfers?.dispose();
      terminalTransfers = null;
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
    const isVisible = visible;
    if (!instance) return;
    const sameProcessSession = attachedProcessId === processId
      && attachedProcessPid === processPid
      && attachedStatus === processStatus
      && attachedVisible === isVisible;
    if (
      sameProcessSession
      && attachedConnected === isConnected
    ) return;

    attachedProcessId = processId;
    attachedProcessPid = processPid;
    attachedConnected = isConnected;
    attachedStatus = processStatus;
    attachedVisible = isVisible;

    if (!isVisible || processStatus !== 'running') {
      flushInput();
      attachmentGeneration += 1;
      inputEnabled = false;
      replayState = null;
      terminalOffset = 0;
      setKeyboardProtocol(0, 0);
      instance.reset();
      hasOutput = false;
      liveOutputPreview = '';
      liveOutputLoaded = false;
      liveOutputRetained = false;
      linkHintVisible = false;
      hoveredLinkUri = null;
      if (isConnected) void client.detachTerminal().catch(() => undefined);
      if (isConnected) scheduleFit();
      return;
    }

    if (!isConnected) {
      // The daemon may still own a healthy PTY even though its control socket is being replaced.
      // Preserve xterm, focus, protocol state, and the parsed offset. Physical input remains
      // accepted and DaemonClient retains it until the native bridge reconnects.
      attachmentGeneration += 1;
      replayState = null;
      inputEnabled = true;
      return;
    }

    const resumingConnection = sameProcessSession;
    flushInput();
    inputEnabled = false;
    replayState = null;
    if (!resumingConnection) {
      terminalOffset = 0;
      setKeyboardProtocol(0, 0);
      instance.reset();
      hasOutput = false;
      liveOutputPreview = '';
      liveOutputLoaded = false;
      liveOutputRetained = false;
      linkHintVisible = false;
      hoveredLinkUri = null;
    }

    const generation = ++attachmentGeneration;
    const state: TerminalReplayState = {
      generation,
      processId,
      replayEndOffset: null,
      parsedThrough: terminalOffset,
      focusReporting: false,
      kittyKeyboardFlags: 0,
      modifyOtherKeys: 0,
      finishing: false,
      focusRequested: true
    };
    replayState = state;
    instance.focus();
    if (!resumingConnection) void loadLiveOutputPreview(state);
    void (async () => {
      // Replay at the PTY's actual viewport dimensions. Starting at xterm's 80x24 default and
      // resizing afterward reflows the active zsh prompt differently from a native terminal.
      await document.fonts.ready;
      await nextAnimationFrame();
      if (replayState !== state) return;
      fitTerminal();
      if (!resumingConnection) {
        await client.resizeTerminal(
          processId,
          instance.rows,
          instance.cols,
          Math.round(host.clientWidth),
          Math.round(host.clientHeight)
        );
        if (replayState !== state) return;
      }

      const attached = await client.attachTerminal(processId, terminalOffset);
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
    const snapshotKey = stoppedOutputSnapshotKey(process, connected);
    if (snapshotKey === retainedOutputSnapshotKey) return;
    retainedOutputSnapshotKey = snapshotKey;

    const shouldLoad = snapshotKey !== null;
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
    if (
      !visible
      || frame.process_id !== process.id
      || process.status !== 'running'
      || !terminal
    ) return;
    const state = replayState;
    const generation = attachmentGeneration;
    if (state) {
      state.kittyKeyboardFlags = frame.kitty_keyboard_flags;
      state.modifyOtherKeys = frame.modify_other_keys;
    }
    setKeyboardProtocol(frame.kitty_keyboard_flags, frame.modify_other_keys);
    terminal.write(Uint8Array.from(frame.data), () => {
      if (
        frame.data.length > 0
        && generation === attachmentGeneration
        && frame.process_id === process.id
      ) {
        hasOutput = true;
        terminalOffset = Math.max(terminalOffset, frame.start_offset + frame.data.length);
      }
      if (!state || replayState !== state || frame.process_id !== state.processId) return;
      state.parsedThrough = Math.max(
        state.parsedThrough,
        frame.start_offset + frame.data.length
      );
      finishReplayIfReady(state);
    });
  }

  async function loadLiveOutputPreview(state: TerminalReplayState): Promise<void> {
    try {
      const output = await client.renderedProcessOutput(state.processId);
      if (replayState !== state || state.generation !== attachmentGeneration) return;
      liveOutputPreview = output.text;
      liveOutputRetained = hasRetainedTerminalOutput(output);
      liveOutputLoaded = true;
      if (!output.text) return;

      await nextAnimationFrame();
      if (replayState !== state || hasOutput || !liveOutputPreviewElement) return;
      liveOutputPreviewElement.scrollTop = liveOutputPreviewElement.scrollHeight;
    } catch (cause) {
      if (replayState === state) onError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  function queueInput(bytes: Uint8Array, userInitiated = false): void {
    // xterm emits both physical keyboard data and terminal-protocol replies through onData.
    // Retained output is replayed into xterm on every attach, so its DA/DSR/XTVERSION/OSC and
    // focus replies must not be routed into the live shell until replay parsing is complete. Real
    // user input remains lossless while replay catches up, just as it is in a native terminal.
    if (!shouldForwardTerminalInput(inputEnabled, userInitiated) || process.status !== 'running') {
      return;
    }
    if (inputProcessId !== null && inputProcessId !== process.id) flushInput();
    inputProcessId = process.id;
    for (const byte of bytes) inputBytes.push(byte);
    if (!inputTimer) inputTimer = setTimeout(flushInput, 4);
  }

  function showTerminalContextMenu(event: MouseEvent): void {
    const instance = terminal;
    if (!instance || !onContextMenu) return;
    const request = contextMenuRequest(event, {
      kind: 'terminal',
      process: { id: process.id, kind: process.kind, name: process.name },
      hasSelection: instance.hasSelection(),
      link: hoveredLinkUri,
      pasteEnabled: process.status === 'running'
    });
    request.restoreFocus = instance.element?.querySelector<HTMLElement>('.xterm-helper-textarea')
      ?? host;
    onContextMenu(request);
  }

  async function runTerminalContextAction(detail: TerminalContextActionDetail): Promise<void> {
    const instance = terminal;
    if (!instance) return;
    try {
      switch (detail.action) {
        case 'terminal-copy': {
          const selection = instance.getSelection();
          if (selection) await writeTerminalClipboardText(selection);
          break;
        }
        case 'terminal-paste':
          await terminalTransfers?.pasteFromClipboard();
          break;
        case 'terminal-open-link':
          if (detail.link) openExternalUrl(detail.link, onError);
          break;
        case 'terminal-copy-link':
          if (detail.link) await writeTerminalClipboardText(detail.link);
          break;
        case 'terminal-select-all':
          instance.selectAll();
          break;
      }
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      instance.focus();
    }
  }

  function consumePendingUserKeyToken(): boolean {
    return pendingUserKeyTokens.shift() !== undefined;
  }

  function removePendingUserKeyToken(token: number | null): void {
    if (token === null) return;
    const index = pendingUserKeyTokens.indexOf(token);
    if (index >= 0) pendingUserKeyTokens.splice(index, 1);
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
      const alreadyFocused = document.activeElement !== null && host.contains(document.activeElement);
      inputEnabled = true;
      if (!state.focusRequested) return;
      if (alreadyFocused && state.focusReporting) {
        // Replay enabled mode 1004 after the DOM focus event, so xterm has no new event to report.
        queueInput(encoder.encode('\x1b[I'), true);
      } else {
        terminal?.focus();
      }
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

  function processNoun(): 'agent' | 'command' | 'terminal' {
    return process.kind;
  }

  function deadTitle(): string {
    const noun = processNoun();
    const label = `${noun[0].toUpperCase()}${noun.slice(1)}`;
    if (process.status === 'crashed') return `${label} crashed`;
    if (process.status === 'stopped') return `${label} stopped`;
    return `${label} exited`;
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
    oncontextmenu={showTerminalContextMenu}
  ></div>
  {#if process.status === 'running' && !hasOutput && liveOutputRetained && liveOutputPreview}
    <pre
      class="terminal-retained-preview"
      bind:this={liveOutputPreviewElement}
      aria-label={`Retained output from ${process.name}`}
    >{liveOutputPreview}</pre>
  {:else if process.status === 'running' && !hasOutput && liveOutputLoaded && !liveOutputRetained}
    <div class="terminal-starting" aria-live="polite">
      <span aria-hidden="true"></span>
      <strong>Waiting for first output…</strong>
    </div>
  {/if}
  {#if process.status === 'running' && !connected}
    <div class="terminal-reconnecting" aria-live="polite" aria-label="Daemon reconnecting; terminal input is queued">
      <span aria-hidden="true"></span>
      <strong>Reconnecting…</strong>
      <small>Keystrokes are queued</small>
    </div>
  {/if}
  {#if processStarting}
    <div class="process-starting" aria-live="polite">
      <span aria-hidden="true"></span>
      <strong>Starting {processNoun()}…</strong>
      <small>The live terminal will appear when the process is ready.</small>
    </div>
  {:else if processNeverRun}
    <div class="not-run-pane" aria-label={`${process.name} not run`}>
      <div class="not-run-empty">
        <span class="not-run-icon" aria-hidden="true">
          <SquareTerminalIcon size={22} strokeWidth={1.7} />
        </span>
        <span class="dead-kicker">Command output</span>
        <h2>Not run yet</h2>
        <p>Selecting a command only opens its output. Run it explicitly when you are ready.</p>
        {#if onStart}
          <Button
            size="sm"
            disabled={!connected || busy}
            aria-busy={busy}
            onclick={() => onStart?.(process)}
          >
            <PlayIcon size={14} strokeWidth={1.8} aria-hidden="true" />
            {busy ? 'Starting…' : 'Run command'}
          </Button>
        {/if}
      </div>
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

  .terminal-reconnecting {
    position: absolute;
    top: 10px;
    right: 10px;
    z-index: 4;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    border: 1px solid color-mix(in srgb, var(--warning-token) 42%, var(--border));
    border-radius: 999px;
    padding: 5px 9px;
    color: var(--terminal-foreground);
    background: color-mix(in srgb, var(--terminal-background) 91%, var(--warning-token));
    font: 500 var(--font-size-xs)/1 var(--terminal-font-family);
    pointer-events: none;
  }

  .terminal-reconnecting span {
    width: 8px;
    height: 8px;
    border: 1px solid color-mix(in srgb, var(--terminal-foreground) 32%, transparent);
    border-top-color: var(--warning-token);
    border-radius: 50%;
    animation: terminal-waiting-spin 800ms linear infinite;
  }

  .terminal-reconnecting small {
    color: color-mix(in srgb, var(--terminal-foreground) 66%, var(--terminal-background));
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

  .terminal-retained-preview {
    position: absolute;
    inset: 0;
    z-index: 1;
    box-sizing: border-box;
    margin: 0;
    overflow: hidden;
    padding: 7px 6px 5px 8px;
    color: var(--terminal-foreground);
    background: var(--terminal-background);
    font: 430 var(--terminal-font-size)/1.18 var(--terminal-font-family);
    white-space: pre;
    pointer-events: none;
  }

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

  .not-run-pane {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    overflow: auto;
    padding: clamp(var(--space-4), 4vw, 40px);
    color: var(--foreground);
    background: var(--background);
  }

  .not-run-empty {
    display: grid;
    width: min(440px, 100%);
    justify-items: center;
    gap: var(--space-2);
    text-align: center;
  }

  .not-run-icon {
    display: grid;
    width: 44px;
    height: 44px;
    margin-bottom: var(--space-1);
    place-items: center;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted-foreground);
    background: var(--card);
  }

  .not-run-empty h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 650;
  }

  .not-run-empty p {
    max-width: 400px;
    margin: 0 0 var(--space-2);
    color: var(--muted-foreground);
    font-size: var(--font-size-sm);
    line-height: 1.45;
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
