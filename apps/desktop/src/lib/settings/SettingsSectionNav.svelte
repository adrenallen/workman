<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import InfoIcon from '@lucide/svelte/icons/info';
  import KeyboardIcon from '@lucide/svelte/icons/keyboard';
  import MonitorIcon from '@lucide/svelte/icons/monitor';
  import PanelLeftIcon from '@lucide/svelte/icons/panel-left';
  import PaletteIcon from '@lucide/svelte/icons/palette';
  import PlugIcon from '@lucide/svelte/icons/plug';
  import ServerIcon from '@lucide/svelte/icons/server';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import type { Component } from 'svelte';
  import * as Tabs from '$lib/components/ui/tabs';
  import {
    selectSettingsSection,
    settingsSection,
    settingsSections,
    type SettingsSectionId
  } from '../settingsSections';

  const sectionIcons: Record<SettingsSectionId, Component> = {
    appearance: PaletteIcon,
    terminal: SquareTerminalIcon,
    sidebar: PanelLeftIcon,
    hotkeys: KeyboardIcon,
    agents: BotIcon,
    tools: MonitorIcon,
    mcp: PlugIcon,
    daemon: ServerIcon,
    about: InfoIcon
  };

  function choose(value: string): void {
    if (settingsSections.some((section) => section.id === value)) {
      selectSettingsSection(value as SettingsSectionId);
    }
  }
</script>

<div class="settings-nav-shell">
  <Tabs.Root class="w-full min-w-0 max-w-full overflow-hidden" value={$settingsSection} onValueChange={choose}>
    <Tabs.List
      variant="line"
      class="section-nav h-auto w-full min-w-0 max-w-full justify-start overflow-hidden rounded-md border border-border bg-card p-1"
      aria-label="Settings sections"
    >
      {#each settingsSections as section}
        {@const Icon = sectionIcons[section.id]}
        <Tabs.Trigger
          id={`settings-tab-${section.id}`}
          value={section.id}
          class="settings-tab h-8 min-w-28 justify-start gap-1.5 rounded px-2 text-left data-active:bg-accent"
          aria-controls={`settings-panel-${section.id}`}
        >
          <Icon class="size-4 shrink-0" strokeWidth={1.8} aria-hidden="true" />
          <span class="copy"><strong>{section.label}</strong></span>
        </Tabs.Trigger>
      {/each}
    </Tabs.List>
  </Tabs.Root>
</div>

<style>
  .settings-nav-shell { min-width: 0; }
  .settings-nav-shell :global(.section-nav) { display: flex; flex-wrap: wrap; align-items: stretch; }
  .settings-nav-shell :global(.settings-tab) { flex: 1 1 calc(20% - var(--space-1)); }
  .copy, .copy strong { display: block; min-width: 0; }
  .copy { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .copy strong { color: inherit; font-size: var(--font-size-sm); font-weight: 650; }

  @media (max-height: 650px) {
    .settings-nav-shell :global(.settings-tab) { height: 28px; }
  }
</style>
