<script lang="ts">
  import { FitAddon } from '@xterm/addon-fit';
  import { SearchAddon } from '@xterm/addon-search';
  import { WebLinksAddon } from '@xterm/addon-web-links';
  import { WebglAddon } from '@xterm/addon-webgl';
  import { Terminal } from '@xterm/xterm';
  import '@xterm/xterm/css/xterm.css';
  import { onMount } from 'svelte';

  import type { DaemonClient, ProcessView, TerminalFrame } from './daemon';

  let {
    client,
    process,
    connected,
    onError
  }: {
    client: DaemonClient;
    process: ProcessView;
    connected: boolean;
    onError: (message: string) => void;
  } = $props();

  let host: HTMLDivElement;
  let searchInput = $state<HTMLInputElement>();
  let terminal = $state<Terminal | null>(null);
  let fitAddon: FitAddon | null = null;
  let searchAddon: SearchAddon | null = null;
  let renderer = $state<'webgl' | 'dom'>('dom');
  let searchOpen = $state(false);
  let searchTerm = $state('');
  let searchResult = $state({ index: -1, count: 0 });
  let streamGap = $state<number | null>(null);
  let expectedOffset = 0;
  let resizeFrame = 0;
  let inputTimer: ReturnType<typeof setTimeout> | null = null;
  let inputProcessId: number | null = null;
  let inputBytes: number[] = [];
  let attachedProcessId: number | null = null;
  let attachedConnected = false;
  let fallbackSearchTerm = '';
  let fallbackSearchIndex = -1;
  let fallbackSearchMatches: Array<{ row: number; col: number; size: number }> = [];
  const encoder = new TextEncoder();

  const searchOptions = {
    incremental: true,
    decorations: {
      matchBackground: '#314945',
      matchOverviewRuler: '#527f74',
      activeMatchBackground: '#b98a4d',
      activeMatchColorOverviewRuler: '#d5a55f'
    }
  };

  onMount(() => {
    const instance = new Terminal({
      allowTransparency: false,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: 'bar',
      fontFamily: '"JetBrains Mono Variable", "SFMono-Regular", Consolas, monospace',
      fontSize: 13,
      fontWeight: 430,
      lineHeight: 1.18,
      scrollback: 10_000,
      smoothScrollDuration: 80,
      theme: {
        background: '#0e1517',
        foreground: '#d7e2dc',
        cursor: '#7bd1b5',
        cursorAccent: '#0e1517',
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
    searchAddon = new SearchAddon({ highlightLimit: 2_000 });
    instance.loadAddon(fitAddon);
    instance.loadAddon(searchAddon);
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
        renderer = 'dom';
      });
      instance.loadAddon(webgl);
      renderer = 'webgl';
    } catch {
      renderer = 'dom';
    }

    instance.attachCustomKeyEventHandler((event) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'f') {
        if (event.type === 'keydown') openSearch();
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

    return () => {
      flushInput();
      if (resizeFrame) cancelAnimationFrame(resizeFrame);
      resizeObserver.disconnect();
      removeTerminalListener();
      dataDisposable.dispose();
      binaryDisposable.dispose();
      void client.detachTerminal().catch(() => undefined);
      instance.dispose();
      terminal = null;
    };
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
    expectedOffset = 0;
    streamGap = null;
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
    if (frame.gap || frame.start_offset !== expectedOffset) streamGap = frame.start_offset;
    expectedOffset = frame.start_offset + frame.data.length;
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

  function openSearch(): void {
    searchOpen = true;
    queueMicrotask(() => {
      searchInput?.focus();
      searchInput?.select();
    });
  }

  function closeSearch(): void {
    searchOpen = false;
    searchAddon?.clearDecorations();
    fallbackSearchTerm = '';
    fallbackSearchIndex = -1;
    fallbackSearchMatches = [];
    searchResult = { index: -1, count: 0 };
    terminal?.focus();
  }

  function findNext(term = searchTerm): void {
    runSearch(term, 1);
  }

  function findPrevious(): void {
    runSearch(searchInput?.value ?? searchTerm, -1);
  }

  function runSearch(term: string, direction: 1 | -1): void {
    searchTerm = term;
    if (!term) {
      searchAddon?.clearDecorations();
      fallbackSearchTerm = '';
      fallbackSearchIndex = -1;
      fallbackSearchMatches = [];
      searchResult = { index: -1, count: 0 };
      return;
    }

    let found = false;
    try {
      found =
        direction === 1
          ? (searchAddon?.findNext(term, searchOptions) ?? false)
          : (searchAddon?.findPrevious(term, searchOptions) ?? false);
    } catch {
      // Some WebKit/WebGL combinations cannot create search decorations.
      // The active-buffer fallback below still provides selection/navigation.
    }
    if (found) {
      fallbackSearchTerm = '';
      fallbackSearchIndex = -1;
      fallbackSearchMatches = [];
      if (searchResult.count === 0) searchResult = { index: 0, count: 1 };
      return;
    }

    // WebKit can occasionally return no result from SearchAddon while its
    // active xterm buffer is still being painted. Keep navigation useful by
    // selecting the same buffer cells directly as a narrow fallback.
    const instance = terminal;
    if (!instance) return;
    if (fallbackSearchTerm !== term) {
      const needle = term.toLocaleLowerCase();
      fallbackSearchMatches = [];
      for (let row = 0; row < instance.buffer.active.length; row += 1) {
        const line = instance.buffer.active.getLine(row);
        if (!line) continue;
        const text = line.translateToString(true).toLocaleLowerCase();
        let col = text.indexOf(needle);
        while (col !== -1) {
          fallbackSearchMatches.push({ row, col, size: term.length });
          col = text.indexOf(needle, col + Math.max(1, term.length));
        }
      }
      fallbackSearchTerm = term;
      fallbackSearchIndex = direction === 1 ? 0 : fallbackSearchMatches.length - 1;
    } else if (fallbackSearchMatches.length > 0) {
      fallbackSearchIndex =
        (fallbackSearchIndex + direction + fallbackSearchMatches.length) %
        fallbackSearchMatches.length;
    }

    const match = fallbackSearchMatches[fallbackSearchIndex];
    if (!match) {
      searchResult = { index: -1, count: 0 };
      return;
    }
    instance.select(match.col, match.row, match.size);
    instance.scrollToLine(match.row);
    searchResult = { index: fallbackSearchIndex, count: fallbackSearchMatches.length };
  }
</script>

<section class="terminal-frame" class:is-stopped={process.status !== 'running'}>
  <header class="terminal-toolbar">
    <div class="terminal-identity">
      <span class="signal-light" class:is-live={process.status === 'running'}></span>
      <strong>{process.name}</strong>
      <span>{process.kind}</span>
      {#if process.pid}<span>pid {process.pid}</span>{/if}
    </div>
    <div class="terminal-tools">
      {#if streamGap !== null}<span class="stream-gap" title="The retained daemon buffer began after the requested byte">history clipped · {streamGap}</span>{/if}
      <span class="renderer">{renderer}</span>
      <button type="button" aria-label="Search terminal" title="Search terminal (⌘F)" onclick={openSearch}>⌕</button>
    </div>
  </header>

  {#if searchOpen}
    <form
      class="terminal-search"
      onsubmit={(event) => {
        event.preventDefault();
        findNext(searchInput?.value ?? searchTerm);
      }}
    >
      <input
        bind:this={searchInput}
        bind:value={searchTerm}
        aria-label="Search terminal output"
        autocapitalize="none"
        placeholder="Find in terminal"
        spellcheck="false"
        oninput={(event) => findNext(event.currentTarget.value)}
        onkeydown={(event) => {
          if (event.key === 'Escape') closeSearch();
          else if (event.key === 'Enter') {
            event.preventDefault();
            findNext(event.currentTarget.value);
          }
        }}
        onkeyup={(event) => {
          if (event.key !== 'Escape' && event.key !== 'Enter') findNext(event.currentTarget.value);
        }}
      />
      <span>{searchResult.count ? `${searchResult.index + 1}/${searchResult.count}` : '0/0'}</span>
      <button type="button" aria-label="Previous match" onclick={() => findPrevious()}>↑</button>
      <button type="submit" aria-label="Next match">↓</button>
      <button type="button" aria-label="Close search" onclick={closeSearch}>×</button>
    </form>
  {/if}

  <div class="terminal-host" bind:this={host} aria-label={`${process.name} terminal`}></div>
  {#if process.status !== 'running'}
    <div class="terminal-state">{process.status} · retained output</div>
  {/if}
</section>

<style>
  .terminal-frame {
    position: relative;
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    min-height: 0;
    overflow: hidden;
    border: 1px solid #273638;
    border-radius: 5px;
    background: #0e1517;
    box-shadow: 0 20px 50px rgb(6 12 13 / 28%);
  }

  .terminal-toolbar {
    display: flex;
    min-height: 42px;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 0 12px 0 15px;
    border-bottom: 1px solid #263638;
    background: #162024;
  }

  .terminal-identity,
  .terminal-tools {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 9px;
  }

  .terminal-identity strong {
    overflow: hidden;
    color: #edf4ef;
    font: 620 12px/1.2 'Archivo Variable', sans-serif;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .terminal-identity > span:not(.signal-light),
  .renderer,
  .stream-gap {
    color: #7f918d;
    font: 500 9px/1 'JetBrains Mono Variable', monospace;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .signal-light {
    width: 7px;
    height: 7px;
    flex: 0 0 auto;
    border-radius: 50%;
    background: #66716f;
    box-shadow: 0 0 0 3px rgb(102 113 111 / 10%);
  }

  .signal-light.is-live {
    background: #6fc6a4;
    box-shadow: 0 0 0 3px rgb(111 198 164 / 12%), 0 0 13px rgb(111 198 164 / 30%);
  }

  .stream-gap {
    color: #c79656;
  }

  .terminal-tools button,
  .terminal-search button {
    display: grid;
    width: 26px;
    height: 26px;
    place-items: center;
    border: 1px solid transparent;
    border-radius: 3px;
    color: #98aaa5;
    background: transparent;
    font: 600 14px/1 'JetBrains Mono Variable', monospace;
    cursor: pointer;
  }

  .terminal-tools button:hover,
  .terminal-search button:hover {
    border-color: #3a5150;
    color: #e2ece6;
    background: #213033;
  }

  .terminal-search {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    padding: 6px 10px;
    border-bottom: 1px solid #263638;
    background: #121b1e;
  }

  .terminal-search input {
    width: min(280px, 45vw);
    height: 28px;
    border: 1px solid #38504e;
    border-radius: 3px;
    outline: none;
    padding: 0 9px;
    color: #dce7e1;
    background: #0c1315;
    font: 500 11px/1 'JetBrains Mono Variable', monospace;
  }

  .terminal-search input:focus {
    border-color: #69b99f;
    box-shadow: 0 0 0 2px rgb(105 185 159 / 14%);
  }

  .terminal-search > span {
    min-width: 48px;
    color: #7f918d;
    font: 500 9px/1 'JetBrains Mono Variable', monospace;
    text-align: center;
  }

  .terminal-host {
    min-width: 0;
    min-height: 0;
    padding: 10px 8px 7px 12px;
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
    font: 500 9px/1 'JetBrains Mono Variable', monospace;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    pointer-events: none;
  }

  @media (max-width: 720px) {
    .terminal-toolbar {
      min-height: 38px;
    }

    .terminal-identity > span:not(.signal-light),
    .renderer,
    .stream-gap {
      display: none;
    }
  }
</style>
