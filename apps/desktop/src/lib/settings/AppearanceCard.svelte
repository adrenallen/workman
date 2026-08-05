<script lang="ts">
  import { onMount } from 'svelte';

  import {
    DEFAULT_APPEARANCE,
    UI_FONT_SCALE_STEPS,
    UI_SCALE_STEPS,
    appearance,
    installedUiFonts,
    resetAppearance,
    uiFontCss,
    updateAppearance,
    type ThemePreference,
    type UiFontId
  } from '../appearance';

  const themes: Array<{ id: ThemePreference; label: string }> = [
    { id: 'light', label: 'Light' },
    { id: 'dark', label: 'Dark' },
    { id: 'system', label: 'System' }
  ];

  let fontChoices = $state(installedUiFonts());

  onMount(() => {
    fontChoices = installedUiFonts();
  });

  function fontScaleLabel(scale: number): string {
    if (scale < 0.9) return 'Extra compact';
    if (scale < 0.97) return 'Compact';
    if (scale > 1.1) return 'Large';
    if (scale > 1.03) return 'Comfortable';
    return 'Default';
  }
</script>

<section class="appearance-section" aria-labelledby="appearance-title">
  <header class="section-heading">
    <div>
      <span class="eyebrow">Interface</span>
      <h2 id="appearance-title">Appearance</h2>
      <p>Choose the type and density used across the workspace.</p>
    </div>
    <p class="terminal-note"><span aria-hidden="true">›_</span> Terminal font lives on the Terminal tab.</p>
  </header>

  <div class="setting-row theme-row">
    <div class="setting-copy"><strong>Theme</strong><small>Follow macOS or keep one palette.</small></div>
    <div class="segmented" role="radiogroup" aria-label="Theme">
      {#each themes as theme}
        <button
          type="button"
          role="radio"
          aria-checked={$appearance.theme === theme.id}
          class:active={$appearance.theme === theme.id}
          onclick={() => updateAppearance({ theme: theme.id })}
        >{theme.label}</button>
      {/each}
    </div>
  </div>

  <label class="setting-row">
    <span class="setting-copy">
      <strong>Interface font family</strong>
      <small>Bundled faces and fonts available on this Mac.</small>
    </span>
    <span class="select-wrap">
      <select
        aria-label="Interface font family"
        value={$appearance.uiFont}
        oninput={(event) => updateAppearance({ uiFont: event.currentTarget.value as UiFontId })}
      >
        {#each fontChoices as font}
          <option value={font.id}>{font.label}{font.bundled ? ' · bundled' : ''}</option>
        {/each}
      </select>
      <span aria-hidden="true">⌄</span>
    </span>
  </label>

  <div class="setting-row scale-row">
    <div class="setting-copy">
      <strong>Interface font scale</strong>
      <small>{fontScaleLabel($appearance.uiFontScale)} · {Math.round($appearance.uiFontScale * 100)}%</small>
    </div>
    <div class="font-scales" role="radiogroup" aria-label="Interface font scale">
      {#each UI_FONT_SCALE_STEPS as scale, index}
        <button
          type="button"
          role="radio"
          aria-label={`${fontScaleLabel(scale)}, ${Math.round(scale * 100)} percent`}
          aria-checked={$appearance.uiFontScale === scale}
          class:active={$appearance.uiFontScale === scale}
          style={`--sample-size: ${11 + index * 2}px`}
          onclick={() => updateAppearance({ uiFontScale: scale })}
        >A</button>
      {/each}
    </div>
  </div>

  <div class="setting-row">
    <div class="setting-copy"><strong>Interface scale</strong><small>Scales controls, rails, and content together.</small></div>
    <div class="segmented compact" role="radiogroup" aria-label="Whole app interface scale">
      {#each UI_SCALE_STEPS as scale}
        <button
          type="button"
          role="radio"
          aria-checked={$appearance.uiScale === scale}
          class:active={$appearance.uiScale === scale}
          onclick={() => updateAppearance({ uiScale: scale })}
        >{Math.round(scale * 100)}%</button>
      {/each}
    </div>
  </div>

  <div class="live-preview" aria-label="Live interface preview">
    <div class="preview-label"><span>Live preview</span><small>{fontChoices.find((font) => font.id === $appearance.uiFont)?.label ?? 'System default'}</small></div>
    <div class="preview-workspace" style={`font-family: ${uiFontCss($appearance.uiFont)}`}>
      <span class="preview-dot"></span>
      <div><strong>Workspace ready</strong><small>3 processes · 1 needs input</small></div>
      <button type="button" tabindex="-1">Open terminal</button>
    </div>
  </div>

  <footer>
    <span>Changes apply immediately and are stored on this Mac.</span>
    <button
      type="button"
      disabled={JSON.stringify($appearance) === JSON.stringify(DEFAULT_APPEARANCE)}
      onclick={() => resetAppearance()}
    >Reset to defaults</button>
  </footer>
</section>

<style>
  .appearance-section { overflow: hidden; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); }
  .section-heading { display: flex; min-height: 68px; align-items: flex-start; justify-content: space-between; gap: 18px; padding: 11px 12px 10px; }
  .eyebrow, small, .terminal-note, .preview-label, footer { font-family: 'JetBrains Mono Variable', monospace; }
  .eyebrow { color: var(--muted); font-size: 7px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
  h2 { margin: 2px 0 0; color: var(--text); font-size: 16px; line-height: 1.15; }
  .section-heading p { margin: 4px 0 0; color: var(--muted); font-size: 10px; }
  .terminal-note { display: flex; max-width: 210px; align-items: center; gap: 7px; border-left: 1px solid var(--border); padding: 4px 0 4px 11px; font-size: 8px !important; line-height: 1.45; }
  .terminal-note span { color: var(--signal); font-weight: 700; }

  .setting-row { display: grid; min-height: 52px; grid-template-columns: minmax(190px, 1fr) minmax(220px, .9fr); align-items: center; gap: 16px; border-top: 1px solid var(--border); padding: 7px 12px; }
  .setting-copy strong, .setting-copy small { display: block; }
  .setting-copy strong { color: var(--text-soft); font-size: 10px; font-weight: 660; }
  .setting-copy small { margin-top: 3px; color: var(--muted); font-size: 7px; line-height: 1.35; }

  .segmented { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); overflow: hidden; border: 1px solid var(--border-strong); border-radius: 3px; background: var(--night); }
  .segmented.compact { grid-template-columns: repeat(4, minmax(0, 1fr)); }
  .segmented button { min-height: 28px; border: 0; border-right: 1px solid var(--border-strong); background: transparent; color: var(--muted); font-size: 9px; cursor: pointer; }
  .segmented button:last-child { border-right: 0; }
  .segmented button:hover { color: var(--text); background: color-mix(in srgb, var(--text) 5%, transparent); }
  .segmented button.active { background: var(--surface-raised); color: var(--text); box-shadow: inset 0 -2px var(--signal); }

  .select-wrap { position: relative; display: block; }
  select { width: 100%; height: 30px; appearance: none; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 28px 0 9px; background: var(--night); color: var(--text-soft); font-size: 9px; cursor: pointer; }
  .select-wrap > span { position: absolute; top: 7px; right: 9px; color: var(--muted); font-size: 11px; pointer-events: none; }

  .font-scales { display: flex; min-height: 33px; align-items: end; justify-content: space-between; gap: 5px; padding: 0 10px; }
  .font-scales button { position: relative; width: 36px; height: 31px; border: 0; padding: 0 0 7px; background: transparent; color: var(--muted); font-family: var(--ui-font-family); font-size: var(--sample-size); line-height: 1; cursor: pointer; }
  .font-scales button::after { position: absolute; right: 5px; bottom: 1px; left: 5px; height: 2px; background: transparent; content: ''; }
  .font-scales button:hover { color: var(--text); }
  .font-scales button.active { color: var(--text); }
  .font-scales button.active::after { background: var(--signal); }

  .live-preview { border-top: 1px solid var(--border); padding: 9px 12px 11px; background: color-mix(in srgb, var(--night) 70%, var(--surface)); }
  .preview-label { display: flex; align-items: center; justify-content: space-between; color: var(--muted); font-size: 7px; letter-spacing: .06em; text-transform: uppercase; }
  .preview-label small { letter-spacing: 0; text-transform: none; }
  .preview-workspace { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 9px; margin-top: 7px; border: 1px solid var(--border); border-radius: 3px; padding: 8px 9px; background: var(--surface-raised); }
  .preview-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--warning); }
  .preview-workspace strong, .preview-workspace small { display: block; }
  .preview-workspace strong { color: var(--text); font-size: 11px; }
  .preview-workspace small { margin-top: 2px; color: var(--muted); font-size: 7px; }
  .preview-workspace button { min-height: 25px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 8px; background: var(--surface); color: var(--text-soft); font-size: 8px; pointer-events: none; }

  footer { display: flex; min-height: 39px; align-items: center; justify-content: space-between; gap: 12px; border-top: 1px solid var(--border); padding: 6px 10px 6px 12px; color: var(--muted); font-size: 7px; }
  footer button { min-height: 26px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 9px; background: var(--surface-raised); color: var(--text-soft); font-size: 8px; cursor: pointer; }
  footer button:disabled { opacity: .38; cursor: default; }

  @media (max-width: 760px) {
    .section-heading { display: block; }
    .terminal-note { max-width: none; margin-top: 8px !important; border-left: 0; padding-left: 0; }
    .setting-row { grid-template-columns: 1fr; gap: 7px; padding-block: 9px; }
  }
</style>
