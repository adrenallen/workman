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
    terminalProfileXtermOptions,
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
  import { isDaemonRequestTimeoutError } from './daemonLog';
  import { EXTERNAL_LINK_TOOLTIP, openExternalUrl } from './externalLinks';
  import { primaryModifier, terminalUnfocusChord } from './primaryModifier';
  import {
    hasRetainedTerminalOutput,
    isUnstyledRetainedSnapshot,
    rawReplayHasGap,
    shouldShowRetainedPreview
  } from './terminalFirstPaint';
  import {
    WEBGL_STABLE_RESET_MS,
    shouldAttemptWebglRecovery,
    webglRecoveryDelay
  } from './terminalRenderer';
  import {
    isQuickPromptPaletteShortcut,
    sanitizeQuickPromptBody
  } from './quickPromptPalette';
  import {
    AGENT_TUI_CLIPBOARD_IMAGE_PASTE,
    clipboardImagePasteRoute,
    shouldForwardTerminalInput
  } from './terminalInput';
  import { encodeTerminalKey, processCycleDirection } from './terminalKeys';
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
    onAppShortcut,
    onQuickPrompts,
    onCycleProcess,
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
    onAppShortcut?: (event: KeyboardEvent) => boolean;
    onQuickPrompts?: () => void;
    onCycleProcess?: (direction: -1 | 1) => void;
    onUnfocus?: () => void;
  } = $props();

  let host: HTMLDivElement;
  let frame: HTMLElement;
  let terminal = $state<Terminal | null>(null);
  let fitAddon: FitAddon | null = null;
  let webglAddon: WebglAddon | null = null;
  let webglRecovering = false;
  let webglRecoveryAttempt = 0;
  let webglUnavailable = false;
  let webglRecoveryTimer: ReturnType<typeof setTimeout> | null = null;
  let webglStabilityTimer: ReturnType<typeof setTimeout> | null = null;
  let terminalTransfers: TerminalTransfers | null = null;
  let hasOutput = $state(false);
  let liveOutputPreviewElement = $state<HTMLPreElement | null>(null);
  let liveOutputPreview = $state('');
  let liveOutputLoaded = $state(false);
  let liveOutputRetained = $state(false);
  let retainedSnapshotOnly = $state(false);
  let replayPreviewAllowed = $state(false);
  let replayComplete = $state(false);
  let replayUnavailableMessage = $state<string | null>(null);
  let replayWatchdogTimer: ReturnType<typeof setTimeout> | null = null;
  let replayWarningTimer: ReturnType<typeof setTimeout> | null = null;
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
  let attachedToDaemon = false;
  let attachmentGeneration = 0;
  let terminalOffset = 0;
  let inputEnabled = false;
  let replayState: TerminalReplayState | null = null;
  let kittyKeyboardFlags = 0;
  let modifyOtherKeys = 0;
  let appliedThemeSignature = '';
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

  function supportsTerminalPlayback(status: ProcessView['status']): boolean {
    return status === 'running' || status === 'stopped' || status === 'exited' || status === 'crashed';
  }

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
    gapDetected: boolean;
  }

  /** Insert through xterm so bracketed-paste mode and replay-safe input ordering are preserved. */
  export function insertQuickPrompt(text: string, submit = false): boolean {
    const instance = terminal;
    if (!instance || process.status !== 'running') return false;
    pendingUserKeyTokens.push(++nextUserKeyToken);
    instance.paste(sanitizeQuickPromptBody(text));
    if (submit) {
      queueInput(encoder.encode('\r'), true);
      flushInput();
    }
    instance.focus();
    return true;
  }

  export function focusInput(): void {
    terminal?.focus();
  }

  onMount(() => {
    const initialPalette = initialAppearance.terminalTheme.palette;
    const initialFontFamily = terminalFontCss(
      initialAppearance.terminalFont,
      initialAppearance.terminalProfileStyle
    );
    const initialProfileOptions = terminalProfileXtermOptions(
      initialAppearance.terminalProfileStyle,
      initialFontFamily,
      initialAppearance.terminalFontSize
    );
    appliedThemeSignature = Object.values(initialPalette).join('|');
    const activateLink = (event: MouseEvent, uri: string) => {
      if (!primaryModifier(event) || event.button !== 0) return;
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
      ...initialProfileOptions,
      fontFamily: initialFontFamily,
      fontSize: initialAppearance.terminalFontSize,
      linkHandler: {
        activate: activateLink,
        hover: showLinkHint,
        leave: hideLinkHint,
        allowNonHttpProtocols: false
      },
      scrollback: 10_000,
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
      imagePasteRoute: () => clipboardImagePasteRoute(process.kind, process.agent_state.tool_type),
      forwardAgentImagePaste: () => {
        queueInput(encoder.encode(AGENT_TUI_CLIPBOARD_IMAGE_PASTE), true);
        flushInput();
      },
      focus: () => instance.focus(),
      reportError: onError,
      setDropActive: (active) => { transferDropActive = active; },
      setPasteSaving: (saving) => { imagePasteSaving = saving; }
    });

    if (!installWebglRenderer(instance, false)) {
      webglRecovering = true;
      scheduleWebglRecovery(instance);
    }
    const recoverVisibleRenderer = () => {
      if (document.visibilityState !== 'visible' || !visible) return;
      if (webglAddon !== null || webglUnavailable) return;
      webglRecovering = webglAddon === null;
      scheduleWebglRecovery(instance);
    };
    document.addEventListener('visibilitychange', recoverVisibleRenderer);
    window.addEventListener('pageshow', recoverVisibleRenderer);
    window.addEventListener('focus', recoverVisibleRenderer);

    instance.attachCustomKeyEventHandler((event) => {
      if (event.type === 'keydown' && onAppShortcut?.(event)) {
        event.preventDefault();
        event.stopPropagation();
        return false;
      }
      if (onQuickPrompts && isQuickPromptPaletteShortcut(event)) {
        event.preventDefault();
        event.stopPropagation();
        if (event.type === 'keydown') onQuickPrompts();
        return false;
      }
      const cycleDirection = onCycleProcess ? processCycleDirection(event) : null;
      if (cycleDirection !== null) {
        event.preventDefault();
        event.stopPropagation();
        if (event.type === 'keydown') onCycleProcess?.(cycleDirection);
        return false;
      }
      let userKeyToken: number | null = null;
      if (event.type === 'keydown') {
        userKeyToken = ++nextUserKeyToken;
        pendingUserKeyTokens.push(userKeyToken);
        setTimeout(() => removePendingUserKeyToken(userKeyToken), 0);
      }
      if (terminalUnfocusChord(event)) {
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
      document.removeEventListener('visibilitychange', recoverVisibleRenderer);
      window.removeEventListener('pageshow', recoverVisibleRenderer);
      window.removeEventListener('focus', recoverVisibleRenderer);
      if (webglRecoveryTimer) clearTimeout(webglRecoveryTimer);
      webglRecoveryTimer = null;
      clearWebglStabilityTimer();
      clearReplayWatchdog();
      clearReplayWarning();
      webglAddon = null;
      void client.detachTerminal().catch(() => undefined);
      instance.dispose();
      terminal = null;
    };
  });

  $effect(() => {
    const settings = $appearance;
    const instance = terminal;
    if (!instance) return;

    const family = terminalFontCss(settings.terminalFont, settings.terminalProfileStyle);
    const profileOptions = terminalProfileXtermOptions(
      settings.terminalProfileStyle,
      family,
      settings.terminalFontSize
    );
    const typographyChanged = instance.options.fontFamily !== family
      || instance.options.fontSize !== settings.terminalFontSize
      || instance.options.lineHeight !== profileOptions.lineHeight
      || instance.options.letterSpacing !== profileOptions.letterSpacing;
    const rendererStyleChanged = typographyChanged
      || instance.options.cursorBlink !== profileOptions.cursorBlink
      || instance.options.cursorStyle !== profileOptions.cursorStyle
      || instance.options.drawBoldTextInBrightColors
        !== profileOptions.drawBoldTextInBrightColors;
    const themeSignature = Object.values(settings.terminalTheme.palette).join('|');
    const themeChanged = themeSignature !== appliedThemeSignature;
    if (typographyChanged) {
      instance.options.fontFamily = family;
      instance.options.fontSize = settings.terminalFontSize;
      instance.options.lineHeight = profileOptions.lineHeight;
      instance.options.letterSpacing = profileOptions.letterSpacing;
    }
    if (rendererStyleChanged) {
      instance.options.cursorBlink = profileOptions.cursorBlink;
      instance.options.cursorStyle = profileOptions.cursorStyle;
      instance.options.drawBoldTextInBrightColors = profileOptions.drawBoldTextInBrightColors;
    }
    if (themeChanged) {
      appliedThemeSignature = themeSignature;
      instance.options.theme = terminalXtermTheme(settings.terminalTheme.palette);
    }
    if (rendererStyleChanged || themeChanged) {
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
    const terminalInput = instance.element?.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (terminalInput) {
      terminalInput.readOnly = processStatus !== 'running';
      terminalInput.setAttribute('aria-readonly', String(processStatus !== 'running'));
      terminalInput.setAttribute(
        'aria-label',
        processStatus === 'running' ? `${process.name} terminal input` : `${process.name} terminal output, read only`
      );
    }
    if (isVisible && webglAddon === null && !webglUnavailable) {
      webglRecovering = true;
      scheduleWebglRecovery(instance);
    }
    const sameProcessSession = attachedProcessId === processId
      && attachedProcessPid === processPid
      && attachedStatus === processStatus
      && attachedVisible === isVisible;
    const transitionedToReadOnly = attachedProcessId === processId
      && attachedStatus === 'running'
      && processStatus !== 'running'
      && supportsTerminalPlayback(processStatus)
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

    if (!isVisible || !supportsTerminalPlayback(processStatus)) {
      flushInput();
      attachmentGeneration += 1;
      attachedToDaemon = false;
      inputEnabled = false;
      replayState = null;
      clearReplayWatchdog();
      clearReplayWarning();
      terminalOffset = 0;
      setKeyboardProtocol(0, 0);
      instance.reset();
      hasOutput = false;
      replayPreviewAllowed = false;
      replayComplete = false;
      replayUnavailableMessage = null;
      liveOutputPreview = '';
      liveOutputLoaded = false;
      liveOutputRetained = false;
      retainedSnapshotOnly = false;
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
      attachedToDaemon = false;
      replayState = null;
      clearReplayWatchdog();
      clearReplayWarning();
      replayUnavailableMessage = null;
      inputEnabled = processStatus === 'running';
      return;
    }

    // Keep the already-rendered xterm buffer when a live process stops. A process opened after
    // it stopped still replays the retained raw PTY stream, including ANSI and cursor state.
    const resumingConnection = sameProcessSession || transitionedToReadOnly;
    flushInput();
    attachedToDaemon = false;
    inputEnabled = false;
    replayState = null;
    clearReplayWatchdog();
    clearReplayWarning();
    replayUnavailableMessage = null;
    if (!resumingConnection) {
      terminalOffset = 0;
      setKeyboardProtocol(0, 0);
      instance.reset();
      hasOutput = false;
      replayPreviewAllowed = false;
      replayComplete = false;
      replayUnavailableMessage = null;
      liveOutputPreview = '';
      liveOutputLoaded = false;
      liveOutputRetained = false;
      retainedSnapshotOnly = false;
      linkHintVisible = false;
      hoveredLinkUri = null;
    }

    const generation = ++attachmentGeneration;
    replayComplete = false;
    const state: TerminalReplayState = {
      generation,
      processId,
      replayEndOffset: null,
      parsedThrough: terminalOffset,
      focusReporting: false,
      kittyKeyboardFlags: 0,
      modifyOtherKeys: 0,
      finishing: false,
      focusRequested: processStatus === 'running',
      gapDetected: false
    };
    replayState = state;
    if (processStatus === 'running') instance.focus();
    if (!resumingConnection && processStatus === 'running') {
      replayPreviewAllowed = true;
      void loadLiveOutputPreview(state);
    }
    void (async () => {
      // Replay at the PTY's actual viewport dimensions. Starting at xterm's 80x24 default and
      // resizing afterward reflows the active zsh prompt differently from a native terminal.
      await document.fonts.ready;
      await nextAnimationFrame();
      if (replayState !== state) return;
      fitTerminal();
      armReplayWatchdog(state);
      const result = await attachTerminalWithRetry(state, instance);
      if (!result || replayState !== state) return;
      const { attached, requestedOffset } = result;
      attachedToDaemon = true;
      // If lifecycle work held the registry during the fast attach, this ordinary resize applies
      // the same geometry later without holding up replay or keyboard readiness.
      scheduleFit();
      state.gapDetected = requestedOffset > 0
        && rawReplayHasGap(requestedOffset, attached.replay_start_offset);
      state.replayEndOffset = attached.replay_end_offset;
      state.parsedThrough = Math.max(state.parsedThrough, attached.replay_start_offset);
      state.focusReporting = attached.focus_reporting;
      state.kittyKeyboardFlags = attached.keyboard_protocol.kitty_flags;
      state.modifyOtherKeys = attached.keyboard_protocol.modify_other_keys;
      setKeyboardProtocol(state.kittyKeyboardFlags, state.modifyOtherKeys);
      armReplayWatchdog(state);
      finishReplayIfReady(state);
    })().catch((cause) => {
      if (replayState !== state) return;
      clearReplayWatchdog();
      replayPreviewAllowed = retainedSnapshotOnly;
      replayComplete = true;
      replayUnavailableMessage = 'Styled terminal replay is unavailable. Reopen this process to retry.';
      onError(cause instanceof Error ? cause.message : String(cause));
    });
  });

  function handleTerminalFrame(frame: TerminalFrame): void {
    if (
      !visible
      || frame.process_id !== process.id
      || !supportsTerminalPlayback(process.status)
      || !terminal
    ) return;
    const state = replayState;
    const generation = attachmentGeneration;
    if (state) {
      state.kittyKeyboardFlags = frame.kitty_keyboard_flags;
      state.modifyOtherKeys = frame.modify_other_keys;
    }
    if (frame.gap && replayComplete) {
      showTransientReplayWarning('Some live styled output was skipped; showing the available tail.');
    } else if (frame.gap && state && terminalOffset > 0) {
      state.gapDetected = true;
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
        retainedSnapshotOnly = false;
        replayPreviewAllowed = false;
      }
      if (!state || replayState !== state || frame.process_id !== state.processId) return;
      state.parsedThrough = Math.max(
        state.parsedThrough,
        frame.start_offset + frame.data.length
      );
      armReplayWatchdog(state);
      finishReplayIfReady(state);
    });
  }

  async function loadLiveOutputPreview(state: TerminalReplayState): Promise<void> {
    try {
      const output = await client.renderedProcessOutput(state.processId);
      if (replayState !== state || state.generation !== attachmentGeneration) return;
      liveOutputPreview = output.text;
      liveOutputRetained = hasRetainedTerminalOutput(output);
      retainedSnapshotOnly = isUnstyledRetainedSnapshot(output);
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

  function installWebglRenderer(instance: Terminal, recovering: boolean): boolean {
    if (webglAddon || webglUnavailable) return webglAddon !== null;
    let addon: WebglAddon | null = null;
    try {
      addon = new WebglAddon();
      addon.onContextLoss(() => {
        if (webglAddon !== addon) return;
        // WKWebView commonly discards hidden canvases. xterm's DOM fallback keeps the configured
        // metrics while a bounded recovery campaign restores WebGL-quality glyph rendering.
        clearWebglStabilityTimer();
        webglAddon = null;
        addon?.dispose();
        if (webglUnavailable) return;
        webglRecovering = true;
        scheduleWebglRecovery(instance);
      });
      webglAddon = addon;
      instance.loadAddon(addon);
      if (webglAddon !== addon) return false;
      webglRecovering = false;
      armWebglStabilityReset(addon);
      if (recovering) {
        instance.clearTextureAtlas();
        instance.refresh(0, Math.max(0, instance.rows - 1));
      }
      return true;
    } catch {
      if (webglAddon === addon) webglAddon = null;
      addon?.dispose();
      return false;
    }
  }

  function scheduleWebglRecovery(instance: Terminal): void {
    if (webglRecoveryTimer || webglUnavailable) return;
    if (!shouldAttemptWebglRecovery({
      terminalVisible: visible,
      documentVisible: document.visibilityState === 'visible',
      hasRenderer: webglAddon !== null,
      recovering: webglRecovering
    })) return;
    const delay = webglRecoveryDelay(webglRecoveryAttempt++);
    if (delay === null) {
      webglRecovering = false;
      webglUnavailable = true;
      return;
    }
    webglRecoveryTimer = setTimeout(() => {
      webglRecoveryTimer = null;
      if (!installWebglRenderer(instance, true)) scheduleWebglRecovery(instance);
    }, delay);
  }

  function armWebglStabilityReset(addon: WebglAddon): void {
    clearWebglStabilityTimer();
    webglStabilityTimer = setTimeout(() => {
      webglStabilityTimer = null;
      if (webglAddon !== addon) return;
      webglRecoveryAttempt = 0;
    }, WEBGL_STABLE_RESET_MS);
  }

  function clearWebglStabilityTimer(): void {
    if (webglStabilityTimer) clearTimeout(webglStabilityTimer);
    webglStabilityTimer = null;
  }

  function scheduleFit(): void {
    if (resizeFrame) cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(() => {
      resizeFrame = 0;
      const instance = terminal;
      if (!instance || !fitTerminal()) return;
      if (!connected) return;
      // The initial terminal.attach carries this geometry on the socket's latency-sensitive
      // lane. Do not put a resize RPC in front of that attach while project hydration is using
      // the ordinary control lane. Stopped processes still publish their next launch size here.
      if (process.status === 'running' && !attachedToDaemon) return;
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
    const screenHeight = instance.element
      ?.querySelector<HTMLElement>('.xterm-screen')
      ?.getBoundingClientRect().height;
    if (screenHeight && instance.rows > 0) {
      frame.style.setProperty('--terminal-cell-height', `${screenHeight / instance.rows}px`);
    }
    return instance;
  }

  function nextAnimationFrame(): Promise<void> {
    return new Promise((resolve) => requestAnimationFrame(() => resolve()));
  }

  async function attachTerminalWithRetry(
    state: TerminalReplayState,
    instance: Terminal
  ): Promise<{
    attached: Awaited<ReturnType<DaemonClient['attachTerminal']>>;
    requestedOffset: number;
  } | null> {
    const retryDelays = [150, 500] as const;
    for (let attempt = 0; ; attempt += 1) {
      const requestedOffset = terminalOffset;
      try {
        const attached = await client.attachTerminal(state.processId, requestedOffset, {
          rows: instance.rows,
          cols: instance.cols,
          pixel_width: Math.round(host.clientWidth),
          pixel_height: Math.round(host.clientHeight)
        });
        return { attached, requestedOffset };
      } catch (cause) {
        const delay = isDaemonRequestTimeoutError(cause) ? retryDelays[attempt] : undefined;
        if (delay === undefined) throw cause;
        await new Promise((resolve) => setTimeout(resolve, delay));
        if (replayState !== state || !connected || !supportsTerminalPlayback(process.status)) return null;
        armReplayWatchdog(state);
      }
    }
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
      clearReplayWatchdog();
      clearReplayWarning();
      replayUnavailableMessage = null;
      replayPreviewAllowed = false;
      replayComplete = true;
      const alreadyFocused = document.activeElement !== null && host.contains(document.activeElement);
      inputEnabled = process.status === 'running';
      if (state.gapDetected) {
        showTransientReplayWarning('Some live styled output was skipped; showing the available tail.');
      }
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

  function clearReplayWatchdog(): void {
    if (replayWatchdogTimer) clearTimeout(replayWatchdogTimer);
    replayWatchdogTimer = null;
  }

  function armReplayWatchdog(state: TerminalReplayState): void {
    clearReplayWatchdog();
    if (
      replayState !== state
      || replayComplete
      || state.finishing
      || (state.replayEndOffset !== null && state.parsedThrough >= state.replayEndOffset)
    ) return;
    replayWatchdogTimer = setTimeout(() => {
      replayWatchdogTimer = null;
      if (replayState !== state || state.generation !== attachmentGeneration || replayComplete) {
        return;
      }
      replayPreviewAllowed = retainedSnapshotOnly;
      replayComplete = true;
      replayUnavailableMessage = 'Styled terminal replay stalled. Reopen this agent to retry.';
    }, 10_000);
  }

  function showTransientReplayWarning(message: string): void {
    clearReplayWarning();
    replayUnavailableMessage = message;
    replayWarningTimer = setTimeout(() => {
      replayWarningTimer = null;
      if (replayUnavailableMessage === message) replayUnavailableMessage = null;
    }, 4_000);
  }

  function clearReplayWarning(): void {
    if (replayWarningTimer) clearTimeout(replayWarningTimer);
    replayWarningTimer = null;
  }

  function processNoun(): 'agent' | 'command' | 'terminal' {
    return process.kind;
  }

  function endedTitle(): string {
    const noun = processNoun();
    const label = `${noun[0].toUpperCase()}${noun.slice(1)}`;
    if (processNeverRun) return `${label} ready`;
    if (process.status === 'crashed') return `${label} crashed`;
    if (process.status === 'stopped') return `${label} stopped`;
    return `${label} exited`;
  }

  function startActionLabel(): string {
    if (process.kind === 'command') return processNeverRun ? 'Run command' : 'Run again';
    if (process.kind === 'agent') return process.agent_session_id ? 'Resume agent' : 'Start agent';
    return 'Start terminal';
  }

  function busyActionLabel(): string {
    if (process.kind === 'command') return 'Running…';
    if (process.kind === 'agent' && process.agent_session_id) return 'Resuming…';
    return 'Starting…';
  }

  function exitSummary(): string {
    if (processNeverRun) return 'No output yet';
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
  class:has-ended-bar={processDead}
  class:is-drop-target={transferDropActive}
>
  <div
    class="terminal-host"
    class:is-hidden={processStarting}
    bind:this={host}
    aria-hidden={processStarting}
    aria-label={`${process.name} terminal`}
    oncontextmenu={showTerminalContextMenu}
  ></div>
  {#if process.status === 'running' && (replayPreviewAllowed || retainedSnapshotOnly) && liveOutputRetained && shouldShowRetainedPreview({ text: liveOutputPreview }, replayComplete, retainedSnapshotOnly)}
    <div class="terminal-retained-surface" class:is-snapshot={retainedSnapshotOnly}>
      {#if retainedSnapshotOnly}
        <span class="terminal-snapshot-label">Unstyled retained snapshot · live output will replace it</span>
      {/if}
      <pre
        class="terminal-retained-preview"
        bind:this={liveOutputPreviewElement}
        aria-label={retainedSnapshotOnly
          ? `Unstyled retained snapshot from ${process.name}`
          : `Retained output from ${process.name}`}
      >{liveOutputPreview}</pre>
    </div>
  {:else if process.status === 'running' && !hasOutput && liveOutputLoaded && !liveOutputRetained}
    <div class="terminal-starting" aria-live="polite">
      <span aria-hidden="true"></span>
      <strong>Waiting for first output…</strong>
    </div>
  {/if}
  {#if supportsTerminalPlayback(process.status) && replayComplete && replayUnavailableMessage}
    <div class="terminal-replay-warning" role="status">{replayUnavailableMessage}</div>
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
  {/if}
  {#if processDead}
    {@const ProcessIcon = process.kind === 'agent' ? BotIcon : SquareTerminalIcon}
    {@const stoppedAt = exitedAtLabel()}
    <button
      type="button"
      class="process-ended-bar"
      class:is-crashed={process.status === 'crashed'}
      disabled={!onStart || !connected || busy}
      aria-busy={busy}
      aria-label={`${endedTitle()}. ${exitSummary()}. ${busy ? busyActionLabel() : startActionLabel()}`}
      onclick={() => onStart?.(process)}
    >
      <span class="ended-status">
        <span class="ended-icon" aria-hidden="true"><ProcessIcon size={15} strokeWidth={1.8} /></span>
        <span class="ended-copy">
          <strong>{endedTitle()}</strong>
          <small>{exitSummary()}{#if stoppedAt}<span> · {stoppedAt}</span>{/if} · Read only</small>
        </span>
      </span>
      <span class="ended-action">
        <span>{busy ? busyActionLabel() : startActionLabel()}</span>
        <PlayIcon size={14} strokeWidth={2} aria-hidden="true" />
      </span>
    </button>
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
    grid-template-rows: minmax(0, 1fr) auto;
    min-height: 0;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--terminal-background);
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

  .terminal-retained-surface {
    position: absolute;
    inset: 0;
    z-index: 1;
    overflow: hidden;
    background: var(--terminal-background);
    pointer-events: none;
  }

  .terminal-retained-preview {
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    margin: 0;
    overflow: hidden;
    padding: 7px 6px 5px 8px;
    color: var(--terminal-foreground);
    background: var(--terminal-background);
    font-family: var(--terminal-font-family);
    font-size: var(--terminal-font-size);
    font-style: normal;
    font-weight: normal;
    line-height: var(
      --terminal-cell-height,
      calc(var(--terminal-font-size) * var(--terminal-line-height) * 1.2)
    );
    letter-spacing: var(--terminal-letter-spacing);
    white-space: pre;
    pointer-events: none;
  }

  .terminal-retained-surface.is-snapshot .terminal-retained-preview {
    padding-top: 36px;
    opacity: .82;
  }

  .terminal-snapshot-label {
    position: absolute;
    top: 8px;
    left: 8px;
    z-index: 1;
    border: 1px solid color-mix(in srgb, var(--warning-token) 42%, var(--border));
    border-radius: 3px;
    padding: 4px 7px;
    color: var(--terminal-foreground);
    background: color-mix(in srgb, var(--terminal-background) 91%, var(--warning-token));
    font: 500 var(--font-size-xs)/1.2 var(--terminal-font-family);
  }

  .terminal-replay-warning {
    position: absolute;
    top: 10px;
    left: 50%;
    z-index: 3;
    max-width: calc(100% - 32px);
    transform: translateX(-50%);
    border: 1px solid color-mix(in srgb, var(--warning-token) 42%, var(--border));
    border-radius: 3px;
    padding: 5px 8px;
    color: var(--terminal-foreground);
    background: color-mix(in srgb, var(--terminal-background) 91%, var(--warning-token));
    font: var(--font-size-xs)/1.3 var(--terminal-font-family);
    text-align: center;
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

  .process-ended-bar {
    display: flex;
    width: 100%;
    min-width: 0;
    min-height: 50px;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    appearance: none;
    border: 0;
    border-top: 1px solid color-mix(in srgb, var(--agent-state-exited) 38%, var(--border));
    padding: 7px 9px 7px 10px;
    color: var(--terminal-foreground);
    background: color-mix(in srgb, var(--terminal-background) 91%, var(--agent-state-exited));
    font-family: var(--terminal-font-family);
    text-align: left;
    cursor: pointer;
  }

  .process-ended-bar:hover:not(:disabled) {
    border-top-color: color-mix(in srgb, var(--signal) 55%, var(--border));
    background: color-mix(in srgb, var(--terminal-background) 88%, var(--signal));
  }

  .process-ended-bar:focus-visible {
    outline: 2px solid var(--ring);
    outline-offset: -2px;
  }

  .process-ended-bar:disabled { cursor: default; }

  .process-ended-bar.is-crashed {
    border-top-color: color-mix(in srgb, var(--destructive) 44%, var(--border));
  }

  .ended-status {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 9px;
  }

  .ended-icon {
    display: grid;
    width: 28px;
    height: 28px;
    flex: none;
    place-items: center;
    border: 1px solid color-mix(in srgb, var(--agent-state-exited) 38%, var(--border));
    border-radius: 4px;
    color: var(--agent-state-exited);
    background: color-mix(in srgb, var(--agent-state-exited) 9%, var(--terminal-background));
  }

  .is-crashed .ended-icon {
    border-color: color-mix(in srgb, var(--destructive) 45%, var(--border));
    color: var(--destructive);
    background: color-mix(in srgb, var(--destructive) 9%, var(--terminal-background));
  }

  .ended-copy { display: grid; min-width: 0; gap: 3px; }
  .ended-copy strong { overflow: hidden; font-size: var(--font-size-sm); line-height: 1; text-overflow: ellipsis; white-space: nowrap; }
  .ended-copy small { overflow: hidden; color: color-mix(in srgb, var(--terminal-foreground) 62%, var(--terminal-background)); font-size: var(--font-size-xs); line-height: 1; text-overflow: ellipsis; white-space: nowrap; }

  .ended-action {
    display: inline-flex;
    min-height: 30px;
    flex: none;
    align-items: center;
    gap: 7px;
    border: 1px solid color-mix(in srgb, var(--signal) 45%, var(--border));
    border-radius: 4px;
    padding: 0 10px;
    color: color-mix(in srgb, var(--signal) 76%, var(--terminal-foreground));
    background: color-mix(in srgb, var(--signal) 10%, var(--terminal-background));
    font-size: var(--font-size-xs);
    font-weight: 650;
  }

  .process-ended-bar:disabled .ended-action { opacity: .48; }

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

  .terminal-frame.has-ended-bar .terminal-link-hint { bottom: 59px; }

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
    .ended-copy small { display: none; }
  }

  @media (max-width: 460px) {
    .ended-action span { display: none; }
    .ended-action { width: 30px; justify-content: center; padding: 0; }
  }

  @media (prefers-reduced-motion: reduce) {
    .terminal-starting span,
    .process-starting > span {
      animation: none;
    }
  }

</style>
