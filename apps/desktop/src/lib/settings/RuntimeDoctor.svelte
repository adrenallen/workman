<script lang="ts">
  import {
    getAgentToolsStore,
    type AgentToolConfigPreview,
    type AgentToolDeepCheck,
    type AgentToolHealth,
    type AgentToolInput,
    type AgentToolsHealth
  } from '../agentTools';
  import type { DaemonClient, Project } from '../daemon';

  interface Props {
    client: DaemonClient;
    project: Project;
    connected: boolean;
    onError: (message: string) => void;
  }

  let { client, project, connected, onError }: Props = $props();
  let registryStore = $derived(getAgentToolsStore(client));
  let health = $state<AgentToolsHealth | null>(null);
  let loading = $state(false);
  let busy = $state<string | null>(null);
  let editor = $state<AgentToolInput | null>(null);
  let preview = $state<AgentToolConfigPreview | null>(null);
  let deepChecks = $state<Record<number, AgentToolDeepCheck>>({});

  $effect(() => {
    if (connected) void runHealthCheck();
  });

  async function refresh(): Promise<void> {
    if (!connected || loading) return;
    await runHealthCheck();
  }

  async function runHealthCheck(): Promise<void> {
    loading = true;
    try {
      health = await client.agentToolsHealth();
    } catch (cause) {
      onError(message(cause));
    } finally {
      loading = false;
    }
  }

  async function toggle(tool: AgentToolHealth): Promise<void> {
    busy = `toggle-${tool.id}`;
    try {
      await client.saveAgentTool({
        id: tool.id,
        name: tool.name,
        command: tool.command,
        tool_type: tool.tool_type,
        enabled: !tool.enabled
      });
      await refreshAfterMutation();
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = null;
    }
  }

  function edit(tool?: AgentToolHealth): void {
    editor = tool
      ? {
          id: tool.id,
          name: tool.name,
          command: tool.command,
          tool_type: tool.tool_type,
          enabled: tool.enabled
        }
      : { name: '', command: '', tool_type: 'custom', enabled: true };
  }

  async function save(): Promise<void> {
    if (!editor) return;
    const draft = editor;
    busy = 'save';
    try {
      await client.saveAgentTool({
        ...draft,
        name: draft.name.trim(),
        command: draft.command.trim(),
        tool_type: draft.tool_type.trim()
      });
      editor = null;
      await refreshAfterMutation();
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = null;
    }
  }

  async function configure(tool: AgentToolHealth): Promise<void> {
    busy = `preview-${tool.id}`;
    try {
      const next = await client.previewAgentToolConfig(tool.id);
      if (next.automatic_wiring) {
        deepChecks = {
          ...deepChecks,
          [tool.id]: {
            agent_tool_id: tool.id,
            process_id: null,
            success: true,
            elapsed_ms: 0,
            message: next.message
          }
        };
      } else {
        preview = next;
      }
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = null;
    }
  }

  async function applyConfiguration(): Promise<void> {
    const previewHash = preview?.preview_sha256;
    if (!preview || !previewHash) return;
    const current = preview;
    busy = `configure-${current.agent_tool_id}`;
    try {
      await client.configureAgentTool(current.agent_tool_id, previewHash);
      preview = null;
      await refreshAfterMutation();
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = null;
    }
  }

  async function deepCheck(tool: AgentToolHealth): Promise<void> {
    busy = `deep-${tool.id}`;
    try {
      const result = await client.deepCheckAgentTool(project.id, tool.id);
      deepChecks = { ...deepChecks, [tool.id]: result };
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = null;
    }
  }

  async function refreshAfterMutation(): Promise<void> {
    await registryStore.refresh(true);
    health = await client.agentToolsHealth();
  }

  function status(tool: AgentToolHealth): string {
    if (!tool.enabled) {
      return tool.found_on_path ? 'Disabled · runtime found' : 'Disabled · not found on PATH';
    }
    return tool.found_on_path ? 'Ready' : 'Not found on PATH';
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="doctor" aria-labelledby="runtime-doctor-title">
  <header class="doctor-header">
    <div class="summary-mark" class:healthy={health?.all_enabled_ready} aria-hidden="true">
      <span>{health?.ready_count ?? '–'}</span><i>/{health?.total_count ?? '–'}</i>
    </div>
    <div class="summary-copy">
      <span class="eyebrow">Runtime Doctor</span>
      <h2 id="runtime-doctor-title">{health?.summary ?? 'Checking agent runtimes…'}</h2>
      <p>Fast local checks only: executable, version, and client configuration.</p>
    </div>
    <button class="refresh" type="button" disabled={!connected || loading} onclick={() => void refresh()}>
      <span aria-hidden="true" class:spinning={loading}>↻</span>{loading ? 'Checking…' : 'Refresh health'}
    </button>
  </header>

  {#if health && health.tools.length > 0}
    <div class="runtime-rail" aria-label={health.summary}>
      {#each health.tools as tool (tool.id)}
        <i
          class:ready={tool.launch_ready}
          class:disabled={!tool.enabled}
          class:missing={!tool.found_on_path}
          title={`${tool.name}: ${status(tool)}`}
        ></i>
      {/each}
    </div>
    <div class="runtime-list">
      {#each health.tools as tool (tool.id)}
        <article class:tool-disabled={!tool.enabled}>
          <div class="runtime-icon" class:ready={tool.launch_ready} aria-hidden="true">
            {tool.found_on_path ? '✓' : '!'}
          </div>
          <div class="runtime-copy">
            <div class="runtime-title">
              <strong>{tool.name}</strong>
              <span class:ready={tool.launch_ready} class:missing={!tool.found_on_path}>{status(tool)}</span>
              {#if tool.source === 'config'}<em>config.yml</em>{/if}
            </div>
            <div class="runtime-facts">
              <span title={tool.version_error ?? undefined}>{tool.version ?? 'Version unavailable'}</span>
              <code title={tool.config_path}>{tool.config_path}</code>
            </div>
            <p>{tool.configuration_note}</p>
            {#if deepChecks[tool.id]}
              <div class:passed={deepChecks[tool.id].success} class="deep-result" aria-live="polite">
                {deepChecks[tool.id].success ? '✓' : '!'} {deepChecks[tool.id].message}
              </div>
            {/if}
          </div>
          <button
            class="toggle"
            class:enabled={tool.enabled}
            type="button"
            role="switch"
            aria-checked={tool.enabled}
            disabled={!connected || busy !== null}
            onclick={() => void toggle(tool)}
          ><i aria-hidden="true"></i><span>{tool.enabled ? 'On' : 'Off'}</span></button>
          <div class="runtime-actions">
            {#if !tool.found_on_path && tool.install_url}
              <a href={tool.install_url} target="_blank" rel="noreferrer">Install ↗</a>
            {/if}
            {#if tool.configuration_mode === 'self_config'}
              <button type="button" disabled={!connected || busy !== null} onclick={() => void configure(tool)}>
                {busy === `preview-${tool.id}` ? 'Preparing…' : 'Configure'}
              </button>
            {/if}
            <button type="button" disabled={!connected || busy !== null || !tool.launch_ready} onclick={() => void deepCheck(tool)}>
              {busy === `deep-${tool.id}` ? 'Running…' : 'Deep check'}
            </button>
            <button type="button" disabled={!connected || busy !== null} onclick={() => edit(tool)}>Edit</button>
          </div>
        </article>
      {/each}
    </div>
  {:else if !loading}
    <div class="empty"><span aria-hidden="true">⌁</span><p>No runtime targets are registered yet.</p></div>
  {/if}

  <button class="add-agent" type="button" disabled={!connected || busy !== null} onclick={() => edit()}>
    <span aria-hidden="true">+</span><strong>Add agent</strong><small>Register another command for this machine</small>
  </button>

  {#if preview}
    <div class="overlay" role="presentation">
      <div class="preview-dialog" role="dialog" aria-modal="true" aria-labelledby="config-preview-title">
        <header>
          <div><span class="eyebrow">Explicit consent</span><h3 id="config-preview-title">Review the complete config write</h3></div>
          <button type="button" aria-label="Close configuration preview" onclick={() => (preview = null)}>×</button>
        </header>
        <p>awm will write exactly this result to <code>{preview.path}</code>. Existing unrelated values are retained.</p>
        <pre>{preview.preview}</pre>
        <footer>
          <span>{preview.already_configured ? 'The awm entry already matches.' : 'No file is changed until you approve.'}</span>
          <div><button class="cancel" type="button" onclick={() => (preview = null)}>Cancel</button><button class="approve" type="button" disabled={busy !== null} onclick={() => void applyConfiguration()}>{busy?.startsWith('configure-') ? 'Writing…' : 'Approve & write'}</button></div>
        </footer>
      </div>
    </div>
  {/if}

  {#if editor}
    <div class="overlay" role="presentation">
      <form
        class="editor-dialog"
        aria-labelledby="runtime-editor-title"
        onsubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <header>
          <div><span class="eyebrow">Runtime target</span><h3 id="runtime-editor-title">{editor.id ? `Edit ${editor.name}` : 'Add agent'}</h3></div>
          <button type="button" aria-label="Close agent editor" onclick={() => (editor = null)}>×</button>
        </header>
        <div class="fields">
          <label><span>Name</span><input type="text" bind:value={editor.name} placeholder="Kimi" /></label>
          <label><span>Tool type</span><input type="text" bind:value={editor.tool_type} list="doctor-tool-types" placeholder="custom" /></label>
          <label class="command"><span>Command</span><input type="text" bind:value={editor.command} placeholder="agent-cli --flag" /></label>
          <datalist id="doctor-tool-types"><option value="claude_code"></option><option value="codex"></option><option value="gemini"></option><option value="opencode"></option><option value="custom"></option></datalist>
          <label class="enabled-check"><input type="checkbox" bind:checked={editor.enabled} /><span>Available for launches</span></label>
        </div>
        <footer><button class="cancel" type="button" onclick={() => (editor = null)}>Cancel</button><button class="approve" type="submit" disabled={busy !== null || !editor.name.trim() || !editor.command.trim() || !editor.tool_type.trim()}>{busy === 'save' ? 'Saving…' : 'Save agent'}</button></footer>
      </form>
    </div>
  {/if}
</section>

<style>
  .doctor { position: relative; overflow: hidden; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); }
  .doctor-header, .runtime-title, .runtime-facts, .runtime-actions, .toggle, .add-agent, .preview-dialog header, .preview-dialog footer, .preview-dialog footer > div, .editor-dialog header, .editor-dialog footer, .enabled-check { display: flex; align-items: center; }
  .doctor-header { gap: 11px; padding: 12px; }
  .summary-mark { display: flex; width: 42px; height: 42px; align-items: baseline; justify-content: center; border: 1px solid #5a4940; background: #2b211b; color: var(--warning); font: 700 17px/42px 'JetBrains Mono Variable', monospace; }
  .summary-mark.healthy { border-color: #356a63; background: #102b2b; color: var(--signal); }
  .summary-mark i { color: #75808a; font-size: 8px; font-style: normal; }
  .summary-copy { min-width: 0; }
  .eyebrow, .runtime-title span, .runtime-title em, .runtime-facts, .runtime-copy p, .deep-result, .toggle, .runtime-actions, .refresh, .add-agent, .preview-dialog p, .preview-dialog pre, .preview-dialog footer, .editor-dialog label > span { font-family: 'JetBrains Mono Variable', monospace; }
  .eyebrow { color: #7f8993; font-size: 7px; font-weight: 650; letter-spacing: 0.09em; text-transform: uppercase; }
  h2 { margin: 3px 0 0; color: var(--text); font-size: 15px; }
  .summary-copy p { margin: 3px 0 0; color: var(--muted); font-size: 9px; }
  .refresh { display: flex; margin-left: auto; align-items: center; gap: 5px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 6px 8px; background: var(--surface-raised); color: var(--text-soft); font-size: 8px; cursor: pointer; }
  .refresh span { color: var(--signal); font-size: 12px; }
  .spinning { animation: spin 0.8s linear infinite; }
  button:disabled { cursor: default; opacity: 0.42; }

  .runtime-rail { display: grid; height: 3px; grid-auto-flow: column; grid-auto-columns: 1fr; gap: 2px; padding: 0 12px; }
  .runtime-rail i { background: #7b4e3a; }
  .runtime-rail i.ready { background: var(--signal); }
  .runtime-rail i.disabled { background: #4a5159; }
  .runtime-rail i.missing { background: var(--fault); }
  .runtime-list { margin-top: 9px; border-top: 1px solid var(--border); }
  article { display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 10px; border-bottom: 1px solid var(--border); padding: 9px 11px; }
  article.tool-disabled { opacity: 0.67; }
  .runtime-icon { display: grid; width: 27px; height: 27px; place-items: center; border: 1px solid #684a40; background: #2a1c18; color: var(--fault); font: 700 9px 'JetBrains Mono Variable', monospace; }
  .runtime-icon.ready { border-color: #356a63; background: #102b2b; color: var(--signal); }
  .runtime-copy { min-width: 0; }
  .runtime-title { gap: 7px; }
  .runtime-title strong { color: #c4ccd1; font-size: 11px; }
  .runtime-title span { color: var(--warning); font-size: 7px; }
  .runtime-title span.ready { color: var(--signal); }
  .runtime-title span.missing { color: var(--fault); }
  .runtime-title em { border: 1px solid #39444d; border-radius: 999px; padding: 1px 5px; color: #717c85; font-size: 6px; font-style: normal; }
  .runtime-facts { min-width: 0; gap: 8px; margin-top: 4px; color: #75818a; font-size: 7px; }
  .runtime-facts span { flex: 0 0 auto; }
  .runtime-facts code { overflow: hidden; color: #64727b; text-overflow: ellipsis; white-space: nowrap; }
  .runtime-copy p { margin: 4px 0 0; color: #61717a; font-size: 7px; }
  .deep-result { margin-top: 5px; color: var(--fault); font-size: 7px; }
  .deep-result.passed { color: var(--signal); }

  .toggle { gap: 4px; border: 0; background: transparent; color: #78858e; font-size: 7px; cursor: pointer; }
  .toggle i { position: relative; width: 25px; height: 13px; border: 1px solid #48525b; border-radius: 999px; background: #20262b; }
  .toggle i::after { position: absolute; top: 2px; left: 2px; width: 7px; height: 7px; border-radius: 50%; background: #68747c; content: ''; transition: transform 120ms ease; }
  .toggle.enabled i { border-color: #3e756f; background: rgb(99 215 197 / 10%); }
  .toggle.enabled i::after { background: var(--signal); transform: translateX(12px); }
  .runtime-actions { flex-wrap: wrap; justify-content: flex-end; gap: 4px; max-width: 230px; }
  .runtime-actions button, .runtime-actions a { border: 1px solid #35434c; border-radius: 2px; padding: 5px 6px; background: transparent; color: #85949c; font-size: 7px; text-decoration: none; cursor: pointer; }
  .runtime-actions button:hover:not(:disabled), .runtime-actions a:hover { border-color: #5a7880; color: #d2dadd; }
  .runtime-actions a { border-color: #66503e; color: var(--warning); }

  .add-agent { width: 100%; gap: 7px; border: 0; padding: 9px 12px; background: #0d1e25; color: #9ab0b7; text-align: left; cursor: pointer; }
  .add-agent > span { display: grid; width: 20px; height: 20px; place-items: center; border: 1px solid #395660; color: var(--signal); font-size: 13px; }
  .add-agent strong { font-size: 8px; }
  .add-agent small { color: #627780; font-size: 7px; }
  .empty { display: flex; min-height: 90px; align-items: center; justify-content: center; gap: 8px; border-top: 1px solid var(--border); color: var(--muted); font-size: 9px; }

  .overlay { position: absolute; z-index: 8; inset: 7px; display: grid; place-items: center; padding: 8px; background: rgb(4 12 16 / 82%); backdrop-filter: blur(3px); }
  .preview-dialog, .editor-dialog { width: min(680px, 100%); overflow: hidden; border: 1px solid #4a7179; border-radius: 4px; background: #0b1c23; box-shadow: 0 18px 50px rgb(0 0 0 / 45%); }
  .preview-dialog header, .editor-dialog header { justify-content: space-between; border-bottom: 1px solid #29434c; padding: 11px 13px; }
  .preview-dialog h3, .editor-dialog h3 { margin: 3px 0 0; color: #dde6e8; font-size: 13px; }
  .preview-dialog header button, .editor-dialog header button { border: 0; background: transparent; color: #7d8d94; font-size: 18px; cursor: pointer; }
  .preview-dialog > p { margin: 0; padding: 10px 13px 0; color: #7f9199; font-size: 8px; line-height: 1.5; }
  .preview-dialog > p code { color: #a7bbc0; }
  .preview-dialog pre { max-height: 260px; overflow: auto; margin: 10px 13px; border: 1px solid #253e47; padding: 10px; background: #071319; color: #a9c4c8; font-size: 8px; line-height: 1.5; white-space: pre-wrap; }
  .preview-dialog footer { justify-content: space-between; gap: 10px; border-top: 1px solid #29434c; padding: 10px 13px; color: #6d8088; font-size: 7px; }
  .preview-dialog footer > div, .editor-dialog footer { gap: 6px; }
  .cancel, .approve { border: 1px solid #3b535c; border-radius: 2px; padding: 7px 9px; font-size: 8px; cursor: pointer; }
  .cancel { background: transparent; color: #87979e; }
  .approve { border-color: var(--signal); background: var(--signal); color: #06191f; font-weight: 680; }

  .fields { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; padding: 13px; }
  .editor-dialog label > span { display: block; margin-bottom: 5px; color: #71858d; font-size: 7px; text-transform: uppercase; }
  .editor-dialog input[type='text'] { width: 100%; border: 1px solid #304b55; border-radius: 2px; outline: 0; padding: 8px; background: #071319; color: #c4d0d3; font: 9px 'JetBrains Mono Variable', monospace; }
  .editor-dialog input:focus { border-color: var(--signal); }
  .command { grid-column: 1 / -1; }
  .enabled-check { grid-column: 1 / -1; gap: 6px; }
  .enabled-check input { accent-color: var(--signal); }
  .enabled-check span { margin: 0 !important; }
  .editor-dialog footer { justify-content: flex-end; border-top: 1px solid #29434c; padding: 10px 13px; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 840px) { article { grid-template-columns: auto minmax(0, 1fr); } .toggle, .runtime-actions { grid-column: 2; justify-self: start; max-width: none; } }
  @media (max-width: 600px) { .doctor-header { align-items: flex-start; flex-wrap: wrap; } .refresh { margin-left: 53px; } .fields { grid-template-columns: 1fr; } .command, .enabled-check { grid-column: auto; } .preview-dialog footer { align-items: flex-start; flex-direction: column; } }
  @media (prefers-reduced-motion: reduce) { .spinning { animation: none; } .toggle i::after { transition: none; } }
</style>
