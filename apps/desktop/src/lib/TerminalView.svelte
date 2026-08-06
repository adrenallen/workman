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
    terminalFontCss
  } from './appearance';
  import { FOCUS_TERMINAL_EVENT } from './contextMenu';
  import type { DaemonClient, ProcessView, TerminalFrame } from './daemon';

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
  let resizeFrame = 0;
  let inputTimer: ReturnType<typeof setTimeout> | null = null;
  let inputProcessId: number | null = null;
  let inputBytes: number[] = [];
  let attachedProcessId: number | null = null;
  let attachedConnected = false;
  const encoder = new TextEncoder();
  const initialAppearance = currentAppearance();

  onMount(() => {
    const instance = new Terminal({
      allowTransparency: false,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: 'bar',
      fontFamily: terminalFontCss(initialAppearance.terminalFont),
      fontSize: initialAppearance.terminalFontSize,
      fontWeight: 430,
      lineHeight: 1.18,
      scrollback: 10_000,
      smoothScrollDuration: 80,
      theme: {
        background: 'var(--background)',
        foreground: '#d7e2dc',
        cursor: '#7bd1b5',
        cursorAccent: 'var(--background)',
        selectionBackground: '#355c55aa',
        black: '#11191b',
        red: '#dc7d76',
        green: '#79c69f',
        yellow: '#d7ad65',
        blue: '#78aecd',
        magenta: '#bca0cf',
        cyan: '#72c8c2',
        white: '#d7e2dc',
        brightBlack: '#62706d',
        brightRed: '#ef958e',
        brightGreen: '#99dab8',
        brightYellow: '#e8c37f',
        brightBlue: '#98c7df',
        brightMagenta: '#d0b7dd',
        brightCyan: '#94dcd6',
        brightWhite: '#f2f6f3'
      }
    });
    fitAddon = new FitAddon();
    instance.loadAddon(fitAddon);
    instance.loadAddon(
      new WebLinksAddon((_event, uri) => {
        if (/^https?:\/\//i.test(uri)) window.open(uri, '_blank', 'noopener,noreferrer');
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
    instance.focus();
    const focusRequested = (event: Event) => {
      const detail = (event as CustomEvent<{ processId?: number }>).detail;
      if (detail?.processId === process.id) instance.focus();
    };
    window.addEventListener(FOCUS_TERMINAL_EVENT, focusRequested);

    return () => {
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
    if (typographyChanged) {
      instance.options.fontFamily = family;
      instance.options.fontSize = settings.terminalFontSize;
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

    instance.reset();
    hasOutput = false;
    if (!isConnected) return;

    let cancelled = false;
    void client
      .attachTerminal(processId)
      .then(() => {
        if (!cancelled) {
          scheduleFit();
          instance.focus();
        }
      })
      .catch((cause) => {
        if (!cancelled) onError(cause instanceof Error ? cause.message : String(cause));
      });
    return () => {
      cancelled = true;
      flushInput();
    };
  });

  function handleTerminalFrame(frame: TerminalFrame): void {
    if (frame.process_id !== process.id || !terminal) return;
    if (frame.data.length > 0) hasOutput = true;
    terminal.write(Uint8Array.from(frame.data));
  }

  function queueInput(bytes: Uint8Array): void {
    if (process.status !== 'running') return;
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
      if (!instance || !fitAddon || host.clientWidth === 0 || host.clientHeight === 0) return;
      fitAddon.fit();
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
    background: #101214;
  }

  .terminal-starting {
    position: absolute;
    top: 12px;
    left: 12px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: #8e959e;
    font: 500 var(--font-size-sm)/1 var(--terminal-font-family);
    pointer-events: none;
  }

  .terminal-starting span {
    width: 10px;
    height: 10px;
    border: 1px solid #56605f;
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
    scrollbar-color: #344746 transparent;
    scrollbar-width: thin;
  }

  .terminal-host :global(.xterm-screen canvas) {
    image-rendering: auto;
  }

  .terminal-state {
    position: absolute;
    right: 14px;
    bottom: 12px;
    border: 1px solid #394547;
    border-radius: 3px;
    padding: 5px 7px;
    color: #83918f;
    background: rgb(16 24 26 / 86%);
    font: 500 var(--font-size-sm)/1 'JetBrains Mono Variable', monospace;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    pointer-events: none;
  }

</style>
