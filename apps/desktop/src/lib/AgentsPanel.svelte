<script lang="ts">
  import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
  import { onMount } from 'svelte';

  import { agentStatusPresentation } from './agentStatus';
  import AgentStatusIndicator from './components/ds/AgentStatusIndicator.svelte';
  import { submitOnEnter } from './formInputConventions';
  import TerminalView from './TerminalView.svelte';
  import {
    getAgentToolsStore,
    parseExtraArgs,
    type AgentTool,
    type AgentToolInput,
    type AgentToolsSnapshot
  } from './agentTools';
  import type { ProcessView } from './daemon';
  import type { AgentsPanelProps } from './workspace';
  import { projectDisplayName } from './worktrees';

  let {
    client,
    project,
    processes,
    selectedProcessId,
    spawnSignal,
    connected,
    onSelectProcess,
    onError
  }: AgentsPanelProps = $props();

  // The App owns one DaemonClient for the lifetime of this mounted section.
  // svelte-ignore state_referenced_locally
  const toolStore = getAgentToolsStore(client);
  let toolSnapshot = $state<AgentToolsSnapshot>(toolStore.current());
  let editingTool = $state<AgentToolInput | null>(null);
  let spawnTool = $state<AgentTool | null>(null);
  let launchName = $state('');
  let launchArgs = $state('');
  let prompt = $state('');
  let saving = $state(false);
  let launching = $state(false);
  let prompting = $state(false);
  let closingId = $state<number | null>(null);
  let spawnNameInput = $state<HTMLInputElement>();
  let toolNameInput = $state<HTMLInputElement>();
  let mounted = false;
  let seenSpawnSignal: number | null = null;
  let deferredSpawn = false;

  const agents = $derived(
    processes.filter((process) => process.kind === 'agent').sort((a, b) => b.id - a.id)
  );
  const selectedAgent = $derived(
    agents.find((process) => process.id === selectedProcessId) ?? null
  );
  const activeAgents = $derived(
    agents.filter((process) => process.status === 'running' || process.status === 'starting').length
  );
  const attentionCount = $derived(
    agents.filter((process) => process.agent_state.needs_input).length
  );

  onMount(() => {
    mounted = true;
    const unsubscribe = toolStore.subscribe((snapshot) => {
      toolSnapshot = snapshot;
      if (deferredSpawn && !snapshot.loading) {
        deferredSpawn = false;
        openDefaultSpawn();
      }
    });
    void toolStore.refresh().catch(reportCause);
    return () => {
      mounted = false;
      unsubscribe();
    };
  });

  $effect(() => {
    const signal = spawnSignal;
    if (seenSpawnSignal === null) {
      seenSpawnSignal = signal;
    } else if (mounted && signal !== seenSpawnSignal) {
      seenSpawnSignal = signal;
      if (toolSnapshot.loading || toolSnapshot.tools.length === 0) deferredSpawn = true;
      else openDefaultSpawn();
    }
  });

  function reportCause(cause: unknown): void {
    onError(cause instanceof Error ? cause.message : String(cause));
  }

  function openDefaultSpawn(): void {
    const tool = toolSnapshot.tools.find((candidate) => candidate.enabled);
    if (!tool) {
      if (toolSnapshot.tools.length === 0) openEditor();
      else onError('Enable an agent tool before starting an agent');
      return;
    }
    openSpawn(tool);
  }

  function openEditor(tool?: AgentTool): void {
    if (tool?.source === 'config') {
      onError(`${tool.name} is managed by the per-user config file`);
      return;
    }
    editingTool = tool
      ? { ...tool }
      : { name: '', command: '', tool_type: 'custom', enabled: true };
    queueMicrotask(() => toolNameInput?.focus());
  }

  async function saveTool(): Promise<void> {
    if (!editingTool || saving) return;
    saving = true;
    try {
      await toolStore.save(editingTool);
      editingTool = null;
    } catch (cause) {
      reportCause(cause);
    } finally {
      saving = false;
    }
  }

  async function toggleTool(tool: AgentTool): Promise<void> {
    if (tool.source === 'config') return;
    try {
      await toolStore.save({ ...tool, enabled: !tool.enabled });
    } catch (cause) {
      reportCause(cause);
    }
  }

  async function removeTool(tool: AgentTool): Promise<void> {
    if (tool.source === 'config') return;
    if (!window.confirm(`Delete the ${tool.name} agent tool?`)) return;
    try {
      await toolStore.remove(tool.id);
    } catch (cause) {
      reportCause(cause);
    }
  }

  function openSpawn(tool: AgentTool): void {
    if (!tool.enabled) {
      onError(`${tool.name} is disabled`);
      return;
    }
    spawnTool = tool;
    launchName = '';
    launchArgs = '';
    queueMicrotask(() => spawnNameInput?.focus());
  }

  async function spawnAgent(): Promise<void> {
    if (!spawnTool || launching) return;
    let extraArgs: string[];
    try {
      extraArgs = parseExtraArgs(launchArgs);
    } catch (cause) {
      reportCause(cause);
      return;
    }
    launching = true;
    try {
      const result = await client.spawnAgent({
        project_id: project.id,
        agent_tool_id: spawnTool.id,
        name: launchName.trim() || undefined,
        extra_args: extraArgs
      });
      spawnTool = null;
      onSelectProcess(result.process_id);
    } catch (cause) {
      reportCause(cause);
    } finally {
      launching = false;
    }
  }

  async function submitPrompt(): Promise<void> {
    const agent = selectedAgent;
    const content = prompt;
    if (!agent || !content || prompting) return;
    prompting = true;
    try {
      await client.submitInput(agent.id, content);
      prompt = '';
    } catch (cause) {
      reportCause(cause);
    } finally {
      prompting = false;
    }
  }

  async function closeAgent(agent: ProcessView): Promise<void> {
    if (closingId !== null) return;
    closingId = agent.id;
    try {
      await client.closeProcess(agent.id);
    } catch (cause) {
      reportCause(cause);
    } finally {
      closingId = null;
    }
  }

  function stateLabel(agent: ProcessView): string {
    return agentStatusPresentation(agent).shortLabel;
  }
