<script lang="ts">
  import { onMount } from 'svelte';

  import {
    DEFAULT_APPEARANCE,
    appearance,
    installedTerminalFonts,
    terminalFontCss,
    terminalThemeFromPreset,
    updateAppearance,
    type TerminalFontId
  } from '../appearance';
  import type { DaemonClient } from '../daemon';
  import type { UserEnvironmentInfo } from '../settings';
  import TerminalThemeControls from './TerminalThemeControls.svelte';

  interface Props {
    client: DaemonClient;
    environment: UserEnvironmentInfo | null;
    connected: boolean;
    onShellChange: (shell: string | null) => Promise<void>;
  }

  let { client, environment, connected, onShellChange }: Props = $props();

  let fontChoices = $state(installedTerminalFonts());
  let shellMode = $state<'auto' | 'custom'>('auto');
  let customShell = $state('');
  let savingShell = $state(false);
  let shellError = $state<string | null>(null);

  $effect(() => {
    if (!environment) return;
    shellMode = environment.configured_shell ? 'custom' : 'auto';
    customShell = environment.configured_shell ?? environment.inferred_shell;
  });

  onMount(() => {
    fontChoices = installedTerminalFonts();
  });

  function setSize(value: number): void {
    updateAppearance({ terminalFontSize: Math.min(20, Math.max(10, value)) });
  }

  async function saveShell(): Promise<void> {
    if (!connected || savingShell) return;
    const shell = shellMode === 'custom' ? customShell.trim() : null;
    if (shellMode === 'custom' && !shell?.startsWith('/')) {
      shellError = 'Enter an absolute shell path, such as /bin/zsh.';
      return;
    }
    savingShell = true;
    shellError = null;
    try {
      await onShellChange(shell);
    } catch (cause) {
      shellError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      savingShell = false;
    }
  }
</script>

<section class="terminal-section" aria-labelledby="terminal-appearance-title">
  <header>
    <div>
      <span class="eyebrow">Terminal</span>
      <h2 id="terminal-appearance-title">Terminal environment</h2>
      <p>Shell, typography, and color shared by terminals and agents.</p>
    </div>
    <span class="geometry">{$appearance.terminalFontSize}px</span>
  </header>

  <div class="setting-row shell-row">
    <div class="setting-copy">
      <strong>Shell</strong>
      <small>Login profiles provide the PATH used by launches and runtime checks.</small>
    </div>
    <div class="shell-control">
      <div class="shell-fields">
        <span class="select-wrap">
          <select
            aria-label="Terminal shell mode"
            value={shellMode}
            disabled={!connected || savingShell}
            oninput={(event) => {
              shellMode = event.currentTarget.value as 'auto' | 'custom';
              shellError = null;
            }}
          >
            <option value="auto">Auto-detect (default)</option>
            <option value="custom">Custom path</option>
          </select>
          <span aria-hidden="true">⌄</span>
        </span>
        {#if shellMode === 'custom'}
          <input
            class="shell-path"
            aria-label="Custom shell path"
            placeholder="/bin/zsh"
            bind:value={customShell}
            disabled={!connected || savingShell}
            autocapitalize="off"
            autocorrect="off"
            spellcheck={false}
            onkeydown={(event) => {
              if (event.key === 'Enter') void saveShell();
            }}
          />
        {/if}
        <button
          class="apply-shell"
          type="button"
          disabled={!connected || !environment || savingShell}
          onclick={() => void saveShell()}
        >{savingShell ? 'Saving…' : 'Apply'}</button>
      </div>
      {#if environment}
        <p class="shell-summary">
          <span>{environment.using_override ? 'Custom' : 'Auto-detected'}</span>
          <code>{environment.active_shell}</code>
          <small>Inferred from {environment.inferred_from}: {environment.inferred_shell}</small>
        </p>
      {:else}
        <p class="shell-summary unavailable">Connect to the daemon to inspect the login shell.</p>
      {/if}
      {#if shellError || environment?.warning}
        <p class="shell-warning" role="status">{shellError ?? environment?.warning}</p>
      {/if}
    </div>
  </div>

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

  <TerminalThemeControls {client} {connected} />

  <div
    class="terminal-preview"
    style={`font-family: ${terminalFontCss($appearance.terminalFont)}; font-size: ${$appearance.terminalFontSize}px; background: ${$appearance.terminalTheme.palette.background}; color: ${$appearance.terminalTheme.palette.foreground}`}
  >
    <div class="terminal-bar"><i></i><span>preview</span><small>80 × 24</small></div>
    <p><span style={`color: ${$appearance.terminalTheme.palette.cyan}`}>wrk</span> › cargo test</p>
    <p class="output" style={`color: ${$appearance.terminalTheme.palette.brightBlack}`}>test result: <strong style={`color: ${$appearance.terminalTheme.palette.green}`}>ok</strong>. 42 passed; 0 failed</p>
    <p><span style={`color: ${$appearance.terminalTheme.palette.cyan}`}>wrk</span> › <i class="cursor" style={`background: ${$appearance.terminalTheme.palette.cursor}`}></i></p>
  </div>

  <footer>
    <span>xterm canvas and PTY geometry update together.</span>
    <button
      type="button"
      disabled={$appearance.terminalFont === DEFAULT_APPEARANCE.terminalFont && $appearance.terminalFontSize === DEFAULT_APPEARANCE.terminalFontSize && $appearance.terminalTheme.id === 'graphite'}
      onclick={() => updateAppearance({
        terminalFont: DEFAULT_APPEARANCE.terminalFont,
        terminalFontSize: DEFAULT_APPEARANCE.terminalFontSize,
        terminalTheme: terminalThemeFromPreset('graphite')
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
  select:disabled, input:disabled, button:disabled { cursor: default; opacity: .5; }

  .shell-row { grid-template-columns: 1fr; align-items: start; gap: 8px; padding-block: 10px; }
  .shell-control { min-width: 0; }
  .shell-fields { display: flex; align-items: center; gap: 6px; }
  .shell-fields .select-wrap { flex: 1 1 180px; }
  .shell-path { width: min(220px, 100%); height: 30px; min-width: 0; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 8px; background: var(--night); color: var(--text-soft); font: var(--font-size-sm)/1 'JetBrains Mono Variable', monospace; }
  .apply-shell { min-width: 58px; height: 30px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 9px; background: var(--surface-raised); color: var(--text-soft); font-size: var(--font-size-xs); cursor: pointer; }
  .shell-summary { display: grid; min-width: 0; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: 4px 6px; margin: 6px 0 0; color: var(--muted); font: var(--font-size-xs)/1.35 'JetBrains Mono Variable', monospace; }
  .shell-summary > span { flex: 0 0 auto; border: 1px solid var(--border); border-radius: 3px; padding: 1px 5px; color: var(--text-soft); }
  .shell-summary code { min-width: 0; overflow: hidden; color: var(--text-soft); text-overflow: ellipsis; white-space: nowrap; }
  .shell-summary small { grid-column: 1 / -1; overflow: hidden; font-size: inherit; text-overflow: ellipsis; white-space: nowrap; }
  .shell-summary.unavailable { display: block; }
  .shell-warning { margin: 5px 0 0; border-left: 2px solid var(--warning-token); padding-left: 7px; color: var(--text-soft); font-size: var(--font-size-xs); line-height: 1.35; }

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
  @media (max-width: 560px) { .shell-fields { align-items: stretch; flex-direction: column; } .shell-fields .select-wrap, .shell-path, .apply-shell { width: 100%; flex: none; } }
</style>
