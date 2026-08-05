<script lang="ts">
  import {
    selectSettingsSection,
    settingsSection,
    settingsSections
  } from '../settingsSections';

  interface Props {
    connected: boolean;
  }

  let { connected }: Props = $props();

  function move(event: KeyboardEvent): void {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const current = settingsSections.findIndex((section) => section.id === $settingsSection);
    const next = event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? settingsSections.length - 1
        : (current + (event.key === 'ArrowLeft' ? -1 : 1) + settingsSections.length) % settingsSections.length;
    const section = settingsSections[next];
    if (!section) return;
    selectSettingsSection(section.id);
    requestAnimationFrame(() => document.getElementById(`settings-tab-${section.id}`)?.focus());
  }
</script>

<div class="section-nav" role="tablist" aria-label="Settings sections" tabindex="-1" onkeydown={move}>
  {#each settingsSections as section}
    <button
      id={`settings-tab-${section.id}`}
      type="button"
      role="tab"
      aria-selected={$settingsSection === section.id}
      aria-controls={`settings-panel-${section.id}`}
      tabindex={$settingsSection === section.id ? 0 : -1}
      class:active={$settingsSection === section.id}
      onclick={() => selectSettingsSection(section.id)}
    >
      <span class="icon" aria-hidden="true">{section.icon}</span>
      <span class="copy"><strong>{section.label}</strong><small>{section.description}</small></span>
      {#if !section.local}<i class:online={connected} aria-label={connected ? 'Daemon connected' : 'Daemon offline'}></i>{/if}
    </button>
  {/each}
</div>

<style>
  .section-nav { display: flex; min-width: 0; overflow-x: auto; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  button { position: relative; display: grid; min-width: 94px; min-height: 48px; flex: 1 0 94px; grid-template-columns: 24px minmax(0, 1fr) auto; align-items: center; gap: 6px; border: 0; border-right: 1px solid var(--border); padding: 6px 7px; background: transparent; color: var(--muted); text-align: left; cursor: pointer; }
  button:last-child { border-right: 0; }
  button::after { position: absolute; right: 7px; bottom: 0; left: 7px; height: 2px; background: transparent; content: ''; }
  button:hover { background: color-mix(in srgb, var(--text) 4%, transparent); color: var(--text-soft); }
  button:focus-visible { position: relative; z-index: 1; outline: 1px solid var(--signal); outline-offset: -2px; }
  button.active { background: var(--surface-raised); color: var(--text); }
  button.active::after { background: var(--signal); }
  .icon { display: grid; width: 23px; height: 23px; place-items: center; border: 1px solid var(--border); border-radius: 3px; background: var(--night); color: var(--muted); font: 650 7px/1 'JetBrains Mono Variable', monospace; }
  button.active .icon { border-color: color-mix(in srgb, var(--signal) 42%, var(--border)); color: var(--signal); }
  .copy, .copy strong, .copy small { display: block; min-width: 0; }
  .copy strong { color: inherit; font-size: 9px; font-weight: 680; }
  .copy small { overflow: hidden; margin-top: 2px; color: var(--muted); font: 6.5px/1.25 'JetBrains Mono Variable', monospace; text-overflow: ellipsis; white-space: nowrap; }
  i { width: 5px; height: 5px; border: 1px solid var(--border-strong); border-radius: 50%; background: transparent; }
  i.online { border-color: var(--signal); background: var(--signal); }

  @media (max-width: 920px) {
    button { min-height: 42px; }
    .copy small { display: none; }
  }

  @media (max-width: 620px) {
    button { min-width: 82px; flex-basis: 82px; grid-template-columns: 21px minmax(0, 1fr); }
    .icon { width: 20px; height: 20px; }
    button > i { display: none; }
  }
</style>
