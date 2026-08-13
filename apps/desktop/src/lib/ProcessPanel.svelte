<script lang="ts">
  import AgentStatusIndicator from '$lib/components/ds/AgentStatusIndicator.svelte';
  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import { agentStatusPresentation } from './agentStatus';
  import type { ProcessView } from './daemon';

  interface Props {
    processes: ProcessView[];
    selectedId: number | null;
    busyId: number | null;
    connected: boolean;
    onSelect: (processId: number) => void;
    onStart: (process: ProcessView) => void;
    onStop: (process: ProcessView) => void;
    onRestart: (process: ProcessView) => void;
    onTrust: (process: ProcessView) => void;
    onSpawnTerminal: () => void;
  }

  let {
    processes,
    selectedId,
    busyId,
    connected,
    onSelect,
    onStart,
    onStop,
    onRestart,
    onTrust,
    onSpawnTerminal
  }: Props = $props();

  function isActive(process: ProcessView): boolean {
    return process.status === 'running' || process.status === 'starting';
  }

  function needsTrust(process: ProcessView): boolean {
    return process.source === 'yml' && process.trust_hash === null;
  }

  function selectProcess(process: ProcessView): void {
    onSelect(process.id);
  }

  function stateLabel(process: ProcessView): string {
    if (needsTrust(process)) return 'review';
    if (process.kind === 'agent') return agentStatusPresentation(process).shortLabel.toLowerCase();
    return process.status;
  }

  function stateTone(process: ProcessView): 'success' | 'warning' | 'danger' | 'neutral' {
    if (process.status === 'crashed') return 'danger';
    if (process.status === 'starting' || process.agent_state.needs_input || needsTrust(process)) return 'warning';
    if (process.status === 'running') return 'success';
    return 'neutral';
  }
</script>

