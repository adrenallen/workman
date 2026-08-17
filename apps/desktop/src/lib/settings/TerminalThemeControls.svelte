<script lang="ts">
  import DownloadIcon from '@lucide/svelte/icons/download';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';

  import {
    TERMINAL_COLOR_KEYS,
    TERMINAL_THEME_PRESETS,
    appearance,
    terminalContrastRatio,
    terminalThemeFromPreset,
    updateAppearance,
    type TerminalPalette,
    type TerminalThemePreset,
    type TerminalThemeSetting
  } from '../appearance';
  import type { DaemonClient } from '../daemon';
  import { applyTerminalThemeImport, importTerminalTheme } from '../settings';

  interface Props {
    client: DaemonClient;
    connected: boolean;
  }

  let { client, connected }: Props = $props();
  let importing = $state(false);
  let importMessage = $state<string | null>(null);
  let customOpen = $state(false);

  let contrast = $derived(terminalContrastRatio($appearance.terminalTheme.palette));
  let lowContrast = $derived(contrast < 4.5);

  const primaryColors: readonly { key: keyof TerminalPalette; label: string }[] = [
    { key: 'background', label: 'Background' },
    { key: 'foreground', label: 'Text' },
    { key: 'cursor', label: 'Cursor' },
    { key: 'selection', label: 'Selection' }
  ];

  function choosePreset(id: TerminalThemePreset['id']): void {
    updateAppearance({ terminalTheme: terminalThemeFromPreset(id) });
    importMessage = null;
  }

  function updateColor(key: keyof TerminalPalette, value: string): void {
    const current = $appearance.terminalTheme;
    const theme: TerminalThemeSetting = {
      id: 'custom',
      name: 'Custom',
      source: null,
      palette: { ...current.palette, [key]: value.toUpperCase() }
    };
    updateAppearance({ terminalTheme: theme });
  }

  async function useTerminalTheme(): Promise<void> {
    if (!connected || importing) return;
    importing = true;
    importMessage = null;
    try {
      const report = await importTerminalTheme(client);
      importMessage = report.message;
      applyTerminalThemeImport(report);
    } catch (cause) {
      importMessage = cause instanceof Error ? cause.message : String(cause);
    } finally {
      importing = false;
    }
  }

  function readableLabel(key: string): string {
    return key.replace(/^bright/, 'bright ').replace(/([A-Z])/g, ' $1').toLowerCase();
  }
</script>

