<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import {
    hotkeyActionLabel,
    hotkeyDefinitions,
    hotkeyDisplayLabel,
    hotkeyFromKeyboardEvent,
    hotkeyPreferences,
    reservedHotkeyLabel,
    resetHotkeyBindings,
    setHotkeyBinding,
    type HotkeyAction,
    type HotkeyDefinition
  } from '../hotkeys';
  import {
    altModifierLabel as alt,
    primaryModifierLabel as mod,
    shiftModifierLabel as shift,
    terminalUnfocusKeys
  } from '../primaryModifier';

  let recording = $state<HotkeyAction | null>(null);
  let message = $state('');

  const projectDefinitions = hotkeyDefinitions.filter(
    (definition) => definition.group === 'projects'
  );
  const creationDefinitions = hotkeyDefinitions.filter(
    (definition) => definition.group === 'creation'
  );
  const chordJoin = mod === '⌘' ? '' : '+';

  function captureHotkey(event: KeyboardEvent, definition: HotkeyDefinition): void {
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      recording = null;
      message = 'Shortcut recording cancelled.';
      return;
    }
    if (
      (event.key === 'Backspace' || event.key === 'Delete')
      && !event.metaKey
      && !event.ctrlKey
      && !event.altKey
      && !event.shiftKey
    ) {
      setHotkeyBinding(definition.id, null);
      recording = null;
      message = `${definition.label} cleared.`;
      return;
    }
    const chord = hotkeyFromKeyboardEvent(event);
    if (!chord) {
      message = `Include ${mod}, ${mod === '⌘' ? '⌃' : 'Meta'}, or ${mod === '⌘' ? '⌥' : 'Alt'} in an app shortcut.`;
      return;
    }
    const reserved = reservedHotkeyLabel(chord);
    if (reserved) {
      message = `${hotkeyDisplayLabel(chord)} is reserved for ${reserved}.`;
      return;
    }
    const displaced = setHotkeyBinding(definition.id, chord);
    recording = null;
    message = displaced
      ? `${definition.label} now uses ${hotkeyDisplayLabel(chord)}; ${hotkeyActionLabel(displaced)} was cleared.`
      : `${definition.label} now uses ${hotkeyDisplayLabel(chord)}.`;
  }

  function clearHotkey(definition: HotkeyDefinition): void {
    setHotkeyBinding(definition.id, null);
    if (recording === definition.id) recording = null;
    message = `${definition.label} cleared.`;
  }

  function reset(): void {
    resetHotkeyBindings();
    recording = null;
    message = 'Default shortcuts restored.';
  }
</script>

