<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import ArrowUpDownIcon from '@lucide/svelte/icons/arrow-up-down';
  import FileTextIcon from '@lucide/svelte/icons/file-text';
  import BotIcon from '@lucide/svelte/icons/bot';
  import TerminalIcon from '@lucide/svelte/icons/terminal';
  import CheckSquareIcon from '@lucide/svelte/icons/square-check';
  import MessageSquareIcon from '@lucide/svelte/icons/message-square';
  import PanelsTopLeftIcon from '@lucide/svelte/icons/panels-top-left';
  import { hotkeyDisplayLabel, hotkeyPreferences, matchesHotkeyAction } from './hotkeys';
  import { primaryModifier, primaryModifierLabel } from './primaryModifier';
  import type { WorkspaceViewState } from './workspaceViewHistory';

  export interface RecentViewItem {
    view: WorkspaceViewState;
    label: string;
    project: string;
    kind: string;
    current?: boolean;
  }

  let { getItems, onChoose, blocked }: {
    getItems: () => RecentViewItem[];
    onChoose: (view: WorkspaceViewState) => void;
    blocked: () => boolean;
  } = $props();
  let items = $state<RecentViewItem[]>([]);
  let index = $state(0);
  let list = $state<HTMLElement | undefined>();
  $effect(() => {
    const selected = index;
    const row = list?.children[selected] as HTMLElement | undefined;
    if (!list || !row) return;
    const top = row.offsetTop;
    if (top < list.scrollTop) list.scrollTop = top;
    else if (top + row.offsetHeight > list.scrollTop + list.clientHeight) list.scrollTop = top + row.offsetHeight - list.clientHeight;
  });
  let primaryHeld = false;
  const icons: Record<string, typeof FileTextIcon> = {
    scratchpad: FileTextIcon, agent: BotIcon, terminal: TerminalIcon,
    command: TerminalIcon, todo: CheckSquareIcon, feedback: MessageSquareIcon
  };

  function choose(selected = index): void {
    const item = items[selected];
    items = [];
    if (item) onChoose(item.view);
  }

  export async function triggerFromNativeMenu(): Promise<void> {
    const held = await invoke<boolean | null>('desktop_primary_modifier_pressed').catch(() => null);
    trigger(held ?? primaryHeld);
  }

  export function trigger(hold = primaryHeld): void {
    if (blocked()) return;
    if (items.length) index = (index + 1) % items.length;
    else {
      items = getItems();
      index = Math.max(0, items.findIndex(item => !item.current));
    }
    if (!hold) choose();
  }

  $effect(() => {
    if (!items.length || !isTauri()) return;
    const snapshot = items;
    let active = true;
    let reading = false;
    const checkRelease = async () => {
      if (reading) return;
      reading = true;
      try {
        const held = await invoke<boolean | null>('desktop_primary_modifier_pressed');
        if (active && items === snapshot) {
          if (held === false) { primaryHeld = false; choose(); }
          else if (held === null) clearInterval(timer);
        }
      } catch { clearInterval(timer); } // Other platforms continue to use DOM keyup.
      finally { reading = false; }
    };
    const timer = setInterval(() => void checkRelease(), 50);
    void checkRelease();
    return () => { active = false; clearInterval(timer); };
  });

  onMount(() => {
    const down = (event: KeyboardEvent) => {
      primaryHeld = primaryModifier(event);
      if (items.length) {
        if (event.key === 'Escape') items = [];
        else if (event.key === 'ArrowUp') index = (index - 1 + items.length) % items.length;
        else if (event.key === 'ArrowDown') index = (index + 1) % items.length;
        else if (event.key === 'Enter') choose();
        else if (matchesHotkeyAction(event, 'previous-view', $hotkeyPreferences)) {
          if (!event.repeat) trigger(primaryHeld);
        } else if (event.key === 'Meta' || event.key === 'Control' || event.key === 'Shift') return;
        // Keep text and other shortcuts out of the underlying editor while cycling.
        event.preventDefault();
        event.stopImmediatePropagation();
      } else if (!blocked() && matchesHotkeyAction(event, 'previous-view', $hotkeyPreferences)) {
        event.preventDefault();
        event.stopImmediatePropagation();
        if (!event.repeat) trigger(primaryHeld);
      }
    };
    const up = (event: KeyboardEvent) => {
      primaryHeld = primaryModifier(event);
      if (items.length && !primaryHeld) {
        event.preventDefault();
        event.stopImmediatePropagation();
        choose();
      }
    };
    const blur = () => { primaryHeld = false; items = []; };
    window.addEventListener('keydown', down, true);
    window.addEventListener('keyup', up, true);
    window.addEventListener('blur', blur);
    return () => {
      window.removeEventListener('keydown', down, true);
      window.removeEventListener('keyup', up, true);
      window.removeEventListener('blur', blur);
    };
  });
