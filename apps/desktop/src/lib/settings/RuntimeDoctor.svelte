<script lang="ts">
  import {
    type AgentToolConfigPreview,
    type AgentToolDeepCheck,
    type AgentToolHealth,
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
  let health = $state<AgentToolsHealth | null>(null);
  let loading = $state(false);
  let busy = $state<string | null>(null);
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
      await runHealthCheck();
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

  function status(tool: AgentToolHealth): string {
    if (!tool.enabled) return 'Disabled';
    if (!tool.found_on_path) return 'Not found';
    return tool.mcp_launch_supported ? 'Ready' : 'MCP unavailable';
  }

  function statusClass(tool: AgentToolHealth): string {
    if (!tool.enabled) return 'neutral';
    if (!tool.found_on_path) return 'missing';
    return tool.mcp_launch_supported ? 'ready' : 'degraded';
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
      <p>Ready means both the executable and isolated Workman MCP wiring are available.</p>
    </div>
    <button class="refresh" type="button" disabled={!connected || loading} onclick={() => void refresh()}>
      <span aria-hidden="true" class:spinning={loading}>↻</span>{loading ? 'Checking…' : 'Refresh health'}
    </button>
  </header>

  {#if health && health.tools.length > 0}
    <div class="runtime-list">
      {#each health.tools as tool (tool.id)}
        <article class:tool-disabled={!tool.enabled}>
          <div class={`runtime-icon ${statusClass(tool)}`} aria-hidden="true">
            {tool.launch_ready ? '✓' : '!'}
          </div>
          <div class="runtime-copy">
            <div class="runtime-title">
              <strong>{tool.name}</strong>
              <span class={`health-badge ${statusClass(tool)}`}>{status(tool)}</span>
              {#if tool.source === 'config'}<em>config.yml</em>{/if}
            </div>
            <div class="runtime-facts">
              <span title={tool.version_error ?? undefined}>{tool.version ?? 'Version unavailable'}</span>
              <code title={tool.config_path}>{tool.config_path}</code>
              <span title={tool.mcp_launch_note}>MCP: {tool.mcp_launch_mechanism}</span>
            </div>
            <p>{tool.configuration_note}</p>
            {#if deepChecks[tool.id]}
              <div class:passed={deepChecks[tool.id].success} class="deep-result" aria-live="polite">
                {deepChecks[tool.id].success ? '✓' : '!'} {deepChecks[tool.id].message}
              </div>
            {/if}
          </div>
          <div class="runtime-actions">
            {#if !tool.found_on_path && tool.install_url}
              <a href={tool.install_url} target="_blank" rel="noreferrer">Install ↗</a>
            {/if}
            {#if tool.configuration_mode === 'self_config'}
              <button type="button" disabled={!connected || busy !== null} onclick={() => void configure(tool)}>
                {busy === `preview-${tool.id}` ? 'Preparing…' : 'Configure'}
              </button>
            {/if}
            <button type="button" title={tool.mcp_launch_note} disabled={!connected || busy !== null || !tool.launch_ready || !tool.mcp_launch_supported} onclick={() => void deepCheck(tool)}>
              {busy === `deep-${tool.id}` ? 'Running…' : 'Deep check'}
            </button>
          </div>
        </article>
      {/each}
    </div>
  {:else if !loading}
    <div class="empty"><span aria-hidden="true">⌁</span><p>No runtime targets are registered yet.</p></div>
  {/if}

  {#if preview}
    <div class="overlay" role="presentation">
      <div class="preview-dialog" role="dialog" aria-modal="true" aria-labelledby="config-preview-title">
        <header>
          <div><span class="eyebrow">Explicit consent</span><h3 id="config-preview-title">Review the complete config write</h3></div>
          <button type="button" aria-label="Close configuration preview" onclick={() => (preview = null)}>×</button>
        </header>
        <p>Workman will write exactly this result to <code>{preview.path}</code>. Existing unrelated values are retained.</p>
        <pre>{preview.preview}</pre>
        <footer>
          <span>{preview.already_configured ? 'The Workman entry already matches.' : 'No file is changed until you approve.'}</span>
          <div><button class="cancel" type="button" onclick={() => (preview = null)}>Cancel</button><button class="approve" type="button" disabled={busy !== null} onclick={() => void applyConfiguration()}>{busy?.startsWith('configure-') ? 'Writing…' : 'Approve & write'}</button></div>
        </footer>
      </div>
    </div>
  {/if}

</section>

<style>
  .doctor { position: relative; overflow: hidden; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); }
  .doctor-header, .runtime-title, .runtime-facts, .runtime-actions, .preview-dialog header, .preview-dialog footer, .preview-dialog footer > div { display: flex; align-items: center; }
  .doctor-header { gap: 11px; padding: 12px; }
  .summary-mark { display: flex; width: 42px; height: 42px; align-items: baseline; justify-content: center; border: 1px solid #5a4940; background: #2b211b; color: var(--warning); font: 700 17px/42px 'JetBrains Mono Variable', monospace; }
  .summary-mark.healthy { border-color: #356a63; background: #102b2b; color: var(--signal); }
  .summary-mark i { color: var(--muted-foreground); font-size: var(--font-size-xs); font-style: normal; }
  .summary-copy { min-width: 0; }
  .eyebrow, .runtime-title span, .runtime-title em, .runtime-facts, .runtime-copy p, .deep-result, .runtime-actions, .refresh, .preview-dialog p, .preview-dialog pre, .preview-dialog footer { font-family: 'JetBrains Mono Variable', monospace; }
  .eyebrow { color: #7f8993; font-size: var(--font-size-xs); font-weight: 650; letter-spacing: 0.09em; text-transform: uppercase; }
  h2 { margin: 3px 0 0; color: var(--text); font-size: 15px; }
  .summary-copy p { margin: 3px 0 0; color: var(--muted); font-size: var(--font-size-sm); }
  .refresh { display: flex; margin-left: auto; align-items: center; gap: 5px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 6px 8px; background: var(--surface-raised); color: var(--text-soft); font-size: var(--font-size-xs); cursor: pointer; }
  .refresh span { color: var(--signal); font-size: 12px; }
  .spinning { animation: spin 0.8s linear infinite; }
  button:disabled { cursor: default; opacity: 0.42; }

  .runtime-list { border-top: 1px solid var(--border); }
  article { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 10px; border-bottom: 1px solid var(--border); padding: 9px 11px; }
  article.tool-disabled { opacity: 0.67; }
  .runtime-icon { display: grid; width: 27px; height: 27px; place-items: center; border: 1px solid #684a40; background: #2a1c18; color: var(--fault); font: 700 var(--font-size-sm) 'JetBrains Mono Variable', monospace; }
  .runtime-icon.ready { border-color: #356a63; background: #102b2b; color: var(--signal); }
  .runtime-icon.degraded { border-color: #6e563c; background: #2b211b; color: var(--warning); }
  .runtime-icon.neutral { border-color: #48525b; background: #20262b; color: #78858e; }
  .runtime-copy { min-width: 0; }
  .runtime-title { gap: 7px; }
  .runtime-title strong { color: #c4ccd1; font-size: var(--font-size-sm); }
  .runtime-title .health-badge { border: 1px solid #48525b; border-radius: 999px; padding: 2px 6px; color: #829099; font-size: var(--font-size-xs); }
  .runtime-title .health-badge.ready { border-color: #356a63; background: rgb(99 215 197 / 8%); color: var(--signal); }
  .runtime-title .health-badge.degraded { border-color: #6e563c; background: rgb(224 165 87 / 9%); color: var(--warning); }
  .runtime-title .health-badge.missing { border-color: #70443f; background: rgb(231 110 101 / 8%); color: var(--fault); }
  .runtime-title em { border: 1px solid var(--border-strong); border-radius: 999px; padding: 1px 5px; color: #717c85; font-size: var(--font-size-xs); font-style: normal; }
  .runtime-facts { min-width: 0; gap: 8px; margin-top: 4px; color: #75818a; font-size: var(--font-size-xs); }
  .runtime-facts span { flex: 0 0 auto; }
  .runtime-facts code { overflow: hidden; color: #64727b; text-overflow: ellipsis; white-space: nowrap; }
  .runtime-copy p { margin: 4px 0 0; color: #61717a; font-size: var(--font-size-xs); }
  .deep-result { margin-top: 5px; color: var(--fault); font-size: var(--font-size-xs); }
  .deep-result.passed { color: var(--signal); }

  .runtime-actions { flex-wrap: wrap; justify-content: flex-end; gap: 4px; max-width: 230px; }
  .runtime-actions button, .runtime-actions a { border: 1px solid var(--border-strong); border-radius: 2px; padding: 5px 6px; background: transparent; color: #85949c; font-size: var(--font-size-xs); text-decoration: none; cursor: pointer; }
  .runtime-actions button:hover:not(:disabled), .runtime-actions a:hover { border-color: #5a7880; color: #d2dadd; }
  .runtime-actions a { border-color: #66503e; color: var(--warning); }

  .empty { display: flex; min-height: 90px; align-items: center; justify-content: center; gap: 8px; border-top: 1px solid var(--border); color: var(--muted); font-size: var(--font-size-sm); }

  .overlay { position: absolute; z-index: 8; inset: 7px; display: grid; place-items: center; padding: 8px; background: rgb(4 12 16 / 82%); backdrop-filter: blur(3px); }
  .preview-dialog { display: grid; width: min(680px, 100%); min-height: 0; max-height: 100%; grid-template-rows: auto auto minmax(0, 1fr) auto; overflow: hidden; border: 1px solid #4a7179; border-radius: 4px; background: #0b1c23; box-shadow: 0 18px 50px rgb(0 0 0 / 45%); }
  .preview-dialog header { justify-content: space-between; border-bottom: 1px solid #29434c; padding: 11px 13px; }
  .preview-dialog h3 { margin: 3px 0 0; color: #dde6e8; font-size: 13px; }
  .preview-dialog header button { border: 0; background: transparent; color: #7d8d94; font-size: 18px; cursor: pointer; }
  .preview-dialog > p { margin: 0; padding: 10px 13px 0; color: #7f9199; font-size: var(--font-size-xs); line-height: 1.5; }
  .preview-dialog > p code { color: #a7bbc0; }
  .preview-dialog pre { min-height: 0; overflow: auto; overscroll-behavior: contain; margin: 10px 13px; border: 1px solid #253e47; padding: 10px; background: var(--background); color: #a9c4c8; font-size: var(--font-size-xs); line-height: 1.5; white-space: pre-wrap; }
  .preview-dialog footer { justify-content: space-between; gap: 10px; border-top: 1px solid #29434c; padding: 10px 13px; color: #6d8088; font-size: var(--font-size-xs); }
  .preview-dialog footer > div { gap: 6px; }
  .cancel, .approve { border: 1px solid #3b535c; border-radius: 2px; padding: 7px 9px; font-size: var(--font-size-xs); cursor: pointer; }
  .cancel { background: transparent; color: #87979e; }
  .approve { border-color: var(--signal); background: var(--signal); color: #06191f; font-weight: 680; }

  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 840px) { article { grid-template-columns: auto minmax(0, 1fr); } .runtime-actions { grid-column: 2; justify-self: start; max-width: none; } }
  @media (max-width: 600px) { .doctor-header { align-items: flex-start; flex-wrap: wrap; } .refresh { margin-left: 53px; } .preview-dialog footer { align-items: flex-start; flex-direction: column; } }
  @media (prefers-reduced-motion: reduce) { .spinning { animation: none; } }
</style>
