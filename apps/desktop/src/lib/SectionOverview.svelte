<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    ariaLabel: string;
    eyebrow: string;
    title: string;
    description: string;
    icon: Snippet;
    summary: Snippet;
    children: Snippet;
    action?: Snippet;
    controls?: Snippet;
    summaryLayout?: 'start' | 'split';
  }

  let {
    ariaLabel,
    eyebrow,
    title,
    description,
    icon,
    summary,
    children,
    action,
    controls,
    summaryLayout = 'start'
  }: Props = $props();
</script>

<section class="section-overview" aria-label={ariaLabel}>
  <header class="overview-heading">
    <span class="overview-icon" aria-hidden="true">{@render icon()}</span>
    <div class="overview-copy">
      <span class="eyebrow">{eyebrow}</span>
      <h2>{title}</h2>
      <p>{description}</p>
    </div>
    {#if action}<div class="overview-action">{@render action()}</div>{/if}
  </header>

  {#if controls}<div class="overview-controls">{@render controls()}</div>{/if}
  <div class="overview-summary" class:split-summary={summaryLayout === 'split'}>{@render summary()}</div>
  <div class="overview-body">{@render children()}</div>
</section>

<style>
  .section-overview {
    container-type: inline-size;
    display: grid;
    width: 100%;
    height: 100%;
    min-width: 0;
    grid-template-rows: auto auto auto minmax(0, 1fr);
    background: var(--background);
    color: var(--foreground);
  }

  .section-overview:not(:has(.overview-controls)) {
    grid-template-rows: auto auto minmax(0, 1fr);
  }

  .overview-heading {
    display: grid;
    min-height: 78px;
    grid-template-columns: 36px minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-3);
    border-bottom: 1px solid var(--border);
    padding: 12px 16px;
  }

  .overview-icon {
    display: grid;
    width: 34px;
    height: 34px;
    place-items: center;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    background: var(--card);
    color: var(--text-soft);
  }

  .overview-icon :global(svg) { width: 18px; height: 18px; }
  .overview-copy { min-width: 0; }
  .overview-copy h2 { margin: 1px 0 0; font-size: var(--font-size-xl); font-weight: 650; letter-spacing: -0.025em; }
  .overview-copy p { margin: 3px 0 0; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .eyebrow, .overview-summary { font-family: var(--terminal-font-family); }
  .eyebrow { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 650; letter-spacing: 0.08em; text-transform: uppercase; }
  .overview-action { justify-self: end; }
  .overview-controls { border-bottom: 1px solid var(--border); background: var(--card); }
  .overview-summary { display: flex; min-height: 28px; align-items: center; gap: var(--space-2); border-bottom: 1px solid var(--border); padding: 0 12px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .overview-summary.split-summary { justify-content: space-between; }
  .overview-body { min-height: 0; overflow: hidden; }

  @container (max-width: 620px) {
    .overview-heading { grid-template-columns: 34px minmax(0, 1fr); }
    .overview-action { grid-column: 1 / -1; justify-self: start; padding-left: 46px; }
  }
</style>
