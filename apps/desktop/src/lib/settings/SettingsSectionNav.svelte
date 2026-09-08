<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import BellIcon from '@lucide/svelte/icons/bell';
  import InfoIcon from '@lucide/svelte/icons/info';
  import KeyboardIcon from '@lucide/svelte/icons/keyboard';
  import NotebookTabsIcon from '@lucide/svelte/icons/notebook-tabs';
  import MonitorIcon from '@lucide/svelte/icons/monitor';
  import MessageSquareTextIcon from '@lucide/svelte/icons/message-square-text';
  import Mic2Icon from '@lucide/svelte/icons/mic-2';
  import PanelLeftIcon from '@lucide/svelte/icons/panel-left';
  import PaletteIcon from '@lucide/svelte/icons/palette';
  import PlugIcon from '@lucide/svelte/icons/plug';
  import ServerIcon from '@lucide/svelte/icons/server';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import { tick, type Component } from 'svelte';
  import * as Select from '$lib/components/ui/select';
  import * as Tabs from '$lib/components/ui/tabs';
  import {
    selectSettingsSection,
    settingsSection,
    settingsSections,
    type SettingsSectionId
  } from '../settingsSections';

  const sectionIcons: Record<SettingsSectionId, Component> = {
    appearance: PaletteIcon,
    profiles: ArchiveIcon,
    terminal: SquareTerminalIcon,
    sidebar: PanelLeftIcon,
    hotkeys: KeyboardIcon,
    notifications: BellIcon,
    feedback: Mic2Icon,
    templates: NotebookTabsIcon,
    agents: BotIcon,
    'quick-prompts': MessageSquareTextIcon,
    tools: MonitorIcon,
    mcp: PlugIcon,
    daemon: ServerIcon,
    about: InfoIcon
  };

  const groups: { label: string; ids: SettingsSectionId[] }[] = [
    { label: 'Workspace', ids: ['appearance', 'terminal', 'sidebar', 'hotkeys', 'notifications', 'feedback'] },
    { label: 'Agents', ids: ['templates', 'agents', 'quick-prompts'] },
    { label: 'Application', ids: ['profiles', 'tools', 'mcp', 'daemon', 'about'] }
  ];
  let nav = $state<HTMLElement | null>(null);
  $effect(() => {
    const section = $settingsSection;
    const element = nav;
    let cancelled = false;
    void tick().then(() => {
      const tab = element?.querySelector<HTMLElement>(`#settings-tab-${section}`);
      if (!cancelled && tab?.offsetParent) tab.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    });
    return () => { cancelled = true; };
  });
  let selected = $derived(settingsSections.find((section) => section.id === $settingsSection)!);

  function choose(value: string | undefined): void {
    if (settingsSections.some((section) => section.id === value)) {
      selectSettingsSection(value as SettingsSectionId);
    }
  }
</script>

<nav bind:this={nav} class="settings-nav-shell" aria-label="Settings sections">
  <div class="compact-nav">
    <Select.Root type="single" value={$settingsSection} onValueChange={choose}>
      <Select.Trigger class="w-full" aria-label="Settings section">{selected.label}</Select.Trigger>
      <Select.Content class="settings-section-options">
        {#each groups as group}
          <Select.Group>
            <Select.Label>{group.label}</Select.Label>
            {#each group.ids as id}
              {@const section = settingsSections.find((item) => item.id === id)!}
              <Select.Item value={id} label={section.label}>{section.label}</Select.Item>
            {/each}
          </Select.Group>
        {/each}
      </Select.Content>
    </Select.Root>
  </div>
  <div class="full-nav">
    <Tabs.Root orientation="vertical" value={$settingsSection} onValueChange={choose}>
      <Tabs.List variant="line" class="section-nav" aria-label="Settings sections">
        {#each groups as group}
          <span class="nav-group" aria-hidden="true">{group.label}</span>
          {#each group.ids as id}
            {@const section = settingsSections.find((item) => item.id === id)!}
            {@const Icon = sectionIcons[id]}
            <Tabs.Trigger id={`settings-tab-${id}`} value={id} class="settings-tab" aria-controls={`settings-panel-${id}`} title={section.description}>
              <Icon class="size-4 shrink-0" strokeWidth={1.8} aria-hidden="true" />
              <span>{section.label}</span>
            </Tabs.Trigger>
          {/each}
        {/each}
      </Tabs.List>
    </Tabs.Root>
  </div>
</nav>

<style>
  .settings-nav-shell { min-width: 0; height: 100%; overflow: auto; padding: 0 14px 16px 0; }
  .compact-nav { display: none; }
  .settings-nav-shell :global(.section-nav) { display: flex; width: 100%; height: auto; flex-direction: column; align-items: stretch; gap: 3px; padding: 0; }
  .nav-group { padding: 20px 10px 8px; color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 550; }
  .nav-group:first-child { padding-top: 0; }
  .settings-nav-shell :global(.settings-tab) { flex: none; justify-content: flex-start; min-width: 0; height: 36px; gap: 10px; border: 0; padding: 0 10px; color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 550; }
  .settings-nav-shell :global(.settings-tab::after) { display: none; }
  .settings-nav-shell :global(.settings-tab:hover) { background: var(--accent); color: var(--foreground); }
  .settings-nav-shell :global(.settings-tab[data-state='active']), .settings-nav-shell :global(.settings-tab[data-active]) { background: var(--brand-surface); color: var(--foreground); box-shadow: inset 3px 0 var(--brand-blue); }
  .settings-nav-shell :global(.settings-tab[data-state='active'] svg), .settings-nav-shell :global(.settings-tab[data-active] svg) { color: var(--brand-blue); }
  @container settings (max-width: 720px) {
    .settings-nav-shell { height: auto; padding: 0; overflow: visible; }
    .full-nav { display: none; }
    .compact-nav { display: block; }
  }
</style>
