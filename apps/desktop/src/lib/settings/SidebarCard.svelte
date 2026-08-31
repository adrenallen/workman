<script lang="ts">
  import { onMount } from 'svelte';
  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';

  import { loadPanelPreference, type PanelPreference } from '../panelPreferences';

  const projectFallback = { collapsed: false, width: 238 };
  const treeFallback = { collapsed: false, width: 280 };

  let projectRail = $state<PanelPreference>(projectFallback);
  let projectTree = $state<PanelPreference>(treeFallback);

  onMount(() => {
    projectRail = loadPanelPreference('project-rail', projectFallback, 176, 340);
    projectTree = loadPanelPreference('section-rail', treeFallback, 220, 420);
  });

  function stateLabel(preference: PanelPreference): string {
    return preference.collapsed ? 'Collapsed' : `${preference.width}px`;
  }
</script>

<section class="sidebar-card" aria-labelledby="sidebar-settings-title">
  <header>
    <div>
      <span class="eyebrow">Workspace</span>
      <h2 id="sidebar-settings-title">Sidebar</h2>
      <p>Rails remember their width and collapsed state locally.</p>
    </div>
    <span class="saved"><StatusIndicator tone="success" label="Sidebar preferences saved locally" />Saved locally</span>
  </header>

  <div class="rail-list">
    <div class="rail-row">
      <span class="rail-preview project" aria-hidden="true"><i></i><i></i><i></i></span>
      <div><strong>Project rail</strong><small>Projects, status, and workspace switching.</small></div>
      <output>{stateLabel(projectRail)}</output>
      <kbd>⌘B</kbd>
    </div>
    <div class="rail-row">
      <span class="rail-preview tree" aria-hidden="true"><i></i><i></i><i></i></span>
      <div><strong>Project tree</strong><small>Agents, terminals, commands, todos, and notes.</small></div>
      <output>{stateLabel(projectTree)}</output>
      <kbd>⌘⇧B</kbd>
    </div>
  </div>

  <div class="guidance">
    <span aria-hidden="true">↔</span>
    <p><strong>Resize directly in the workspace</strong><small>Drag either rail edge, or focus its resize handle and use the arrow keys. Changes are saved automatically.</small></p>
  </div>

  <footer>
    <span>More sidebar display options will live here as they become available.</span>
  </footer>
</section>

<style>
  .sidebar-card { overflow: hidden; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); }
  header { display: flex; min-height: 68px; align-items: flex-start; justify-content: space-between; gap: 16px; padding: 11px 12px 10px; }
  .eyebrow, small, output, kbd, .saved, footer { font-family: 'JetBrains Mono Variable', monospace; }
  .eyebrow { color: var(--muted); font-size: var(--font-size-xs); font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
  h2 { margin: 2px 0 0; color: var(--text); font-size: 16px; line-height: 1.15; }
  header p { margin: 4px 0 0; color: var(--muted); font-size: var(--font-size-sm); }
  .saved { display: flex; align-items: center; gap: 6px; border: 1px solid var(--border); border-radius: 3px; padding: 5px 7px; background: var(--night); color: var(--muted); font-size: var(--font-size-xs); }

  .rail-list { border-top: 1px solid var(--border); }
  .rail-row { display: grid; min-height: 59px; grid-template-columns: 42px minmax(0, 1fr) 62px 44px; align-items: center; gap: 11px; padding: 7px 12px; }
  .rail-row + .rail-row { border-top: 1px solid var(--border); }
  .rail-row strong, .rail-row small { display: block; }
  .rail-row strong { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 660; }
  .rail-row small { margin-top: 3px; color: var(--muted); font-size: var(--font-size-xs); line-height: 1.35; }
  output { color: var(--text-soft); font-size: var(--font-size-xs); text-align: right; }
  kbd { display: grid; min-height: 23px; place-items: center; border: 1px solid var(--border-strong); border-bottom-color: color-mix(in srgb, var(--text) 30%, var(--border)); border-radius: 3px; background: var(--surface-raised); color: var(--text-soft); font-size: var(--font-size-xs); }
  .rail-preview { display: flex; width: 40px; height: 34px; align-items: stretch; gap: 2px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 3px; background: var(--night); }
  .rail-preview i { display: block; border-radius: 1px; background: color-mix(in srgb, var(--text) 10%, var(--surface)); }
  .rail-preview i:first-child { width: 8px; background: color-mix(in srgb, var(--signal) 25%, var(--surface)); }
  .rail-preview i:nth-child(2) { width: 11px; }
  .rail-preview i:last-child { flex: 1; background: transparent; }
  .rail-preview.tree i:first-child { width: 5px; }
  .rail-preview.tree i:nth-child(2) { width: 15px; background: color-mix(in srgb, var(--signal) 18%, var(--surface)); }

  .guidance { display: grid; grid-template-columns: 29px minmax(0, 1fr); align-items: center; gap: 9px; border-top: 1px solid var(--border); padding: 9px 12px; background: color-mix(in srgb, var(--night) 68%, var(--surface)); }
  .guidance > span { display: grid; width: 28px; height: 28px; place-items: center; border: 1px solid var(--border); border-radius: 3px; color: var(--signal); font-size: 13px; }
  .guidance p { margin: 0; }
  .guidance strong, .guidance small { display: block; }
  .guidance strong { color: var(--text-soft); font-size: var(--font-size-sm); }
  .guidance small { margin-top: 3px; color: var(--muted); font-size: var(--font-size-xs); line-height: 1.4; }
  footer { min-height: 36px; border-top: 1px solid var(--border); padding: 11px 12px; color: var(--muted); font-size: var(--font-size-xs); }

  @media (max-width: 660px) { .rail-row { grid-template-columns: 36px minmax(0, 1fr) auto; } .rail-row output { display: none; } }
</style>
