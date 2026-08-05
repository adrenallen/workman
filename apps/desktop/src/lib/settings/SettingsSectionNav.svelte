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
  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import {
    selectSettingsSection,
    settingsSection,
    settingsSections,
    type SettingsSectionId
  } from '../settingsSections';

  interface Props {
    connected: boolean;
  }

  let { connected }: Props = $props();

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

<Tabs.Root class="w-full min-w-0 max-w-full overflow-hidden" value={$settingsSection} onValueChange={choose}>
  <Tabs.List
    variant="line"
    class="section-nav h-auto w-full min-w-0 max-w-full justify-start overflow-x-auto rounded-md border border-border bg-card p-1"
    aria-label="Settings sections"
  >
    {#each settingsSections as section}
      {@const Icon = sectionIcons[section.id]}
      <Tabs.Trigger
        id={`settings-tab-${section.id}`}
        value={section.id}
        class="settings-tab h-11 min-w-[8.5rem] flex-none justify-start gap-2 rounded px-2 text-left data-active:bg-accent"
        aria-controls={`settings-panel-${section.id}`}
      >
        <Icon class="size-4 shrink-0" strokeWidth={1.8} aria-hidden="true" />
        <span class="copy"><strong>{section.label}</strong><small>{section.description}</small></span>
        {#if !section.local}
          <StatusIndicator
            tone={connected ? 'success' : 'neutral'}
            label={connected ? `Daemon connected · ${section.label} settings available` : `Daemon disconnected · ${section.label} settings unavailable`}
          />
        {/if}
      </Tabs.Trigger>
    {/each}
  </Tabs.List>
</Tabs.Root>

<style>
  .copy, .copy strong, .copy small { display: block; min-width: 0; }
  .copy strong { color: inherit; font-size: var(--font-size-sm); font-weight: 650; }
  .copy small { overflow: hidden; margin-top: 1px; color: var(--muted); font: var(--font-size-xs)/1.2 'JetBrains Mono Variable', monospace; text-overflow: ellipsis; white-space: nowrap; }

  @media (max-width: 920px) {
    .copy small { display: none; }
  }
</style>