<section class="process-panel" aria-label="Project processes">
  <header>
    <div>
      <span>Process deck</span>
      <strong>{processes.length.toString().padStart(2, '0')} registered</strong>
    </div>
    <button
      class="spawn"
      type="button"
      disabled={!connected || busyId !== null}
      onclick={onSpawnTerminal}
    >
      <span aria-hidden="true">+</span> Terminal
    </button>
  </header>

  {#if processes.length === 0}
    <div class="empty">
      <span aria-hidden="true">⌁</span>
      <div>
        <strong>No processes are registered</strong>
        <p>Add commands in <code>workman.yml</code>, or spawn a terminal to start working now.</p>
      </div>
      <button type="button" disabled={!connected || busyId !== null} onclick={onSpawnTerminal}>
        <span aria-hidden="true">+</span> Spawn terminal
      </button>
    </div>
  {:else}
    <div class="process-list">
      {#each processes as process (process.id)}
        <article
          class:active={process.id === selectedId}
          class:untrusted={needsTrust(process)}
          class="process-row"
        >
          <button
            class="process-primary"
            type="button"
            title={`Open ${process.name}`}
            onclick={() => selectProcess(process)}
          >
            {#if process.kind === 'agent'}
              <AgentStatusIndicator {process} />
            {:else}
              <StatusIndicator tone={stateTone(process)} label={`${process.name} · ${stateLabel(process)}`} />
            {/if}
            <span class="process-copy">
              <span class="process-title">
                <strong>{process.name}</strong>
                <small>{process.kind}</small>
              </span>
              <span class="command">{process.command ?? process.working_dir}</span>
            </span>
            <span class="state">
              {stateLabel(process)}
            </span>
          </button>

          <div class="actions" aria-label={`${process.name} actions`}>
            {#if needsTrust(process)}
              <button type="button" disabled={busyId !== null} onclick={() => onTrust(process)}>
                Review
              </button>
            {:else if isActive(process)}
              <button type="button" disabled={busyId !== null} onclick={() => onStop(process)}>
                Stop
              </button>
              <button type="button" disabled={busyId !== null} onclick={() => onRestart(process)}>
                Restart
              </button>
            {:else}
              <button class="run" type="button" disabled={busyId !== null} onclick={() => onStart(process)}>
                Run
              </button>
              {#if process.status !== 'stopped'}
                <button type="button" disabled={busyId !== null} onclick={() => onRestart(process)}>
                  Restart
                </button>
              {/if}
            {/if}
          </div>
        </article>
      {/each}
    </div>
  {/if}
</section>

<style>
  .process-panel {
    min-width: 0;
    margin: 0;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
  }

  header {
    display: flex;
    min-height: 36px;
    align-items: center;
    justify-content: space-between;
    padding: 5px 7px 5px 10px;
    border-bottom: 1px solid var(--border);
  }

  header div,
  header span,
  header strong {
    display: block;
  }

  header div > span,
  header strong,
  .spawn,
  .process-title small,
  .command,
  .state,
  .actions button,
  code {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  header div > span {
    color: var(--text-soft);
    font-size: var(--font-size-sm);
    font-weight: 650;
    letter-spacing: 0.09em;
    text-transform: uppercase;
  }

  header strong {
    margin-top: 2px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .spawn {
    display: flex;
    align-items: center;
    gap: 6px;
    border: 1px solid #464b52;
    border-radius: 2px;
    padding: 5px 8px;
    background: var(--accent);
    color: var(--foreground);
    font-size: var(--font-size-xs);
    font-weight: 650;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .spawn span {
    color: #a4aab2;
    font-size: 12px;
    line-height: 0;
  }

  .spawn:hover:not(:disabled) {
    border-color: #6b727b;
  }

  .process-list {
    max-height: min(220px, 28vh);
    overflow-y: auto;
    scrollbar-color: #2b4353 transparent;
    scrollbar-width: thin;
  }

  .process-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    border-bottom: 1px solid rgb(39 61 73 / 66%);
  }

  .process-row:last-child {
    border-bottom: 0;
  }

  .process-row.active {
    background: #23262a;
    box-shadow: inset 2px 0 #737a83;
  }

  .process-row.untrusted {
    background: linear-gradient(90deg, rgb(228 174 91 / 9%), transparent 72%);
    box-shadow: inset 2px 0 #e4ae5b;
  }

  .process-primary {
    display: grid;
    min-width: 0;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    border: 0;
    padding: 7px 9px;
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  .process-primary:hover {
    background: rgb(216 226 233 / 3%);
  }

  .process-copy,
  .process-title,
  .process-title strong,
  .process-title small,
  .command {
    display: block;
    min-width: 0;
  }

  .process-title {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .process-title strong,
  .command {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .process-title strong {
    color: var(--foreground);
    font-size: 12px;
    font-weight: 590;
  }

  .process-title small {
    color: #7d848d;
    font-size: var(--font-size-xs);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .command {
    margin-top: 3px;
    color: var(--muted-foreground);
    font-size: var(--font-size-sm);
  }

  .state {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 5px 7px 5px 0;
  }

  .actions button {
    border: 1px solid var(--border-strong);
    border-radius: 2px;
    padding: 5px 7px;
    background: var(--accent);
    color: #b4b9c0;
    font-size: var(--font-size-xs);
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .actions button:hover:not(:disabled),
  .actions button.run {
    border-color: #6a717a;
    color: #f0f1f2;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.42;
  }

  .empty {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 11px;
    min-height: 108px;
    padding: 16px;
    color: #969ca4;
  }

  .empty > span {
    color: var(--text-soft);
    font-family: 'JetBrains Mono Variable', monospace;
  }

  .empty strong { display: block; color: var(--foreground); font-size: 12px; }

  .empty p {
    margin: 5px 0 0;
    font-size: var(--font-size-sm);
    line-height: 1.5;
  }

  .empty button { display: flex; align-items: center; gap: 6px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 6px 9px; background: var(--accent); color: var(--foreground); font-size: var(--font-size-sm); font-weight: 650; cursor: pointer; }
  .empty button span { color: #a6acb4; font: 13px 'JetBrains Mono Variable', monospace; }

  code {
    color: #9fb2bb;
    font-size: var(--font-size-sm);
  }

  @media (max-width: 820px) {
    .process-row {
      grid-template-columns: 1fr;
    }

    .actions {
      padding: 0 10px 8px 29px;
    }

    .empty { grid-template-columns: auto minmax(0, 1fr); }
    .empty button { grid-column: 2; justify-self: start; }
  }
</style>
