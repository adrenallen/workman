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
    projectId: number;
    projectTitle: string;
    openPopoverKey: string | null;
    onOpenPopoverChange: (key: string | null) => void;
    compact?: boolean;
    onSelect: (process: ProcessView) => void;
    onShowAll: (kind: ProcessKind) => void;
  }

  let {
    activity,
    processes,
    projectId,
    projectTitle,
    openPopoverKey,
    onOpenPopoverChange,
    compact = false,
    onSelect,
    onShowAll
  }: Props = $props();

  const kinds = ['agent', 'terminal', 'command'] as const;

  function popoverKey(kind: ProcessKind): string {
    return `${projectId}:process:${kind}`;
  }

  function kindIsOpen(kind: ProcessKind): boolean {
    return openPopoverKey === popoverKey(kind);
  }

  function kindProcesses(kind: ProcessKind): ProcessView[] {
    const byId = new Map(processes.map((process) => [process.id, process]));
    return activity[kind].processIds
      .map((id) => byId.get(id))
      .filter((process): process is ProcessView => process !== undefined);
  }

  function changeOpen(kind: ProcessKind, open: boolean): void {
    if (open) {
      onOpenPopoverChange(popoverKey(kind));
    } else if (kindIsOpen(kind)) {
      onOpenPopoverChange(null);
    }
  }

  function togglePopover(kind: ProcessKind, event: MouseEvent): void {
    event.stopPropagation();
    onOpenPopoverChange(kindIsOpen(kind) ? null : popoverKey(kind));
  }

  function chooseProcess(process: ProcessView): void {
    onOpenPopoverChange(null);
    onSelect(process);
  }

  function showAll(kind: ProcessKind): void {
    onOpenPopoverChange(null);
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
    if (process.status === 'crashed') return 'crashed';
    if (process.status === 'stopped' || process.status === 'exited') return process.status;
    if (process.kind === 'agent') {
      if (agentNeedsInput(process)) return 'needs input';
      if (String(process.agent_state.state) === 'waiting') return 'waiting';
      if (process.status === 'starting') return 'starting';
      return process.agent_state.working ? 'working' : 'idle';
    }
    if (process.status === 'starting') return 'starting';
    if (process.kind === 'terminal') {
      return activity.terminal.activeProcessIds.includes(process.id) ? 'live' : 'idle';
    }
    return 'running';
  }

  function compactCount(count: number): string {
    return count > 99 ? '99+' : String(count);
  }
</script>

<span
  class:compact
  class="project-kind-indicators"
  data-project-kind-indicators
  data-compact={compact ? 'true' : 'false'}
  aria-label={`Processes in ${projectTitle}`}
  role="group"
