<script lang="ts">
  import { onMount } from 'svelte';

  import CountBadge from './CountBadge.svelte';
  import InlineTreeRename from './InlineTreeRename.svelte';
  import MemoryBadge from './MemoryBadge.svelte';
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
    connected: boolean;
    onSelect: (selection: ProjectTreeSelection) => void;
    onCreateTodo: () => void;
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
    connected,
    onSelect,
    onCreateTodo,
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
  const groupIcon: Record<ProjectTreeGroup, string> = {
    todos: '◇',
    agents: '◎',
    terminals: '>_',
    commands: '▤',
    scratchpads: '≡'
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
      const saved = localStorage.getItem('gbuild.tree.groups.v1');
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
      localStorage.setItem('gbuild.tree.groups.v1', JSON.stringify(openGroups));
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

  function processGlyph(process: ProcessView): string {
    switch (processAttention(process)) {
      case 'working': return '';
      case 'attention': return '!';
      case 'done': return '✓';
      case 'error': return '×';
      default: return '';
    }
  }

  function lineageTone(rollup: AgentAttentionRollup): 'attention' | 'working' | 'error' | 'idle' {
    if (rollup.needsInput > 0) return 'attention';
    if (rollup.crashed > 0) return 'error';
    if (rollup.working > 0) return 'working';
    return 'idle';
  }

  function lineageTitle(rollup: AgentAttentionRollup): string {
    const states = [];
    if (rollup.needsInput > 0) states.push(`${rollup.needsInput} need input`);
    if (rollup.working > 0) states.push(`${rollup.working} working`);
    if (rollup.crashed > 0) states.push(`${rollup.crashed} crashed`);
    const suffix = states.length > 0 ? ` · ${states.join(', ')}` : '';
    return `${rollup.total} nested agent${rollup.total === 1 ? '' : 's'}${suffix}`;
  }

  function todoGlyph(todo: TodoSummary): string {
    if (todo.is_blocked) return '!';
    if (todo.status === 'in_progress') return '◒';
    if (todo.status === 'backlog') return '◇';
    return '○';
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
        <span aria-hidden="true">⌕</span>
        <input bind:value={query} placeholder="Filter processes..." aria-label="Filter project tree" />
        {#if query}<button type="button" aria-label="Clear filter" onclick={() => (query = '')}>×</button>{/if}
      </label>
    {/if}
    <button
      class="collapse-button"
      type="button"
      aria-label={`${collapsed ? 'Expand' : 'Collapse'} project tree`}
      title={`${collapsed ? 'Expand' : 'Collapse'} project tree (⌘⇧B)`}
      onclick={onToggleCollapse}
    >{collapsed ? '›' : '‹'}</button>
  </header>

  <div class="tree-groups" aria-label="Project items" role="tree" tabindex="-1" onkeydown={handleTreeKeys}>
    {#each groupOrder as group}
      <section class="tree-group" class:closed={!openGroups[group]}>
        <button
          class="group-header"
          type="button"
          data-tree-row
          data-group={group}
          aria-expanded={openGroups[group]}
          title={collapsed ? groupLabel[group] : undefined}
          onclick={() => toggleGroup(group)}
        >
          <span class="caret" aria-hidden="true">{openGroups[group] ? '⌄' : '›'}</span>
          <span class="group-icon" aria-hidden="true">{groupIcon[group]}</span>
          <strong>{groupLabel[group]}</strong>
          <CountBadge value={groupCount(group)} tone={groupTone(group)} />
        </button>

        {#if openGroups[group] && !collapsed}
          <div class="group-rows">
            {#if group === 'todos'}
              {#each visibleTodos as todo (todo.id)}
                <button
                  type="button"
                  class="tree-row"
                  class:selected={selection?.key === `todo:${todo.id}`}
                  data-tree-row
                  data-context-kind="todo"
                  data-context-id={todo.id}
                  onclick={() => onSelect(projectTreeSelection('todo', todo.id, project.id, todo.title))}
                  oncontextmenu={(event) => openPointerMenu(event, todoTarget(todo))}
                  onkeydown={(event) => openKeyboardMenu(event, todoTarget(todo))}
                >
                  <span class:attention={todo.is_blocked} class="attention-dot" aria-hidden="true">{todoGlyph(todo)}</span>
                  <span class="row-copy"><strong>{todo.title}</strong></span>
                  {#if todo.comment_count > 0}<span class="row-meta">{todo.comment_count}</span>{/if}
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
                    <span class={`attention-dot ${processAttention(process)}`} aria-hidden="true">{processGlyph(process)}</span>
                    <span class="row-copy"><strong>{process.name}</strong></span>
                    {#if row.rollup.total > 0 || stats}
                      <span class="row-badges">
                        {#if row.rollup.total > 0}
                          <span
                            class={`lineage-rollup ${lineageTone(row.rollup)}`}
                            title={lineageTitle(row.rollup)}
                            aria-label={lineageTitle(row.rollup)}
                          >↳{row.rollup.total}</span>
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
                    <span class={`attention-dot ${processAttention(process)}`} aria-hidden="true">{processGlyph(process)}</span>
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
                    <span class={`attention-dot ${processAttention(process)}`} aria-hidden="true">{processGlyph(process)}</span>
                    <span class="row-copy"><strong>{process.name}</strong><small>{process.command ?? 'Command'}</small></span>
                    <span class="row-badges">{#if stats}{#if stats.descendant_count > 0}<CountBadge prefix="+" value={stats.descendant_count} title={`${stats.descendant_count} subprocesses`} />{/if}<MemoryBadge bytes={stats.memory_bytes} />{/if}{#if !isRunning(process)}<span class="run-hint">Run</span>{/if}</span>
                  </button>
                {/if}
              {:else}
                <p class="empty-row">{query ? 'No matching commands' : 'No commands in gbuild.yml'}</p>
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
                    <span class="attention-dot note" aria-hidden="true">≡</span>
                    <span class="row-copy"><strong>{scratchpad.name}</strong></span>
                    <span class="row-meta">r{scratchpad.revision}</span>
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
    <button type="button" data-tree-row title="Settings" onclick={onOpenSettings}>
      <span aria-hidden="true">⚙</span><strong>Settings</strong>
    </button>
    <i class:online={connected} aria-label={connected ? 'Daemon online' : 'Daemon offline'}></i>
  </footer>
</section>

<style>
  .project-tree { display: grid; width: 100%; height: 100%; min-width: 0; grid-template-rows: auto minmax(0, 1fr) auto; background: #141619; color: var(--text-soft); }
  .tree-toolbar { display: flex; min-height: 38px; align-items: center; gap: 5px; padding: 5px 6px; border-bottom: 1px solid var(--border); }
  .tree-filter { display: flex; min-width: 0; flex: 1; align-items: center; gap: 5px; height: 28px; border: 1px solid #3a3f46; border-radius: 3px; padding: 0 7px; background: #111315; color: var(--muted); }
  .tree-filter input { min-width: 0; flex: 1; border: 0; outline: 0; padding: 0; background: transparent; color: var(--text); font-size: 10px; }
  .tree-filter input::placeholder { color: #6f7680; }
  .tree-filter button { border: 0; padding: 2px; background: transparent; color: var(--muted); cursor: pointer; }
  .collapse-button { display: grid; width: 27px; height: 28px; flex: none; place-items: center; border: 1px solid #3a3f46; border-radius: 3px; background: #202328; color: #a7adb5; font: 600 13px/1 'JetBrains Mono Variable', monospace; cursor: pointer; }
  .collapse-button:hover { border-color: #656c75; color: #fff; }

  .tree-groups { min-height: 0; overflow-y: auto; padding: 3px 0 5px; scrollbar-color: #41464d transparent; scrollbar-width: thin; }
  .tree-group { border-bottom: 1px solid #25292e; }
  .group-header { display: grid; width: 100%; min-height: 28px; grid-template-columns: 13px 16px minmax(0, 1fr) auto; align-items: center; gap: 4px; border: 0; padding: 3px 7px 3px 6px; background: transparent; color: #9da3ab; text-align: left; cursor: pointer; }
  .group-header:hover { background: #1d2024; }
  .group-header:focus-visible { position: relative; z-index: 1; }
  .group-header strong { overflow: hidden; font-size: 9px; font-weight: 700; letter-spacing: 0.055em; text-overflow: ellipsis; text-transform: uppercase; white-space: nowrap; }
  .caret { color: #6f7680; font: 10px 'JetBrains Mono Variable', monospace; }
  .group-icon { color: #8d949d; font: 9px 'JetBrains Mono Variable', monospace; text-align: center; }
  .row-meta, .run-hint { flex: none; border: 1px solid #373c43; border-radius: 3px; padding: 1px 4px; color: #969da6; background: #1d2024; font: 7px 'JetBrains Mono Variable', monospace; }
  .group-rows { padding: 0 4px 4px 13px; }
  .tree-row, .add-row, .show-all { display: grid; width: 100%; min-height: 28px; align-items: center; border: 0; border-radius: 3px; background: transparent; color: #c8ccd1; text-align: left; cursor: pointer; }
  .tree-row { position: relative; grid-template-columns: 17px minmax(0, 1fr) auto; gap: 4px; padding: 3px 5px; }
  .project-tree :global(.tree-row[data-reorderable='true']) { cursor: grab; }
  .project-tree :global(.tree-row[data-reorder-dragging='true']) { opacity: 0.42; }
  .project-tree :global(.tree-row[data-reorder-drop]::after) { position: absolute; z-index: 3; right: 4px; left: 4px; height: 1px; background: var(--signal); box-shadow: 0 0 0 1px rgb(95 214 183 / 16%), 0 0 8px rgb(95 214 183 / 48%); content: ''; pointer-events: none; }
  .project-tree :global(.tree-row[data-reorder-drop='before']::after) { top: -1px; }
  .project-tree :global(.tree-row[data-reorder-drop='after']::after) { bottom: -1px; }
  .agent-row.agent-child { width: calc(100% - min(calc(var(--agent-depth) * 12px), 48px)); grid-template-columns: 12px 17px minmax(0, 1fr) auto; margin-left: min(calc(var(--agent-depth) * 12px), 48px); }
  .tree-row:hover, .add-row:hover, .show-all:hover { background: #202328; }
  .tree-row.selected { background: #292d32; color: #fff; box-shadow: inset 2px 0 #7a818a; }
  .row-copy { min-width: 0; }
  .row-copy strong, .row-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row-copy strong { font-size: 10px; font-weight: 590; }
  .row-copy small { margin-top: 1px; color: #777e87; font: 7px 'JetBrains Mono Variable', monospace; }
  .row-badges { display: flex; min-width: 0; align-items: center; justify-content: flex-end; gap: 3px; }
  .lineage-glyph { color: #687e74; font: 10px/1 'JetBrains Mono Variable', monospace; transform: translateY(-1px); }
  .lineage-rollup { display: inline-flex; min-width: 20px; height: 18px; align-items: center; justify-content: center; border: 1px solid #3d4643; border-radius: 3px; padding: 0 4px; background: #19201d; color: #8ca297; font: 650 8px/1 'JetBrains Mono Variable', monospace; }
  .lineage-rollup.attention { border-color: color-mix(in srgb, var(--warning) 48%, #30343a); color: var(--warning); }
  .lineage-rollup.working { border-color: color-mix(in srgb, var(--signal) 42%, #30343a); color: var(--signal); }
  .lineage-rollup.error { border-color: color-mix(in srgb, var(--fault) 42%, #30343a); color: var(--fault); }
  .attention-dot { display: grid; width: 12px; height: 12px; place-items: center; border: 1px solid #565d66; border-radius: 50%; color: #89909a; font: 700 7px/1 'JetBrains Mono Variable', monospace; }
  .attention-dot.working { border: 2px solid #3f554c; border-top-color: var(--signal); animation: spin 0.85s linear infinite; }
  .attention-dot.idle { border-color: var(--signal); }
  .attention-dot.attention, .attention-dot.attention-dot.attention { border-color: var(--warning); color: var(--warning); }
  .attention-dot.done { border: 0; color: #858c95; }
  .attention-dot.error { border-color: var(--fault); color: var(--fault); }
  .attention-dot.note { border: 0; border-radius: 0; color: #969da6; }
  .add-row, .show-all { grid-template-columns: 1fr; padding: 3px 5px 3px 22px; color: #858c95; font-size: 9px; }
  .add-row { color: #aeb3ba; }
  .empty-row { margin: 0; padding: 5px 5px 5px 22px; color: #686f78; font-size: 9px; }
  .run-hint { border-color: #44504a; color: #aab8b0; }

  .tree-footer { display: flex; align-items: center; gap: 6px; min-height: 38px; padding: 5px 6px; border-top: 1px solid var(--border); }
  .tree-footer button { display: flex; min-width: 0; flex: 1; align-items: center; gap: 7px; height: 28px; border: 1px solid #3a3f46; border-radius: 3px; padding: 0 8px; background: #1d2024; color: #aeb3ba; cursor: pointer; }
  .tree-footer button:hover { border-color: #5e656e; background: #25282d; color: #fff; }
  .tree-footer button span { font-size: 11px; }
  .tree-footer button strong { font-size: 9px; font-weight: 620; }
  .tree-footer i { width: 6px; height: 6px; flex: none; border-radius: 50%; background: #626972; }
  .tree-footer i.online { background: var(--signal); }

  .project-tree.collapsed .tree-toolbar { justify-content: center; padding-inline: 0; }
  .project-tree.collapsed .tree-groups { padding-inline: 4px; }
  .project-tree.collapsed .tree-group { border: 0; }
  .project-tree.collapsed .group-header { min-height: 36px; grid-template-columns: 1fr; justify-items: center; padding: 4px; }
  .project-tree.collapsed .caret,
  .project-tree.collapsed .group-header strong,
  .project-tree.collapsed .group-header :global(.badge) { display: none; }
  .project-tree.collapsed .group-icon { font-size: 11px; }
  .project-tree.collapsed .tree-footer { justify-content: center; padding-inline: 4px; }
  .project-tree.collapsed .tree-footer button { flex: none; width: 34px; justify-content: center; padding: 0; }
  .project-tree.collapsed .tree-footer button strong { display: none; }
  .project-tree.collapsed .tree-footer i { display: none; }

  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .attention-dot.working { animation: none; } }
</style>
