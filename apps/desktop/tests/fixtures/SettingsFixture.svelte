<script lang="ts">
  import SettingsPanel from '../../src/lib/SettingsPanel.svelte';
  import TerminalAppearanceCard from '../../src/lib/settings/TerminalAppearanceCard.svelte';
  import NotificationsCard from '../../src/lib/settings/NotificationsCard.svelte';
  import AgentDoneToasts from '../../src/lib/AgentDoneToasts.svelte';
  import { nativeNotificationPreferences } from '../../src/lib/nativeNotifications';
  import type { UserEnvironmentInfo } from '../../src/lib/settings';
  import WelcomeProject from '../../src/lib/WelcomeProject.svelte';
  import type { DaemonClient, ProcessView } from '../../src/lib/daemon';
  import { selectSettingsSection } from '../../src/lib/settingsSections';
  const query = new URLSearchParams(location.search);
  const welcome = query.has('welcome');
  if (query.has('notifications')) nativeNotificationPreferences.set({ enabled: false, mode: 'top_level', needsInput: true, soundEnabled: false });
  const notificationProcesses = [
    { id: 1, kind: 'agent', project_id: 1, spawned_by_process_id: null },
    { id: 2, kind: 'agent', project_id: 1, spawned_by_process_id: 1 }
  ] as ProcessView[];
  let notices = $state([
    { id: 'parent-done', processId: 1, projectId: 1, name: 'Parent', kind: 'agent_done' as const },
    { id: 'child-done', processId: 2, projectId: 1, name: 'Child', kind: 'agent_done' as const },
    { id: 'parent-input', processId: 1, projectId: 1, name: 'Parent input', kind: 'needs_input' as const },
    { id: 'child-input', processId: 2, projectId: 1, name: 'Child input', kind: 'needs_input' as const }
  ]);
  selectSettingsSection('templates');
  const tools = ['Codex', 'Claude'].map((name, i) => ({ id: i + 1, name, tool_type: name.toLowerCase(), command: name.toLowerCase(), enabled: true, source: 'local', resume_args: null, continue_args: null, icon_data_url: null }));
  let templates = [
    { id: 1, profile_id: 1, name: 'Orchestrator Agent', agent_tool_id: 2, extra_args: ['--model', 'opus'], prompt: 'Plan the work, coordinate focused tasks, and review the result before reporting back.', sort_order: 0, created_at: 0, updated_at: 0 },
    { id: 2, profile_id: 1, name: 'General Agent', agent_tool_id: 1, extra_args: [], prompt: 'Investigate the task, implement a focused solution, and verify the behavior.', sort_order: 1, created_at: 0, updated_at: 0 }
  ];
  let typingPause = { enabled: true, delay_ms: 10_000 };
  const client = {
    control: async (method: string, params: typeof typingPause) => {
      if (method === 'settings.typing_pause_get') return typingPause;
      if (method === 'settings.typing_pause_update') {
        if (query.has('fail-save')) throw new Error('Fixture save failed');
        typingPause = params;
        action = JSON.stringify(params);
        return typingPause;
      }
      throw new Error('No daemon in layout fixture');
    },
    listAgentTools: async () => tools,
    listAgentTemplates: async () => templates,
    reorderAgentTemplates: async (ids: number[]) => { templates = ids.map(id => templates.find(t => t.id === id)!); return templates; },
    saveAgentTemplate: async (draft: typeof templates[number]) => { templates = templates.map(t => t.id === draft.id ? {...t, ...draft} : t); return draft; }
  } as unknown as DaemonClient;
  let action = $state('');
  let environment = $state<UserEnvironmentInfo>({
    active_shell: '/bin/zsh', configured_shell: null, inferred_shell: '/bin/zsh',
    inferred_from: 'account', using_override: false, capture_mode: 'interactive_login',
    resolved_path: '/usr/bin:/bin', capture_error: null, warning: null,
    agent_shell_mode: 'auto', agent_launch_summary: '/bin/zsh -l -i -c',
    agent_shell_mode_supported: !query.has('windows')
  });
</script>
<div class="fixture">
  {#if query.has('notifications')}
    <div><NotificationsCard /></div>
    <AgentDoneToasts {notices} processes={notificationProcesses} onOpen={() => {}}
      onDismiss={id => notices = notices.filter(notice => notice.id !== id)} />
  {:else if query.has('terminal-shell')}
    <TerminalAppearanceCard {client} {environment} connected={true}
      onShellChange={async shell => { environment = { ...environment, configured_shell: shell }; }}
      onAgentShellModeChange={async mode => {
        if (query.has('fail-save')) throw new Error('Fixture save failed');
        environment = { ...environment, agent_shell_mode: mode };
        action = mode;
      }} />
  {:else if welcome}
    <WelcomeProject connected={true} busy={false} onAddProject={() => action = 'add'} onProfiles={() => action = 'profiles'} />
  {:else}
    <SettingsPanel {client} project={null} connection={{status: 'connected', daemon_version: '0.1.14'}} updateFlow={{ kind: 'idle' }} onApplyUpdate={async () => {}} onRestartUpdate={async () => {}} onDismissUpdate={() => {}} onError={() => {}} onProfileSwitched={() => {}} />
  {/if}
</div>
<output hidden data-testid="action">{action}</output>
<style>
  :global(html), :global(body), :global(#app) { height: 100%; margin: 0; }
  .fixture { display: grid; width: 100%; height: 100%; min-height: 0; }
</style>
