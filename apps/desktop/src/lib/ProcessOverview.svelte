<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import PlayIcon from '@lucide/svelte/icons/play';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';

  import { Button } from '$lib/components/ui/button';
  import AgentStatusIndicator from './components/ds/AgentStatusIndicator.svelte';
  import StatusIndicator from './components/ds/StatusIndicator.svelte';
  import SectionOverview from './SectionOverview.svelte';
  import { agentStatusPresentation } from './agentStatus';
  import type { ProcessKind, ProcessView, Project } from './daemon';

  type OverviewKind = Extract<ProcessKind, 'agent' | 'terminal' | 'command'>;

  interface Props {
    kind: OverviewKind;
    processes: ProcessView[];
    onSelect: (process: ProcessView) => void;
    onCreate: () => void;
    project?: Project | null;
  }

  let { kind, processes, onSelect, onCreate, project = null }: Props = $props();

  const copy = {
    agent: {
      eyebrow: 'Project assistants',
      title: 'Agents',
      description: 'Your AI coding assistants for this project.',
      singular: 'agent',
      action: 'Add agent',
      empty: 'No agents are running for this project.'
    },
    terminal: {
      eyebrow: 'Interactive processes',
      title: 'Terminals',
      description: 'Shell sessions attached to this project.',
      singular: 'terminal',
      action: 'New terminal',
      empty: 'No terminal sessions are open for this project.'
    },
    command: {
      eyebrow: 'Configured processes',
      title: 'Commands',
      description: 'Saved commands ready to run for this project.',
      singular: 'command',
      action: 'Add command',
      empty: 'No commands are configured for this project.'
    }
  } as const;

  let section = $derived(copy[kind]);
  let matchingProcesses = $derived(processes.filter((process) => process.kind === kind));
  let runningCount = $derived(matchingProcesses.filter(isRunning).length);
  let workingCount = $derived(
    kind === 'agent'
      ? matchingProcesses.filter((process) => agentStatusPresentation(process).state === 'working').length
      : runningCount
  );
  let waitingCount = $derived(
    kind === 'agent'
      ? matchingProcesses.filter((process) => agentStatusPresentation(process).state === 'waiting').length
      : 0
  );
  let attentionCount = $derived(
    kind === 'agent'
      ? matchingProcesses.filter((process) => agentStatusPresentation(process).state === 'needs_input').length
      : 0
  );

  function isRunning(process: ProcessView): boolean {
    return process.status === 'running' || process.status === 'starting';
  }

  function stateLabel(process: ProcessView): string {
    if (process.kind === 'agent') {
      const state = agentStatusPresentation(process).shortLabel;
      return process.agent_state.unread ? `${state} · unread` : state;
    }
    if (process.status === 'starting') return 'Starting';
    if (process.status === 'running') return 'Running';
    if (process.status === 'crashed') return 'Crashed';
    if (process.status === 'stopped') return 'Stopped';
    return 'Exited';
  }

  function stateTone(process: ProcessView): 'success' | 'warning' | 'danger' | 'neutral' {
    if (process.status === 'crashed') return 'danger';
    if (process.status === 'starting') return 'warning';
    if (process.status === 'running') return 'success';
    return 'neutral';
  }

  function secondaryCopy(process: ProcessView): string {
    if (process.kind === 'terminal') return process.command ?? process.working_dir;
    if (process.kind === 'agent') {
      const tool = process.agent_state.tool_type?.replaceAll('_', ' ');
      return `${tool ?? 'agent'} · #${process.id}`;
    }
    return process.command ?? 'No command configured';
  }
</script>

<SectionOverview
  ariaLabel={`${section.title} overview`}
  eyebrow={section.eyebrow}
  title={section.title}
  description={section.description}
  {project}
>
  {#snippet icon()}
    {#if kind === 'agent'}
      <BotIcon strokeWidth={1.8} />
    {:else if kind === 'terminal'}
      <SquareTerminalIcon strokeWidth={1.8} />
    {:else}
      <PlayIcon strokeWidth={1.8} />
    {/if}
  {/snippet}

  {#snippet action()}
    <Button size="sm" onclick={onCreate}><PlusIcon size={13} aria-hidden="true" /> {section.action}</Button>
  {/snippet}

  {#snippet summary()}
    <span>{matchingProcesses.length} {matchingProcesses.length === 1 ? section.singular : `${section.singular}s`}</span>
    {#if kind === 'agent'}
      <span class="summary-divider" aria-hidden="true">·</span>
      <span class="active">{workingCount} working</span>
      <span class="summary-divider" aria-hidden="true">·</span>
      <span class:attention={waitingCount > 0}>{waitingCount} waiting</span>
      <span class="summary-divider" aria-hidden="true">·</span>
      <span class:attention={attentionCount > 0}>{attentionCount} need input</span>
    {:else}
      <span class="summary-divider" aria-hidden="true">·</span>
      <span class="active">{runningCount} running</span>
    {/if}
  {/snippet}

  <div class="process-ledger" aria-live="polite">
    {#each matchingProcesses as process (process.id)}
      <button
        type="button"
        class="process-row"
        title={`Open ${process.name}`}
        onclick={() => onSelect(process)}
      >
        {#if process.kind === 'agent'}
          <AgentStatusIndicator {process} />
        {:else}
          <StatusIndicator tone={stateTone(process)} label={`${process.name} · ${stateLabel(process)}`} />
        {/if}
        <span class="process-ref">#{process.id}</span>
        <span class="process-copy">
          <strong>{process.name}</strong>
          <small>{secondaryCopy(process)}</small>
        </span>
        <span class:attention={process.kind === 'agent' && process.agent_state.needs_input} class="process-state">{stateLabel(process)}</span>
      </button>
    {:else}
      <div class="empty-results">
        <strong>{section.empty}</strong>
        <p>Use the section action to add the first {section.singular}.</p>
        <Button size="sm" variant="outline" onclick={onCreate}>{section.action}</Button>
      </div>
    {/each}
  </div>
</SectionOverview>

<style>
  .summary-divider { color: var(--muted-foreground); }
  .active { color: var(--success); }
  .attention { color: var(--warning-token); }
  .process-ledger { height: 100%; min-height: 0; overflow-y: auto; padding: 4px 7px 10px; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .process-row { display: grid; width: 100%; min-height: 42px; grid-template-columns: 16px 42px minmax(160px, 1fr) minmax(88px, auto); align-items: center; gap: var(--space-2); border: 0; border-bottom: 1px solid var(--border); padding: 4px 10px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .process-row:hover { background: var(--popover); }
  .process-ref, .process-copy small, .process-state { font-family: var(--terminal-font-family); font-size: var(--font-size-xs); }
  .process-ref { color: var(--muted-foreground); }
  .process-copy { min-width: 0; }
  .process-copy strong, .process-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .process-copy strong { font-size: var(--font-size-sm); font-weight: 590; }
  .process-copy small { margin-top: 1px; color: var(--muted-foreground); }
  .process-state { justify-self: end; color: var(--muted-foreground); font-weight: 650; letter-spacing: 0.045em; text-transform: uppercase; }
  .empty-results { display: grid; min-height: 220px; place-content: center; justify-items: center; text-align: center; }
  .empty-results strong { font-size: var(--font-size-base); }
  .empty-results p { max-width: 380px; margin: 5px 0 10px; color: var(--muted-foreground); font-size: var(--font-size-sm); }

  @container (max-width: 640px) {
    .process-row { grid-template-columns: 16px 38px minmax(0, 1fr); }
    .process-state { display: none; }
  }
</style>