>
    {#each kinds as kind (kind)}
      {@const detail = activity[kind]}
      <span
        class="project-kind-indicator"
        data-tone={detail.tone}
      >
        <Popover.Root open={kindIsOpen(kind)} onOpenChange={(open) => changeOpen(kind, open)}>
          <Popover.Trigger>
            {#snippet child({ props })}
              <IconButton
                {...props}
                class={compact ? 'project-kind-pip' : 'project-kind-trigger'}
                label={detail.label}
                data-project-kind={kind}
                data-project-kind-compact={compact ? 'true' : 'false'}
                aria-expanded={kindIsOpen(kind)}
                onclick={(event) => togglePopover(kind, event)}
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
                      {#if detail.active > 1}<small>{compactCount(detail.active)}</small>{/if}
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
            aria-label={`${kindTitle(kind)} in ${projectTitle}`}
            data-project-kind-popover={kind}
          >
            <header class="kind-popover-header">
              <span data-tone={detail.tone} aria-hidden="true">
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
                <small>{detail.label}</small>
              </span>
            </header>
            <div class="kind-process-list">
              {#each kindProcesses(kind) as process (process.id)}
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
              {:else}
                <p class="kind-empty">No {kindTitle(kind).toLocaleLowerCase()} in this project</p>
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

<style>
  .project-kind-indicators { display: inline-flex; min-width: 0; min-height: 20px; flex: none; align-items: center; gap: 0; }
  .project-kind-indicator { --indicator-tone: var(--agent-state-working); display: inline-flex; color: var(--indicator-tone); }
  .project-kind-indicator[data-tone='needs-input'] { --indicator-tone: var(--warning-token); }
  .project-kind-indicator[data-tone='idle'] { --indicator-tone: var(--muted-foreground); }
  .project-kind-indicator :global(.project-kind-trigger) { width: 18px; min-width: 18px; height: 20px; border: 1px solid var(--border); border-radius: var(--radius); padding: 0; color: inherit; background: var(--card); }
  .kind-glyph { position: relative; display: grid; width: 14px; height: 14px; place-items: center; }
  .kind-glyph small { position: absolute; top: -5px; right: -5px; display: grid; min-width: 11px; height: 11px; place-items: center; border: 1px solid var(--card); border-radius: 999px; padding: 0 1px; color: var(--card); background: var(--indicator-tone); font: 750 8px/1 var(--terminal-font-family); text-align: center; }

  .project-kind-indicators.compact { position: absolute; z-index: 4; right: 0; bottom: 0; align-items: end; gap: 0; }
  .compact .project-kind-indicator :global(.project-kind-pip) { display: grid; width: 12px; min-width: 12px; height: 12px; place-items: center; border: 0; border-radius: 0; padding: 0; background: transparent; color: transparent; }
  .compact .project-kind-indicator :global(.project-kind-pip:focus-visible) { outline: 1px solid var(--ring); outline-offset: 1px; box-shadow: none; }
  .pip-mark { display: block; width: 7px; height: 7px; border: 1px solid var(--card); border-radius: 999px; background: var(--agent-state-working); }
  .project-kind-indicator[data-tone='needs-input'] .pip-mark { background: var(--warning-token); }
  .project-kind-indicator[data-tone='idle'] .pip-mark { background: var(--muted-foreground); opacity: .58; }

  .kind-popover-header { display: flex; min-width: 0; align-items: center; gap: var(--space-2); border-bottom: 1px solid var(--border); padding: 4px 5px 7px; }
  .kind-popover-header > span:first-child { display: grid; width: 24px; height: 24px; flex: none; place-items: center; border: 1px solid var(--border); border-radius: var(--radius); color: var(--agent-state-working); background: var(--card); }
  .kind-popover-header > span:first-child[data-tone='needs-input'] { color: var(--warning-token); }
  .kind-popover-header > span:last-child { min-width: 0; }
  .kind-popover-header strong, .kind-popover-header small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .kind-popover-header strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 650; }
  .kind-popover-header small { margin-top: 1px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .kind-process-list { display: grid; gap: 1px; padding: 4px 0; }
  .kind-empty { margin: 0; padding: 8px 6px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .kind-process { display: grid; min-height: 34px; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: var(--space-2); border: 0; border-radius: var(--radius); padding: 4px 6px; background: transparent; text-align: left; cursor: pointer; }
  .kind-process:hover, .kind-process:focus-visible { outline: none; background: var(--accent); }
  .process-name { overflow: hidden; color: var(--foreground); font-size: var(--font-size-sm); font-weight: 590; text-overflow: ellipsis; white-space: nowrap; }
  .process-state { color: var(--agent-state-working); font: 600 var(--font-size-xs)/1 var(--terminal-font-family); white-space: nowrap; }
  .kind-process.needs-input .process-state { color: var(--warning-token); }
  .show-all { width: 100%; min-height: 28px; border: 0; border-top: 1px solid var(--border); border-radius: 0 0 var(--radius) var(--radius); padding: 6px; background: transparent; color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 620; text-align: left; cursor: pointer; }
  .show-all:hover, .show-all:focus-visible { outline: none; color: var(--foreground); background: var(--accent); }
</style>
