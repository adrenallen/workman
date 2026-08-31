<script lang="ts">
  import {
    PROJECT_ICON_COLOR_CHOICES,
    sidebarIdentityColorValue,
    type SidebarIdentityColor
  } from './projectAppearance';

  interface Props {
    value: SidebarIdentityColor | null;
    disabled?: boolean;
    onChange: (value: SidebarIdentityColor | null) => void;
  }

  let { value, disabled = false, onChange }: Props = $props();
</script>

<div class="color-grid" role="radiogroup" aria-label="Name color">
  <button
    class:selected={value === null}
    type="button"
    role="radio"
    aria-checked={value === null}
    {disabled}
    onclick={() => onChange(null)}
  >
    <span class="color-swatch automatic" aria-hidden="true"></span>
    <span>Default</span>
  </button>
  {#each PROJECT_ICON_COLOR_CHOICES as choice (choice.id)}
    <button
      class:selected={value === choice.id}
      type="button"
      role="radio"
      aria-checked={value === choice.id}
      {disabled}
      onclick={() => onChange(choice.id)}
    >
      <span
        class="color-swatch"
        style:background={sidebarIdentityColorValue(choice.id)}
        aria-hidden="true"
      ></span>
      <span>{choice.label}</span>
    </button>
  {/each}
</div>

<style>
  .color-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 4px; }
  button { display: flex; min-width: 0; min-height: 30px; align-items: center; gap: 7px; border: 1px solid var(--border); border-radius: var(--radius); padding: 4px 7px; background: var(--card); color: var(--text-soft); cursor: pointer; }
  button:hover:not(:disabled) { border-color: var(--border-strong); background: var(--accent); }
  button.selected { border-color: var(--ring); background: color-mix(in srgb, var(--ring) 9%, var(--card)); color: var(--foreground); }
  button > span:last-child { overflow: hidden; font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }
  .color-swatch { width: 10px; height: 10px; flex: none; border: 1px solid color-mix(in srgb, currentColor 25%, transparent); border-radius: 999px; }
  .color-swatch.automatic { background: linear-gradient(135deg, var(--foreground) 0 48%, var(--muted-foreground) 52% 100%); }
  button:disabled { cursor: default; opacity: 0.45; }
  @media (max-width: 480px) { .color-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
