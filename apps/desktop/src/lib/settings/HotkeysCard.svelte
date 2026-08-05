<script lang="ts">
  const groups = [
    {
      title: 'Move through the workspace',
      shortcuts: [
        { keys: ['⌘', '← / →'], label: 'Move between the project rail, tree, and main frame' },
        { keys: ['⌘', '↑ / ↓'], label: 'Select the previous or next process' },
        { keys: ['↑ / ↓'], label: 'Move through a focused rail or tree list' },
        { keys: ['← / →'], label: 'Collapse or expand a focused tree group' },
        { keys: ['↵'], label: 'Open or activate the focused row' }
      ]
    },
    {
      title: 'Jump and act',
      shortcuts: [
        { keys: ['⌘', 'K'], label: 'Quick jump or create in any project' },
        { keys: ['⇧', 'F10'], label: 'Open the focused row context menu' },
        { keys: ['⌥', '↑ / ↓'], label: 'Reorder the focused project or process' },
        { keys: ['esc'], label: 'Close the active overlay or dialog' }
      ]
    },
    {
      title: 'Panels and terminal',
      shortcuts: [
        { keys: ['⌘', 'B'], label: 'Collapse or expand the project rail' },
        { keys: ['⌘', '⇧', 'B'], label: 'Collapse or expand the project tree' },
        { keys: ['⌘', 'U'], label: 'Unfocus the terminal and return to the tree' },
        { keys: ['⌘', 'F'], label: 'Search the focused terminal buffer' },
        { keys: ['⌘', '/'], label: 'Open the keyboard reference overlay' }
      ]
    }
  ];
</script>

<section class="hotkeys-card" aria-labelledby="hotkeys-title">
  <header>
    <div>
      <span class="eyebrow">Keyboard</span>
      <h2 id="hotkeys-title">Hotkeys</h2>
      <p>The complete workspace reference, available here or with <kbd>⌘/</kbd>.</p>
    </div>
    <span class="scope">macOS</span>
  </header>

  <div class="groups">
    {#each groups as group}
      <section class="group">
        <h3>{group.title}</h3>
        <div class="shortcut-list">
          {#each group.shortcuts as shortcut}
            <div class="shortcut-row">
              <span class="keys">{#each shortcut.keys as key}<kbd>{key}</kbd>{/each}</span>
              <span>{shortcut.label}</span>
            </div>
          {/each}
        </div>
      </section>
    {/each}
  </div>

  <footer>
    <span>Text fields keep editing keys. Terminal input keeps every key except <strong>⌘U Unfocus</strong>.</span>
  </footer>
</section>

<style>
  .hotkeys-card { overflow: hidden; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); }
  header { display: flex; min-height: 68px; align-items: flex-start; justify-content: space-between; gap: 16px; padding: 11px 12px 10px; }
  .eyebrow, .scope, kbd, h3, footer { font-family: 'JetBrains Mono Variable', monospace; }
  .eyebrow { color: var(--muted); font-size: 7px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
  h2 { margin: 2px 0 0; color: var(--text); font-size: 16px; line-height: 1.15; }
  header p { margin: 4px 0 0; color: var(--muted); font-size: 10px; }
  header p kbd { vertical-align: 1px; }
  .scope { border: 1px solid var(--border); border-radius: 3px; padding: 5px 7px; background: var(--night); color: var(--muted); font-size: 7px; }

  .groups { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); border-top: 1px solid var(--border); }
  .group { min-width: 0; border-bottom: 1px solid var(--border); }
  .group:nth-child(odd) { border-right: 1px solid var(--border); }
  .group:last-child { grid-column: 1 / -1; border-bottom: 0; border-right: 0; }
  h3 { min-height: 29px; margin: 0; border-bottom: 1px solid var(--border); padding: 9px 11px 7px; color: var(--muted); font-size: 7px; letter-spacing: .06em; text-transform: uppercase; }
  .shortcut-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .group:not(:last-child) .shortcut-list { grid-template-columns: 1fr; }
  .shortcut-row { display: grid; min-height: 41px; grid-template-columns: 108px minmax(0, 1fr); align-items: center; gap: 8px; border-bottom: 1px solid var(--border); padding: 6px 10px; color: var(--text-soft); font-size: 8px; line-height: 1.35; }
  .shortcut-row:last-child { border-bottom: 0; }
  .group:last-child .shortcut-row:nth-child(odd) { border-right: 1px solid var(--border); }
  .keys { display: flex; align-items: center; gap: 3px; }
  kbd { display: inline-grid; min-width: 22px; min-height: 21px; place-items: center; border: 1px solid var(--border-strong); border-bottom-color: color-mix(in srgb, var(--text) 30%, var(--border)); border-radius: 3px; padding: 1px 5px; background: var(--surface-raised); color: var(--text-soft); font-size: 7px; }
  footer { min-height: 38px; border-top: 1px solid var(--border); padding: 11px 12px; color: var(--muted); font-size: 7px; }
  footer strong { color: var(--text-soft); font-weight: 650; }

  @media (max-width: 760px) {
    .groups, .shortcut-list, .group:last-child .shortcut-list { grid-template-columns: 1fr; }
    .group, .group:nth-child(odd), .group:last-child { grid-column: auto; border-right: 0; border-bottom: 1px solid var(--border); }
    .group:last-child { border-bottom: 0; }
    .group:last-child .shortcut-row:nth-child(odd) { border-right: 0; }
  }
</style>
