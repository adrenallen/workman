<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
  import Mic2Icon from '@lucide/svelte/icons/mic-2';
  import NotebookTextIcon from '@lucide/svelte/icons/notebook-text';
  import PlayIcon from '@lucide/svelte/icons/play';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import GripVerticalIcon from '@lucide/svelte/icons/grip-vertical';
  import SearchIcon from '@lucide/svelte/icons/search';
  import SettingsIcon from '@lucide/svelte/icons/settings';
  import SquareIcon from '@lucide/svelte/icons/square';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import UserRoundCheckIcon from '@lucide/svelte/icons/user-round-check';
  import XIcon from '@lucide/svelte/icons/x';
  import { onMount } from 'svelte';
  import type { Component } from 'svelte';

  import CountBadge from './CountBadge.svelte';
  import CreationDraftTreeRow from './CreationDraftTreeRow.svelte';
  import AgentBrandMark from './AgentBrandMark.svelte';
  import InlineTreeRename from './InlineTreeRename.svelte';
  import MemoryBadge from './MemoryBadge.svelte';
  import AgentStatusIndicator from './components/ds/AgentStatusIndicator.svelte';
  import IconButton from './components/ds/IconButton.svelte';
  import StatusIndicator from './components/ds/StatusIndicator.svelte';
  import TodoStatusIndicator from './components/ds/TodoStatusIndicator.svelte';
  import TooltipLabel from './components/ds/TooltipLabel.svelte';
  import {
    agentLineageRows,
    type AgentAttentionRollup
  } from './agentLineage';
  import type { ProcessKind, ProcessView, Project } from './daemon';
  import type { AgentTool } from './agentTools';
  import type { ScratchpadSummary, TodoSummary } from './coordination';
  import { feedbackDuration, feedbackStatusLabel, type RecordedFeedbackSummary } from './recordedFeedback';
  import type { CreationDraft } from './creationDrafts';
  import {
    contextMenuRequest,
    keyboardContextMenuRequest,
    type ContextMenuRequest,
    type ContextMenuTarget
  } from './contextMenu';
  import { liveStats, type ProcessRuntimeStats } from './liveStats';
  import { hotkeyDisplayLabel, hotkeyPreferences } from './hotkeys';
  import {
    defaultProjectTreeGroupOrder,
    normalizeProjectTreeGroupOrder,
    projectTreeSelection,
    projectTreeGroupOrderStorageKey,
    type ProjectTreeGroup,
    type ProjectTreeSelection
  } from './projectTree';
  import {
    selectedInTreeGroup,
    updateProjectTreeMultiSelection,
    type ProjectTreeBulkAction,
    type ProjectTreeMultiSelectGroup,
    type ProjectTreeMultiSelection
  } from './projectTreeMultiSelect';
  import { processActivity, processActivityTone } from './processActivity';
  import { todoClaimLabel, todoClaimState } from './todoPresentation';
  import { projectDisplayName } from './worktrees';
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
    agentTools: AgentTool[];
    todos: TodoSummary[];
    scratchpads: ScratchpadSummary[];
    feedback: RecordedFeedbackSummary[];
    showFeedback: boolean;
    drafts: CreationDraft[];
    selection: ProjectTreeSelection | null;
    multiSelection: ProjectTreeMultiSelection | null;
    collapsed: boolean;
    onSelect: (selection: ProjectTreeSelection) => void;
    onMultiSelectionChange: (selection: ProjectTreeMultiSelection | null) => void;
    onBulkAction: (action: ProjectTreeBulkAction) => void;
    bulkBusy: boolean;
    onCreateTodo: () => void;
    onBrowseTodos: () => void;
    onBrowseScratchpads: () => void;
    onBrowseFeedback: () => void;
    onBrowseProcesses: (kind: ProcessKind) => void;
    onAddAgent: () => void;
    onAddTerminal: () => void;
    onAddCommand: () => void;
    onAddScratchpad: () => void;
    onStartFeedback: () => void;
    processBusyId: number | null;
    onStartProcess: (process: ProcessView) => void;
    onStopCommand: (process: ProcessView) => void;
    onRestartCommand: (process: ProcessView) => void;
    onOpenSettings: () => void;
    onToggleCollapse: () => void;
    reordering: boolean;
    onReorderProcesses: (kind: ProcessKind, orderedIds: number[]) => void;
    onReorderTodos: (orderedIds: number[]) => void;
    onReorderScratchpads: (orderedIds: number[]) => void;
    renameTarget: ContextMenuTarget | null;
    onContextMenu: (request: ContextMenuRequest) => void;
    onMiddleClick: (target: ContextMenuTarget) => void;
    onRenameSubmit: (name: string) => void;
    onRenameCancel: () => void;
  }

  let {
    project,
    processes,
    agentTools,
    todos,
    scratchpads,
    feedback,
    showFeedback,
    drafts,
    selection,
    multiSelection,
    collapsed,
    onSelect,
    onMultiSelectionChange,
    onBulkAction,
    bulkBusy,
    onCreateTodo,
    onBrowseTodos,
    onBrowseScratchpads,
    onBrowseFeedback,
    onBrowseProcesses,
    onAddAgent,
    onAddTerminal,
    onAddCommand,
    onAddScratchpad,
    onStartFeedback,
    processBusyId,
    onStartProcess,
    onStopCommand,
    onRestartCommand,
    onOpenSettings,
    onToggleCollapse,
    reordering,
    onReorderProcesses,
    onReorderTodos,
    onReorderScratchpads,
    renameTarget,
    onContextMenu,
    onMiddleClick,
    onRenameSubmit,
    onRenameCancel
  }: Props = $props();

  let projectName = $derived(projectDisplayName(project));

  const groupId: Record<ProjectTreeGroup, number> = {
    todos: 1,
    agents: 2,
    terminals: 3,
    commands: 4,
    feedback: 5,
    scratchpads: 6
  };
  const groupLabel: Record<ProjectTreeGroup, string> = {
    todos: 'Todos',
    agents: 'Agents',
    terminals: 'Terminals',
    commands: 'Commands',
    feedback: 'Feedback',
    scratchpads: 'Scratchpads'
  };
  const groupIcon: Record<ProjectTreeGroup, Component> = {
    todos: CircleCheckIcon,
    agents: BotIcon,
    terminals: SquareTerminalIcon,
    commands: PlayIcon,
    feedback: Mic2Icon,
    scratchpads: NotebookTextIcon
  };

  const sidebarTodoLimit = 7;

  let query = $state('');
  let groupOrder = $state<ProjectTreeGroup[]>([...defaultProjectTreeGroupOrder]);
  let selectionAnchors = $state<Partial<Record<ProjectTreeMultiSelectGroup, number>>>({});
  let openGroups = $state<Record<ProjectTreeGroup, boolean>>({
    todos: true,
    agents: true,
    terminals: true,
    commands: true,
    feedback: true,
    scratchpads: true
  });

  let agents = $derived(processes.filter((process) => process.kind === 'agent'));
  let waitingAgents = $derived(agents.filter((process) => process.agent_state.state === 'waiting'));
  let terminals = $derived(processes.filter((process) => process.kind === 'terminal'));
  let commands = $derived(processes.filter((process) => process.kind === 'command'));
  let openTodos = $derived(todos.filter((todo) => !todo.completed));
  let matchingTodos = $derived.by(() => openTodos
    .filter((todo) => matchesQuery(todo.title))
    .sort((left, right) => {
      const claimPriority = Number(todoClaimState(right) === 'claimed')
        - Number(todoClaimState(left) === 'claimed');
      return claimPriority || left.sort_order - right.sort_order || left.id - right.id;
    }));
  let visibleTodos = $derived(matchingTodos.slice(0, sidebarTodoLimit));
  let hiddenTodoCount = $derived(Math.max(0, matchingTodos.length - sidebarTodoLimit));
  let visibleAgentRows = $derived(agentLineageRows(agents, query));
  let visibleTerminals = $derived(
    terminals.filter((process) => matchesQuery(`${workingDirLabel(process.working_dir)} ${process.name}`))
  );
  let visibleCommands = $derived(
    commands.filter((process) => matchesQuery(`${process.name} ${process.command ?? ''}`))
  );
  let visibleTodoDrafts = $derived(drafts.filter((draft) =>
    draft.projectId === project.id && draft.kind === 'todo' && matchesQuery(draft.title)
  ));
  let visibleAgentDrafts = $derived(drafts.filter((draft) =>
    draft.projectId === project.id
    && draft.kind === 'agent'
    && matchesQuery(`${draft.name} ${draft.prompt}`)
  ));
  let visibleCommandDrafts = $derived(drafts.filter((draft) =>
    draft.projectId === project.id
    && draft.kind === 'command'
    && matchesQuery(`${draft.name} ${draft.command}`)
  ));
  let orderedScratchpads = $derived(
    [...scratchpads].sort((left, right) => left.sort_order - right.sort_order || left.id - right.id)
  );
  let visibleScratchpads = $derived(
    orderedScratchpads.filter((scratchpad) => matchesQuery(`${scratchpad.name} ${scratchpad.tags.join(' ')}`))
  );
  let visibleFeedback = $derived(feedback
    .filter((item) => matchesQuery(item.title))
    .sort((left, right) => Number(left.archived) - Number(right.archived) || right.updated_at - left.updated_at));
  let visibleGroupOrder = $derived(groupOrder.filter((group) => group !== 'feedback' || showFeedback));
  let projectCounts = $derived($liveStats.counts[project.id]);

  onMount(() => {
    try {
      const saved = localStorage.getItem('workman.tree.groups.v1');
      if (saved) {
        const parsed = JSON.parse(saved) as Partial<Record<ProjectTreeGroup, boolean>>;
        openGroups = { ...openGroups, ...parsed };
      }
    } catch {
      // Group expansion remains usable if local storage is unavailable.
    }
    try {
      const saved = localStorage.getItem(projectTreeGroupOrderStorageKey);
      if (saved) groupOrder = normalizeProjectTreeGroupOrder(JSON.parse(saved));
    } catch {
      // The default group order remains usable if local storage is unavailable.
    }
  });

  function matchesQuery(value: string): boolean {
    const needle = query.trim().toLowerCase();
    return !needle || value.toLowerCase().includes(needle);
  }

  function agentTool(process: ProcessView): AgentTool | null {
    return process.agent_tool_id === null
      ? null
      : agentTools.find((tool) => tool.id === process.agent_tool_id) ?? null;
  }

  function toggleGroup(group: ProjectTreeGroup): void {
    openGroups = { ...openGroups, [group]: !openGroups[group] };
    try {
      localStorage.setItem('workman.tree.groups.v1', JSON.stringify(openGroups));
    } catch {
      // Persistence is a convenience; the tree still works without it.
    }
  }

  function openGroup(group: ProjectTreeGroup): void {
    if (group === 'todos') {
      onBrowseTodos();
    } else if (group === 'scratchpads') {
      onBrowseScratchpads();
    } else if (group === 'feedback') {
      onBrowseFeedback();
    } else if (group === 'agents') {
      onBrowseProcesses('agent');
    } else if (group === 'terminals') {
      onBrowseProcesses('terminal');
    } else {
      onBrowseProcesses('command');
    }
  }

  function createInGroup(group: ProjectTreeGroup): void {
    switch (group) {
      case 'todos': onCreateTodo(); break;
      case 'agents': onAddAgent(); break;
      case 'terminals': onAddTerminal(); break;
      case 'commands': onAddCommand(); break;
      case 'feedback': onStartFeedback(); break;
      case 'scratchpads': onAddScratchpad(); break;
    }
  }

  function groupReorderOptions(group: ProjectTreeGroup): ReorderItemOptions {
    return {
      id: groupId[group],
      group: `project-tree-groups:${project.id}`,
      handle: '.group-drag-handle',
      disabled: collapsed,
      label: `${groupLabel[group]} section`,
      onDrop: handleGroupDrop,
      onKeyboardMove: moveGroupFromKeyboard
    };
  }

  function handleGroupDrop(drop: ReorderDrop): void {
    const reorderedIds = moveOrderedId(
      groupOrder.map((group) => groupId[group]),
      drop.sourceId,
      drop.targetId,
      drop.placement
    );
    const next = reorderedIds.flatMap((id) => {
      const group = defaultProjectTreeGroupOrder.find((candidate) => groupId[candidate] === id);
      return group ? [group] : [];
    });
    if (next.every((group, index) => group === groupOrder[index])) return;
    groupOrder = next;
    try {
      localStorage.setItem(projectTreeGroupOrderStorageKey, JSON.stringify(groupOrder));
    } catch {
      // Reordering still works for this session when local storage is unavailable.
    }
  }

  function moveGroupFromKeyboard(id: number, direction: ReorderDirection): void {
    const index = visibleGroupOrder.findIndex((group) => groupId[group] === id);
    const target = visibleGroupOrder[index + direction];
    if (!target) return;
    handleGroupDrop({
      sourceId: id,
      targetId: groupId[target],
      placement: direction < 0 ? 'before' : 'after'
    });
  }

  function groupCreateLabel(group: ProjectTreeGroup): string {
    switch (group) {
      case 'todos': return 'New todo';
      case 'agents': return 'Add agent';
      case 'terminals': return 'New terminal';
      case 'commands': return 'Add command';
      case 'feedback': return 'Record feedback';
      case 'scratchpads': return 'New scratchpad';
    }
  }

  function selectProcess(process: ProcessView): void {
    onSelect(
      projectTreeSelection(process.kind, process.id, project.id, processLabel(process))
    );
  }

  function selectGroupItem(
    event: MouseEvent,
    group: ProjectTreeMultiSelectGroup,
    id: number,
    orderedIds: number[],
    open: () => void
  ): void {
    const modifier = event.metaKey || event.ctrlKey || event.shiftKey;
    if (!modifier) {
      selectionAnchors[group] = id;
      onMultiSelectionChange(null);
      open();
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    const next = updateProjectTreeMultiSelection(multiSelection, {
      group,
      id,
      orderedIds,
      anchorId: selectionAnchors[group] ?? null,
      toggle: event.metaKey || event.ctrlKey,
      range: event.shiftKey
    });
    selectionAnchors[group] = id;
    onMultiSelectionChange(next);
  }

  function openSelectablePointerMenu(
    event: MouseEvent,
    target: ContextMenuTarget,
    group: ProjectTreeMultiSelectGroup,
    id: number,
    orderedIds: number[]
  ): void {
    if (event.ctrlKey) {
      selectGroupItem(event, group, id, orderedIds, () => undefined);
      return;
    }
    openPointerMenu(event, target);
  }

  function multiSelected(group: ProjectTreeMultiSelectGroup, id: number): boolean {
    return selectedInTreeGroup(multiSelection, group, id);
  }

  function selectedCount(group: ProjectTreeGroup): number {
    return multiSelection?.group === group ? multiSelection.ids.length : 0;
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

  function feedbackTarget(feedback: RecordedFeedbackSummary): ContextMenuTarget {
    return {
      kind: 'feedback',
      feedback,
      selection: projectTreeSelection('feedback', feedback.id, project.id, feedback.title)
    };
  }

  function openPointerMenu(event: MouseEvent, target: ContextMenuTarget): void {
    onContextMenu(contextMenuRequest(event, target));
  }

  function openKeyboardMenu(event: KeyboardEvent, target: ContextMenuTarget): void {
    const request = keyboardContextMenuRequest(event, target);
    if (request) onContextMenu(request);
  }

  function preventMiddleMouseDefault(event: MouseEvent): void {
    if (event.button === 1) event.preventDefault();
  }

  function handleMiddleClick(event: MouseEvent, target: ContextMenuTarget): void {
    if (event.button !== 1) return;
    event.preventDefault();
    event.stopPropagation();
    onMiddleClick(target);
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

  function processStatusTone(
    process: ProcessView,
    stats?: ProcessRuntimeStats
  ): ReturnType<typeof processActivityTone> {
    return processActivityTone(processActivity(process, stats).state);
  }

  function processStatusLabel(process: ProcessView, stats?: ProcessRuntimeStats): string {
    return processActivity(process, stats).label;
  }

  function lineageTone(rollup: AgentAttentionRollup): 'needs-input' | 'working' | 'waiting' | 'error' | 'neutral' {
    if (rollup.needsInput > 0) return 'needs-input';
    if (rollup.crashed > 0) return 'error';
    if (rollup.working > 0) return 'working';
    if (rollup.waiting > 0) return 'waiting';
    return 'neutral';
  }

  function lineageTitle(rollup: AgentAttentionRollup): string {
    const states = [];
    if (rollup.needsInput > 0) states.push(`${rollup.needsInput} need input`);
    if (rollup.working > 0) states.push(`${rollup.working} working`);
    if (rollup.waiting > 0) states.push(`${rollup.waiting} waiting for timer`);
    if (rollup.crashed > 0) states.push(`${rollup.crashed} crashed`);
    if (rollup.unread > 0) states.push(`${rollup.unread} unread`);
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
      case 'feedback': return String(feedback.length);
      case 'scratchpads': return String(scratchpads.length);
    }
  }

  function groupTone(
    group: ProjectTreeGroup
  ): 'neutral' | 'working' | 'needs-input' | 'waiting' | 'error' | 'attention' {
    if (group === 'todos' && openTodos.some((todo) => todo.is_blocked)) return 'attention';
    if (group === 'feedback') {
      if (feedback.some((item) => !item.archived && item.status === 'failed')) return 'error';
      if (feedback.some((item) => !item.archived && (item.status === 'recording' || item.status === 'transcribing'))) return 'working';
      return 'neutral';
    }
    if (group === 'todos' || group === 'scratchpads') return 'neutral';
    const states = processesForGroup(group)
      .map((process) => processActivity(process, runtimeStats(process)).state);
    if (states.includes('needs_input')) return 'needs-input';
    if (states.includes('crashed')) return 'error';
    if (states.includes('working')) return 'working';
    if (states.includes('waiting')) return 'waiting';
    return 'neutral';
  }

  function groupCountTitle(group: ProjectTreeGroup): string {
    const value = groupCount(group);
    if (group === 'todos' || group === 'scratchpads' || group === 'feedback') return `${value} ${group}`;
    const [running, total] = value.split('/');
    const unread = group === 'agents'
      ? agents.filter((process) => process.agent_state.unread).length
      : 0;
    const groupProcesses = processesForGroup(group);
    const active = groupProcesses
      .filter((process) => processActivity(process, runtimeStats(process)).state === 'working')
      .length;
    const waiting = groupProcesses
      .filter((process) => processActivity(process, runtimeStats(process)).state === 'waiting')
      .length;
    return `${running} running of ${total} ${group} · ${active} actively working${waiting > 0 ? ` · ${waiting} waiting` : ''}${unread > 0 ? ` · ${unread} unread` : ''}`;
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

  function todoReorderOptions(todo: TodoSummary): ReorderItemOptions {
    const bucket = todosInBucket(todo);
    return {
      id: todo.id,
      group: `todo:${project.id}:${todoIsClaimed(todo) ? 'claimed' : 'ordinary'}`,
      disabled: reordering || Boolean(query.trim()) || bucket.length < 2,
      label: todo.title,
      onDrop: (drop) => handleTodoDrop(todo, drop),
      onKeyboardMove: (id, direction) => moveTodoFromKeyboard(todo, id, direction)
    };
  }

  function scratchpadReorderOptions(scratchpad: ScratchpadSummary): ReorderItemOptions {
    return {
      id: scratchpad.id,
      group: `scratchpad:${project.id}`,
      disabled: reordering || Boolean(query.trim()) || orderedScratchpads.length < 2,
      label: scratchpad.name,
      onDrop: handleScratchpadDrop,
      onKeyboardMove: moveScratchpadFromKeyboard
    };
  }

  function todoIsClaimed(todo: TodoSummary): boolean {
    return todoClaimState(todo) === 'claimed';
  }

  function todosInBucket(todo: TodoSummary): TodoSummary[] {
    const claimed = todoIsClaimed(todo);
    return matchingTodos.filter((candidate) => todoIsClaimed(candidate) === claimed);
  }

  function handleTodoDrop(todo: TodoSummary, drop: ReorderDrop): void {
    const bucket = todosInBucket(todo);
    const reorderedBucket = moveOrderedId(
      bucket.map((candidate) => candidate.id),
      drop.sourceId,
      drop.targetId,
      drop.placement
    );
    const claimed = matchingTodos.filter(todoIsClaimed).map((candidate) => candidate.id);
    const ordinary = matchingTodos.filter((candidate) => !todoIsClaimed(candidate)).map((candidate) => candidate.id);
    onReorderTodos(todoIsClaimed(todo)
      ? [...reorderedBucket, ...ordinary]
      : [...claimed, ...reorderedBucket]);
  }

  function moveTodoFromKeyboard(
    todo: TodoSummary,
    todoId: number,
    direction: ReorderDirection
  ): void {
    const bucket = todosInBucket(todo);
    const index = bucket.findIndex((candidate) => candidate.id === todoId);
    const target = bucket[index + direction];
    if (!target) return;
    handleTodoDrop(todo, {
      sourceId: todoId,
      targetId: target.id,
      placement: direction < 0 ? 'before' : 'after'
    });
  }

  function handleScratchpadDrop(drop: ReorderDrop): void {
    onReorderScratchpads(moveOrderedId(
      orderedScratchpads.map((scratchpad) => scratchpad.id),
      drop.sourceId,
      drop.targetId,
      drop.placement
    ));
  }

  function moveScratchpadFromKeyboard(
    scratchpadId: number,
    direction: ReorderDirection
  ): void {
    const index = orderedScratchpads.findIndex((scratchpad) => scratchpad.id === scratchpadId);
    const target = orderedScratchpads[index + direction];
    if (!target) return;
    handleScratchpadDrop({
      sourceId: scratchpadId,
      targetId: target.id,
      placement: direction < 0 ? 'before' : 'after'
    });
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

<section class="project-tree" class:collapsed aria-label={`${projectName} project tree`}>
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
      shortcut={hotkeyDisplayLabel($hotkeyPreferences['toggle-project-tree']) || undefined}
      onclick={onToggleCollapse}
    >
      {#snippet icon()}
        {#if collapsed}<ChevronRightIcon size={15} />{:else}<ChevronLeftIcon size={15} />{/if}
      {/snippet}
    </IconButton>
  </header>

  <div class="tree-groups" aria-label="Project items" role="tree" tabindex="-1" onkeydown={handleTreeKeys}>
    {#each visibleGroupOrder as group}
      {@const GroupIcon = groupIcon[group]}
      <section class="tree-group" class:closed={!openGroups[group]}>
        <div class="group-header" use:reorderItem={groupReorderOptions(group)}>
          {#if !collapsed}
            <TooltipLabel label={`Drag to reorder ${groupLabel[group]}`} tabindex={-1}>
              <button
                class="group-drag-handle"
                type="button"
                aria-label={`Drag to reorder the ${groupLabel[group]} section`}
              >
                <GripVerticalIcon size={13} strokeWidth={1.8} aria-hidden="true" />
              </button>
            </TooltipLabel>
          {/if}
          <button
            class="group-toggle"
            type="button"
            data-tree-row
            data-group={group}
            aria-expanded={openGroups[group]}
            aria-label={`${openGroups[group] ? 'Collapse' : 'Expand'} ${groupLabel[group]}`}
            onclick={() => toggleGroup(group)}
          >
            <span class="caret" aria-hidden="true">
              {#if openGroups[group]}<ChevronDownIcon size={13} />{:else}<ChevronRightIcon size={13} />{/if}
            </span>
          </button>
          <button
            class="group-title"
            type="button"
            data-tree-row
            data-group={group}
            aria-expanded={openGroups[group]}
            onclick={() => openGroup(group)}
          >
            <span class="group-icon">
              <GroupIcon size={14} strokeWidth={1.8} aria-hidden="true" />
              {#if collapsed && group === 'agents' && waitingAgents[0]}
                <span class="collapsed-agent-waiting">
                  <AgentStatusIndicator process={waitingAgents[0]} />
                </span>
              {/if}
            </span>
            <strong>{groupLabel[group]}</strong>
          </button>
          <span class="group-badges">
            <CountBadge value={groupCount(group)} tone={groupTone(group)} title={groupCountTitle(group)} />
            {#if group === 'agents' && agents.some((process) => process.agent_state.unread)}
              {@const unreadCount = agents.filter((process) => process.agent_state.unread).length}
              <TooltipLabel label={`${unreadCount} unread finished agent${unreadCount === 1 ? '' : 's'}`}>
                <span class="unread-group-rollup" aria-label={`${unreadCount} unread finished agents`}>
                  <span aria-hidden="true"></span>{unreadCount}
                </span>
              </TooltipLabel>
            {/if}
          </span>
          {#if !collapsed}
            <IconButton
              class="group-create size-6 rounded-sm"
              label={`${groupCreateLabel(group)} in ${projectName}`}
              shortcut={group === 'feedback' ? hotkeyDisplayLabel($hotkeyPreferences['start-feedback']) || undefined : undefined}
              data-tree-row
              onclick={() => createInGroup(group)}
            >
              {#snippet icon()}<PlusIcon size={13} strokeWidth={1.9} />{/snippet}
            </IconButton>
          {/if}
        </div>

        {#if openGroups[group] && !collapsed}
          <div class="group-rows">
            {#if group !== 'commands' && group !== 'feedback' && selectedCount(group) > 1}
              <div class="bulk-action-bar" role="toolbar" aria-label={`${selectedCount(group)} selected ${group}`}>
                <strong>{selectedCount(group)} selected</strong>
                <span class="bulk-action-buttons">
                  {#if group === 'agents' || group === 'terminals'}
                    <button type="button" disabled={bulkBusy} onclick={() => onBulkAction('stop')}>Stop</button>
                    <button class="destructive" type="button" disabled={bulkBusy} onclick={() => onBulkAction('close')}>Close</button>
                  {:else if group === 'todos'}
                    <button type="button" disabled={bulkBusy} onclick={() => onBulkAction('complete')}>Complete</button>
                    <button class="destructive" type="button" disabled={bulkBusy} onclick={() => onBulkAction('delete')}>Delete</button>
                  {:else}
                    <button type="button" disabled={bulkBusy} onclick={() => onBulkAction('archive')}>Archive</button>
                    <button class="destructive" type="button" disabled={bulkBusy} onclick={() => onBulkAction('delete')}>Delete</button>
                  {/if}
                  <button class="clear" type="button" disabled={bulkBusy} aria-label="Clear selection · Esc" onclick={() => onMultiSelectionChange(null)}><XIcon size={12} /></button>
                </span>
              </div>
            {/if}
            {#if group === 'todos'}
              {#each visibleTodos as todo (todo.id)}
                <button
                  type="button"
                  class="tree-row todo-row"
                  class:selected={selection?.key === `todo:${todo.id}`}
                  class:multi-selected={multiSelected('todos', todo.id)}
                  data-todo-state={todoClaimState(todo)}
                  data-tree-row
                  data-context-kind="todo"
                  data-context-id={todo.id}
                  use:reorderItem={todoReorderOptions(todo)}
                  onclick={(event) => selectGroupItem(event, 'todos', todo.id, visibleTodos.map((candidate) => candidate.id), () => onSelect(projectTreeSelection('todo', todo.id, project.id, todo.title)))}
                  onmousedown={preventMiddleMouseDefault}
                  onauxclick={(event) => handleMiddleClick(event, todoTarget(todo))}
                  oncontextmenu={(event) => openSelectablePointerMenu(event, todoTarget(todo), 'todos', todo.id, visibleTodos.map((candidate) => candidate.id))}
                  onkeydown={(event) => openKeyboardMenu(event, todoTarget(todo))}
                >
                  <span class="todo-state-rail" aria-hidden="true"></span>
                  <TodoStatusIndicator state={todoClaimState(todo)} label={todoStatusLabel(todo)} />
                  <span class="row-copy"><strong>{todo.title}</strong></span>
                  {#if todo.assignee === 'user' || todo.comment_count > 0}
                    <span class="row-badges">
                      {#if todo.assignee === 'user'}
                        <TooltipLabel label="Assigned to you">
                          <span class="todo-assigned-marker" aria-label="Assigned to you"><UserRoundCheckIcon size={12} strokeWidth={1.8} aria-hidden="true" /></span>
                        </TooltipLabel>
                      {/if}
                      {#if todo.comment_count > 0}<span class="row-meta" aria-label={`${todo.comment_count} comments`}>{todo.comment_count}</span>{/if}
                    </span>
                  {/if}
                </button>
              {/each}
              {#each visibleTodoDrafts as draft (draft.id)}
                <CreationDraftTreeRow {draft} {selection} {onSelect} {onContextMenu} />
              {/each}
              {#if visibleTodos.length === 0 && visibleTodoDrafts.length === 0}
                <p class="empty-row">{query ? 'No matching todos' : 'No open todos'}</p>
              {/if}
              {#if hiddenTodoCount > 0}
                <button
                  class="show-all"
                  type="button"
                  data-tree-row
                  aria-label={`${hiddenTodoCount} more todos; browse all todos`}
                  onclick={onBrowseTodos}
                >
                  +{hiddenTodoCount} more
                </button>
              {/if}
              {#if openTodos.length === 0}
                <button class="add-row" type="button" data-tree-row onclick={onCreateTodo}>+ Add todo</button>
              {/if}
            {:else if group === 'agents'}
              {#each visibleAgentRows as row (row.process.id)}
                {@const process = row.process}
                {@const stats = runtimeStats(process)}
                {#if renameTarget?.kind === 'process' && renameTarget.process.id === process.id}
                  <InlineTreeRename value={process.name} label="Agent name" depth={row.depth} onSubmit={onRenameSubmit} onCancel={onRenameCancel} />
                {:else}
                  <div
                    class="process-row-shell agent-row-shell"
                    class:has-actions={!isRunning(process)}
                    class:agent-child={row.depth > 0}
                    class:selected={selection?.key === `agent:${process.id}`}
                    class:multi-selected={multiSelected('agents', process.id)}
                    style={`--agent-depth: ${row.depth}`}
                    aria-busy={processBusyId === process.id}
                  >
                    <button
                      type="button"
                      class="tree-row agent-row"
                      data-tree-row
                      data-context-kind="agent"
                      data-context-id={process.id}
                      use:reorderItem={reorderOptions(process)}
                      onclick={(event) => selectGroupItem(event, 'agents', process.id, visibleAgentRows.map((candidate) => candidate.process.id), () => selectProcess(process))}
                      onmousedown={preventMiddleMouseDefault}
                      onauxclick={(event) => handleMiddleClick(event, processTarget(process))}
                      oncontextmenu={(event) => openSelectablePointerMenu(event, processTarget(process), 'agents', process.id, visibleAgentRows.map((candidate) => candidate.process.id))}
                      onkeydown={(event) => openKeyboardMenu(event, processTarget(process))}
                    >
                      {#if row.depth > 0}<span class="lineage-glyph" aria-hidden="true">└</span>{/if}
                      <AgentStatusIndicator {process} />
                      <span class="row-copy"><strong>{process.name}</strong></span>
                      <span class="row-badges">
                          {#if process.agent_state.unread}
                            <TooltipLabel label="Unread: agent finished while no timer was watching">
                              <span class="agent-unread-dot" aria-label={`${process.name} has unread finished output`}></span>
                            </TooltipLabel>
                          {/if}
                          {#if row.rollup.total > 0}
                            <CountBadge prefix="↳" value={row.rollup.total} tone={lineageTone(row.rollup)} title={lineageTitle(row.rollup)} />
                          {/if}
                          {#if row.rollup.unread > 0}
                            <TooltipLabel label={`${row.rollup.unread} unread finished descendant agent${row.rollup.unread === 1 ? '' : 's'}`}>
                              <span class="unread-lineage-rollup" aria-label={`${row.rollup.unread} unread descendant agents`}>
                                <span aria-hidden="true"></span>{row.rollup.unread}
                              </span>
                            </TooltipLabel>
                          {/if}
                          <AgentBrandMark tool={agentTool(process)} fallbackName={process.name} fallbackToolType={process.agent_state.tool_type} />
                          {#if stats?.descendant_count}
                            <CountBadge prefix="+" value={stats.descendant_count} title={`${stats.descendant_count} subprocesses`} />
                          {/if}
                          {#if stats}<MemoryBadge bytes={stats.memory_bytes} />{/if}
                        </span>
                    </button>
                    {#if !isRunning(process)}
                      <div class="process-actions" aria-label={`${process.name} actions`}>
                        <IconButton
                          class="size-6 rounded-sm text-success hover:text-success"
                          label={`Start agent ${process.name}`}
                          disabled={processBusyId !== null}
                          onclick={() => onStartProcess(process)}
                        >
                          {#snippet icon()}<PlayIcon size={13} strokeWidth={1.8} />{/snippet}
                        </IconButton>
                      </div>
                    {/if}
                  </div>
                {/if}
              {/each}
              {#each visibleAgentDrafts as draft (draft.id)}
                <CreationDraftTreeRow {draft} {selection} {onSelect} {onContextMenu} />
              {/each}
              {#if visibleAgentRows.length === 0 && visibleAgentDrafts.length === 0}
                <p class="empty-row">{query ? 'No matching agents' : 'No agents'}</p>
              {/if}
              {#if agents.length === 0}
                <button class="add-row" type="button" data-tree-row onclick={onAddAgent}>+ Add agent</button>
              {/if}
            {:else if group === 'terminals'}
              {#each visibleTerminals as process (process.id)}
                {@const stats = runtimeStats(process)}
                {#if renameTarget?.kind === 'process' && renameTarget.process.id === process.id}
                  <InlineTreeRename value={process.name} label="Terminal name" onSubmit={onRenameSubmit} onCancel={onRenameCancel} />
                {:else}
                  <div
                    class="process-row-shell"
                    class:has-actions={!isRunning(process)}
                    class:selected={selection?.key === `terminal:${process.id}`}
                    class:multi-selected={multiSelected('terminals', process.id)}
                    aria-busy={processBusyId === process.id}
                  >
                    <button
                      type="button"
                      class="tree-row"
                      data-tree-row
                      data-context-kind="terminal"
                      data-context-id={process.id}
                      use:reorderItem={reorderOptions(process)}
                      onclick={(event) => selectGroupItem(event, 'terminals', process.id, visibleTerminals.map((candidate) => candidate.id), () => selectProcess(process))}
                      onmousedown={preventMiddleMouseDefault}
                      onauxclick={(event) => handleMiddleClick(event, processTarget(process))}
                      oncontextmenu={(event) => openSelectablePointerMenu(event, processTarget(process), 'terminals', process.id, visibleTerminals.map((candidate) => candidate.id))}
                      onkeydown={(event) => openKeyboardMenu(event, processTarget(process))}
                    >
                      <StatusIndicator tone={processStatusTone(process, stats)} label={processStatusLabel(process, stats)} />
                      <span class="row-copy"><strong>{workingDirLabel(process.working_dir)}</strong></span>
                      {#if stats}<span class="row-badges">{#if stats.descendant_count > 0}<CountBadge prefix="+" value={stats.descendant_count} title={`${stats.descendant_count} subprocesses`} />{/if}<MemoryBadge bytes={stats.memory_bytes} /></span>{/if}
                    </button>
                    {#if !isRunning(process)}
                      <div class="process-actions" aria-label={`${process.name} actions`}>
                        <IconButton
                          class="size-6 rounded-sm text-success hover:text-success"
                          label={`Start terminal ${process.name}`}
                          disabled={processBusyId !== null}
                          onclick={() => onStartProcess(process)}
                        >
                          {#snippet icon()}<PlayIcon size={13} strokeWidth={1.8} />{/snippet}
                        </IconButton>
                      </div>
                    {/if}
                  </div>
                {/if}
              {:else}
                <p class="empty-row">{query ? 'No matching terminals' : 'No terminals'}</p>
              {/each}
              {#if terminals.length === 0}
                <button class="add-row" type="button" data-tree-row onclick={onAddTerminal}>+ New terminal</button>
              {/if}
            {:else if group === 'commands'}
              {#each visibleCommands as process (process.id)}
                {@const stats = runtimeStats(process)}
                {#if renameTarget?.kind === 'process' && renameTarget.process.id === process.id}
                  <InlineTreeRename value={process.name} label="Command name" onSubmit={onRenameSubmit} onCancel={onRenameCancel} />
                {:else}
                  <div
                    class="command-row-shell"
                    class:selected={selection?.key === `command:${process.id}`}
                    aria-busy={processBusyId === process.id}
                  >
                    <button
                      type="button"
                      class="tree-row command-row"
                      data-tree-row
                      data-context-kind="command"
                      data-context-id={process.id}
                      use:reorderItem={reorderOptions(process)}
                      onclick={() => selectProcess(process)}
                      oncontextmenu={(event) => openPointerMenu(event, processTarget(process))}
                      onkeydown={(event) => openKeyboardMenu(event, processTarget(process))}
                    >
                      <StatusIndicator tone={processStatusTone(process, stats)} label={processStatusLabel(process, stats)} />
                      <span class="row-copy"><strong>{process.name}</strong><small>{process.command ?? 'Command'}</small></span>
                      <span class="row-badges">{#if stats}{#if stats.descendant_count > 0}<CountBadge prefix="+" value={stats.descendant_count} title={`${stats.descendant_count} subprocesses`} />{/if}<MemoryBadge bytes={stats.memory_bytes} />{/if}</span>
                    </button>
                    <div class="command-actions" aria-label={`${process.name} actions`}>
                      {#if isRunning(process)}
                        <IconButton class="size-6 rounded-sm" label={`Restart ${process.name}`} disabled={processBusyId !== null} onclick={() => onRestartCommand(process)}>
                          {#snippet icon()}<RefreshCwIcon size={13} strokeWidth={1.8} />{/snippet}
                        </IconButton>
                        <IconButton class="size-6 rounded-sm hover:text-destructive" label={`Stop ${process.name}`} disabled={processBusyId !== null} onclick={() => onStopCommand(process)}>
                          {#snippet icon()}<SquareIcon size={12} strokeWidth={1.8} />{/snippet}
                        </IconButton>
                      {:else}
                        <IconButton class="size-6 rounded-sm text-success hover:text-success" label={`Start ${process.name}`} disabled={processBusyId !== null} onclick={() => onStartProcess(process)}>
                          {#snippet icon()}<PlayIcon size={13} strokeWidth={1.8} />{/snippet}
                        </IconButton>
                      {/if}
                    </div>
                  </div>
                {/if}
              {/each}
              {#each visibleCommandDrafts as draft (draft.id)}
                <CreationDraftTreeRow {draft} {selection} {onSelect} {onContextMenu} />
              {/each}
              {#if visibleCommands.length === 0 && visibleCommandDrafts.length === 0}
                <p class="empty-row">{query ? 'No matching commands' : 'No commands in workman.yml'}</p>
              {/if}
              {#if commands.length === 0}
                <button class="add-row" type="button" data-tree-row onclick={onAddCommand}>+ Add command</button>
              {/if}
            {:else if group === 'feedback'}
              {#each visibleFeedback as item (item.id)}
                <button
                  type="button"
                  class="tree-row feedback-row"
                  class:archived={item.archived}
                  class:selected={selection?.key === `feedback:${item.id}`}
                  data-tree-row
                  onclick={() => onSelect(projectTreeSelection('feedback', item.id, project.id, item.title))}
                  onmousedown={preventMiddleMouseDefault}
                  onauxclick={(event) => handleMiddleClick(event, feedbackTarget(item))}
                  oncontextmenu={(event) => openPointerMenu(event, feedbackTarget(item))}
                  onkeydown={(event) => openKeyboardMenu(event, feedbackTarget(item))}
                >
                  <StatusIndicator
                    tone={item.status === 'failed' ? 'danger' : item.status === 'recording' || item.status === 'transcribing' ? 'success' : 'neutral'}
                    state={item.status === 'recording' || item.status === 'transcribing' ? 'working' : item.status === 'failed' ? 'crashed' : 'idle'}
                    label={feedbackStatusLabel(item.status)}
                  />
                  <span class="row-copy"><strong>{item.title}</strong><small>{item.archived ? 'Archived · ' : ''}{feedbackStatusLabel(item.status)} · {feedbackDuration(item.duration_ms)} · {item.snapshot_count} snap{item.snapshot_count === 1 ? '' : 's'}</small></span>
                </button>
              {:else}
                <p class="empty-row">{query ? 'No matching feedback' : 'No recorded feedback'}</p>
              {/each}
              {#if feedback.length === 0}
                <button class="add-row" type="button" data-tree-row onclick={onStartFeedback}>+ Record feedback</button>
              {/if}
            {:else}
              {#each visibleScratchpads as scratchpad (scratchpad.id)}
                {#if renameTarget?.kind === 'scratchpad' && renameTarget.scratchpad.id === scratchpad.id}
                  <InlineTreeRename value={scratchpad.name} label="Scratchpad name" onSubmit={onRenameSubmit} onCancel={onRenameCancel} />
                {:else}
                  <button
                    type="button"
                    class="tree-row scratchpad-row"
                    class:selected={selection?.key === `scratchpad:${scratchpad.id}`}
                    class:multi-selected={multiSelected('scratchpads', scratchpad.id)}
                    data-tree-row
                    data-context-kind="scratchpad"
                    data-context-id={scratchpad.id}
                    use:reorderItem={scratchpadReorderOptions(scratchpad)}
                    onclick={(event) => selectGroupItem(event, 'scratchpads', scratchpad.id, visibleScratchpads.map((candidate) => candidate.id), () => onSelect(projectTreeSelection('scratchpad', scratchpad.id, project.id, scratchpad.name)))}
                    onmousedown={preventMiddleMouseDefault}
                    onauxclick={(event) => handleMiddleClick(event, scratchpadTarget(scratchpad))}
                    oncontextmenu={(event) => openSelectablePointerMenu(event, scratchpadTarget(scratchpad), 'scratchpads', scratchpad.id, visibleScratchpads.map((candidate) => candidate.id))}
                    onkeydown={(event) => openKeyboardMenu(event, scratchpadTarget(scratchpad))}
                  >
                    <span class="scratchpad-ref" aria-label={`Scratchpad #${scratchpad.id} · revision ${scratchpad.revision}`}>#{scratchpad.id}</span>
                    <span class="row-copy"><strong>{scratchpad.name}</strong></span>
                    <span class="row-badges">
                      {#if scratchpad.unresolved_comment_count > 0}<CountBadge value={scratchpad.unresolved_comment_count} title={`${scratchpad.unresolved_comment_count} unresolved scratchpad comments`} />{/if}
                      <span class="row-meta" aria-label={`Scratchpad revision ${scratchpad.revision}`}>r{scratchpad.revision}</span>
                    </span>
                  </button>
                {/if}
              {:else}
                <p class="empty-row">{query ? 'No matching scratchpads' : 'No scratchpads'}</p>
              {/each}
              {#if scratchpads.length === 0}
                <button class="add-row" type="button" data-tree-row onclick={onAddScratchpad}>+ Add scratchpad</button>
              {/if}
            {/if}
          </div>
        {/if}
      </section>
    {/each}
  </div>

  <footer class="tree-footer">
    <IconButton class="size-7" label="Open Settings" shortcut={hotkeyDisplayLabel($hotkeyPreferences['open-settings']) || undefined} data-tree-row onclick={onOpenSettings}>
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
  .group-header { position: relative; display: grid; width: 100%; min-height: 28px; grid-template-columns: 17px 19px minmax(0, 1fr) auto 28px; align-items: center; gap: 0; padding: 3px; color: var(--text-soft); }
  .group-drag-handle { display: grid; width: 17px; height: 22px; place-items: center; border: 0; border-radius: 3px; padding: 0; background: transparent; color: var(--muted-foreground); opacity: .42; cursor: grab; }
  .group-drag-handle:hover, .group-drag-handle:focus-visible, .group-header:hover .group-drag-handle { background: color-mix(in srgb, var(--muted) 70%, transparent); color: var(--foreground); opacity: 1; }
  .group-drag-handle:active { cursor: grabbing; }
  .group-toggle, .group-title { min-width: 0; min-height: 22px; border: 0; border-radius: var(--radius); padding: 0; background: transparent; color: inherit; cursor: pointer; }
  .group-toggle { display: grid; place-items: center; }
  .group-title { display: grid; grid-template-columns: 16px minmax(0, 1fr); align-items: center; gap: 4px; text-align: left; }
  .group-badges { display: flex; align-items: center; gap: 4px; }
  .group-header :global(.group-create) { justify-self: center; }
  .group-header:hover { background: var(--popover); }
  .project-tree :global(.group-header[data-reorder-dragging='true']) { opacity: .42; }
  .project-tree :global(.group-header[data-reorder-drop]::after) { position: absolute; z-index: 4; right: 4px; left: 4px; height: 2px; border-radius: 1px; background: var(--ring); content: ''; pointer-events: none; }
  .project-tree :global(.group-header[data-reorder-drop='before']::after) { top: -1px; }
  .project-tree :global(.group-header[data-reorder-drop='after']::after) { bottom: -1px; }
  .group-toggle:focus-visible, .group-title:focus-visible { position: relative; z-index: 1; }
  .group-header strong { overflow: hidden; font-size: var(--font-size-sm); font-weight: 700; letter-spacing: 0.055em; text-overflow: ellipsis; text-transform: uppercase; white-space: nowrap; }
  .caret { color: var(--muted-foreground); font: var(--font-size-sm) 'JetBrains Mono Variable', monospace; }
  .group-icon { color: var(--muted-foreground); font: var(--font-size-sm) 'JetBrains Mono Variable', monospace; text-align: center; }
  .row-meta { flex: none; border: 1px solid var(--border-strong); border-radius: 3px; padding: 1px 4px; color: var(--text-soft); background: var(--popover); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .group-rows { padding: 0 4px 4px 13px; }
  .bulk-action-bar { display: flex; min-height: 26px; align-items: center; justify-content: space-between; gap: 4px; margin-bottom: 2px; padding: 2px 3px 2px 6px; border-block: 1px solid var(--border); color: var(--text-soft); background: color-mix(in srgb, var(--accent) 8%, var(--card)); }
  .bulk-action-bar > strong { font: 650 var(--font-size-xs) 'JetBrains Mono Variable', monospace; white-space: nowrap; }
  .bulk-action-buttons { display: flex; min-width: 0; align-items: center; gap: 1px; }
  .bulk-action-buttons button { height: 21px; border: 0; border-radius: 2px; padding: 0 5px; background: transparent; color: var(--text-soft); font-size: var(--font-size-xs); cursor: pointer; }
  .bulk-action-buttons button:hover:not(:disabled) { background: var(--popover); color: var(--foreground); }
  .bulk-action-buttons button.destructive { color: var(--destructive); }
  .bulk-action-buttons button.clear { display: grid; width: 21px; place-items: center; padding: 0; color: var(--muted-foreground); }
  .bulk-action-buttons button:disabled { opacity: 0.5; cursor: default; }
  .tree-row, .add-row, .show-all { display: grid; width: 100%; min-height: 28px; align-items: center; border: 0; border-radius: 3px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .tree-row { position: relative; grid-template-columns: 17px minmax(0, 1fr) auto; gap: 4px; padding: 3px 5px; }
  .todo-row { min-height: 24px; grid-template-columns: 2px 15px minmax(0, 1fr) auto; gap: 3px; padding-block: 1px; }
  .todo-row .todo-state-rail { align-self: stretch; border-radius: 1px; background: var(--todo-state-open); opacity: 0.55; }
  .todo-row[data-todo-state='claimed'] .todo-state-rail { background: var(--todo-state-claimed); opacity: 1; }
  .todo-row[data-todo-state='blocked'] .todo-state-rail { background: var(--todo-state-blocked); opacity: 1; }
  .todo-row[data-todo-state='completed'] .todo-state-rail { background: var(--todo-state-completed); opacity: 0.45; }
  .todo-row[data-todo-state='claimed'] { background: color-mix(in srgb, var(--todo-state-claimed) 5%, transparent); }
  .todo-row[data-todo-state='blocked'] { background: color-mix(in srgb, var(--todo-state-blocked) 6%, transparent); }
  .todo-row .row-copy strong { font-size: var(--font-size-xs); font-weight: 570; }
  .todo-assigned-marker { display: grid; width: 18px; height: 18px; place-items: center; border: 1px solid var(--border-strong); border-radius: 3px; background: var(--popover); color: var(--text-soft); }
  .project-tree :global(.tree-row[data-reorderable='true']) { cursor: grab; }
  .project-tree :global(.tree-row[data-reorder-dragging='true']) { opacity: 0.42; cursor: grabbing; }
  .project-tree :global(.tree-row[data-reorder-drop]::after) { position: absolute; z-index: 3; right: 4px; left: 4px; height: 2px; border-radius: 1px; background: var(--ring); content: ''; pointer-events: none; }
  .project-tree :global(.tree-row[data-reorder-drop='before']::after) { top: -1px; }
  .project-tree :global(.tree-row[data-reorder-drop='after']::after) { bottom: -1px; }
  .agent-row-shell.agent-child { width: calc(100% - min(calc(var(--agent-depth) * 12px), 48px)); margin-left: min(calc(var(--agent-depth) * 12px), 48px); }
  .agent-row-shell.agent-child .agent-row { grid-template-columns: 12px 17px minmax(0, 1fr) auto; }
  .tree-row:hover, .add-row:hover, .show-all:hover { background: var(--popover); }
  .tree-row.selected { background: var(--accent); color: #fff; box-shadow: inset 2px 0 var(--muted-foreground); }
  .feedback-row.archived:not(.selected) { opacity: 0.58; }
  .tree-row.multi-selected { background: color-mix(in srgb, var(--ring) 15%, var(--card)); color: var(--foreground); box-shadow: inset 2px 0 color-mix(in srgb, var(--ring) 72%, var(--border)); }
  .process-row-shell { display: grid; grid-template-columns: minmax(0, 1fr); align-items: center; border-radius: 3px; }
  .process-row-shell.has-actions { grid-template-columns: minmax(0, 1fr) 28px; }
  .process-row-shell:hover { background: var(--popover); }
  .process-row-shell.selected { background: var(--accent); color: #fff; box-shadow: inset 2px 0 var(--muted-foreground); }
  .process-row-shell.multi-selected { background: color-mix(in srgb, var(--ring) 15%, var(--card)); color: var(--foreground); box-shadow: inset 2px 0 color-mix(in srgb, var(--ring) 72%, var(--border)); }
  .process-row-shell.multi-selected .tree-row { color: var(--foreground); }
  .process-row-shell .tree-row:hover { background: transparent; }
  .process-row-shell.selected .tree-row { color: #fff; }
  .process-actions { display: flex; width: 28px; align-items: center; justify-content: center; opacity: 0; transition: opacity 120ms ease; }
  .process-row-shell:hover .process-actions, .process-row-shell:focus-within .process-actions { opacity: 1; }
  .command-row-shell { display: grid; grid-template-columns: minmax(0, 1fr) 52px; align-items: center; border-radius: 3px; }
  .command-row-shell:hover { background: var(--popover); }
  .command-row-shell.selected { background: var(--accent); color: #fff; box-shadow: inset 2px 0 var(--muted-foreground); }
  .command-row-shell .command-row:hover { background: transparent; }
  .command-actions { display: flex; width: 52px; align-items: center; justify-content: flex-end; gap: 2px; padding-right: 2px; opacity: 0; transition: opacity 120ms ease; }
  .command-row-shell:hover .command-actions, .command-row-shell:focus-within .command-actions { opacity: 1; }
  .row-copy { min-width: 0; }
  .row-copy strong, .row-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row-copy strong { font-size: var(--font-size-sm); font-weight: 590; }
  .row-copy small { margin-top: 1px; color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .row-badges { display: flex; min-width: 0; align-items: center; justify-content: flex-end; gap: 3px; }
  .lineage-glyph { color: #687e74; font: var(--font-size-sm)/1 'JetBrains Mono Variable', monospace; transform: translateY(-1px); }
  .agent-unread-dot { display: block; width: 7px; height: 7px; flex: none; border-radius: 999px; background: var(--notification-unread); box-shadow: 0 0 0 2px color-mix(in srgb, var(--notification-unread) 17%, transparent); }
  .unread-lineage-rollup, .unread-group-rollup { display: inline-flex; height: 18px; align-items: center; justify-content: center; gap: 3px; border: 1px solid color-mix(in srgb, var(--notification-unread) 45%, var(--border)); border-radius: 999px; padding: 0 5px; color: var(--notification-unread-foreground); background: color-mix(in srgb, var(--notification-unread) 9%, var(--popover)); font: 650 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace; }
  .unread-lineage-rollup > span, .unread-group-rollup > span { width: 5px; height: 5px; border-radius: 999px; background: var(--notification-unread); }
  .unread-group-rollup { margin-left: -4px; }
  .scratchpad-row { grid-template-columns: 34px minmax(0, 1fr) auto; }
  .scratchpad-ref { color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .add-row, .show-all { grid-template-columns: 1fr; padding: 3px 5px 3px 22px; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .add-row { color: var(--text-soft); }
  .empty-row { margin: 0; padding: 5px 5px 5px 22px; color: #686f78; font-size: var(--font-size-sm); }
  .tree-footer { display: flex; min-height: 38px; align-items: center; justify-content: flex-end; padding: 5px 6px; border-top: 1px solid var(--border); }

  @media (prefers-reduced-motion: reduce) { .command-actions, .process-actions { transition: none; } }

  .project-tree.collapsed .tree-toolbar { justify-content: center; padding-inline: 0; }
  .project-tree.collapsed .tree-groups { padding-inline: 4px; }
  .project-tree.collapsed .tree-group { border: 0; }
  .project-tree.collapsed .group-header { min-height: 36px; grid-template-columns: 1fr; justify-items: center; padding: 4px; }
  .project-tree.collapsed .group-toggle,
  .project-tree.collapsed .group-drag-handle,
  .project-tree.collapsed .group-title strong,
  .project-tree.collapsed .group-header :global(.badge) { display: none; }
  .project-tree.collapsed .group-title { width: 100%; grid-template-columns: 1fr; justify-items: center; }
  .project-tree.collapsed .group-icon { position: relative; font-size: var(--font-size-sm); }
  .project-tree.collapsed .collapsed-agent-waiting { position: absolute; top: -7px; right: -10px; }
  .project-tree.collapsed .tree-footer { justify-content: center; padding-inline: 4px; }
</style>
