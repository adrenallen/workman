<script lang="ts">
  import { onMount } from 'svelte';

  import {
    DEFAULT_APPEARANCE,
    appearance,
    installedTerminalFonts,
    terminalFontCss,
    updateAppearance,
    type TerminalFontId
  } from '../appearance';

  let fontChoices = $state(installedTerminalFonts());

  onMount(() => {
    fontChoices = installedTerminalFonts();
  });

  function setSize(value: number): void {
    updateAppearance({ terminalFontSize: Math.min(20, Math.max(10, value)) });
  }
</script>

<section class="terminal-section" aria-labelledby="terminal-appearance-title">
  <header>
    <div>
      <span class="eyebrow">Terminal</span>
      <h2 id="terminal-appearance-title">Terminal typography</h2>
      <p>Terminal changes refit the visible PTY immediately.</p>
    </div>
    <span class="geometry">{$appearance.terminalFontSize}px</span>
  </header>

  <label class="setting-row">
    <span class="setting-copy"><strong>Font family</strong><small>Bundled or installed monospace faces.</small></span>
    <span class="select-wrap">
      <select
        aria-label="Terminal font family"
        value={$appearance.terminalFont}
        oninput={(event) => updateAppearance({ terminalFont: event.currentTarget.value as TerminalFontId })}
      >
        {#each fontChoices as font}
          <option value={font.id}>{font.label}{font.bundled ? ' · bundled' : ''}</option>
        {/each}
      </select>
      <span aria-hidden="true">⌄</span>
    </span>
  </label>

  <div class="setting-row">
    <div class="setting-copy"><strong>Font size</strong><small>10–20 px · refits rows and columns.</small></div>
    <div class="size-control">
      <button type="button" aria-label="Decrease terminal font size" disabled={$appearance.terminalFontSize <= 10} onclick={() => setSize($appearance.terminalFontSize - 1)}>−</button>
      <input
        type="range"
        min="10"
        max="20"
        step="1"
        value={$appearance.terminalFontSize}
        aria-label="Terminal font size"
        oninput={(event) => setSize(event.currentTarget.valueAsNumber)}
      />
      <output>{$appearance.terminalFontSize}px</output>
      <button type="button" aria-label="Increase terminal font size" disabled={$appearance.terminalFontSize >= 20} onclick={() => setSize($appearance.terminalFontSize + 1)}>+</button>
    </div>
  </div>

  <div class="terminal-preview" style={`font-family: ${terminalFontCss($appearance.terminalFont)}; font-size: ${$appearance.terminalFontSize}px`}>
    <div class="terminal-bar"><i></i><span>preview</span><small>80 × 24</small></div>
    <p><span>wrk</span> › cargo test</p>
    <p class="output">test result: <strong>ok</strong>. 42 passed; 0 failed</p>
    <p><span>wrk</span> › <i class="cursor"></i></p>
  </div>

  <footer>
    <span>xterm canvas and PTY geometry update together.</span>
    <button
      type="button"
      disabled={$appearance.terminalFont === DEFAULT_APPEARANCE.terminalFont && $appearance.terminalFontSize === DEFAULT_APPEARANCE.terminalFontSize}
      onclick={() => updateAppearance({
        terminalFont: DEFAULT_APPEARANCE.terminalFont,
        terminalFontSize: DEFAULT_APPEARANCE.terminalFontSize
      })}
    >Reset terminal</button>
  </footer>
</section>

<style>
  .terminal-section { overflow: hidden; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); }
  header { display: flex; min-height: 68px; align-items: flex-start; justify-content: space-between; gap: 16px; padding: 11px 12px 10px; }
  .eyebrow, small, .geometry, output, footer { font-family: 'JetBrains Mono Variable', monospace; }
  .eyebrow { color: var(--muted); font-size: var(--font-size-xs); font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
  h2 { margin: 2px 0 0; color: var(--text); font-size: 16px; line-height: 1.15; }
  header p { margin: 4px 0 0; color: var(--muted); font-size: var(--font-size-sm); }
  .geometry { border: 1px solid var(--border); border-radius: 3px; padding: 5px 7px; background: var(--night); color: var(--text-soft); font-size: var(--font-size-xs); }

  .setting-row { display: grid; min-height: 52px; grid-template-columns: minmax(190px, 1fr) minmax(220px, .9fr); align-items: center; gap: 16px; border-top: 1px solid var(--border); padding: 7px 12px; }
  .setting-copy strong, .setting-copy small { display: block; }
  .setting-copy strong { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 660; }
  .setting-copy small { margin-top: 3px; color: var(--muted); font-size: var(--font-size-xs); }
  .select-wrap { position: relative; display: block; }
  select { width: 100%; height: 30px; appearance: none; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 28px 0 9px; background: var(--night); color: var(--text-soft); font-size: var(--font-size-sm); cursor: pointer; }
  .select-wrap > span { position: absolute; top: 7px; right: 9px; color: var(--muted); font-size: var(--font-size-sm); pointer-events: none; }

  .size-control { display: grid; grid-template-columns: 28px minmax(110px, 1fr) 42px 28px; align-items: center; gap: 6px; }
  .size-control button { width: 28px; height: 28px; border: 1px solid var(--border-strong); border-radius: 3px; background: var(--surface-raised); color: var(--text-soft); font-size: 13px; cursor: pointer; }
  .size-control button:disabled { opacity: .35; cursor: default; }
  input[type='range'] { width: 100%; accent-color: var(--signal); }
  output { color: var(--text-soft); font-size: var(--font-size-xs); text-align: center; }

  .terminal-preview { min-height: 116px; border-top: 1px solid var(--border); padding: 9px 12px 10px; background: var(--background); color: #d7e2dc; line-height: 1.42; }
  .terminal-bar { display: flex; align-items: center; gap: 7px; margin: -1px 0 8px; color: #71827e; font: var(--font-size-xs)/1 'JetBrains Mono Variable', monospace; text-transform: uppercase; }
  .terminal-bar i { width: 6px; height: 6px; border-radius: 50%; background: var(--signal); }
  .terminal-bar small { margin-left: auto; font-size: var(--font-size-xs); }
  .terminal-preview p { margin: 2px 0; white-space: nowrap; }
  .terminal-preview p > span { color: #7bd1b5; }
  .terminal-preview .output { color: #83918f; }
  .terminal-preview .output strong { color: #99dab8; font-weight: 500; }
  .cursor { display: inline-block; width: .55em; height: 1.05em; vertical-align: -.17em; background: #7bd1b5; }

  footer { display: flex; min-height: 39px; align-items: center; justify-content: space-between; gap: 12px; border-top: 1px solid var(--border); padding: 6px 10px 6px 12px; color: var(--muted); font-size: var(--font-size-xs); }
  footer button { min-height: 26px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 9px; background: var(--surface-raised); color: var(--text-soft); font-size: var(--font-size-xs); cursor: pointer; }
  footer button:disabled { opacity: .38; cursor: default; }

  @media (max-width: 760px) { .setting-row { grid-template-columns: 1fr; gap: 7px; padding-block: 9px; } }
</style>
