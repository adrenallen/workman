<script lang="ts">
  import { onMount, tick } from 'svelte';

  import type { ContextActionId, ContextMenuItem } from './contextMenu';

  interface Props {
    x: number;
    y: number;
    title: string;
    subtitle: string;
    items: ContextMenuItem[];
    onSelect: (id: ContextActionId) => void;
    onClose: () => void;
  }

  let { x, y, title, subtitle, items, onSelect, onClose }: Props = $props();

  let menu: HTMLDivElement;
  let left = $state(0);
  let top = $state(0);
  let activeIndex = $state(0);

  onMount(() => {
    void placeAndFocus();
    const reposition = () => void place();
    window.addEventListener('resize', reposition);
    return () => window.removeEventListener('resize', reposition);
  });

  $effect(() => {
    items;
    activeIndex = firstEnabledIndex();
    void placeAndFocus();
  });

  async function placeAndFocus(): Promise<void> {
    await tick();
    place();
    await tick();
    focusActive();
  }

  function place(): void {
    if (!menu) return;
    const margin = 8;
    const bounds = menu.getBoundingClientRect();
    left = Math.max(margin, Math.min(x, window.innerWidth - bounds.width - margin));
    top = Math.max(margin, Math.min(y, window.innerHeight - bounds.height - margin));
  }

  function firstEnabledIndex(): number {
    const index = items.findIndex((item) => !item.disabled);
    return index < 0 ? 0 : index;
  }

  function focusActive(): void {
    menu?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')[activeIndex]?.focus();
  }

  function moveActive(delta: -1 | 1): void {
    if (!items.some((item) => !item.disabled)) return;
    let next = activeIndex;
    do {
      next = (next + delta + items.length) % items.length;
    } while (items[next]?.disabled);
    activeIndex = next;
    focusActive();
  }

  function choose(item: ContextMenuItem): void {
    if (!item.disabled) onSelect(item.id);
  }

  function handleKey(event: KeyboardEvent): void {
    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        event.stopImmediatePropagation();
        onClose();
        break;
      case 'ArrowDown':
        event.preventDefault();
        moveActive(1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        moveActive(-1);
        break;
      case 'Home':
        event.preventDefault();
        activeIndex = firstEnabledIndex();
        focusActive();
        break;
      case 'End': {
        event.preventDefault();
        const reverseIndex = [...items].reverse().findIndex((item) => !item.disabled);
        activeIndex = reverseIndex < 0 ? 0 : items.length - 1 - reverseIndex;
        focusActive();
        break;
      }
    }
  }
</script>

<svelte:window onkeydown={handleKey} />

<button
  class="dismiss-layer"
  type="button"
  tabindex="-1"
  aria-label="Close context menu"
  onclick={onClose}
  oncontextmenu={(event) => {
    event.preventDefault();
    onClose();
  }}
></button>

<div
  class="context-menu"
  bind:this={menu}
  role="menu"
  tabindex="-1"
  aria-label={`${title} actions`}
  style:left={`${left}px`}
  style:top={`${top}px`}
  oncontextmenu={(event) => event.preventDefault()}
>
  <header>
    <span>{subtitle}</span>
    <strong>{title}</strong>
  </header>
  <div class="items">
    {#each items as item, index (item.id)}
      {#if item.separatorBefore}<div class="separator" role="separator"></div>{/if}
      <button
        class:destructive={item.destructive}
        class:active={index === activeIndex}
        type="button"
        role="menuitem"
        tabindex={index === activeIndex ? 0 : -1}
        disabled={item.disabled}
        aria-keyshortcuts={item.shortcut}
        onclick={() => choose(item)}
        onpointerenter={() => {
          if (!item.disabled) activeIndex = index;
        }}
      >
        <span class="signal" aria-hidden="true"></span>
        <span class="copy">
          <span class="label">{item.label}</span>
          {#if item.detail}<span class="detail">{item.detail}</span>{/if}
        </span>
        {#if item.shortcut}<kbd>{item.shortcut}</kbd>{/if}
      </button>
    {/each}
  </div>
</div>

<style>
  .dismiss-layer {
    position: fixed;
    inset: 0;
    z-index: 1090;
    width: 100%;
    height: 100%;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    cursor: default;
  }

  .context-menu {
    position: fixed;
    z-index: 1091;
    width: min(236px, calc(100vw - 16px));
    overflow: hidden;
    border: 1px solid #4a5057;
    border-radius: 4px;
    background: #171a1e;
    color: #e6e8eb;
    box-shadow: 0 16px 42px rgb(0 0 0 / 48%), 0 2px 10px rgb(0 0 0 / 55%);
    animation: menu-in 90ms cubic-bezier(.2, .8, .2, 1);
  }

  header {
    display: grid;
    gap: 2px;
    min-width: 0;
    padding: 8px 10px 7px 12px;
    border-bottom: 1px solid #30353b;
    background: #1d2126;
  }

  header span,
  kbd,
  .detail {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  header span {
    color: #8f98a3;
    font-size: 8px;
    font-weight: 680;
    letter-spacing: .12em;
  }

  header strong {
    overflow: hidden;
    color: #f1f3f5;
    font-size: 11px;
    font-weight: 660;
    line-height: 1.3;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .items {
    padding: 4px;
  }

  .items button {
    position: relative;
    display: grid;
    grid-template-columns: 3px minmax(0, 1fr) auto;
    align-items: center;
    width: 100%;
    min-height: 28px;
    margin: 0;
    padding: 4px 7px 4px 4px;
    overflow: hidden;
    border: 0;
    border-radius: 2px;
    background: transparent;
    color: #d8dce0;
    font: inherit;
    text-align: left;
  }

  .items button:hover,
  .items button.active,
  .items button:focus-visible {
    outline: none;
    background: #293139;
    color: #fff;
  }

  .items button:disabled {
    color: #666e77;
    cursor: default;
  }

  .items button:disabled:hover {
    background: transparent;
  }

  .signal {
    align-self: stretch;
    width: 2px;
    border-radius: 2px;
    background: transparent;
  }

  .items button:hover .signal,
  .items button.active .signal,
  .items button:focus-visible .signal {
    background: #55b6c9;
  }

  .items button.destructive {
    color: #e8a2a2;
  }

  .items button.destructive:hover .signal,
  .items button.destructive.active .signal,
  .items button.destructive:focus-visible .signal {
    background: #d86767;
  }

  .copy {
    display: grid;
    gap: 1px;
    min-width: 0;
    padding-left: 6px;
  }

  .label {
    overflow: hidden;
    font-size: 11px;
    font-weight: 530;
    line-height: 1.25;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    overflow: hidden;
    color: #8f98a3;
    font-size: 8px;
    line-height: 1.2;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  kbd {
    padding-left: 8px;
    color: #858e98;
    font-size: 8px;
    font-weight: 560;
  }

  .separator {
    height: 1px;
    margin: 4px 5px;
    background: #30353b;
  }

  @keyframes menu-in {
    from { opacity: 0; transform: scale(.985) translateY(-2px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  @media (prefers-reduced-motion: reduce) {
    .context-menu { animation: none; }
  }
</style>