</script>

{#if items.length}
  <div class="recent-backdrop" role="presentation">
    <section class="recent-switcher" aria-label="Recent tabs">
      <header><strong>Recent tabs</strong><span>Last 10 minutes</span></header>
      <div bind:this={list} role="listbox" aria-label="Recent tabs" aria-activedescendant={`recent-tab-${index}`} tabindex="-1">
        {#each items as item, i}
          {@const Icon = icons[item.kind] ?? PanelsTopLeftIcon}
          <button id={`recent-tab-${i}`} type="button" role="option" aria-selected={i === index}
            class:active={i === index} tabindex="-1" onmousedown={event => event.preventDefault()} onclick={() => choose(i)}>
            <Icon size={16} strokeWidth={1.8} />
            <span class="recent-label">{item.label}<small>{item.project}</small></span>
            {#if item.current}<span class="current-label">Current</span>{/if}
          </button>
        {/each}
      </div>
      <footer><ArrowUpDownIcon size={12} /><span>{hotkeyDisplayLabel($hotkeyPreferences['previous-view'])} or ↑ ↓ to cycle · Release {primaryModifierLabel} to open · Esc to cancel</span></footer>
    </section>
  </div>
{/if}

<style>
  .recent-backdrop { position: fixed; inset: 0; z-index: 200; display: grid; place-items: center; background: rgb(0 0 0 / 22%); }
  .recent-switcher { width: min(460px, calc(100vw - 48px)); overflow: hidden; border: 1px solid var(--border); border-radius: 12px; background: var(--popover); color: var(--popover-foreground); box-shadow: 0 16px 60px rgb(0 0 0 / 30%); animation: appear 120ms ease-out; }
  header { display: flex; justify-content: space-between; gap: 12px; padding: 16px 16px 12px; font-size: 12px; }
  header span, footer, small, .current-label { color: var(--muted-foreground); }
  [role='listbox'] { position: relative; display: grid; gap: 3px; padding: 0 7px 7px; max-height: min(560px, calc(100vh - 180px)); overflow-y: auto; }
  button { display: flex; align-items: center; gap: 12px; width: 100%; min-width: 0; padding: 10px; border: 1px solid transparent; border-radius: 6px; text-align: left; color: var(--foreground); cursor: pointer; }
  button.active { background: var(--accent); border-color: var(--border-strong); }
  button:hover { background: var(--accent); }
  .recent-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; font-weight: 500; }
  small { display: block; font-size: var(--font-size-xs); font-weight: 400; margin-top: 2px; overflow: hidden; text-overflow: ellipsis; }
  .current-label { font-size: var(--font-size-xs); }
  footer { display: flex; align-items: center; gap: 6px; padding: 10px 16px; border-top: 1px solid var(--border); font-size: var(--font-size-xs); }
  @keyframes appear { from { opacity: 0; transform: translateY(4px) scale(.99); } to { opacity: 1; transform: none; } }
  @media (prefers-reduced-motion: reduce) { .recent-switcher { animation: none; } }
</style>