{#snippet hotkeyRow(definition: HotkeyDefinition)}
  {@const chord = $hotkeyPreferences[definition.id]}
  <div class="hotkey-row">
    <div class="hotkey-copy">
      <strong>{definition.label}</strong>
      <small>{definition.description}</small>
    </div>
    <button
      type="button"
      class:recording={recording === definition.id}
      class="recorder"
      aria-label={`Set shortcut for ${definition.label}`}
      aria-pressed={recording === definition.id}
      onclick={() => {
        recording = recording === definition.id ? null : definition.id;
        message = recording === definition.id
          ? `Press a shortcut for ${definition.label}. Escape cancels; Delete clears.`
          : 'Shortcut recording cancelled.';
      }}
      onkeydown={(event) => {
        if (recording === definition.id) captureHotkey(event, definition);
      }}
    >
      {#if recording === definition.id}
        <span>Press shortcut…</span>
      {:else if chord}
        <kbd>{hotkeyDisplayLabel(chord)}</kbd>
      {:else}
        <span>Not set</span>
      {/if}
    </button>
    <Button
      variant="ghost"
      size="sm"
      disabled={chord === null}
      aria-label={`Clear shortcut for ${definition.label}`}
      onclick={() => clearHotkey(definition)}
    >Clear</Button>
  </div>
{/snippet}

<section class="hotkeys-card" aria-labelledby="hotkeys-title">
  <header>
    <div>
      <span class="eyebrow">Keyboard</span>
      <h2 id="hotkeys-title">Hotkeys</h2>
      <p>Jump between projects and create work without leaving the keyboard.</p>
    </div>
    <Button variant="outline" size="sm" onclick={reset}>Restore defaults</Button>
  </header>

  <div class="guidance" aria-live="polite">
    <span>{message || `Click a shortcut, then press a key combination. Changes are saved locally.`}</span>
    <small>Escape cancels · Delete clears</small>
  </div>

  <section class="group" aria-labelledby="project-hotkeys-title">
    <div class="group-heading">
      <div>
        <h3 id="project-hotkeys-title">Project rail</h3>
        <p>Slots follow the visual order in the left rail, including projects inside folders.</p>
      </div>
      <span class="scope">1–9</span>
    </div>
    <div class="hotkey-grid project-grid">
      {#each projectDefinitions as definition (definition.id)}
        {@render hotkeyRow(definition)}
      {/each}
    </div>
  </section>

  <section class="group" aria-labelledby="creation-hotkeys-title">
    <div class="group-heading">
      <div>
        <h3 id="creation-hotkeys-title">Create in current project</h3>
        <p>New agent defaults to <kbd>{chordJoin === '' ? `${mod}N` : `${mod}+N`}</kbd>; the other actions are ready to bind.</p>
      </div>
      <span class="scope">Current</span>
    </div>
    <div class="hotkey-grid creation-grid">
      {#each creationDefinitions as definition (definition.id)}
        {@render hotkeyRow(definition)}
      {/each}
    </div>
  </section>

  <section class="group built-ins" aria-labelledby="built-in-hotkeys-title">
    <div class="group-heading">
      <div>
        <h3 id="built-in-hotkeys-title">Built-in workspace shortcuts</h3>
        <p>These remain fixed and cannot be assigned above.</p>
      </div>
    </div>
    <div class="built-in-grid">
      <div><kbd>{chordJoin === '' ? `${mod}K` : `${mod}+K`}</kbd><span>Quick jump</span></div>
      <div><kbd>{chordJoin === '' ? `${mod}/` : `${mod}+/`}</kbd><span>Keyboard reference</span></div>
      <div><kbd>{chordJoin === '' ? `${mod}B` : `${mod}+B`}</kbd><span>Toggle project rail</span></div>
      <div><kbd>{terminalUnfocusKeys.join(chordJoin)}</kbd><span>Unfocus terminal</span></div>
      <div><kbd>{chordJoin === '' ? `${mod}${shift}P` : `${mod}+${shift}+P`}</kbd><span>Quick prompts</span></div>
      <div><kbd>{chordJoin === '' ? `${mod}← / →` : `${mod}+← / →`}</kbd><span>Move between panels</span></div>
      <div><kbd>{chordJoin === '' ? `${mod}F` : `${mod}+F`}</kbd><span>Search terminal</span></div>
      <div><kbd>{chordJoin === '' ? `${alt}↑ / ↓` : `${alt}+↑ / ↓`}</kbd><span>Reorder focused item</span></div>
      <div><kbd>{chordJoin === '' ? `${mod},` : `${mod}+,`}</kbd><span>Settings</span></div>
    </div>
  </section>

  <footer>
    Project and creation shortcuts work from text fields and terminal input. A project shortcut keeps following its rail position when projects are reordered.
  </footer>
</section>

<style>
  .hotkeys-card { overflow: hidden; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); }
  header { display: flex; min-height: 72px; align-items: flex-start; justify-content: space-between; gap: 16px; padding: 12px; }
  .eyebrow, .scope, small, kbd, footer, .guidance { font-family: 'JetBrains Mono Variable', monospace; }
  .eyebrow { color: var(--muted); font-size: var(--font-size-xs); font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
  h2 { margin: 2px 0 0; color: var(--text); font-size: 16px; line-height: 1.15; }
  header p, .group-heading p { margin: 4px 0 0; color: var(--muted); font-size: var(--font-size-sm); line-height: 1.4; }
  .guidance { display: flex; min-height: 38px; align-items: center; justify-content: space-between; gap: 12px; border-top: 1px solid var(--border); padding: 8px 12px; background: color-mix(in srgb, var(--signal) 5%, var(--night)); color: var(--text-soft); font-size: var(--font-size-xs); }
  .guidance small { flex: none; color: var(--muted); }
  .group { border-top: 1px solid var(--border); }
  .group-heading { display: flex; min-height: 54px; align-items: flex-start; justify-content: space-between; gap: 16px; padding: 10px 12px; }
  h3 { margin: 0; color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 700; letter-spacing: .025em; }
  .scope { border: 1px solid var(--border); border-radius: 3px; padding: 4px 7px; background: var(--night); color: var(--muted); font-size: var(--font-size-xs); }
  .hotkey-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); border-top: 1px solid var(--border); }
  .hotkey-row { display: grid; min-height: 61px; grid-template-columns: minmax(0, 1fr) 112px 48px; align-items: center; gap: 7px; padding: 7px 8px 7px 12px; }
  .hotkey-row:nth-child(odd) { border-right: 1px solid var(--border); }
  .hotkey-row:nth-child(n + 3) { border-top: 1px solid var(--border); }
  .hotkey-copy { min-width: 0; }
  .hotkey-copy strong, .hotkey-copy small { display: block; }
  .hotkey-copy strong { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 650; }
  .hotkey-copy small { overflow: hidden; margin-top: 3px; color: var(--muted); font-size: 10px; line-height: 1.35; text-overflow: ellipsis; white-space: nowrap; }
  .recorder { display: grid; min-height: 31px; place-items: center; border: 1px solid var(--border-strong); border-radius: 4px; padding: 3px 7px; background: var(--night); color: var(--muted); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; cursor: pointer; }
  .recorder:hover, .recorder:focus-visible { border-color: var(--ring); color: var(--text-soft); outline: 0; }
  .recorder.recording { border-color: var(--signal); background: color-mix(in srgb, var(--signal) 10%, var(--night)); color: var(--foreground); box-shadow: 0 0 0 2px color-mix(in srgb, var(--signal) 15%, transparent); }
  kbd { display: inline-grid; min-width: 25px; min-height: 22px; place-items: center; border: 1px solid var(--border-strong); border-bottom-color: color-mix(in srgb, var(--text) 30%, var(--border)); border-radius: 3px; padding: 1px 5px; background: var(--surface-raised); color: var(--text-soft); font-size: var(--font-size-xs); white-space: nowrap; }
  .built-in-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); border-top: 1px solid var(--border); }
  .built-in-grid > div { display: grid; min-height: 44px; grid-template-columns: 88px minmax(0, 1fr); align-items: center; gap: 8px; border-bottom: 1px solid var(--border); padding: 7px 12px; color: var(--text-soft); font-size: var(--font-size-xs); }
  .built-in-grid > div:not(:nth-child(3n)) { border-right: 1px solid var(--border); }
  .built-in-grid > div:nth-last-child(-n + 3) { border-bottom: 0; }
  footer { border-top: 1px solid var(--border); padding: 11px 12px; color: var(--muted); font-size: var(--font-size-xs); line-height: 1.5; }

  @media (max-width: 900px) {
    .hotkey-grid, .built-in-grid { grid-template-columns: 1fr; }
    .hotkey-row:nth-child(odd), .built-in-grid > div:not(:nth-child(3n)) { border-right: 0; }
    .hotkey-row:nth-child(n + 2), .built-in-grid > div:nth-child(n + 2) { border-top: 1px solid var(--border); }
    .built-in-grid > div { border-bottom: 0; }
  }
  @media (max-width: 620px) {
    .hotkey-row { grid-template-columns: minmax(0, 1fr) 104px; }
    .hotkey-row :global(button:last-child) { display: none; }
    .guidance small { display: none; }
  }
</style>
