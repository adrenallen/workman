<script lang="ts">
  import SettingsPanel from '../../src/lib/SettingsPanel.svelte';
  import WelcomeProject from '../../src/lib/WelcomeProject.svelte';
  import type { DaemonClient } from '../../src/lib/daemon';
  import { selectSettingsSection } from '../../src/lib/settingsSections';
  const query = new URLSearchParams(location.search);
  const welcome = query.has('welcome');
  selectSettingsSection('templates');
  const tools = ['Codex', 'Claude'].map((name, i) => ({ id: i + 1, name, tool_type: name.toLowerCase(), command: name.toLowerCase(), enabled: true, source: 'local', resume_args: null, continue_args: null, icon_data_url: null }));
  let templates = [
    { id: 1, profile_id: 1, name: 'Orchestrator Agent', agent_tool_id: 2, extra_args: ['--model', 'opus'], prompt: 'Plan the work, coordinate focused tasks, and review the result before reporting back.', sort_order: 0, created_at: 0, updated_at: 0 },
    { id: 2, profile_id: 1, name: 'General Agent', agent_tool_id: 1, extra_args: [], prompt: 'Investigate the task, implement a focused solution, and verify the behavior.', sort_order: 1, created_at: 0, updated_at: 0 }
  ];
  const client = {
    control: async () => { throw new Error('No daemon in layout fixture'); },
    listAgentTools: async () => tools,
    listAgentTemplates: async () => templates,
    reorderAgentTemplates: async (ids: number[]) => { templates = ids.map(id => templates.find(t => t.id === id)!); return templates; },
    saveAgentTemplate: async (draft: typeof templates[number]) => { templates = templates.map(t => t.id === draft.id ? {...t, ...draft} : t); return draft; }
  } as unknown as DaemonClient;
  let action = $state('');
</script>
<div class="fixture">
  {#if welcome}
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