</script>

<section class="agents-workspace" aria-label="Agents">
  <div class="registry-column">
    <header class="section-intro">
      <div>
        <span class="eyebrow">Agent registry</span>
        <h2>Choose the hands for this workspace.</h2>
        <p>Commands stay local. Every launch gets its own terminal and attention signal.</p>
      </div>
      <button class="add-tool" type="button" onclick={() => openEditor()} disabled={!connected}>
        <span aria-hidden="true">+</span> Add tool
      </button>
    </header>

    <div class="tool-list" aria-busy={toolSnapshot.loading}>
      {#if toolSnapshot.loading && toolSnapshot.tools.length === 0}
        <div class="empty-card">Reading the local agent registry…</div>
      {:else if toolSnapshot.tools.length === 0}
        <button class="empty-card action" type="button" onclick={() => openEditor()}>
          <strong>No agent tools configured</strong>
          <span>Add the command for Codex, Claude, Gemini, OpenCode, or another local agent.</span>
        </button>
      {:else}
        {#each toolSnapshot.tools as tool (tool.id)}
          <article class="tool-card" class:disabled={!tool.enabled}>
            <div class="tool-mark" aria-hidden="true">{tool.name.slice(0, 2).toUpperCase()}</div>
            <div class="tool-copy">
              <div class="tool-heading">
                <strong>{tool.name}</strong>
                <span>{tool.tool_type.replaceAll('_', ' ')}</span>
                {#if tool.source === 'config'}<span>config</span>{/if}
              </div>
              <code>{tool.command}</code>
            </div>
            <div class="tool-actions">
              <button
                class="toggle"
                class:on={tool.enabled}
                type="button"
                role="switch"
                aria-checked={tool.enabled}
                aria-label={`${tool.enabled ? 'Disable' : 'Enable'} ${tool.name}`}
                disabled={!connected || tool.source === 'config'}
                onclick={() => toggleTool(tool)}
              ><span></span></button>
              <button type="button" disabled={tool.source === 'config'} onclick={() => openEditor(tool)}>Edit</button>
              <button class="spawn" type="button" onclick={() => openSpawn(tool)} disabled={!connected || !tool.enabled}>
                Spawn
              </button>
            </div>
          </article>
        {/each}
      {/if}
    </div>
  </div>

  <div class="mission-column">
    <header class="mission-header">
      <div>
        <span class="eyebrow">Live roster</span>
        <strong>{activeAgents} active</strong>
      </div>
      {#if attentionCount > 0}
        <span class="attention-summary" title={`${attentionCount} agents need input`}><CircleAlertIcon size={12} aria-hidden="true" />{attentionCount} need input</span>
      {:else}
        <span class="quiet-summary">All clear</span>
      {/if}
    </header>

    {#if agents.length === 0}
      <div class="roster-empty">
        <div class="radar" aria-hidden="true"><i></i></div>
        <strong>No agents on deck</strong>
        <p>Spawn a configured tool to give it a terminal in {projectDisplayName(project)}.</p>
        <button type="button" onclick={openDefaultSpawn} disabled={!connected}>Spawn an agent</button>
      </div>
    {:else}
      <div class="agent-roster">
        {#each agents as agent (agent.id)}
          <article class="agent-row" class:selected={agent.id === selectedProcessId}>
            <button class="agent-primary" type="button" onclick={() => onSelectProcess(agent.id)}>
              <AgentStatusIndicator process={agent} size="lg" />
              <span class="agent-copy">
                <strong>{agent.name}</strong>
                <small>{agent.agent_state.tool_type?.replaceAll('_', ' ') ?? 'agent'} · #{agent.id}</small>
              </span>
              <span class="agent-state" data-state={agentStatusPresentation(agent).state}>{stateLabel(agent)}</span>
            </button>
            <button
              class="close-agent"
              type="button"
              disabled={closingId !== null}
              aria-label={`Close ${agent.name}`}
              title="Close agent session"
              onclick={() => closeAgent(agent)}
            >×</button>
          </article>
        {/each}
      </div>
    {/if}
  </div>

  {#if selectedAgent}
    <section class="agent-console" aria-label={`${selectedAgent.name} console`}>
      <TerminalView client={client} process={selectedAgent} {connected} {onError} />
      <form class="prompt-composer" onsubmit={(event) => { event.preventDefault(); void submitPrompt(); }}>
        <label for="agent-prompt">
          <span>{selectedAgent.agent_state.needs_input ? 'Reply requested' : 'Prompt agent'}</span>
          <small>Enter sends · Shift+Enter adds a line</small>
        </label>
        <textarea
          id="agent-prompt"
          bind:value={prompt}
          rows="2"
          placeholder={selectedAgent.agent_state.needs_input ? 'Answer the agent or trust prompt…' : `Send a prompt to ${selectedAgent.name}…`}
          disabled={!connected || selectedAgent.status !== 'running'}
          use:submitOnEnter
        ></textarea>
        <button type="submit" disabled={!connected || selectedAgent.status !== 'running' || !prompt || prompting}>
          {prompting ? 'Sending…' : 'Send'}
          <span aria-hidden="true">↵</span>
        </button>
      </form>
    </section>
  {/if}
</section>

{#if editingTool}
  <div class="dialog-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) editingTool = null; }}>
    <form class="dialog" aria-label={editingTool.id ? 'Edit agent tool' : 'Add agent tool'} onsubmit={(event) => { event.preventDefault(); void saveTool(); }}>
      <header>
        <div>
          <span class="eyebrow">Registry entry</span>
          <h3>{editingTool.id ? `Edit ${editingTool.name}` : 'Add an agent tool'}</h3>
        </div>
        <button type="button" aria-label="Close" onclick={() => editingTool = null}>×</button>
      </header>
      <label>
        <span>Name</span>
        <input bind:this={toolNameInput} bind:value={editingTool.name} required placeholder="Aider" />
      </label>
      <label>
        <span>Command</span>
        <input bind:value={editingTool.command} required autocapitalize="off" autocorrect="off" spellcheck={false} placeholder="aider --model sonnet" />
        <small>Include default flags here. Per-launch arguments are appended safely.</small>
      </label>
      <label>
        <span>Tool type</span>
        <input bind:value={editingTool.tool_type} required autocapitalize="off" autocorrect="off" spellcheck={false} placeholder="custom" />
      </label>
      <label class="enabled-field">
        <input type="checkbox" bind:checked={editingTool.enabled} />
        <span>Available for new agents</span>
      </label>
      <footer>
        {#if editingTool.id}
          <button class="delete" type="button" onclick={() => { const tool = editingTool as AgentTool; editingTool = null; void removeTool(tool); }}>Delete</button>
        {/if}
        <span></span>
        <button type="button" onclick={() => editingTool = null}>Cancel</button>
        <button class="primary" type="submit" disabled={saving}>{saving ? 'Saving…' : 'Save tool'}</button>
      </footer>
    </form>
  </div>
{/if}

{#if spawnTool}
  <div class="dialog-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) spawnTool = null; }}>
    <form class="dialog spawn-dialog" aria-label={`Spawn ${spawnTool.name}`} onsubmit={(event) => { event.preventDefault(); void spawnAgent(); }}>
      <header>
        <div>
          <span class="eyebrow">New agent</span>
          <h3>Launch {spawnTool.name}</h3>
        </div>
        <button type="button" aria-label="Close" onclick={() => spawnTool = null}>×</button>
      </header>
      <div class="launch-command"><span>$</span><code>{spawnTool.command}</code></div>
      <label>
        <span>Session name <i>optional</i></span>
        <input bind:this={spawnNameInput} bind:value={launchName} placeholder={`${spawnTool.name.toLowerCase()} worker`} />
      </label>
      <label>
        <span>Extra arguments <i>optional</i></span>
        <input bind:value={launchArgs} autocapitalize="off" autocorrect="off" spellcheck={false} placeholder='--model "gpt-5"' />
        <small>Quotes group one argument. Workman passes each value without shell reinterpretation.</small>
      </label>
      <footer>
        <span></span><span></span>
        <button type="button" onclick={() => spawnTool = null}>Cancel</button>
        <button class="primary" type="submit" disabled={launching}>{launching ? 'Launching…' : 'Launch agent'}</button>
      </footer>
    </form>
  </div>
{/if}

<style>
  .agents-workspace {
    display: grid;
    min-height: 0;
    grid-template-columns: minmax(360px, 0.9fr) minmax(320px, 0.72fr);
    gap: 10px;
    padding: 12px 16px 20px;
    color: #dfe2e5;
  }

  .registry-column,
  .mission-column,
  .agent-console {
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
  }

  .section-intro,
  .mission-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    border-bottom: 1px solid var(--border);
    padding: 11px 12px 10px;
  }

  .eyebrow,
  h2,
  h3,
  .section-intro p,
  .add-tool,
  .tool-heading span,
  code,
  .tool-actions button,
  .agent-copy small,
  .agent-state,
  .attention-summary,
  .quiet-summary,
  .prompt-composer,
  .dialog label,
  .dialog footer,
  .launch-command {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  .eyebrow {
    display: block;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.13em;
    text-transform: uppercase;
  }

  h2,
  h3 {
    margin: 3px 0 0;
    color: var(--foreground);
    font-size: 15px;
    font-weight: 620;
    letter-spacing: -0.025em;
  }

  .section-intro p {
    max-width: 520px;
    margin: 4px 0 0;
    color: var(--text-soft);
    font-size: var(--font-size-sm);
    line-height: 1.45;
  }

  button,
  input,
  textarea {
    font: inherit;
  }

  button:focus-visible,
  input:focus-visible,
  textarea:focus-visible {
    outline: 2px solid var(--muted-foreground);
    outline-offset: 2px;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }

  .add-tool,
  .roster-empty button {
    flex: 0 0 auto;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    padding: 6px 8px;
    color: var(--foreground);
    background: var(--accent);
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .add-tool span {
    margin-right: 4px;
    color: #a4abb3;
  }

  .tool-list,
  .agent-roster {
    max-height: min(390px, 44vh);
    overflow-y: auto;
    scrollbar-color: #2b4353 transparent;
    scrollbar-width: thin;
  }

  .tool-card {
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    border-bottom: 1px solid var(--border);
    padding: 6px 9px;
    transition: background 120ms ease, opacity 120ms ease;
  }

  .tool-card:last-child,
  .agent-row:last-child {
    border-bottom: 0;
  }

  .tool-card:hover {
    background: var(--popover);
  }

  .tool-card.disabled {
    opacity: 0.52;
  }

  .tool-mark {
    display: grid;
    width: 29px;
    height: 29px;
    place-items: center;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    color: #b9bec5;
    background: var(--accent);
    font: 700 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace;
    letter-spacing: 0.04em;
  }

  .tool-copy,
  .tool-heading,
  .tool-heading strong,
  .tool-heading span,
  .tool-copy code {
    min-width: 0;
  }

  .tool-heading {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .tool-heading strong {
    overflow: hidden;
    color: var(--foreground);
    font-size: 12px;
    font-weight: 620;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tool-heading span {
    color: #7d848d;
    font-size: var(--font-size-xs);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .tool-copy code {
    display: block;
    overflow: hidden;
    margin-top: 2px;
    color: #888f98;
    font-size: var(--font-size-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tool-actions {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .tool-actions button:not(.toggle) {
    border: 1px solid transparent;
    border-radius: 3px;
    padding: 5px 7px;
    color: var(--text-soft);
    background: transparent;
    font-size: var(--font-size-xs);
    font-weight: 650;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .tool-actions button:hover:not(:disabled) {
    border-color: #38515b;
    color: #c7d6da;
    background: #17282f;
  }

  .tool-actions .spawn {
    border-color: #4b5057 !important;
    color: #d8dbde !important;
    background: var(--accent) !important;
  }

  .toggle {
    position: relative;
    width: 27px;
    height: 15px;
    border: 1px solid #3a4b51;
    border-radius: 10px;
    padding: 0;
    background: #18252a;
    cursor: pointer;
  }

  .toggle span {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: #67777b;
    transition: transform 140ms ease, background 140ms ease;
  }

  .toggle.on {
    border-color: #3e746f;
    background: #17322f;
  }

  .toggle.on span {
    transform: translateX(12px);
    background: var(--signal);
  }

  .mission-header {
    min-height: 52px;
    align-items: center;
  }

  .mission-header strong {
    display: block;
    margin-top: 3px;
    color: #e7e9eb;
    font-size: 12px;
    font-weight: 620;
  }

  .attention-summary,
  .quiet-summary {
    display: flex;
    align-items: center;
    gap: 6px;
    color: #83969e;
    font-size: var(--font-size-xs);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .attention-summary {
    color: #dfb46b;
  }

  .agent-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 34px;
    border-bottom: 1px solid var(--border);
  }

  .agent-row.selected {
    background: var(--accent);
    box-shadow: inset 2px 0 var(--muted-foreground);
  }

  .agent-primary {
    display: grid;
    min-width: 0;
    grid-template-columns: 28px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    border: 0;
    padding: 8px 7px 8px 10px;
    color: inherit;
    background: transparent;
    text-align: left;
    cursor: pointer;
  }

  .agent-primary:hover {
    background: var(--popover);
  }

  .agent-copy,
  .agent-copy strong,
  .agent-copy small {
    display: block;
    min-width: 0;
  }

  .agent-copy strong {
    overflow: hidden;
    color: var(--foreground);
    font-size: 12px;
    font-weight: 610;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .agent-copy small {
    margin-top: 2px;
    color: #7f868f;
    font-size: var(--font-size-xs);
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .agent-state {
    color: #8c939c;
    font-size: var(--font-size-xs);
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .agent-state[data-state='working'] { color: var(--success); }
  .agent-state[data-state='needs_input'] { color: var(--warning-token); }
  .agent-state[data-state='waiting'] { color: var(--information); }
  .agent-state[data-state='exited'] { color: var(--destructive); }

  .close-agent {
    align-self: center;
    width: 25px;
    height: 25px;
    border: 1px solid transparent;
    border-radius: 3px;
    color: #667b84;
    background: transparent;
    font: 500 14px/1 'JetBrains Mono Variable', monospace;
    cursor: pointer;
  }

  .close-agent:hover:not(:disabled) {
    border-color: #694447;
    color: #dc8c87;
    background: #2b2024;
  }

  .agent-console {
    display: grid;
    min-height: 420px;
    grid-column: 1 / -1;
    grid-template-rows: minmax(340px, 1fr) auto;
    overflow: hidden;
    padding: 7px;
  }

  .prompt-composer {
    display: grid;
    grid-template-columns: 150px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    border: 1px solid #29444b;
    border-radius: 4px;
    margin-top: 6px;
    padding: 7px 8px;
    background: var(--popover);
  }

  .prompt-composer label span,
  .prompt-composer label small {
    display: block;
  }

  .prompt-composer label span {
    color: var(--text-soft);
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .prompt-composer label small {
    margin-top: 4px;
    color: #58727c;
    font-size: var(--font-size-xs);
  }

  .prompt-composer textarea {
    min-height: 38px;
    max-height: 168px;
    resize: none;
    border: 1px solid #334e56;
    border-radius: 3px;
    outline: 0;
    padding: 8px 9px;
    color: #dce7e9;
    background: #0b181d;
    font: 500 var(--font-size-sm)/1.4 'JetBrains Mono Variable', monospace;
  }

  .prompt-composer button {
    display: flex;
    align-items: center;
    gap: 8px;
    border: 1px solid #4b5057;
    border-radius: 3px;
    padding: 7px 9px;
    color: var(--foreground);
    background: #292c31;
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .prompt-composer button span {
    color: #a8aeb6;
    font-size: 12px;
  }

  .empty-card,
  .roster-empty {
    color: #708792;
    font: 500 var(--font-size-sm)/1.6 'JetBrains Mono Variable', monospace;
  }

  .empty-card {
    display: block;
    width: 100%;
    border: 0;
    padding: 18px 12px;
    background: transparent;
    text-align: left;
  }

  .empty-card.action {
    cursor: pointer;
  }

  .empty-card strong,
  .empty-card span {
    display: block;
  }

  .empty-card strong {
    color: var(--text-soft);
    font-size: var(--font-size-sm);
  }

  .empty-card span {
    margin-top: 5px;
  }

  .roster-empty {
    display: grid;
    min-height: 170px;
    place-items: center;
    align-content: center;
    padding: 18px;
    text-align: center;
  }

  .roster-empty strong {
    margin-top: 14px;
    color: #c2d0d4;
    font-size: var(--font-size-sm);
  }

  .roster-empty p {
    max-width: 300px;
    margin: 5px 0 15px;
  }

  .radar {
    position: relative;
    display: grid;
    width: 44px;
    height: 44px;
    place-items: center;
    border: 1px solid #34505a;
    border-radius: 50%;
  }

  .radar::after,
  .radar i {
    content: '';
    position: absolute;
    background: #34505a;
  }

  .radar::after {
    width: 1px;
    height: 100%;
  }

  .radar i {
    width: 100%;
    height: 1px;
  }

  .dialog-backdrop {
    position: fixed;
    z-index: 80;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 16px;
    background: rgb(4 5 6 / 74%);
  }

  .dialog {
    width: min(500px, 100%);
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 0;
    color: var(--foreground);
    background: var(--popover);
    box-shadow: 0 18px 55px rgb(0 0 0 / 42%);
  }

  .dialog header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    border-bottom: 1px solid #2b414b;
    padding: 16px 17px 14px;
  }

  .dialog h3 {
    font-size: 14px;
  }

  .dialog header button {
    width: 27px;
    height: 27px;
    border: 1px solid transparent;
    border-radius: 3px;
    color: #758a93;
    background: transparent;
    font: 500 15px/1 'JetBrains Mono Variable', monospace;
    cursor: pointer;
  }

  .dialog header button:hover {
    border-color: #435962;
    color: #d2dee1;
  }

  .dialog > label {
    display: block;
    padding: 12px 17px 0;
  }

  .dialog > label > span {
    display: block;
    margin-bottom: 6px;
    color: #8299a2;
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .dialog label i {
    color: #536d77;
    font-style: normal;
    font-weight: 500;
  }

  .dialog input:not([type='checkbox']) {
    width: 100%;
    box-sizing: border-box;
    border: 1px solid #344d57;
    border-radius: 3px;
    outline: 0;
    padding: 9px 10px;
    color: #dce6e9;
    background: #0b181e;
    font: 500 var(--font-size-sm)/1.2 'JetBrains Mono Variable', monospace;
  }

  .dialog label small {
    display: block;
    margin-top: 5px;
    color: #58717b;
    font-size: var(--font-size-xs);
    line-height: 1.5;
  }

  .dialog .enabled-field {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .dialog .enabled-field span {
    margin: 0;
    color: #9bafb6;
  }

  .dialog footer {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
    gap: 7px;
    margin-top: 15px;
    border-top: 1px solid #2b414b;
    padding: 11px 13px;
  }

  .dialog footer button {
    border: 1px solid #3b5058;
    border-radius: 3px;
    padding: 8px 10px;
    color: #94a8af;
    background: #172830;
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .dialog footer .primary {
    border-color: #3d766f;
    color: #c1dfda;
    background: #17322f;
  }

  .dialog footer .delete {
    border-color: #654348;
    color: #cd8d88;
    background: #281d22;
  }

  .launch-command {
    display: flex;
    gap: 8px;
    margin: 14px 17px 1px;
    border: 1px solid #2c4850;
    border-radius: 3px;
    padding: 9px 10px;
    color: #789099;
    background: #0b181d;
    font-size: var(--font-size-sm);
  }

  .launch-command span {
    color: var(--signal);
  }

  @media (max-width: 920px) {
    .agents-workspace {
      grid-template-columns: 1fr;
    }

    .agent-console {
      grid-column: 1;
    }
  }

  @media (max-width: 620px) {
    .agents-workspace {
      padding: 12px;
    }

    .section-intro {
      align-items: center;
    }

    .section-intro p,
    .tool-heading span,
    .agent-state {
      display: none;
    }

    .tool-card {
      grid-template-columns: 30px minmax(0, 1fr);
    }

    .tool-actions {
      grid-column: 1 / -1;
      justify-content: flex-end;
    }

    .prompt-composer {
      grid-template-columns: 1fr auto;
    }

    .prompt-composer label {
      grid-column: 1 / -1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .toggle span,
    .tool-card {
      transition: none;
    }
  }
</style>
