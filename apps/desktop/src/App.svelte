<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';

  import EmptyState from './lib/EmptyState.svelte';
  import ProcessStatusBar from './lib/ProcessStatusBar.svelte';
  import ProjectTree from './lib/ProjectTree.svelte';
  import ScratchpadDetailView from './lib/ScratchpadDetailView.svelte';
  import SettingsPanel from './lib/SettingsPanel.svelte';
  import TerminalView from './lib/TerminalView.svelte';
  import TodoDetailView from './lib/TodoDetailView.svelte';
  import TrustReviewDialog from './lib/TrustReview.svelte';
  import type { AgentTool } from './lib/agentTools';
  import type {
    CoordinationSnapshot,
    NewTodoInput,
    ScratchpadRead,
    TodoDetail,
    TodoPriority
  } from './lib/coordination';
  import {
    DaemonClient,
    type ConnectionStatus,
    type ProcessView,
    type Project,
    type TrustReview
  } from './lib/daemon';
  import {
    clampPanelWidth,
    loadPanelPreference,
    savePanelPreference,
    startPanelResize
  } from './lib/panelPreferences';
  import {
    isProcessSelection,
    projectTreeSelection,
    type ProjectTreeSelection
  } from './lib/projectTree';

  const client = new DaemonClient();
  const projectRailBounds = { min: 176, max: 340 };
  const treeRailBounds = { min: 220, max: 420 };
  const collapsedProjectRailWidth = 58;
  const collapsedTreeRailWidth = 54;

  let projects = $state<Project[]>([]);
  let processes = $state<ProcessView[]>([]);
  let coordination = $state<CoordinationSnapshot | null>(null);
  let connection = $state<ConnectionStatus>({ status: 'connecting', message: null, port: null });
  let selection = $state<ProjectTreeSelection | null>(null);
  let todoDetail = $state<TodoDetail | null>(null);
  let scratchpadRead = $state<ScratchpadRead | null>(null);
  let detailLoading = $state(false);
  let detailBusy = $state(false);
  let busy = $state(false);
  let processBusyId = $state<number | null>(null);
  let loadedProjectId = $state<number | null>(null);
  let processRequest = 0;
  let coordinationRequest = 0;
  let error = $state<string | null>(null);
  let renameId = $state<number | null>(null);
  let renameValue = $state('');
  let settingsOpen = $state(false);
  let trustReview = $state<TrustReview | null>(null);
  let trustBusy = $state(false);
  let projectRailWidth = $state(238);
  let projectRailCollapsed = $state(false);
  let treeRailWidth = $state(280);
  let treeRailCollapsed = $state(false);

  let dialog = $state<'todo' | 'scratchpad' | 'agent' | 'command' | null>(null);
  let todoTitle = $state('');
  let todoBody = $state('');
  let todoPriority = $state<TodoPriority>('medium');
  let todoTags = $state('');
  let scratchpadName = $state('');
  let scratchpadContent = $state('');
  let agentTools = $state<AgentTool[]>([]);
  let agentToolsLoading = $state(false);

  let selectedProject = $derived(projects.find((project) => project.selected) ?? null);
  let selectedProcess = $derived(
    selection && isProcessSelection(selection)
      ? processes.find((process) => process.id === selection?.id) ?? null
      : null
  );
  let treeProcesses = $derived([
    ...processes.filter((process) => process.kind === 'agent'),
    ...processes.filter((process) => process.kind === 'terminal'),
    ...processes.filter((process) => process.kind === 'command')
  ]);
  let frameItemLabel = $derived(settingsOpen ? 'Settings' : (selection?.label ?? 'Project'));

  $effect(() => {
    const projectId = selectedProject?.id ?? null;
    const connected = connection.status === 'connected';
    if (!connected || projectId === null) {
      processes = [];
      coordination = null;
      selection = null;
      todoDetail = null;
      scratchpadRead = null;
      settingsOpen = false;
      loadedProjectId = null;
      return;
    }
    if (loadedProjectId !== projectId) {
      loadedProjectId = projectId;
      processes = [];
      coordination = null;
      selection = null;
      todoDetail = null;
      scratchpadRead = null;
      settingsOpen = false;
      void loadProject(projectId);
    }
  });

  onMount(() => {
    const projectPreference = loadPanelPreference(
      'project-rail',
      { collapsed: false, width: projectRailWidth },
      projectRailBounds.min,
      projectRailBounds.max
    );
    projectRailWidth = projectPreference.width;
    projectRailCollapsed = projectPreference.collapsed;
    const treePreference = loadPanelPreference(
      'section-rail',
      { collapsed: false, width: treeRailWidth },
      treeRailBounds.min,
      treeRailBounds.max
    );
    treeRailWidth = treePreference.width;
    treeRailCollapsed = treePreference.collapsed;

    let active = true;
    const stopStatuses = client.onProcessStatuses((next) => {
      if (!active || !selectedProject) return;
      applyProcesses(next.filter((process) => process.project_id === selectedProject?.id));
    });
    const projectTimer = setInterval(() => {
      if (active && connection.status === 'connected' && !busy) void refreshProjects();
    }, 5000);
    const coordinationTimer = setInterval(() => {
      if (active && connection.status === 'connected' && selectedProject) {
        void refreshCoordination(selectedProject.id, false);
      }
    }, 2500);

    void client
      .start(
        (status) => { if (active) applyConnectionStatus(status); },
        (message) => { if (active) error = message; }
      )
      .then((status) => { if (active) applyConnectionStatus(status); })
      .catch(reportError);

    return () => {
      active = false;
      clearInterval(projectTimer);
      clearInterval(coordinationTimer);
      stopStatuses();
      client.close();
    };
  });

  function applyConnectionStatus(status: ConnectionStatus): void {
    const reconnected = connection.status !== 'connected' && status.status === 'connected';
    connection = status;
    if (reconnected) {
      void client.subscribeProcessStatuses().catch(reportError);
      void refreshProjects();
    }
  }

  function handleShortcut(event: KeyboardEvent): void {
    const target = event.target as HTMLElement | null;
    if (
      target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target?.isContentEditable
    ) return;
    if (event.metaKey && !event.altKey && event.key.toLowerCase() === 'b') {
      event.preventDefault();
      if (event.shiftKey) toggleTreeRail();
      else toggleProjectRail();
      return;
    }
    if (event.key === 'Escape') {
      if (dialog) dialog = null;
      else if (settingsOpen) settingsOpen = false;
      else clearSelection();
    }
  }

  async function loadProject(projectId: number): Promise<void> {
    try {
      await client.syncConfig(projectId);
    } catch (cause) {
      reportError(cause);
    }
    await Promise.all([refreshProcesses(projectId), refreshCoordination(projectId, true)]);
  }

  async function refreshProjects(): Promise<void> {
    busy = true;
    try {
      projects = await client.projects();
    } catch (cause) {
      reportError(cause);
    } finally {
      busy = false;
    }
  }

  async function refreshProcesses(projectId: number): Promise<void> {
    const request = ++processRequest;
    try {
      const next = await client.processes(projectId);
      if (request === processRequest && selectedProject?.id === projectId) applyProcesses(next);
    } catch (cause) {
      if (request === processRequest) reportError(cause);
    }
  }

  function applyProcesses(next: ProcessView[]): void {
    processes = next;
    if (selection && isProcessSelection(selection)) {
      const process = next.find((candidate) => candidate.id === selection?.id);
      if (process) selection = projectTreeSelection(process.kind, process.id, process.project_id, processLabel(process));
    }
  }

  async function refreshCoordination(projectId: number, showLoading: boolean): Promise<void> {
    const request = ++coordinationRequest;
    if (showLoading) detailLoading = true;
    try {
      const next = await client.coordinationSnapshot(projectId);
      if (request === coordinationRequest && selectedProject?.id === projectId) coordination = next;
    } catch (cause) {
      if (request === coordinationRequest) reportError(cause);
    } finally {
      if (showLoading && request === coordinationRequest) detailLoading = false;
    }
  }

  async function selectTreeItem(next: ProjectTreeSelection): Promise<void> {
    if (!selectedProject || next.projectId !== selectedProject.id) return;
    settingsOpen = false;
    selection = next;
    todoDetail = null;
    scratchpadRead = null;

    if (next.kind === 'todo') {
      await loadTodo(next.id);
    } else if (next.kind === 'scratchpad') {
      await loadScratchpad(next.id);
    } else if (next.kind === 'command') {
      const process = processes.find((candidate) => candidate.id === next.id);
      if (process && process.status !== 'running' && process.status !== 'starting') {
        if (process.source === 'yml' && process.trust_hash === null) await openTrustReview(process);
        else await startProcess(process);
      }
    }
  }

  async function loadTodo(todoId: number): Promise<void> {
    if (!selectedProject) return;
    detailLoading = true;
    try {
      todoDetail = await client.coordinationTodo(selectedProject.id, todoId);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailLoading = false;
    }
  }

  async function loadScratchpad(scratchpadId: number): Promise<void> {
    if (!selectedProject) return;
    detailLoading = true;
    try {
      scratchpadRead = await client.coordinationScratchpad(selectedProject.id, scratchpadId);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailLoading = false;
    }
  }

  async function startProcess(process: ProcessView): Promise<void> {
    processBusyId = process.id;
    try {
      await client.startProcess(process.id);
      await refreshProcesses(process.project_id);
    } catch (cause) {
      reportError(cause);
    } finally {
      processBusyId = null;
    }
  }

  async function spawnTerminal(): Promise<void> {
    if (!selectedProject || processBusyId !== null) return;
    processBusyId = -1;
    try {
      const process = await client.spawnTerminal(selectedProject.id);
      await refreshProcesses(selectedProject.id);
      await selectTreeItem(projectTreeSelection('terminal', process.id, process.project_id, processLabel(process)));
    } catch (cause) {
      reportError(cause);
    } finally {
      processBusyId = null;
    }
  }

  async function openAgentDialog(): Promise<void> {
    dialog = 'agent';
    agentToolsLoading = true;
    try {
      agentTools = (await client.listAgentTools()).filter((tool) => tool.enabled);
    } catch (cause) {
      reportError(cause);
    } finally {
      agentToolsLoading = false;
    }
  }

  async function spawnAgent(tool: AgentTool): Promise<void> {
    if (!selectedProject) return;
    detailBusy = true;
    try {
      const result = await client.spawnAgent({
        project_id: selectedProject.id,
        agent_tool_id: tool.id,
        extra_args: []
      });
      dialog = null;
      await refreshProcesses(selectedProject.id);
      const process = processes.find((candidate) => candidate.id === result.process_id);
      if (process) await selectTreeItem(projectTreeSelection('agent', process.id, process.project_id, process.name));
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function createTodo(): Promise<void> {
    if (!selectedProject || !todoTitle.trim()) return;
    detailBusy = true;
    const input: NewTodoInput = {
      title: todoTitle.trim(),
      body: todoBody.trim(),
      priority: todoPriority,
      tags: todoTags.split(',').map((tag) => tag.trim()).filter(Boolean)
    };
    try {
      const todo = await client.coordinationTodoCreate(selectedProject.id, input);
      resetTodoForm();
      dialog = null;
      await refreshCoordination(selectedProject.id, false);
      await selectTreeItem(projectTreeSelection('todo', todo.id, todo.project_id, todo.title));
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function createScratchpad(): Promise<void> {
    if (!selectedProject || !scratchpadName.trim()) return;
    detailBusy = true;
    try {
      const scratchpad = await client.coordinationScratchpadCreate(selectedProject.id, {
        name: scratchpadName.trim(), content: scratchpadContent.trim(), tags: []
      });
      scratchpadName = '';
      scratchpadContent = '';
      dialog = null;
      await refreshCoordination(selectedProject.id, false);
      await selectTreeItem(
        projectTreeSelection('scratchpad', scratchpad.id, scratchpad.project_id, scratchpad.name)
      );
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function completeTodo(completed: boolean): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo') return;
    detailBusy = true;
    try {
      await client.coordinationTodoComplete(selectedProject.id, selection.id, completed);
      await Promise.all([loadTodo(selection.id), refreshCoordination(selectedProject.id, false)]);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function commentTodo(body: string): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo') return;
    detailBusy = true;
    try {
      await client.coordinationTodoComment(selectedProject.id, selection.id, body);
      await Promise.all([loadTodo(selection.id), refreshCoordination(selectedProject.id, false)]);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  function resetTodoForm(): void {
    todoTitle = '';
    todoBody = '';
    todoPriority = 'medium';
    todoTags = '';
  }

  function clearSelection(): void {
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
  }

  function selectProcessById(processId: number): void {
    const process = treeProcesses.find((candidate) => candidate.id === processId);
    if (process) void selectTreeItem(projectTreeSelection(process.kind, process.id, process.project_id, processLabel(process)));
  }

  function processLabel(process: ProcessView): string {
    if (process.kind !== 'terminal') return process.name;
    const parts = process.working_dir.split('/').filter(Boolean);
    return parts[0] === 'Users' && parts.length > 2 ? `~/${parts.slice(2).join('/')}` : process.working_dir;
  }

  async function openTrustReview(process: ProcessView): Promise<void> {
    processBusyId = process.id;
    try {
      trustReview = await client.trustReview(process.id);
    } catch (cause) {
      reportError(cause);
    } finally {
      processBusyId = null;
    }
  }

  async function approveTrust(): Promise<void> {
    if (!trustReview || !selectedProject) return;
    const review = trustReview;
    trustBusy = true;
    try {
      await client.trustYmlProcess(review.process_id, review.expected_hash);
      trustReview = null;
      await refreshProcesses(selectedProject.id);
      const process = processes.find((candidate) => candidate.id === review.process_id);
      if (process) await startProcess(process);
    } catch (cause) {
      reportError(cause);
    } finally {
      trustBusy = false;
    }
  }

  async function registerProject(): Promise<void> {
    const path = await open({ directory: true, multiple: false, title: 'Register a project folder' });
    if (typeof path !== 'string') return;
    busy = true;
    try {
      projects = await client.register(path);
    } catch (cause) {
      reportError(cause);
    } finally {
      busy = false;
    }
  }

  async function selectProject(project: Project): Promise<void> {
    if (project.selected || busy) return;
    busy = true;
    try {
      projects = await client.select(project.id);
    } catch (cause) {
      reportError(cause);
    } finally {
      busy = false;
    }
  }

  function beginRename(project: Project): void {
    renameId = project.id;
    renameValue = project.display_name ?? project.name;
  }

  function cancelRename(): void {
    renameId = null;
    renameValue = '';
  }

  async function commitRename(): Promise<void> {
    if (renameId === null || !renameValue.trim()) return;
    const projectId = renameId;
    const name = renameValue.trim();
    cancelRename();
    try {
      projects = await client.rename(projectId, name);
    } catch (cause) {
      reportError(cause);
    }
  }

  function focusRename(node: HTMLInputElement): void {
    queueMicrotask(() => { node.focus(); node.select(); });
  }

  function focusDialogInput(node: HTMLInputElement): void {
    queueMicrotask(() => node.focus());
  }

  function projectLabel(project: Project): string {
    return project.display_name ?? project.name;
  }

  function persistProjectRail(): void {
    savePanelPreference('project-rail', { collapsed: projectRailCollapsed, width: projectRailWidth });
  }

  function persistTreeRail(): void {
    savePanelPreference('section-rail', { collapsed: treeRailCollapsed, width: treeRailWidth });
  }

  function toggleProjectRail(): void {
    projectRailCollapsed = !projectRailCollapsed;
    persistProjectRail();
  }

  function toggleTreeRail(): void {
    if (!selectedProject) return;
    treeRailCollapsed = !treeRailCollapsed;
    persistTreeRail();
  }

  function resizeRailFromKeyboard(event: KeyboardEvent, rail: 'project' | 'tree'): void {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    const delta = event.key === 'ArrowLeft' ? -12 : 12;
    if (rail === 'project') {
      projectRailWidth = clampPanelWidth(projectRailWidth + delta, projectRailBounds.min, projectRailBounds.max);
      persistProjectRail();
    } else {
      treeRailWidth = clampPanelWidth(treeRailWidth + delta, treeRailBounds.min, treeRailBounds.max);
      persistTreeRail();
    }
  }

  function reportError(cause: unknown): void {
    error = cause instanceof Error ? cause.message : String(cause);
  }
</script>

<svelte:window onkeydown={handleShortcut} />

<svelte:head>
  <title>{selectedProject ? `${projectLabel(selectedProject)} - ${frameItemLabel}` : 'gbuild'}</title>
</svelte:head>

<main
  class="app-shell"
  class:no-project={selectedProject === null}
  style={`--project-rail-width: ${projectRailCollapsed ? collapsedProjectRailWidth : projectRailWidth}px; --tree-rail-width: ${treeRailCollapsed ? collapsedTreeRailWidth : treeRailWidth}px;`}
>
  <aside class="project-rail" class:collapsed={projectRailCollapsed} aria-label="Projects">
    <header class="brand" data-tauri-drag-region>
      <div class="brand-mark" aria-hidden="true"><span></span><span></span><span></span></div>
      <div class="brand-copy"><strong>gbuild</strong><span>local workspaces</span></div>
      <button
        class="rail-toggle"
        type="button"
        aria-label={`${projectRailCollapsed ? 'Expand' : 'Collapse'} project rail`}
        title={`${projectRailCollapsed ? 'Expand' : 'Collapse'} project rail (⌘B)`}
        onclick={toggleProjectRail}
      >{projectRailCollapsed ? '›' : '‹'}</button>
    </header>

    <div class="rail-label"><span>Projects</span><small>{projects.length.toString().padStart(2, '0')}</small></div>
    <div class="project-list" aria-live="polite">
      {#if projects.length === 0 && connection.status === 'connected' && !busy}
        <div class="project-empty"><strong>No projects</strong><p>Register a folder to begin.</p><button type="button" onclick={() => void registerProject()}>Register folder</button></div>
      {/if}
      {#each projects as project (project.id)}
        <article class:active={project.selected} class="project-row">
          {#if renameId === project.id}
            <form class="rename-form" onsubmit={(event) => { event.preventDefault(); void commitRename(); }}>
              <input aria-label="Project name" bind:value={renameValue} use:focusRename onkeydown={(event) => { if (event.key === 'Escape') cancelRename(); }} />
              <button type="submit">Save</button>
            </form>
          {:else}
            <button class="project-select" type="button" aria-current={project.selected ? 'page' : undefined} aria-label={`${projectLabel(project)}, ${project.status}`} onclick={() => void selectProject(project)}>
              <span class="status-dot" class:error={project.status === 'error'} class:running={project.status === 'running'} aria-hidden="true"></span>
              <span class="project-glyph" aria-hidden="true">{projectLabel(project).slice(0, 1).toUpperCase()}</span>
              <span class="project-copy"><strong>{projectLabel(project)}</strong><small>{project.path}</small></span>
            </button>
            <button class="rename-button" type="button" aria-label={`Rename ${projectLabel(project)}`} title="Rename project" onclick={() => beginRename(project)}>···</button>
          {/if}
        </article>
      {/each}
    </div>
    <footer class="project-footer">
      <button class="register-button" type="button" disabled={connection.status !== 'connected' || busy} onclick={() => void registerProject()}><span aria-hidden="true">+</span><span class="button-copy">Register project</span></button>
    </footer>
    {#if !projectRailCollapsed}
      <button
        type="button"
        class="resize-handle"
        aria-label="Resize project rail"
        title={`Resize project rail · ${projectRailWidth}px · arrow keys`}
        onkeydown={(event) => resizeRailFromKeyboard(event, 'project')}
        onpointerdown={(event) => startPanelResize(event, {
          current: projectRailWidth, min: projectRailBounds.min, max: projectRailBounds.max,
          onResize: (width) => (projectRailWidth = width), onEnd: persistProjectRail
        })}
      ></button>
    {/if}
  </aside>

  {#if selectedProject}
    <aside class="tree-rail" aria-label={`${projectLabel(selectedProject)} items`}>
      <ProjectTree
        project={selectedProject}
        {processes}
        todos={coordination?.todos ?? []}
        scratchpads={coordination?.scratchpads ?? []}
        {selection}
        collapsed={treeRailCollapsed}
        connected={connection.status === 'connected'}
        onSelect={(next) => void selectTreeItem(next)}
        onCreateTodo={() => (dialog = 'todo')}
        onAddAgent={() => void openAgentDialog()}
        onAddTerminal={() => void spawnTerminal()}
        onAddCommand={() => (dialog = 'command')}
        onAddScratchpad={() => (dialog = 'scratchpad')}
        onOpenSettings={() => { settingsOpen = true; dialog = null; }}
        onToggleCollapse={toggleTreeRail}
      />
      {#if !treeRailCollapsed}
        <button
          type="button"
          class="resize-handle"
          aria-label="Resize project tree"
          title={`Resize project tree · ${treeRailWidth}px · arrow keys`}
          onkeydown={(event) => resizeRailFromKeyboard(event, 'tree')}
          onpointerdown={(event) => startPanelResize(event, {
            current: treeRailWidth, min: treeRailBounds.min, max: treeRailBounds.max,
            onResize: (width) => (treeRailWidth = width), onEnd: persistTreeRail
          })}
        ></button>
      {/if}
    </aside>
  {/if}

  <section class="main-frame" class:empty={selectedProject === null}>
    {#if selectedProject}
      <header class="document-title" data-tauri-drag-region>
        <div class="title-side"><span>{selection?.kind ?? 'project'}</span></div>
        <h1>{projectLabel(selectedProject)} - {frameItemLabel}</h1>
        <div class="title-side right">
          {#if settingsOpen}<button type="button" onclick={() => (settingsOpen = false)}>Done</button>{/if}
        </div>
      </header>
      {#if error}
        <button class="error-banner" type="button" onclick={() => (error = null)}><span>{error}</span><strong>Dismiss</strong></button>
      {/if}
      <div class="item-viewer">
        {#if settingsOpen}
          <SettingsPanel {client} project={selectedProject} {connection} onError={reportError} />
        {:else if selectedProcess}
          {#key selectedProcess.id}
            <div class="terminal-view"><TerminalView {client} process={selectedProcess} connected={connection.status === 'connected'} onError={reportError} /></div>
          {/key}
        {:else if selection?.kind === 'todo'}
          <TodoDetailView detail={todoDetail} loading={detailLoading} busy={detailBusy} onComplete={(completed) => void completeTodo(completed)} onComment={(body) => void commentTodo(body)} />
        {:else if selection?.kind === 'scratchpad'}
          <ScratchpadDetailView read={scratchpadRead} loading={detailLoading} onRefresh={() => void loadScratchpad(selection?.id ?? 0)} />
        {:else}
          <EmptyState eyebrow="Project tree" title="Select an item" body="Choose a todo, agent, terminal, command, or scratchpad from the project tree." actionLabel="New terminal" icon="↖" onAction={() => void spawnTerminal()} />
        {/if}
      </div>
      {#if selectedProcess}
        <ProcessStatusBar
          {client}
          project={selectedProject}
          process={selectedProcess}
          processes={treeProcesses}
          connected={connection.status === 'connected'}
          onUnfocus={clearSelection}
          onSelectProcess={selectProcessById}
          onError={reportError}
        />
      {/if}
    {:else}
      <div class="onboarding">
        <span>Local workspaces</span><h1>Register a project</h1><p>Choose a repository to see its work tree.</p>
        <button type="button" disabled={connection.status !== 'connected' || busy} onclick={() => void registerProject()}>+ Register project</button>
      </div>
    {/if}
  </section>
</main>

{#if dialog}
  <div class="dialog-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) dialog = null; }}>
    {#if dialog === 'todo'}
      <form class="dialog" aria-label="Create todo" onsubmit={(event) => { event.preventDefault(); void createTodo(); }}>
        <header><div><span>New todo</span><h2>Add work to the tree</h2></div><button type="button" aria-label="Close" onclick={() => (dialog = null)}>×</button></header>
        <label><span>Title</span><input bind:value={todoTitle} placeholder="What needs to happen?" use:focusDialogInput /></label>
        <label><span>Notes <small>optional</small></span><textarea bind:value={todoBody} rows="4" placeholder="Outcome, constraints, or context"></textarea></label>
        <div class="dialog-row"><label><span>Priority</span><select bind:value={todoPriority}><option value="high">High</option><option value="medium">Medium</option><option value="low">Low</option></select></label><label><span>Tags</span><input bind:value={todoTags} placeholder="ui, follow-up" /></label></div>
        <footer><button type="button" onclick={() => (dialog = null)}>Cancel</button><button class="primary" type="submit" disabled={detailBusy || !todoTitle.trim()}>Create todo</button></footer>
      </form>
    {:else if dialog === 'scratchpad'}
      <form class="dialog" aria-label="Create scratchpad" onsubmit={(event) => { event.preventDefault(); void createScratchpad(); }}>
        <header><div><span>New scratchpad</span><h2>Add a shared note</h2></div><button type="button" aria-label="Close" onclick={() => (dialog = null)}>×</button></header>
        <label><span>Name</span><input bind:value={scratchpadName} placeholder="Release notes" use:focusDialogInput /></label>
        <label><span>Content <small>optional</small></span><textarea bind:value={scratchpadContent} rows="7" placeholder="Write the first note in Markdown"></textarea></label>
        <footer><button type="button" onclick={() => (dialog = null)}>Cancel</button><button class="primary" type="submit" disabled={detailBusy || !scratchpadName.trim()}>Create scratchpad</button></footer>
      </form>
    {:else if dialog === 'agent'}
      <section class="dialog" aria-label="Add agent">
        <header><div><span>New agent</span><h2>Choose an agent tool</h2></div><button type="button" aria-label="Close" onclick={() => (dialog = null)}>×</button></header>
        <div class="agent-choices">
          {#if agentToolsLoading}<p>Loading agent tools…</p>{:else}{#each agentTools as tool (tool.id)}<button type="button" disabled={detailBusy} onclick={() => void spawnAgent(tool)}><strong>{tool.name}</strong><small>{tool.command}</small><span>Spawn</span></button>{:else}<p>No enabled agent tools. Add one in Settings.</p>{/each}{/if}
        </div>
        <footer><button type="button" onclick={() => { dialog = null; settingsOpen = true; }}>Open Settings</button><button type="button" onclick={() => (dialog = null)}>Cancel</button></footer>
      </section>
    {:else}
      <section class="dialog" aria-label="Add command">
        <header><div><span>Project command</span><h2>Add it to gbuild.yml</h2></div><button type="button" aria-label="Close" onclick={() => (dialog = null)}>×</button></header>
        <div class="command-help"><code>{selectedProject?.path}/gbuild.yml</code><p>Define the command in this project file, then refresh the tree. Commands stay reviewable and versioned with the repository.</p></div>
        <footer><button type="button" onclick={() => (dialog = null)}>Cancel</button><button class="primary" type="button" onclick={() => { if (selectedProject) void loadProject(selectedProject.id); dialog = null; }}>Refresh commands</button></footer>
      </section>
    {/if}
  </div>
{/if}

{#if trustReview}
  <TrustReviewDialog review={trustReview} busy={trustBusy} onApprove={() => void approveTrust()} onClose={() => (trustReview = null)} />
{/if}

<style>
  .app-shell { display: grid; width: 100%; height: 100%; grid-template-columns: var(--project-rail-width) var(--tree-rail-width) minmax(0, 1fr); background: var(--night); }
  .app-shell.no-project { grid-template-columns: var(--project-rail-width) minmax(0, 1fr); }
  .project-rail, .tree-rail, .main-frame { min-width: 0; min-height: 0; }
  .project-rail, .tree-rail { position: relative; border-right: 1px solid var(--border); }
  .project-rail { display: flex; flex-direction: column; background: #17191c; }

  .brand { position: relative; display: flex; min-height: 46px; align-items: center; gap: 8px; padding: 7px 7px 7px 9px; user-select: none; }
  .brand-mark { display: flex; width: 24px; height: 24px; align-items: flex-end; gap: 3px; padding: 4px; border: 1px solid #454a51; background: #202328; }
  .brand-mark span { width: 3px; background: #9ca3ad; }
  .brand-mark span:nth-child(1) { height: 6px; } .brand-mark span:nth-child(2) { height: 14px; } .brand-mark span:nth-child(3) { height: 10px; }
  .brand-copy { min-width: 0; flex: 1; }
  .brand-copy strong, .brand-copy span { display: block; }
  .brand-copy strong { color: #f3f4f6; font-size: 13px; font-weight: 680; }
  .brand-copy span { margin-top: 1px; color: #777e87; font-size: 8px; }
  .rail-toggle { display: grid; width: 25px; height: 26px; flex: none; place-items: center; border: 1px solid #3b4047; border-radius: 3px; background: #1d2024; color: #a3a9b1; font: 600 13px/1 'JetBrains Mono Variable', monospace; cursor: pointer; }
  .rail-toggle:hover { border-color: #656c75; color: #fff; }

  .rail-label { display: flex; align-items: center; justify-content: space-between; min-height: 26px; border-top: 1px solid var(--border); padding: 4px 8px; color: #a2a8b0; font-size: 8px; font-weight: 680; letter-spacing: 0.04em; text-transform: uppercase; }
  .rail-label small { color: #707780; font-size: 8px; }
  .project-list { min-height: 0; flex: 1; overflow-y: auto; padding: 2px 5px 6px; scrollbar-color: #42474f transparent; scrollbar-width: thin; }
  .project-row { position: relative; display: flex; min-height: 40px; margin: 1px 0; border: 1px solid transparent; border-radius: 3px; }
  .project-row:hover { background: #202328; }
  .project-row.active { border-color: #41464d; background: #25282d; box-shadow: inset 2px 0 #777f89; }
  .project-select { display: flex; min-width: 0; flex: 1; align-items: center; gap: 7px; border: 0; padding: 5px 7px; background: transparent; text-align: left; cursor: pointer; }
  .status-dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: #626972; }
  .status-dot.running { background: var(--signal); } .status-dot.error { background: var(--fault); }
  .project-glyph { display: none; }
  .project-copy { min-width: 0; }
  .project-copy strong, .project-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .project-copy strong { color: #d4d7dc; font-size: 11px; font-weight: 620; }
  .project-copy small { margin-top: 1px; color: #777e87; font-size: 8px; }
  .rename-button { width: 25px; border: 0; background: transparent; color: transparent; cursor: pointer; }
  .project-row:hover .rename-button, .rename-button:focus-visible { color: #89909a; }
  .rename-form { display: flex; width: 100%; align-items: center; gap: 4px; padding: 4px; }
  .rename-form input { min-width: 0; flex: 1; border: 1px solid #4a4f57; padding: 5px; background: #111315; color: var(--text); font-size: 10px; }
  .rename-form button { border: 1px solid #4a4f57; padding: 5px; background: #292d32; color: var(--text); font-size: 8px; }
  .project-empty { margin: 5px; border: 1px dashed #3b4047; padding: 10px; }
  .project-empty strong { color: #d4d7dc; font-size: 11px; } .project-empty p { margin: 3px 0 8px; color: var(--muted); font-size: 9px; }
  .project-empty button { border: 1px solid #4a4f57; border-radius: 3px; padding: 5px 7px; background: #25282d; color: var(--text); font-size: 9px; }
  .project-footer { padding: 6px; border-top: 1px solid var(--border); }
  .register-button { display: flex; width: 100%; min-height: 29px; align-items: center; justify-content: center; gap: 6px; border: 1px solid #42474f; border-radius: 3px; background: #202328; color: #d1d5db; font-size: 9px; font-weight: 620; cursor: pointer; }
  .register-button span:first-child { color: #a0a6ae; font-size: 13px; }
  .register-button:disabled { cursor: not-allowed; opacity: 0.45; }

  .resize-handle { position: absolute; z-index: 8; top: 0; right: -3px; bottom: 0; width: 6px; border: 0; padding: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .resize-handle::after { position: absolute; top: 0; right: 2px; bottom: 0; width: 1px; background: transparent; content: ''; }
  .resize-handle:hover::after, .resize-handle:focus-visible::after { background: #7a818a; }

  .project-rail.collapsed .brand { padding-inline: 6px 4px; }
  .project-rail.collapsed .brand-copy, .project-rail.collapsed .rail-label span, .project-rail.collapsed .project-copy, .project-rail.collapsed .rename-button, .project-rail.collapsed .button-copy, .project-rail.collapsed .project-empty { display: none; }
  .project-rail.collapsed .brand-mark { width: 23px; height: 23px; }
  .project-rail.collapsed .rail-toggle { width: 20px; margin-left: 1px; }
  .project-rail.collapsed .rail-label { justify-content: center; padding-inline: 0; }
  .project-rail.collapsed .project-list { padding-inline: 4px; }
  .project-rail.collapsed .project-row { min-height: 38px; }
  .project-rail.collapsed .project-select { position: relative; justify-content: center; padding: 4px; }
  .project-rail.collapsed .project-glyph { display: grid; width: 25px; height: 25px; place-items: center; border: 1px solid #41464d; border-radius: 3px; color: #c5c9ce; background: #202328; font-size: 10px; font-weight: 680; }
  .project-rail.collapsed .status-dot { position: absolute; z-index: 1; right: 6px; bottom: 6px; border: 1px solid #17191c; }
  .project-rail.collapsed .project-footer { padding: 5px; }

  .main-frame { position: relative; display: grid; grid-template-rows: auto auto minmax(0, 1fr) auto; overflow: hidden; background: var(--night); }
  .main-frame.empty { display: flex; }
  .document-title { display: grid; min-height: 38px; grid-template-columns: minmax(90px, 1fr) auto minmax(90px, 1fr); align-items: center; gap: 8px; border-bottom: 1px solid var(--border); padding: 4px 8px; background: #15171a; }
  .document-title h1 { overflow: hidden; margin: 0; color: #e4e6e9; font-size: 12px; font-weight: 620; text-align: center; text-overflow: ellipsis; white-space: nowrap; }
  .title-side { min-width: 0; color: #747b84; font: 7px 'JetBrains Mono Variable', monospace; text-transform: uppercase; }
  .title-side.right { display: flex; justify-content: flex-end; }
  .title-side button { border: 1px solid #444950; border-radius: 3px; padding: 4px 7px; background: #24272b; color: #c8ccd1; font-size: 9px; cursor: pointer; }
  .error-banner { display: flex; align-items: center; justify-content: space-between; gap: 10px; border: 0; border-bottom: 1px solid rgb(220 107 107 / 38%); padding: 5px 8px; background: rgb(120 44 44 / 18%); color: #efa5a5; font-size: 9px; text-align: left; cursor: pointer; }
  .error-banner strong { font-size: 8px; }
  .item-viewer { min-width: 0; min-height: 0; overflow: hidden; }
  .terminal-view { width: 100%; height: 100%; padding: 5px; }
  .terminal-view > :global(.terminal-frame) { width: 100%; height: 100%; }
  .onboarding { display: grid; width: min(440px, calc(100% - 36px)); place-items: start; align-content: center; margin: auto; }
  .onboarding > span { color: var(--muted); font-size: 9px; text-transform: uppercase; }
  .onboarding h1 { margin: 5px 0 0; color: #f0f1f3; font-size: 28px; }
  .onboarding p { margin: 7px 0 13px; color: #969da6; font-size: 12px; }
  .onboarding button { border: 1px solid #4a4f57; border-radius: 3px; padding: 7px 9px; background: #25282d; color: var(--text); font-size: 10px; cursor: pointer; }

  .dialog-backdrop { position: fixed; z-index: 80; inset: 0; display: grid; place-items: center; padding: 16px; background: rgb(3 4 5 / 74%); }
  .dialog { width: min(500px, 100%); border: 1px solid #4a4f57; border-radius: 4px; background: #1c1f23; color: #e2e4e6; box-shadow: 0 18px 55px rgb(0 0 0 / 42%); }
  .dialog > header { display: flex; align-items: start; justify-content: space-between; border-bottom: 1px solid var(--border); padding: 11px 13px 9px; }
  .dialog > header span, .dialog label > span { color: #9299a2; font: 700 8px 'JetBrains Mono Variable', monospace; text-transform: uppercase; }
  .dialog h2 { margin: 3px 0 0; color: #f0f1f3; font-size: 17px; }
  .dialog > header button { border: 0; background: transparent; color: #a0a6ae; font-size: 18px; cursor: pointer; }
  .dialog > label, .dialog-row { margin: 10px 13px 0; }
  .dialog label { display: grid; gap: 4px; }
  .dialog label small { color: #6f7680; font: inherit; }
  .dialog input, .dialog textarea, .dialog select { width: 100%; border: 1px solid #41464d; border-radius: 3px; outline: 0; padding: 7px 8px; background: #111315; color: var(--text); font-size: 10px; }
  .dialog textarea { resize: vertical; line-height: 1.4; }
  .dialog-row { display: grid; grid-template-columns: 0.45fr 1fr; gap: 8px; }
  .dialog > footer { display: flex; justify-content: flex-end; gap: 6px; margin-top: 12px; border-top: 1px solid var(--border); padding: 8px 13px; }
  .dialog > footer button { min-height: 28px; border: 1px solid #484d54; border-radius: 3px; padding: 0 9px; background: #25282d; color: #c4c8cd; font-size: 9px; cursor: pointer; }
  .dialog > footer .primary { border-color: #666d76; background: #30343a; color: #f0f1f3; font-weight: 650; }
  .dialog button:disabled { cursor: not-allowed; opacity: 0.45; }
  .agent-choices { display: grid; max-height: 280px; overflow-y: auto; padding: 5px; }
  .agent-choices > button { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 2px 8px; border: 0; border-bottom: 1px solid var(--border); padding: 8px; background: transparent; color: #c8ccd1; text-align: left; cursor: pointer; }
  .agent-choices > button:hover { background: #25282d; }
  .agent-choices strong { font-size: 10px; } .agent-choices small { overflow: hidden; color: var(--muted); font: 8px 'JetBrains Mono Variable', monospace; text-overflow: ellipsis; white-space: nowrap; }
  .agent-choices span { grid-row: 1 / 3; grid-column: 2; align-self: center; color: #aeb3ba; font-size: 9px; }
  .agent-choices p, .command-help { margin: 0; padding: 13px; color: #969da6; font-size: 10px; }
  .command-help code { display: block; overflow-x: auto; border: 1px solid #3b4047; padding: 7px; background: #111315; color: #d4d7db; font-size: 9px; }
  .command-help p { margin: 8px 0 0; line-height: 1.5; }

  @media (max-width: 760px) {
    .document-title { grid-template-columns: 50px minmax(0, 1fr) 50px; }
    .project-copy small { display: none; }
  }
</style>
