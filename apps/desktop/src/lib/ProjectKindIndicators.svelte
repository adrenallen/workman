<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import PlayIcon from '@lucide/svelte/icons/play';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import * as Popover from '$lib/components/ui/popover';
  import type { ProcessKind, ProcessView } from './daemon';
  import type { ProjectKindActivityRollup } from './processActivity';

  interface Props {
    activity: ProjectKindActivityRollup;
    processes: ProcessView[];
    projectTitle: string;
    compact?: boolean;
    onSelect: (process: ProcessView) => void;
    onShowAll: (kind: ProcessKind) => void;
  }

  let {
    activity,
    processes,
    projectTitle,
    compact = false,
    onSelect,
    onShowAll
  }: Props = $props();

  const kinds = ['agent', 'terminal', 'command'] as const;
  let openKind = $state<ProcessKind | null>(null);
  let visibleKinds = $derived(kinds.filter((kind) => activity[kind].active > 0));

  $effect(() => {
    if (openKind && activity[openKind].active === 0) openKind = null;
  });

  function activeProcesses(kind: ProcessKind): ProcessView[] {
    const byId = new Map(processes.map((process) => [process.id, process]));
    return activity[kind].activeProcessIds
      .map((id) => byId.get(id))
      .filter((process): process is ProcessView => process !== undefined);
  }

  function changeOpen(kind: ProcessKind, open: boolean): void {
    if (open) {
      openKind = kind;
    } else if (openKind === kind) {
      openKind = null;
    }
  }

  function chooseProcess(process: ProcessView): void {
    openKind = null;
    onSelect(process);
  }

  function showAll(kind: ProcessKind): void {
    openKind = null;
    onShowAll(kind);
  }

  function kindTitle(kind: ProcessKind): string {
    return kind === 'agent' ? 'Agents' : kind === 'terminal' ? 'Terminals' : 'Commands';
  }

  function agentNeedsInput(process: ProcessView): boolean {
    return process.kind === 'agent'
      && (process.agent_state.needs_input || String(process.agent_state.state) === 'needs_input');
  }

  function stateLabel(process: ProcessView): string {
    if (process.kind === 'agent') {
      const state = agentNeedsInput(process) ? 'needs input' : 'working';
      const activityAt = process.agent_state.last_content_change_at
        ?? process.agent_state.last_output_at;
      return activityAt == null ? state : `${state} · ${relativeTime(activityAt)}`;
    }
    if (process.status === 'starting') return 'starting';
    return process.kind === 'terminal' ? 'live' : 'running';
  }

  function relativeTime(timestamp: number): string {
    const seconds = Math.max(0, Math.floor((Date.now() - timestamp) / 1_000));
    if (seconds < 5) return 'active now';
    if (seconds < 60) return `${seconds}s ago`;
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m ago`;
    return `${Math.floor(minutes / 60)}h ago`;
  }
</script>

{#if visibleKinds.length > 0}
  <span
    class:compact
    class="project-kind-indicators"
    data-project-kind-indicators
    data-compact={compact ? 'true' : 'false'}
    aria-label={`Running processes in ${projectTitle}`}
    role="group"
  >
    {#each visibleKinds as kind (kind)}
      {@const detail = activity[kind]}
      <span
        class:attention={kind === 'agent' && detail.needsInput > 0}
        class="project-kind-indicator"
      >
        <Popover.Root open={openKind === kind} onOpenChange={(open) => changeOpen(kind, open)}>
          <Popover.Trigger>
            {#snippet child({ props })}
              <IconButton
                {...props}
                class={compact ? 'project-kind-pip' : 'project-kind-trigger'}
                label={detail.activeLabel}
                data-project-kind={kind}
                data-project-kind-compact={compact ? 'true' : 'false'}
              >
                {#snippet icon()}
                  {#if compact}
                    <span class="pip-mark" aria-hidden="true"></span>
                  {:else}
                    <span class="kind-glyph" aria-hidden="true">
                      {#if kind === 'agent'}
                        <BotIcon size={13} strokeWidth={1.9} />
                      {:else if kind === 'terminal'}
                        <SquareTerminalIcon size={13} strokeWidth={1.9} />
                      {:else}
                        <PlayIcon size={13} strokeWidth={1.9} />
                      {/if}
                      {#if detail.active > 1}<small>{detail.active}</small>{/if}
                    </span>
                  {/if}
                {/snippet}
              </IconButton>
            {/snippet}
          </Popover.Trigger>
          <Popover.Content
            side="right"
            align="start"
            sideOffset={6}
            class="w-64 gap-0 p-1.5"
            aria-label={`${kindTitle(kind)} running in ${projectTitle}`}
            data-project-kind-popover={kind}
          >
            <header class="kind-popover-header">
              <span class:attention={kind === 'agent' && detail.needsInput > 0} aria-hidden="true">
                {#if kind === 'agent'}
                  <BotIcon size={14} strokeWidth={1.9} />
                {:else if kind === 'terminal'}
                  <SquareTerminalIcon size={14} strokeWidth={1.9} />
                {:else}
                  <PlayIcon size={14} strokeWidth={1.9} />
                {/if}
              </span>
              <span>
                <strong>{kindTitle(kind)}</strong>
                <small>{detail.activeLabel}</small>
              </span>
            </header>
            <div class="kind-process-list">
              {#each activeProcesses(kind) as process (process.id)}
                <button
                  type="button"
                  class:needs-input={agentNeedsInput(process)}
                  class="kind-process"
                  data-project-process-id={process.id}
                  onclick={() => chooseProcess(process)}
                >
                  <span class="process-name">{process.name}</span>
                  <span class="process-state">{stateLabel(process)}</span>
                </button>
              {/each}
            </div>
            <button class="show-all" type="button" onclick={() => showAll(kind)}>
              Show all {kindTitle(kind).toLocaleLowerCase()}
            </button>
          </Popover.Content>
        </Popover.Root>
      </span>
    {/each}
  </span>
{/if}

<style>
  .project-kind-indicators { display: inline-flex; flex: none; align-items: center; gap: 1px; }
  .project-kind-indicator { display: inline-flex; color: var(--agent-state-working); }
  .project-kind-indicator.attention { color: var(--warning-token); }
  .project-kind-indicator :global(.project-kind-trigger) { width: auto; min-width: 24px; height: 24px; gap: 2px; border-radius: var(--radius); padding: 0 4px; color: inherit; }
  .kind-glyph { display: inline-flex; align-items: center; gap: 2px; }
  .kind-glyph small { min-width: 8px; color: currentColor; font: 700 9px/1 var(--terminal-font-family); text-align: center; }

  .project-kind-indicators.compact { position: absolute; z-index: 4; right: 1px; bottom: 2px; align-items: end; gap: 1px; }
  .compact .project-kind-indicator :global(.project-kind-pip) { width: 7px; min-width: 7px; height: 7px; border: 1px solid var(--card); border-radius: 999px; padding: 0; background: var(--agent-state-working); color: transparent; }
  .compact .project-kind-indicator.attention :global(.project-kind-pip) { background: var(--warning-token); }
  .compact .project-kind-indicator :global(.project-kind-pip:focus-visible) { outline: 1px solid var(--ring); outline-offset: 1px; box-shadow: none; }
  .pip-mark { display: block; width: 100%; height: 100%; border-radius: inherit; }

  .kind-popover-header { display: flex; min-width: 0; align-items: center; gap: var(--space-2); border-bottom: 1px solid var(--border); padding: 4px 5px 7px; }
  .kind-popover-header > span:first-child { display: grid; width: 24px; height: 24px; flex: none; place-items: center; border: 1px solid var(--border); border-radius: var(--radius); color: var(--agent-state-working); background: var(--card); }
  .kind-popover-header > span:first-child.attention { color: var(--warning-token); }
  .kind-popover-header > span:last-child { min-width: 0; }
  .kind-popover-header strong, .kind-popover-header small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .kind-popover-header strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 650; }
  .kind-popover-header small { margin-top: 1px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .kind-process-list { display: grid; gap: 1px; padding: 4px 0; }
  .kind-process { display: grid; min-height: 34px; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: var(--space-2); border: 0; border-radius: var(--radius); padding: 4px 6px; background: transparent; text-align: left; cursor: pointer; }
  .kind-process:hover, .kind-process:focus-visible { outline: none; background: var(--accent); }
  .process-name { overflow: hidden; color: var(--foreground); font-size: var(--font-size-sm); font-weight: 590; text-overflow: ellipsis; white-space: nowrap; }
  .process-state { color: var(--agent-state-working); font: 600 var(--font-size-xs)/1 var(--terminal-font-family); white-space: nowrap; }
  .kind-process.needs-input .process-state { color: var(--warning-token); }
  .show-all { width: 100%; min-height: 28px; border: 0; border-top: 1px solid var(--border); border-radius: 0 0 var(--radius) var(--radius); padding: 6px; background: transparent; color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 620; text-align: left; cursor: pointer; }
  .show-all:hover, .show-all:focus-visible { outline: none; color: var(--foreground); background: var(--accent); }
</style>
