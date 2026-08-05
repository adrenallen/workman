<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
  import NotebookTextIcon from '@lucide/svelte/icons/notebook-text';
  import PlayIcon from '@lucide/svelte/icons/play';
  import SearchIcon from '@lucide/svelte/icons/search';
  import SettingsIcon from '@lucide/svelte/icons/settings';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import XIcon from '@lucide/svelte/icons/x';
  import { onMount } from 'svelte';
  import type { Component } from 'svelte';

  import CountBadge from './CountBadge.svelte';
  import InlineTreeRename from './InlineTreeRename.svelte';
  import MemoryBadge from './MemoryBadge.svelte';
  import AgentStatusIndicator from './components/ds/AgentStatusIndicator.svelte';
  import IconButton from './components/ds/IconButton.svelte';
  import StatusIndicator from './components/ds/StatusIndicator.svelte';
  import TooltipLabel from './components/ds/TooltipLabel.svelte';
  import {
    agentLineageRows,
    type AgentAttentionRollup
  } from './agentLineage';
  import type { ProcessKind, ProcessView, Project } from './daemon';
  import type { ScratchpadSummary, TodoSummary } from './coordination';
  import {
    contextMenuRequest,
    keyboardContextMenuRequest,
    type ContextMenuRequest,
    type ContextMenuTarget
  } from './contextMenu';
  import { liveStats, type ProcessRuntimeStats } from './liveStats';
  import {
    projectTreeSelection,
    type ProjectTreeGroup,
    type ProjectTreeSelection
  } from './projectTree';
  import { todoClaimLabel, todoClaimState, todoClaimTone } from './todoPresentation';
  import {
    moveOrderedId,
    moveTreeOrderBlock,
    reorderItem,
    siblingTarget,
    type ReorderDirection,
    type ReorderDrop,
    type ReorderItemOptions,
    type TreeOrderItem
  } from './reorder';

  interface Props {
    project: Project;
    processes: ProcessView[];
    todos: TodoSummary[];
    scratchpads: ScratchpadSummary[];
    selection: ProjectTreeSelection | null;
    collapsed: boolean;
    onSelect: (selection: ProjectTreeSelection) => void;
    onCreateTodo: () => void;
    onBrowseTodos: () => void;
    onAddAgent: () => void;
    onAddTerminal: () => void;
    onAddCommand: () => void;
    onAddScratchpad: () => void;
    onOpenSettings: () => void;
    onToggleCollapse: () => void;
    reordering: boolean;
    onReorderProcesses: (kind: ProcessKind, orderedIds: number[]) => void;
    renameTarget: ContextMenuTarget | null;
    onContextMenu: (request: ContextMenuRequest) => void;
    onRenameSubmit: (name: string) => void;
    onRenameCancel: () => void;
  }

  let {
    project,
    processes,
    todos,
    scratchpads,
    selection,
    collapsed,
    onSelect,
    onCreateTodo,
    onBrowseTodos,
    onAddAgent,
    onAddTerminal,
    onAddCommand,
    onAddScratchpad,
    onOpenSettings,
    onToggleCollapse,
    reordering,
    onReorderProcesses,
    renameTarget,
    onContextMenu,
    onRenameSubmit,
    onRenameCancel
  }: Props = $props();

  const groupOrder: ProjectTreeGroup[] = [
    'todos',
    'agents',
    'terminals',
    'commands',
    'scratchpads'
  ];
  const groupLabel: Record<ProjectTreeGroup, string> = {
    todos: 'Todos',
    agents: 'Agents',
    terminals: 'Terminals',
    commands: 'Commands',
    scratchpads: 'Scratchpads'
  };
  const groupIcon: Record<ProjectTreeGroup, Component> = {
    todos: CircleCheckIcon,
    agents: BotIcon,
    terminals: SquareTerminalIcon,
    commands: PlayIcon,
    scratchpads: NotebookTextIcon
  };

  let query = $state('');
  let showAllTodos = $state(false);
  let openGroups = $state<Record<ProjectTreeGroup, boolean>>({
    todos: true,
    agents: true,
    terminals: true,
    commands: true,
    scratchpads: true
  });

  let agents = $derived(processes.filter((process) => process.kind === 'agent'));
  let terminals = $derived(processes.filter((process) => process.kind === 'terminal'));
  let commands = $derived(processes.filter((process) => process.kind === 'command'));
  let openTodos = $derived(todos.filter((todo) => !todo.completed));
  let visibleTodos = $derived.by(() => {
    const matches = openTodos.filter((todo) => matchesQuery(todo.title));
    return showAllTodos || query.trim() ? matches : matches.slice(0, 5);
  });
  let visibleAgentRows = $derived(agentLineageRows(agents, query));
  let visibleTerminals = $derived(
    terminals.filter((process) => matchesQuery(`${workingDirLabel(process.working_dir)} ${process.name}`))
  );
  let visibleCommands = $derived(
    commands.filter((process) => matchesQuery(`${process.name} ${process.command ?? ''}`))
  );
  let visibleScratchpads = $derived(
    scratchpads.filter((scratchpad) => matchesQuery(`${scratchpad.name} ${scratchpad.tags.join(' ')}`))
  );
  let projectCounts = $derived($liveStats.counts[project.id]);

  onMount(() => {
    try {
      const saved = localStorage.getItem('workman.tree.groups.v1');
      if (!saved) return;
      const parsed = JSON.parse(saved) as Partial<Record<ProjectTreeGroup, boolean>>;
      openGroups = { ...openGroups, ...parsed };
    } catch {
      // Group expansion remains usable if local storage is unavailable.
    }
  });

  function matchesQuery(value: string): boolean {
    const needle = query.trim().toLowerCase();
    return !needle || value.toLowerCase().includes(needle);
  }

  function toggleGroup(group: ProjectTreeGroup): void {
    openGroups = { ...openGroups, [group]: !openGroups[group] };
    try {
      localStorage.setItem('workman.tree.groups.v1', JSON.stringify(openGroups));
    } catch {
      // Persistence is a convenience; the tree still works without it.
    }
  }

  function selectProcess(process: ProcessView): void {
    onSelect(
      projectTreeSelection(process.kind, process.id, project.id, processLabel(process))
    );
  }

  function processTarget(process: ProcessView): ContextMenuTarget {
    return {
      kind: 'process',
      process,
      selection: projectTreeSelection(
        process.kind,
        process.id,
        project.id,
        processLabel(process)
      )
    };
  }

  function todoTarget(todo: TodoSummary): ContextMenuTarget {
    return {
      kind: 'todo',
      todo,
      selection: projectTreeSelection('todo', todo.id, project.id, todo.title)
    };
  }

  function scratchpadTarget(scratchpad: ScratchpadSummary): ContextMenuTarget {
    return {
      kind: 'scratchpad',
      scratchpad,
      selection: projectTreeSelection('scratchpad', scratchpad.id, project.id, scratchpad.name)
    };
  }

  function openPointerMenu(event: MouseEvent, target: ContextMenuTarget): void {
    onContextMenu(contextMenuRequest(event, target));
  }

  function openKeyboardMenu(event: KeyboardEvent, target: ContextMenuTarget): void {
    const request = keyboardContextMenuRequest(event, target);
    if (request) onContextMenu(request);
  }

  function processLabel(process: ProcessView): string {
    return process.kind === 'terminal' ? workingDirLabel(process.working_dir) : process.name;
  }

  function workingDirLabel(path: string): string {
    const parts = path.split('/').filter(Boolean);
    if (parts[0] === 'Users' && parts.length > 2) return `~/${parts.slice(2).join('/')}`;
    return path;
  }

  function isRunning(process: ProcessView): boolean {
    return process.status === 'running' || process.status === 'starting';
  }

  function processAttention(process: ProcessView): 'working' | 'idle' | 'attention' | 'done' | 'error' {
    if (process.status === 'crashed') return 'error';
    if (process.status === 'exited' || process.status === 'stopped') return 'done';
    if (process.kind === 'agent' && process.agent_state.needs_input) return 'attention';
    if (process.kind === 'agent' && process.agent_state.working) return 'working';
    return isRunning(process) ? 'idle' : 'done';
  }

  function processStatusTone(process: ProcessView): 'success' | 'warning' | 'danger' | 'neutral' {
    const state = processAttention(process);
    if (state === 'attention') return 'warning';
    if (state === 'error') return 'danger';
    if (state === 'working' || state === 'idle') return 'success';
    return 'neutral';
  }

  function processStatusLabel(process: ProcessView): string {
    switch (processAttention(process)) {
      case 'working': return `${process.name} · working`;
      case 'attention': return `${process.name} · needs input`;
      case 'error': return `${process.name} · crashed`;
      case 'idle': return `${process.name} · running and idle`;
      default: return `${process.name} · ${process.status}`;
    }
  }

  function lineageTone(rollup: AgentAttentionRollup): 'attention' | 'working' | 'waiting' | 'error' | 'idle' {
    if (rollup.needsInput > 0) return 'attention';
    if (rollup.crashed > 0) return 'error';
    if (rollup.working > 0) return 'working';
    if (rollup.waiting > 0) return 'waiting';
    return 'idle';
  }

  function lineageTitle(rollup: AgentAttentionRollup): string {
    const states = [];
    if (rollup.needsInput > 0) states.push(`${rollup.needsInput} need input`);
    if (rollup.working > 0) states.push(`${rollup.working} working`);
    if (rollup.waiting > 0) states.push(`${rollup.waiting} waiting for timer`);
    if (rollup.crashed > 0) states.push(`${rollup.crashed} crashed`);
    const suffix = states.length > 0 ? ` · ${states.join(', ')}` : '';
    return `${rollup.total} nested agent${rollup.total === 1 ? '' : 's'}${suffix}`;
  }

  function todoStatusLabel(todo: TodoSummary): string {
    return `${todo.title} · ${todoClaimLabel(todo)}`;
  }

  function groupCount(group: ProjectTreeGroup): string {
    switch (group) {
      case 'todos': return String(openTodos.length);
      case 'agents': return `${projectCounts?.agent_running ?? agents.filter(isRunning).length}/${projectCounts?.agent_total ?? agents.length}`;
      case 'terminals': return `${projectCounts?.terminal_running ?? terminals.filter(isRunning).length}/${projectCounts?.terminal_total ?? terminals.length}`;
      case 'commands': return `${projectCounts?.command_running ?? commands.filter(isRunning).length}/${projectCounts?.command_total ?? commands.length}`;
      case 'scratchpads': return String(scratchpads.length);
    }
  }

  function groupTone(group: ProjectTreeGroup): 'neutral' | 'running' | 'attention' {
    if (group === 'todos' && openTodos.some((todo) => todo.is_blocked)) return 'attention';
    if (group === 'agents' && agents.some((process) => process.agent_state.needs_input)) return 'attention';
    if (group !== 'todos' && group !== 'scratchpads' && processesForGroup(group).some(isRunning)) return 'running';
    return 'neutral';
  }

  function groupCountTitle(group: ProjectTreeGroup): string {
    const value = groupCount(group);
    if (group === 'todos' || group === 'scratchpads') return `${value} ${group}`;
    const [running, total] = value.split('/');
    return `${running} running of ${total} ${group}`;
  }

  function processesForGroup(group: ProjectTreeGroup): ProcessView[] {
    if (group === 'agents') return agents;
    if (group === 'terminals') return terminals;
    if (group === 'commands') return commands;
    return [];
  }

  function runtimeStats(process: ProcessView): ProcessRuntimeStats | undefined {
    return $liveStats.processes[process.id];
  }

  function reorderOptions(process: ProcessView): ReorderItemOptions {
    const group = processesForKind(process.kind);
    return {
      id: process.id,
      group: `process:${project.id}:${process.kind}`,
      disabled: reordering || Boolean(query.trim()) || group.length < 2,
      label: processLabel(process),
      onDrop: (drop) => handleProcessDrop(process.kind, drop),
      onKeyboardMove: (id, direction) => moveProcessFromKeyboard(process.kind, id, direction)
    };
  }

  function processesForKind(kind: ProcessKind): ProcessView[] {
    if (kind === 'agent') return agents;
    if (kind === 'terminal') return terminals;
    return commands;
  }

  function handleProcessDrop(kind: ProcessKind, drop: ReorderDrop): void {
    const orderedIds = kind === 'agent'
      ? moveTreeOrderBlock(agentTreeOrder(), drop.sourceId, drop.targetId, drop.placement)
      : moveOrderedId(
          processesForKind(kind).map((process) => process.id),
          drop.sourceId,
          drop.targetId,
          drop.placement
        );
    onReorderProcesses(kind, orderedIds);
  }

  function moveProcessFromKeyboard(
    kind: ProcessKind,
    processId: number,
    direction: ReorderDirection
  ): void {
    if (kind === 'agent') {
      const items = agentTreeOrder();
      const targetId = siblingTarget(items, processId, direction);
      if (targetId === null) return;
      handleProcessDrop(kind, {
        sourceId: processId,
        targetId,
        placement: direction < 0 ? 'before' : 'after'
      });
      return;
    }

    const orderedIds = processesForKind(kind).map((process) => process.id);
    const index = orderedIds.indexOf(processId);
    const targetId = orderedIds[index + direction];
    if (targetId === undefined) return;
    handleProcessDrop(kind, {
      sourceId: processId,
      targetId,
      placement: direction < 0 ? 'before' : 'after'
    });
  }

  function agentTreeOrder(): TreeOrderItem[] {
    const stack: number[] = [];
    return agentLineageRows(agents, '').map((row) => {
      const parentId = row.depth > 0 ? (stack[row.depth - 1] ?? null) : null;
      stack[row.depth] = row.process.id;
      stack.length = row.depth + 1;
      return { id: row.process.id, parentId };
    });
  }

  function handleTreeKeys(event: KeyboardEvent): void {
    const target = event.target as HTMLElement | null;
    if (!target || target.matches('input')) return;
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if ((event.key === 'ArrowLeft' || event.key === 'ArrowRight') && target.dataset.group) {
      const group = target.dataset.group as ProjectTreeGroup;
      const shouldOpen = event.key === 'ArrowRight';
      if (openGroups[group] !== shouldOpen) toggleGroup(group);
      event.preventDefault();
      return;
    }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    const rows = Array.from(
      (event.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[data-tree-row]:not(:disabled)')
    );
    const current = rows.indexOf(target.closest<HTMLElement>('[data-tree-row]') ?? target);
    const delta = event.key === 'ArrowDown' ? 1 : -1;
    const next = current < 0 ? 0 : Math.min(rows.length - 1, Math.max(0, current + delta));
    rows[next]?.focus();
    event.preventDefault();
  }
</script>

<section class="project-tree" class:collapsed aria-label={`${project.name} project tree`}>
  <header class="tree-toolbar" data-tauri-drag-region>
    {#if !collapsed}
      <label class="tree-filter">
        <SearchIcon size={14} strokeWidth={1.8} aria-hidden="true" />
        <input bind:value={query} placeholder="Filter processes..." aria-label="Filter project tree" />
        {#if query}
          <IconButton label="Clear project filter" onclick={() => (query = '')}>
            {#snippet icon()}<XIcon size={13} />{/snippet}
          </IconButton>
        {/if}
      </label>
    {/if}
    <IconButton
      class="size-7 shrink-0 rounded border border-border bg-card"
      label={`${collapsed ? 'Expand' : 'Collapse'} project tree`}
      shortcut="⌘⇧B"
      onclick={onToggleCollapse}
    >
      {#snippet icon()}
        {#if collapsed}<ChevronRightIcon size={15} />{:else}<ChevronLeftIcon size={15} />{/if}
      {/snippet}
    </IconButton>
  </header>

  <div class="tree-groups" aria-label="Project items" role="tree" tabindex="-1" onkeydown={handleTreeKeys}>
    {#each groupOrder as group}
      {@const GroupIcon = groupIcon[group]}
      <section class="tree-group" class:closed={!openGroups[group]}>
        <button
          class="group-header"
          type="button"
          data-tree-row
          data-group={group}
          aria-expanded={openGroups[group]}
          title={group === 'todos' ? 'Browse all todos · Left/Right collapses this group' : collapsed ? groupLabel[group] : undefined}
          onclick={() => (group === 'todos' ? onBrowseTodos() : toggleGroup(group))}
        >
          <span class="caret" aria-hidden="true">
            {#if openGroups[group]}<ChevronDownIcon size={13} />{:else}<ChevronRightIcon size={13} />{/if}
          </span>
          <span class="group-icon" aria-hidden="true"><GroupIcon size={14} strokeWidth={1.8} /></span>
          <strong>{groupLabel[group]}</strong>
          <CountBadge value={groupCount(group)} tone={groupTone(group)} title={groupCountTitle(group)} />
        </button>

        {#if openGroups[group] && !collapsed}
          <div class="group-rows">
            {#if group === 'todos'}
              {#each visibleTodos as todo (todo.id)}
                <button
                  type="button"
                  class="tree-row todo-row"
                  class:selected={selection?.key === `todo:${todo.id}`}
                  data-todo-state={todoClaimState(todo)}
                  data-tree-row
                  data-context-kind="todo"
                  data-context-id={todo.id}
                  onclick={() => onSelect(projectTreeSelection('todo', todo.id, project.id, todo.title))}
                  oncontextmenu={(event) => openPointerMenu(event, todoTarget(todo))}
                  onkeydown={(event) => openKeyboardMenu(event, todoTarget(todo))}
                >
                  <span class="todo-state-rail" aria-hidden="true"></span>
                  <StatusIndicator tone={todoClaimTone(todo)} label={todoStatusLabel(todo)} />
                  <span class="row-copy"><strong>{todo.title}</strong></span>
                  {#if todo.comment_count > 0}<span class="row-meta" title={`${todo.comment_count} comments`}>{todo.comment_count}</span>{/if}
                </button>
              {:else}
                <p class="empty-row">{query ? 'No matching todos' : 'No open todos'}</p>
              {/each}
              {#if !query && openTodos.length > 5}
                <button class="show-all" type="button" data-tree-row onclick={() => (showAllTodos = !showAllTodos)}>
                  {showAllTodos ? 'Show first 5' : `Show all ${openTodos.length} todos`}
                </button>
              {/if}
              <button class="add-row" type="button" data-tree-row onclick={onCreateTodo}>+ Add todo</button>
            {:else if group === 'agents'}
              {#each visibleAgentRows as row (row.process.id)}
                {@const process = row.process}
                {@const stats = runtimeStats(process)}
                {#if renameTarget?.kind === 'process' && renameTarget.process.id === process.id}
                  <InlineTreeRename value={process.name} label="Agent name" depth={row.depth} onSubmit={onRenameSubmit} onCancel={onRenameCancel} />
                {:else}
                  <button
                    type="button"
                    class="tree-row agent-row"
                    class:agent-child={row.depth > 0}
                    class:selected={selection?.key === `agent:${process.id}`}
                    style={`--agent-depth: ${row.depth}`}
                    data-tree-row
                    data-context-kind="agent"
                    data-context-id={process.id}
                    use:reorderItem={reorderOptions(process)}
                    onclick={() => selectProcess(process)}
                    oncontextmenu={(event) => openPointerMenu(event, processTarget(process))}
                    onkeydown={(event) => openKeyboardMenu(event, processTarget(process))}
                  >
                    {#if row.depth > 0}<span class="lineage-glyph" aria-hidden="true">└</span>{/if}
                    <AgentStatusIndicator {process} />
                    <span class="row-copy"><strong>{process.name}</strong></span>
                    {#if row.rollup.total > 0 || stats}
                      <span class="row-badges">
                        {#if row.rollup.total > 0}
                          <TooltipLabel label={lineageTitle(row.rollup)}>
                            <span
                              class={`lineage-rollup ${lineageTone(row.rollup)}`}
                              aria-label={lineageTitle(row.rollup)}
                            >↳{row.rollup.total}</span>
                          </TooltipLabel>
                        {/if}
                        {#if stats?.descendant_count}
                          <CountBadge prefix="+" value={stats.descendant_count} title={`${stats.descendant_count} subprocesses`} />
                        {/if}
                        {#if stats}<MemoryBadge bytes={stats.memory_bytes} />{/if}
                      </span>
                    {/if}
                  </button>
                {/if}
              {:else}
                <p class="empty-row">{query ? 'No matching agents' : 'No agents'}</p>
              {/each}
              <button class="add-row" type="button" data-tree-row onclick={onAddAgent}>+ Add agent</button>
            {:else if group === 'terminals'}
              {#each visibleTerminals as process (process.id)}
                {@const stats = runtimeStats(process)}
                {#if renameTarget?.kind === 'process' && renameTarget.process.id === process.id}
                  <InlineTreeRename value={process.name} label="Terminal name" onSubmit={onRenameSubmit} onCancel={onRenameCancel} />
                {:else}
                  <button
                    type="button"
                    class="tree-row"
                    class:selected={selection?.key === `terminal:${process.id}`}
                    data-tree-row
                    data-context-kind="terminal"
                    data-context-id={process.id}
                    use:reorderItem={reorderOptions(process)}
                    onclick={() => selectProcess(process)}
                    oncontextmenu={(event) => openPointerMenu(event, processTarget(process))}
                    onkeydown={(event) => openKeyboardMenu(event, processTarget(process))}
                  >
                    <StatusIndicator tone={processStatusTone(process)} label={processStatusLabel(process)} />
                    <span class="row-copy"><strong>{workingDirLabel(process.working_dir)}</strong></span>
                    {#if stats}<span class="row-badges">{#if stats.descendant_count > 0}<CountBadge prefix="+" value={stats.descendant_count} title={`${stats.descendant_count} subprocesses`} />{/if}<MemoryBadge bytes={stats.memory_bytes} /></span>{/if}
                  </button>
                {/if}
              {:else}
                <p class="empty-row">{query ? 'No matching terminals' : 'No terminals'}</p>
              {/each}
              <button class="add-row" type="button" data-tree-row onclick={onAddTerminal}>+ New terminal</button>
            {:else if group === 'commands'}
              {#each visibleCommands as process (process.id)}
                {@const stats = runtimeStats(process)}
                {#if renameTarget?.kind === 'process' && renameTarget.process.id === process.id}
                  <InlineTreeRename value={process.name} label="Command name" onSubmit={onRenameSubmit} onCancel={onRenameCancel} />
                {:else}
                  <button
                    type="button"
                    class="tree-row command-row"
                    class:selected={selection?.key === `command:${process.id}`}
                    data-tree-row
                    data-context-kind="command"
                    data-context-id={process.id}
                    use:reorderItem={reorderOptions(process)}
                    onclick={() => selectProcess(process)}
                    oncontextmenu={(event) => openPointerMenu(event, processTarget(process))}
                    onkeydown={(event) => openKeyboardMenu(event, processTarget(process))}
                  >
                    <StatusIndicator tone={processStatusTone(process)} label={processStatusLabel(process)} />
                    <span class="row-copy"><strong>{process.name}</strong><small>{process.command ?? 'Command'}</small></span>
                    <span class="row-badges">{#if stats}{#if stats.descendant_count > 0}<CountBadge prefix="+" value={stats.descendant_count} title={`${stats.descendant_count} subprocesses`} />{/if}<MemoryBadge bytes={stats.memory_bytes} />{/if}{#if !isRunning(process)}<span class="run-hint">Run</span>{/if}</span>
                  </button>
                {/if}
              {:else}
                <p class="empty-row">{query ? 'No matching commands' : 'No commands in workman.yml'}</p>
              {/each}
              <button class="add-row" type="button" data-tree-row onclick={onAddCommand}>+ Add command</button>
            {:else}
              {#each visibleScratchpads as scratchpad (scratchpad.id)}
                {#if renameTarget?.kind === 'scratchpad' && renameTarget.scratchpad.id === scratchpad.id}
                  <InlineTreeRename value={scratchpad.name} label="Scratchpad name" onSubmit={onRenameSubmit} onCancel={onRenameCancel} />
                {:else}
                  <button
                    type="button"
                    class="tree-row"
                    class:selected={selection?.key === `scratchpad:${scratchpad.id}`}
                    data-tree-row
                    data-context-kind="scratchpad"
                    data-context-id={scratchpad.id}
                    onclick={() => onSelect(projectTreeSelection('scratchpad', scratchpad.id, project.id, scratchpad.name))}
                    oncontextmenu={(event) => openPointerMenu(event, scratchpadTarget(scratchpad))}
                    onkeydown={(event) => openKeyboardMenu(event, scratchpadTarget(scratchpad))}
                  >
                    <TooltipLabel label={`Scratchpad · revision ${scratchpad.revision}`}>
                      <NotebookTextIcon class="scratchpad-icon" size={14} strokeWidth={1.8} aria-hidden="true" />
                    </TooltipLabel>
                    <span class="row-copy"><strong>{scratchpad.name}</strong></span>
                    <span class="row-meta" title={`Scratchpad revision ${scratchpad.revision}`}>r{scratchpad.revision}</span>
                  </button>
                {/if}
              {:else}
                <p class="empty-row">{query ? 'No matching scratchpads' : 'No scratchpads'}</p>
              {/each}
              <button class="add-row" type="button" data-tree-row onclick={onAddScratchpad}>+ Add scratchpad</button>
            {/if}
          </div>
        {/if}
      </section>
    {/each}
  </div>

  <footer class="tree-footer">
    <IconButton class="size-7" label="Open Settings" shortcut="⌘," data-tree-row onclick={onOpenSettings}>
      {#snippet icon()}<SettingsIcon size={15} strokeWidth={1.8} />{/snippet}
    </IconButton>
  </footer>
</section>

<style>
  .project-tree { display: grid; width: 100%; height: 100%; min-width: 0; grid-template-rows: auto minmax(0, 1fr) auto; background: var(--card); color: var(--text-soft); }
  .tree-toolbar { display: flex; min-height: 38px; align-items: center; gap: 5px; padding: 5px 6px; border-bottom: 1px solid var(--border); }
  .tree-filter { display: flex; min-width: 0; flex: 1; align-items: center; gap: 5px; height: 28px; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 7px; background: var(--background); color: var(--muted); }
  .tree-filter input { min-width: 0; flex: 1; border: 0; outline: 0; padding: 0; background: transparent; color: var(--text); font-size: var(--font-size-sm); }
  .tree-filter input::placeholder { color: var(--muted-foreground); }

  .tree-groups { min-height: 0; overflow-y: auto; padding: 3px 0 5px; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .tree-group { border-bottom: 1px solid var(--border); }
  .group-header { display: grid; width: 100%; min-height: 28px; grid-template-columns: 13px 16px minmax(0, 1fr) auto; align-items: center; gap: 4px; border: 0; padding: 3px 7px 3px 6px; background: transparent; color: var(--text-soft); text-align: left; cursor: pointer; }
  .group-header:hover { background: var(--popover); }
  .group-header:focus-visible { position: relative; z-index: 1; }
  .group-header strong { overflow: hidden; font-size: var(--font-size-sm); font-weight: 700; letter-spacing: 0.055em; text-overflow: ellipsis; text-transform: uppercase; white-space: nowrap; }
  .caret { color: var(--muted-foreground); font: var(--font-size-sm) 'JetBrains Mono Variable', monospace; }
  .group-icon { color: var(--muted-foreground); font: var(--font-size-sm) 'JetBrains Mono Variable', monospace; text-align: center; }
  .row-meta, .run-hint { flex: none; border: 1px solid var(--border-strong); border-radius: 3px; padding: 1px 4px; color: var(--text-soft); background: var(--popover); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .group-rows { padding: 0 4px 4px 13px; }
  .tree-row, .add-row, .show-all { display: grid; width: 100%; min-height: 28px; align-items: center; border: 0; border-radius: 3px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .tree-row { position: relative; grid-template-columns: 17px minmax(0, 1fr) auto; gap: 4px; padding: 3px 5px; }
  .todo-row { min-height: 24px; grid-template-columns: 2px 15px minmax(0, 1fr) auto; gap: 3px; padding-block: 1px; }
  .todo-row .todo-state-rail { align-self: stretch; border-radius: 1px; background: var(--ring); }
  .todo-row[data-todo-state='claimed'] .todo-state-rail { background: var(--warning); }
  .todo-row[data-todo-state='blocked'] .todo-state-rail { background: var(--destructive); }
  .todo-row[data-todo-state='completed'] .todo-state-rail { background: var(--muted-foreground); opacity: 0.6; }
  .todo-row[data-todo-state='claimed'] { background: color-mix(in srgb, var(--warning) 5%, transparent); }
  .todo-row[data-todo-state='blocked'] { background: color-mix(in srgb, var(--destructive) 6%, transparent); }
  .todo-row .row-copy strong { font-size: var(--font-size-xs); font-weight: 570; }
  .project-tree :global(.tree-row[data-reorderable='true']) { cursor: grab; }
  .project-tree :global(.tree-row[data-reorder-dragging='true']) { opacity: 0.42; }
  .project-tree :global(.tree-row[data-reorder-drop]::after) { position: absolute; z-index: 3; right: 4px; left: 4px; height: 1px; background: var(--signal); box-shadow: 0 0 0 1px rgb(95 214 183 / 16%), 0 0 8px rgb(95 214 183 / 48%); content: ''; pointer-events: none; }
  .project-tree :global(.tree-row[data-reorder-drop='before']::after) { top: -1px; }
  .project-tree :global(.tree-row[data-reorder-drop='after']::after) { bottom: -1px; }
  .agent-row.agent-child { width: calc(100% - min(calc(var(--agent-depth) * 12px), 48px)); grid-template-columns: 12px 17px minmax(0, 1fr) auto; margin-left: min(calc(var(--agent-depth) * 12px), 48px); }
  .tree-row:hover, .add-row:hover, .show-all:hover { background: var(--popover); }
  .tree-row.selected { background: var(--accent); color: #fff; box-shadow: inset 2px 0 var(--muted-foreground); }
  .row-copy { min-width: 0; }
  .row-copy strong, .row-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row-copy strong { font-size: var(--font-size-sm); font-weight: 590; }
  .row-copy small { margin-top: 1px; color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .row-badges { display: flex; min-width: 0; align-items: center; justify-content: flex-end; gap: 3px; }
  .lineage-glyph { color: #687e74; font: var(--font-size-sm)/1 'JetBrains Mono Variable', monospace; transform: translateY(-1px); }
  .lineage-rollup { display: inline-flex; min-width: 20px; height: 18px; align-items: center; justify-content: center; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 4px; background: #19201d; color: #8ca297; font: 650 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace; }
  .lineage-rollup.attention { border-color: color-mix(in srgb, var(--warning) 48%, var(--border)); color: var(--warning); }
  .lineage-rollup.working { border-color: color-mix(in srgb, var(--signal) 42%, var(--border)); color: var(--signal); }
  .lineage-rollup.waiting { border-color: color-mix(in srgb, var(--information) 42%, var(--border)); color: var(--information); }
  .lineage-rollup.error { border-color: color-mix(in srgb, var(--fault) 42%, var(--border)); color: var(--fault); }
  .scratchpad-icon { color: var(--muted-foreground); }
  .add-row, .show-all { grid-template-columns: 1fr; padding: 3px 5px 3px 22px; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .add-row { color: var(--text-soft); }
  .empty-row { margin: 0; padding: 5px 5px 5px 22px; color: #686f78; font-size: var(--font-size-sm); }
  .run-hint { border-color: #44504a; color: #aab8b0; }

  .tree-footer { display: flex; min-height: 38px; align-items: center; justify-content: flex-end; padding: 5px 6px; border-top: 1px solid var(--border); }

  .project-tree.collapsed .tree-toolbar { justify-content: center; padding-inline: 0; }
  .project-tree.collapsed .tree-groups { padding-inline: 4px; }
  .project-tree.collapsed .tree-group { border: 0; }
  .project-tree.collapsed .group-header { min-height: 36px; grid-template-columns: 1fr; justify-items: center; padding: 4px; }
  .project-tree.collapsed .caret,
  .project-tree.collapsed .group-header strong,
  .project-tree.collapsed .group-header :global(.badge) { display: none; }
  .project-tree.collapsed .group-icon { font-size: var(--font-size-sm); }
  .project-tree.collapsed .tree-footer { justify-content: center; padding-inline: 4px; }
</style>
