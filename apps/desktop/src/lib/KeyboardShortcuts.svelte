<script lang="ts">
  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  const groups = [
    {
      title: 'Move through the workspace',
      shortcuts: [
        { keys: ['⌘', '⌥', '← / →'], label: 'Move between project rail, tree, and main frame' },
        { keys: ['↑ / ↓'], label: 'Move through the focused project or tree list' },
        { keys: ['← / →'], label: 'Collapse or expand a focused tree group' },
        { keys: ['↵'], label: 'Open or activate the focused row' },
        { keys: ['Tab'], label: 'Move through controls in the focused panel' }
      ]
    },
    {
      title: 'Jump and act',
      shortcuts: [
        { keys: ['⌘', 'K'], label: 'Quick jump or create in any project' },
        { keys: ['⇧', 'F10'], label: 'Open the focused row’s context menu' },
        { keys: ['⌥', '↑ / ↓'], label: 'Reorder the focused project or process' },
        { keys: ['⌘', '/'], label: 'Show or close this shortcuts reference' },
        { keys: ['esc'], label: 'Close the active overlay or dialog' }
      ]
    },
    {
      title: 'Panels and terminal',
      shortcuts: [
        { keys: ['⌘', 'B'], label: 'Collapse or expand the project rail' },
        { keys: ['⌘', '⇧', 'B'], label: 'Collapse or expand the project tree' },
        { keys: ['⌘', 'U'], label: 'Unfocus the terminal and return to the project tree' },
        { keys: ['⌘', 'F'], label: 'Search the focused terminal buffer' },
        { keys: ['Tab', '↵'], label: 'Use Unfocus, Previous, and Next in the process bar' }
      ]
    }
  ];

  function focusDialog(node: HTMLElement): void {
    queueMicrotask(() => node.focus());
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' || (event.metaKey && event.key === '/')) {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  }
</script>

<div
  class="shortcuts-backdrop"
  role="presentation"
  onpointerdown={(event) => { if (event.target === event.currentTarget) onClose(); }}
>
  <div
    class="shortcuts-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="shortcuts-title"
    tabindex="-1"
    use:focusDialog
    onkeydown={handleKeydown}
  >
    <header>
      <div>
        <span>Keyboard reference</span>
        <h2 id="shortcuts-title">Work without leaving the keys</h2>
      </div>
      <button type="button" aria-label="Close keyboard shortcuts" onclick={onClose}>×</button>
    </header>

    <div class="shortcut-groups">
      {#each groups as group}
        <section>
          <h3>{group.title}</h3>
          <div class="shortcut-list">
            {#each group.shortcuts as shortcut}
              <div class="shortcut-row">
                <span class="keys">
                  {#each shortcut.keys as key}<kbd>{key}</kbd>{/each}
                </span>
                <span>{shortcut.label}</span>
              </div>
            {/each}
          </div>
        </section>
      {/each}
    </div>

    <footer>
      <span>Terminal input keeps every key except the explicit <strong>⌘U Unfocus</strong> escape.</span>
      <button type="button" onclick={onClose}>Done</button>
    </footer>
  </div>
</div>

<style>
  .shortcuts-backdrop { position: fixed; z-index: 1100; inset: 0; display: grid; place-items: center; padding: 18px; background: rgb(5 7 9 / 68%); }
  .shortcuts-dialog { display: grid; width: min(760px, calc(100vw - 36px)); max-height: min(650px, calc(100vh - 36px)); grid-template-rows: auto minmax(0, 1fr) auto; overflow: hidden; border: 1px solid #555b63; border-radius: 5px; outline: 0; background: #17191c; box-shadow: 0 18px 48px rgb(0 0 0 / 44%); color: var(--text); }
  header { display: flex; min-height: 54px; align-items: center; justify-content: space-between; gap: 12px; border-bottom: 1px solid var(--border); padding: 7px 10px 7px 12px; background: #1b1e22; }
  header span, header h2 { display: block; margin: 0; }
  header span { color: #858c95; font: 7px 'JetBrains Mono Variable', monospace; letter-spacing: 0.055em; text-transform: uppercase; }
  header h2 { margin-top: 3px; color: #edf0f2; font-size: 13px; font-weight: 680; }
  header button { display: grid; width: 28px; height: 28px; place-items: center; border: 1px solid #41464d; border-radius: 3px; background: #202328; color: #aeb4bc; font-size: 16px; cursor: pointer; }
  header button:hover { border-color: #656c75; color: #fff; }
  .shortcut-groups { min-height: 0; overflow-y: auto; padding: 7px; scrollbar-color: #454b53 transparent; scrollbar-width: thin; }
  section { border: 1px solid #2d3137; border-radius: 3px; background: #141619; }
  section + section { margin-top: 6px; }
  h3 { min-height: 27px; margin: 0; border-bottom: 1px solid #292d32; padding: 7px 9px 5px; color: #9da4ad; font-size: 8px; font-weight: 700; letter-spacing: 0.055em; text-transform: uppercase; }
  .shortcut-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .shortcut-row { display: grid; min-height: 39px; grid-template-columns: 126px minmax(0, 1fr); align-items: center; gap: 8px; border-bottom: 1px solid #24282d; padding: 5px 8px; color: #b4bac2; font-size: 9px; }
  .shortcut-row:nth-child(odd) { border-right: 1px solid #24282d; }
  .shortcut-row:nth-last-child(-n + 2) { border-bottom: 0; }
  .keys { display: flex; align-items: center; gap: 3px; }
  kbd { display: inline-grid; min-width: 23px; min-height: 21px; place-items: center; border: 1px solid #444a52; border-bottom-color: #616873; border-radius: 3px; padding: 1px 5px; background: #23262b; color: #d0d4d9; font: 8px 'JetBrains Mono Variable', monospace; }
  footer { display: flex; min-height: 42px; align-items: center; justify-content: space-between; gap: 12px; border-top: 1px solid var(--border); padding: 6px 8px 6px 11px; background: #15171a; color: #818892; font-size: 8px; }
  footer strong { color: #b9bfc6; font-weight: 650; }
  footer button { min-height: 27px; border: 1px solid #4b5159; border-radius: 3px; padding: 0 13px; background: #25282d; color: #d9dce0; font-size: 9px; font-weight: 650; cursor: pointer; }
  footer button:hover { border-color: #686f78; background: #2b2f34; }

  @media (max-width: 660px) {
    .shortcut-list { grid-template-columns: 1fr; }
    .shortcut-row, .shortcut-row:nth-child(odd) { border-right: 0; border-bottom: 1px solid #24282d; }
    .shortcut-row:last-child { border-bottom: 0; }
    footer span { max-width: 70%; }
  }

  @media (prefers-reduced-motion: no-preference) {
    .shortcuts-dialog { animation: shortcuts-enter 100ms ease-out; }
    @keyframes shortcuts-enter { from { opacity: 0; transform: translateY(-3px); } }
  }
</style>
