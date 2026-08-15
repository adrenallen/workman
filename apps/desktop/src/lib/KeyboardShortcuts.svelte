<script lang="ts">
  import XIcon from '@lucide/svelte/icons/x';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { ScrollArea } from '$lib/components/ui/scroll-area';

  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  const groups = [
    {
      title: 'Move through the workspace',
      shortcuts: [
        { keys: ['⌘', '← / →'], label: 'Move between project rail, tree, and main frame' },
        { keys: ['⌘', '↑ / ↓'], label: 'Select the previous or next process' },
        { keys: ['⌘', '⌥', '← / →'], label: 'Panel traversal alias' },
        { keys: ['↑ / ↓'], label: 'Move through the focused project or tree list' },
        { keys: ['← / →'], label: 'Collapse or expand a focused tree group' },
        { keys: ['↵'], label: 'Open or activate the focused row' }
      ]
    },
    {
      title: 'Jump and act',
      shortcuts: [
        { keys: ['⌘', 'K'], label: 'Quick jump or create in any project' },
        { keys: ['⌘', '⇧', 'P'], label: 'Search quick prompts for the selected agent' },
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

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' || (event.metaKey && event.key === '/')) {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
  <Dialog.Content
    class="grid max-h-[min(650px,calc(100dvh-36px))] w-[min(760px,calc(100vw-36px))] max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden rounded-lg border border-border bg-popover p-0 text-foreground shadow-2xl"
    showCloseButton={false}
    aria-labelledby="shortcuts-title"
    onkeydown={handleKeydown}
  >
    <header>
      <div>
        <span>Keyboard reference</span>
        <h2 id="shortcuts-title">Work without leaving the keys</h2>
      </div>
      <Button class="text-muted-foreground" variant="ghost" size="icon-sm" aria-label="Close keyboard shortcuts" onclick={onClose}>
        <XIcon size={15} />
      </Button>
    </header>

    <ScrollArea class="min-h-0 p-2">
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
    </ScrollArea>

    <footer>
      <span>Text fields keep Home/End. Terminal input keeps every key except <strong>⌘U Unfocus</strong>.</span>
      <Button class="shrink-0" variant="outline" size="sm" onclick={onClose}>Done</Button>
    </footer>
  </Dialog.Content>
</Dialog.Root>

<style>
  header { display: flex; min-height: 54px; align-items: center; justify-content: space-between; gap: 12px; border-bottom: 1px solid var(--border); padding: 7px 10px 7px 12px; background: var(--popover); }
  header span, header h2 { display: block; margin: 0; }
  header span { color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; letter-spacing: 0.055em; text-transform: uppercase; }
  header h2 { margin-top: 3px; color: var(--foreground); font-size: 13px; font-weight: 680; }
  section { border: 1px solid var(--border); border-radius: 3px; background: var(--card); }
  section + section { margin-top: 6px; }
  h3 { min-height: 27px; margin: 0; border-bottom: 1px solid var(--accent); padding: 7px 9px 5px; color: var(--text-soft); font-size: var(--font-size-xs); font-weight: 700; letter-spacing: 0.055em; text-transform: uppercase; }
  .shortcut-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .shortcut-row { display: grid; min-height: 39px; grid-template-columns: 126px minmax(0, 1fr); align-items: center; gap: 8px; border-bottom: 1px solid var(--border); padding: 5px 8px; color: var(--text-soft); font-size: var(--font-size-sm); }
  .shortcut-row:nth-child(odd) { border-right: 1px solid var(--border); }
  .shortcut-row:nth-last-child(-n + 2) { border-bottom: 0; }
  .keys { display: flex; align-items: center; gap: 3px; }
  kbd { display: inline-grid; min-width: 23px; min-height: 21px; place-items: center; border: 1px solid #444a52; border-bottom-color: #616873; border-radius: 3px; padding: 1px 5px; background: var(--accent); color: var(--foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  footer { display: flex; min-height: 42px; align-items: center; justify-content: space-between; gap: 12px; border-top: 1px solid var(--border); padding: 6px 8px 6px 11px; background: var(--card); color: var(--muted-foreground); font-size: var(--font-size-xs); }
  footer strong { color: var(--text-soft); font-weight: 650; }

  @media (max-width: 660px) {
    .shortcut-list { grid-template-columns: 1fr; }
    .shortcut-row, .shortcut-row:nth-child(odd) { border-right: 0; border-bottom: 1px solid var(--border); }
    .shortcut-row:last-child { border-bottom: 0; }
    footer span { max-width: 70%; }
  }

</style>