<div class="theme-block">
  <div class="theme-heading">
    <div>
      <strong>Color theme</strong>
      <small>Independent of Workman’s light or dark interface.</small>
    </div>
    <span class:warning={lowContrast}>{contrast.toFixed(1)}:1</span>
  </div>

  <div class="presets" aria-label="Terminal color presets">
    {#each TERMINAL_THEME_PRESETS as preset}
      <button
        type="button"
        class:active={$appearance.terminalTheme.id === preset.id}
        aria-pressed={$appearance.terminalTheme.id === preset.id}
        onclick={() => choosePreset(preset.id)}
      >
        <span class="preset-name">{preset.name}</span>
        <span class="swatches" aria-hidden="true">
          {#each [preset.palette.background, preset.palette.foreground, preset.palette.red, preset.palette.yellow, preset.palette.green, preset.palette.blue, preset.palette.magenta, preset.palette.cyan] as color}
            <i style={`background: ${color}`}></i>
          {/each}
        </span>
        <small>{preset.description}</small>
      </button>
    {/each}
  </div>

  <div class="theme-actions">
    <button class="import-button" type="button" disabled={!connected || importing} onclick={() => void useTerminalTheme()}>
      <DownloadIcon size={14} strokeWidth={1.8} />
      {importing
        ? 'Reading profiles…'
        : $appearance.terminalTheme.id === 'imported'
          ? 'Re-import terminal theme'
          : 'Use my terminal’s theme'}
    </button>
    <button class="custom-toggle" type="button" aria-expanded={customOpen} onclick={() => customOpen = !customOpen}>
      Full custom
      <span class:open={customOpen}><ChevronDownIcon size={14} strokeWidth={1.8} /></span>
    </button>
  </div>

  {#if $appearance.terminalTheme.id === 'imported'}
    <p class="imported-source">
      <span>Imported</span>
      <strong>{$appearance.terminalTheme.name}</strong>
      {#if $appearance.terminalTheme.source}<small>{$appearance.terminalTheme.source}</small>{/if}
    </p>
  {/if}
  {#if importMessage}
    <p class="import-message" role="status">{importMessage}</p>
  {/if}
  {#if lowContrast}
    <p class="contrast-warning" role="status">
      Text contrast is below 4.5:1. This palette may be difficult to read.
    </p>
  {/if}

  {#if customOpen}
    <div class="custom-editor">
      <div class="primary-grid">
        {#each primaryColors as color}
          <label>
            <span>{color.label}</span>
            <span class="color-field">
              <input
                type="color"
                value={$appearance.terminalTheme.palette[color.key]}
                aria-label={`${color.label} color`}
                oninput={(event) => updateColor(color.key, event.currentTarget.value)}
              />
              <code>{$appearance.terminalTheme.palette[color.key]}</code>
            </span>
          </label>
        {/each}
      </div>
      <div class="ansi-grid" aria-label="ANSI palette">
        {#each TERMINAL_COLOR_KEYS as key}
          <label title={readableLabel(key)}>
            <input
              type="color"
              value={$appearance.terminalTheme.palette[key]}
              aria-label={`${readableLabel(key)} ANSI color`}
              oninput={(event) => updateColor(key, event.currentTarget.value)}
            />
            <span>{readableLabel(key)}</span>
          </label>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .theme-block { border-top: 1px solid var(--border); padding: 10px 12px 12px; }
  .theme-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .theme-heading strong, .theme-heading small { display: block; }
  .theme-heading strong { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 660; }
  .theme-heading small { margin-top: 3px; color: var(--muted); font: var(--font-size-xs)/1.35 'JetBrains Mono Variable', monospace; }
  .theme-heading > span { border: 1px solid var(--border); border-radius: 3px; padding: 3px 6px; color: var(--muted); font: 600 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace; }
  .theme-heading > span.warning { border-color: color-mix(in srgb, var(--warning-token) 55%, var(--border)); color: var(--warning-token); }

  .presets { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 7px; margin-top: 9px; }
  .presets button { min-width: 0; border: 1px solid var(--border); border-radius: 4px; padding: 8px; background: var(--night); color: var(--text-soft); text-align: left; cursor: pointer; }
  .presets button:hover { border-color: var(--border-strong); }
  .presets button.active { border-color: color-mix(in srgb, var(--signal) 65%, var(--border)); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--signal) 28%, transparent); }
  .preset-name { display: block; font-size: var(--font-size-sm); font-weight: 680; }
  .presets small { display: block; min-height: 28px; margin-top: 6px; color: var(--muted); font: var(--font-size-xs)/1.35 'JetBrains Mono Variable', monospace; }
  .swatches { display: flex; height: 10px; overflow: hidden; margin-top: 6px; border: 1px solid var(--border); border-radius: 2px; }
  .swatches i { flex: 1; min-width: 0; }

  .theme-actions { display: flex; gap: 7px; margin-top: 9px; }
  .theme-actions button { display: inline-flex; min-height: 30px; align-items: center; justify-content: center; gap: 6px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 9px; background: var(--surface-raised); color: var(--text-soft); font-size: var(--font-size-xs); cursor: pointer; }
  .theme-actions button:disabled { cursor: default; opacity: .5; }
  .import-button { flex: 1; }
  .custom-toggle > span { display: inline-flex; transition: transform 150ms ease; }
  .custom-toggle > span.open { transform: rotate(180deg); }

  .imported-source { display: flex; align-items: center; gap: 6px; margin: 8px 0 0; color: var(--text-soft); font: var(--font-size-xs)/1.3 'JetBrains Mono Variable', monospace; }
  .imported-source span { border: 1px solid color-mix(in srgb, var(--signal) 50%, var(--border)); border-radius: 3px; padding: 1px 5px; color: var(--signal); text-transform: uppercase; }
  .imported-source small { margin-left: auto; color: var(--muted); }
  .import-message, .contrast-warning { margin: 7px 0 0; border-left: 2px solid var(--border-strong); padding-left: 7px; color: var(--muted); font-size: var(--font-size-xs); line-height: 1.4; }
  .contrast-warning { border-color: var(--warning-token); color: var(--text-soft); }

  .custom-editor { margin-top: 10px; border: 1px solid var(--border); border-radius: 4px; padding: 9px; background: color-mix(in srgb, var(--night) 72%, transparent); }
  .primary-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 7px; }
  .primary-grid label { display: flex; align-items: center; justify-content: space-between; gap: 8px; color: var(--muted); font-size: var(--font-size-xs); }
  .color-field { display: flex; height: 27px; align-items: center; gap: 6px; border: 1px solid var(--border); border-radius: 3px; padding: 2px 6px 2px 3px; background: var(--surface); }
  input[type='color'] { width: 22px; height: 21px; appearance: none; overflow: hidden; border: 0; border-radius: 2px; padding: 0; background: transparent; cursor: pointer; }
  input[type='color']::-webkit-color-swatch-wrapper { padding: 0; }
  input[type='color']::-webkit-color-swatch { border: 0; border-radius: 2px; }
  .color-field code { color: var(--text-soft); font: var(--font-size-xs)/1 'JetBrains Mono Variable', monospace; }
  .ansi-grid { display: grid; grid-template-columns: repeat(8, minmax(0, 1fr)); gap: 5px; margin-top: 9px; }
  .ansi-grid label { min-width: 0; color: var(--muted); font: var(--font-size-xs)/1.15 'JetBrains Mono Variable', monospace; text-align: center; }
  .ansi-grid input { width: 100%; height: 23px; }
  .ansi-grid span { display: block; overflow: hidden; margin-top: 3px; text-overflow: ellipsis; white-space: nowrap; }

  @media (max-width: 660px) { .presets { grid-template-columns: 1fr; } .presets small { min-height: 0; } .ansi-grid { grid-template-columns: repeat(4, 1fr); } }
</style>
