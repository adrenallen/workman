<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';

  import AgentBrandMark from '../AgentBrandMark.svelte';
  import {
    getAgentToolsStore,
    type AgentTool,
    type AgentToolHealth,
    type AgentToolInput,
    type AgentToolsSnapshot
  } from '../agentTools';
  import type { DaemonClient } from '../daemon';

  interface Props {
    client: DaemonClient;
    connected: boolean;
    onError: (message: string) => void;
  }

  let { client, connected, onError }: Props = $props();
  let store = $derived(getAgentToolsStore(client));
  let snapshot = $state<AgentToolsSnapshot>({ tools: [], loading: false, error: null });
  let editing = $state<number | 'new' | null>(null);
  let draft = $state<AgentToolInput>(emptyDraft());
  let saving = $state(false);
  let busyId = $state<number | null>(null);
  let healthById = $state<Record<number, AgentToolHealth>>({});
  let editedTool = $derived(
    typeof editing === 'number'
      ? snapshot.tools.find((tool) => tool.id === editing) ?? null
      : null
  );

  $effect(() => {
    snapshot = store.current();
    return store.subscribe((next) => (snapshot = next));
  });

  $effect(() => {
    if (connected) {
      void store.refresh().catch((cause) => onError(message(cause)));
      void refreshHealth();
    }
  });

  function emptyDraft(): AgentToolInput {
    return { name: '', command: '', tool_type: 'custom', enabled: true };
  }

  function beginNew(): void {
    draft = emptyDraft();
    editing = 'new';
  }

  function beginEdit(tool: AgentTool): void {
    draft = {
      id: tool.id,
      name: tool.name,
      command: tool.command,
      tool_type: tool.tool_type,
      enabled: tool.enabled
    };
    editing = tool.id;
  }

  async function save(): Promise<void> {
    if (!draft.name.trim() || !draft.command.trim() || !draft.tool_type.trim()) return;
    saving = true;
    try {
      await store.save({
        ...draft,
        name: draft.name.trim(),
        command: draft.command.trim(),
        tool_type: draft.tool_type.trim()
      });
      editing = null;
      await refreshHealth();
    } catch (cause) {
      onError(message(cause));
    } finally {
      saving = false;
    }
  }

  async function toggle(tool: AgentTool): Promise<void> {
    busyId = tool.id;
    try {
      await store.save({ ...tool, enabled: !tool.enabled });
      await refreshHealth();
    } catch (cause) {
      onError(message(cause));
    } finally {
      busyId = null;
    }
  }

  async function remove(tool: AgentTool): Promise<void> {
    if (!window.confirm(`Delete the ${tool.name} agent tool? Existing agents may require it.`)) return;
    busyId = tool.id;
    try {
      await store.remove(tool.id);
      if (editing === tool.id) editing = null;
      await refreshHealth();
    } catch (cause) {
      onError(message(cause));
    } finally {
      busyId = null;
    }
  }

  async function chooseIcon(tool: AgentTool): Promise<void> {
    const sourcePath = await open({
      directory: false,
      multiple: false,
      title: `Choose an icon for ${tool.name}`,
      filters: [{
        name: 'Images',
        extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp', 'ico']
      }]
    });
    if (typeof sourcePath !== 'string') return;
    busyId = tool.id;
    try {
      await store.setIcon(tool.id, sourcePath);
    } catch (cause) {
      onError(message(cause));
    } finally {
      busyId = null;
    }
  }

  async function removeIcon(tool: AgentTool): Promise<void> {
    busyId = tool.id;
    try {
      await store.removeIcon(tool.id);
    } catch (cause) {
      onError(message(cause));
    } finally {
      busyId = null;
    }
  }

  async function move(tool: AgentTool, direction: -1 | 1): Promise<void> {
    const index = snapshot.tools.findIndex((candidate) => candidate.id === tool.id);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= snapshot.tools.length) return;
    busyId = tool.id;
    try {
      const reordered = [...snapshot.tools];
      [reordered[index], reordered[target]] = [reordered[target], reordered[index]];
      await store.reorder(reordered.map((candidate) => candidate.id));
    } catch (cause) {
      onError(message(cause));
    } finally {
      busyId = null;
    }
  }

  async function refreshHealth(): Promise<void> {
    try {
      const health = await client.agentToolsHealth();
      healthById = Object.fromEntries(health.tools.map((tool) => [tool.id, tool]));
    } catch (cause) {
      onError(message(cause));
    }
  }

  function healthLabel(tool: AgentTool): string {
    const health = healthById[tool.id];
    if (!tool.enabled) return 'Disabled';
    if (!health) return 'Checking…';
    if (!health.found_on_path) return 'Not found';
    return health.mcp_launch_supported ? 'Ready' : 'MCP unavailable';
  }

  function healthClass(tool: AgentTool): string {
    const health = healthById[tool.id];
    if (!tool.enabled || !health) return 'neutral';
    if (!health.found_on_path) return 'missing';
    return health.mcp_launch_supported ? 'ready' : 'degraded';
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="card agent-tools-card" aria-labelledby="agent-tools-title">
  <header>
    <div>
      <span class="eyebrow">Active profile</span>
      <h2 id="agent-tools-title">Agent tools</h2>
      <p>Edit the launch command, availability, custom mark, and order for the loaded profile.</p>
    </div>
    <button class="add" type="button" disabled={!connected || editing !== null} onclick={beginNew}>
      <span aria-hidden="true">+</span> Add tool
    </button>
  </header>

  {#if snapshot.loading && snapshot.tools.length === 0}
    <div class="loading"><i aria-hidden="true"></i> Reading the shared registry…</div>
  {:else if snapshot.tools.length === 0}
    <div class="empty">
      <span aria-hidden="true">⌘</span>
      <div><strong>No agent tools registered</strong><p>Add a command such as <code>claude</code> or <code>codex</code> to make it available to Agents.</p></div>
      <button type="button" disabled={!connected} onclick={beginNew}>Add the first tool</button>
    </div>
  {:else}
    <div class="tool-list">
      {#each snapshot.tools as tool, index (tool.id)}
        <article class:disabled={!tool.enabled}>
          <div class="order-actions" aria-label={`Reorder ${tool.name}`}>
            <button type="button" aria-label={`Move ${tool.name} up`} title="Move up" disabled={!connected || busyId !== null || index === 0} onclick={() => void move(tool, -1)}>↑</button>
            <button type="button" aria-label={`Move ${tool.name} down`} title="Move down" disabled={!connected || busyId !== null || index === snapshot.tools.length - 1} onclick={() => void move(tool, 1)}>↓</button>
          </div>
          <div class="tool-mark"><AgentBrandMark {tool} size={20} /></div>
          <div class="tool-copy">
            <div><strong>{tool.name}</strong><span>{tool.tool_type}</span>{#if tool.source === 'config'}<span>config</span>{/if}</div>
            <code>{tool.command}</code>
          </div>
          <span class={`health-badge ${healthClass(tool)}`} title={healthById[tool.id]?.mcp_launch_note ?? healthLabel(tool)}>{healthLabel(tool)}</span>
          <button
            class:enabled={tool.enabled}
            class="toggle"
            type="button"
            role="switch"
            aria-checked={tool.enabled}
            title={`${tool.name} · ${tool.enabled ? 'enabled' : 'disabled'}`}
            disabled={!connected || busyId !== null}
            onclick={() => void toggle(tool)}
          ><i aria-hidden="true"></i><span>{tool.enabled ? 'Enabled' : 'Disabled'}</span></button>
          <div class="row-actions">
            <button type="button" disabled={!connected || editing !== null || busyId !== null} onclick={() => beginEdit(tool)}>Edit</button>
            <button class="delete" type="button" disabled={!connected || busyId !== null} onclick={() => void remove(tool)}>Delete</button>
          </div>
        </article>
      {/each}
    </div>
  {/if}

  {#if editing !== null}
    <form
      class="editor"
      onsubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <header>
        <div><span class="eyebrow">Registry entry</span><h3>{editing === 'new' ? 'Add agent tool' : `Edit ${draft.name}`}</h3></div>
        <button type="button" class="close" aria-label="Close editor" onclick={() => (editing = null)}>×</button>
      </header>
      <div class="fields">
        <label><span>Name</span><input type="text" bind:value={draft.name} placeholder="Claude" /></label>
        <label><span>Tool type</span><input type="text" bind:value={draft.tool_type} list="agent-tool-types" placeholder="claude_code" autocapitalize="off" autocorrect="off" spellcheck={false} /></label>
        <label class="command"><span>Command and arguments</span><input type="text" bind:value={draft.command} placeholder="claude --dangerously-skip-permissions" autocapitalize="off" autocorrect="off" spellcheck={false} /></label>
        <datalist id="agent-tool-types"><option value="claude"></option><option value="claude_code"></option><option value="codex"></option><option value="gemini"></option><option value="opencode"></option><option value="kimi"></option><option value="grok"></option><option value="custom"></option></datalist>
        <label class="enabled-check"><input type="checkbox" bind:checked={draft.enabled} /><span>Available for new agents</span></label>
        <div class="icon-override">
          <span class="icon-preview"><AgentBrandMark tool={editedTool} fallbackName={draft.name || 'Agent'} fallbackToolType={draft.tool_type} size={26} /></span>
          <span class="icon-copy"><strong>List icon</strong><small>{editedTool?.icon_data_url ? 'Custom image override' : 'Automatic brand mark or monogram'}</small></span>
          <span class="icon-actions">
            <button type="button" disabled={!editedTool || busyId !== null} onclick={() => editedTool && void chooseIcon(editedTool)}>Choose image…</button>
            {#if editedTool?.icon_data_url}<button class="remove-icon" type="button" disabled={busyId !== null} onclick={() => void removeIcon(editedTool)}>Use default</button>{/if}
          </span>
        </div>
      </div>
      <footer>
        <p>Saved to the per-user config; unknown top-level settings remain untouched.</p>
        <div><button type="button" class="cancel" onclick={() => (editing = null)}>Cancel</button><button type="submit" class="save" disabled={saving || !draft.name.trim() || !draft.command.trim() || !draft.tool_type.trim()}>{saving ? 'Saving…' : 'Save tool'}</button></div>
      </footer>
    </form>
  {/if}
</section>

<style>
  .card { position: relative; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); }
  .card > header, .card > header > div, .add, .tool-copy > div, .toggle, .row-actions, .editor > header, .editor footer, .editor footer > div, .empty { display: flex; align-items: center; }
  .card > header { justify-content: space-between; gap: 12px; padding: 11px 12px 10px; }
  .card > header > div { flex-wrap: wrap; gap: 5px 10px; }
  .card > header p { width: 100%; margin: 0; color: var(--text-soft); font-size: var(--font-size-sm); }
  .eyebrow, .add, .tool-copy span, .tool-copy code, .health-badge, .toggle, .row-actions, .order-actions, .loading, .editor label > span, .editor footer p { font-family: 'JetBrains Mono Variable', monospace; }
  .eyebrow { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 650; letter-spacing: 0.08em; text-transform: uppercase; }
  h2 { margin: 0; color: var(--foreground); font-size: 16px; }
  .add { gap: 5px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 6px 8px; background: var(--accent); color: var(--foreground); font-size: var(--font-size-xs); font-weight: 650; cursor: pointer; }
  .add span { color: var(--text-soft); font-size: 13px; }
  button:disabled { opacity: 0.42; cursor: default; }

  .tool-list { border-top: 1px solid #243e49; }
  article { display: grid; grid-template-columns: auto auto minmax(0, 1fr) auto auto auto; align-items: center; gap: 10px; border-bottom: 1px solid var(--border); padding: 8px 10px; }
  article:last-child { border-bottom: 0; }
  article.disabled { opacity: 0.58; }
  .tool-mark { display: grid; width: 31px; height: 31px; place-items: center; border: 1px solid var(--border-strong); background: var(--background); color: var(--text-soft); }
  .tool-copy { min-width: 0; }
  .tool-copy > div { gap: 7px; }
  .tool-copy strong { color: var(--text-soft); font-size: var(--font-size-sm); }
  .tool-copy span { border: 1px solid #304c57; border-radius: 999px; padding: 2px 5px; color: #708a94; font-size: var(--font-size-xs); text-transform: uppercase; }
  .tool-copy code { display: block; overflow: hidden; margin-top: 4px; color: #66808a; font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }

  .order-actions { display: grid; gap: 2px; }
  .order-actions button { width: 20px; height: 16px; border: 1px solid #29444e; border-radius: 2px; padding: 0; background: transparent; color: #78909a; font-size: 10px; line-height: 14px; cursor: pointer; }
  .order-actions button:hover:not(:disabled) { border-color: #55727b; color: #cbd9dc; }
  .health-badge { border: 1px solid #3b4a50; border-radius: 999px; padding: 3px 7px; color: #839098; font-size: var(--font-size-xs); white-space: nowrap; }
  .health-badge.ready { border-color: #356a63; background: rgb(99 215 197 / 8%); color: var(--signal); }
  .health-badge.degraded { border-color: #6e563c; background: rgb(224 165 87 / 9%); color: var(--warning); }
  .health-badge.missing { border-color: #70443f; background: rgb(231 110 101 / 8%); color: var(--fault); }

  .toggle { gap: 5px; border: 0; background: transparent; color: #718993; font-size: var(--font-size-xs); cursor: pointer; }
  .toggle i { position: relative; width: 25px; height: 13px; border: 1px solid #405862; border-radius: 999px; background: #152832; }
  .toggle i::after { position: absolute; top: 2px; left: 2px; width: 7px; height: 7px; border-radius: 50%; background: #647983; content: ''; transition: transform 120ms ease; }
  .toggle.enabled i { border-color: #3e756f; background: rgb(99 215 197 / 11%); }
  .toggle.enabled i::after { background: var(--signal); transform: translateX(12px); }
  .row-actions { gap: 4px; }
  .row-actions button { border: 1px solid #304b56; border-radius: 2px; padding: 5px 7px; background: transparent; color: #8198a1; font-size: var(--font-size-xs); cursor: pointer; }
  .row-actions button:hover:not(:disabled) { border-color: #5a7780; color: #d3e0e3; }
  .row-actions .delete:hover:not(:disabled) { border-color: var(--fault); color: var(--fault); }

  .loading, .empty { min-height: 118px; border-top: 1px solid #243e49; justify-content: center; gap: 10px; padding: 18px; color: #67808a; font-size: var(--font-size-xs); }
  .loading i { width: 12px; height: 12px; border: 1px solid #3b555f; border-top-color: var(--signal); border-radius: 50%; animation: spin 0.8s linear infinite; }
  .empty { justify-content: flex-start; }
  .empty > span { color: #4f6d77; font-size: 23px; }
  .empty strong { color: #a8bbc1; font-size: var(--font-size-sm); }
  .empty p { margin: 3px 0 0; color: #607984; font-size: var(--font-size-sm); }
  .empty code { color: #8bc9c0; }
  .empty button { margin-left: auto; border: 1px solid #3c5e67; padding: 7px 9px; background: #112b35; color: #bccdd1; font-size: var(--font-size-xs); }

  .editor { position: absolute; z-index: 3; inset: 8px; overflow: auto; border: 1px solid #47717a; border-radius: 4px; background: #0c2029; box-shadow: 0 18px 45px rgb(0 0 0 / 35%); }
  .editor > header { justify-content: space-between; border-bottom: 1px solid #29444f; padding: 13px 15px; }
  .editor h3 { margin: 3px 0 0; color: #dce7e9; font-size: 14px; }
  .close { border: 0; background: transparent; color: #78909a; font-size: 20px; cursor: pointer; }
  .fields { display: grid; grid-template-columns: 1fr 1fr; gap: 11px; padding: 15px; }
  .editor label > span { display: block; margin-bottom: 5px; color: #708994; font-size: var(--font-size-xs); text-transform: uppercase; }
  .editor input[type='text'] { width: 100%; border: 1px solid #304c57; border-radius: 2px; outline: 0; padding: 9px; background: var(--background); color: var(--text-soft); font-family: 'JetBrains Mono Variable', monospace; font-size: var(--font-size-sm); }
  .editor input:focus { border-color: var(--signal); }
  .command { grid-column: 1 / -1; }
  .enabled-check { display: flex; grid-column: 1 / -1; align-items: center; gap: 7px; }
  .enabled-check input { accent-color: var(--signal); }
  .enabled-check span { margin: 0 !important; }
  .icon-override { display: grid; grid-column: 1 / -1; grid-template-columns: 36px minmax(0, 1fr) auto; align-items: center; gap: 9px; border: 1px solid var(--border); border-radius: 3px; padding: 8px; background: var(--card); }
  .icon-preview { display: grid; width: 34px; height: 34px; place-items: center; border: 1px solid var(--border-strong); border-radius: 3px; background: var(--background); color: var(--text-soft); }
  .icon-copy strong, .icon-copy small { display: block; }
  .icon-copy strong { color: var(--text-soft); font-size: var(--font-size-sm); }
  .icon-copy small { margin-top: 2px; color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .icon-actions { display: flex; align-items: center; gap: 5px; }
  .icon-actions button { border: 1px solid var(--border-strong); border-radius: 2px; padding: 6px 8px; background: transparent; color: var(--text-soft); font-size: var(--font-size-xs); cursor: pointer; }
  .icon-actions .remove-icon { color: var(--muted-foreground); }
  .editor footer { justify-content: space-between; gap: 12px; border-top: 1px solid #29444f; padding: 11px 15px; }
  .editor footer p { margin: 0; color: #607984; font-size: var(--font-size-xs); }
  .editor footer > div { gap: 6px; }
  .editor footer button { border: 1px solid #38545e; border-radius: 2px; padding: 7px 9px; font-size: var(--font-size-xs); cursor: pointer; }
  .cancel { background: transparent; color: #899da5; }
  .save { background: var(--signal); color: #071a20; font-weight: 680; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 860px) { article { grid-template-columns: auto auto minmax(0, 1fr); } .health-badge, .toggle, .row-actions { grid-column: 3; justify-self: start; } .fields { grid-template-columns: 1fr; } .command, .enabled-check, .icon-override { grid-column: auto; } .icon-override { grid-template-columns: 36px minmax(0, 1fr); } .icon-actions { grid-column: 2; } .editor footer { align-items: flex-start; flex-direction: column; } }
</style>
