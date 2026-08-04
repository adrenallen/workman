<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';

  import AgentsPanel from './lib/AgentsPanel.svelte';
  import CoordinationView from './lib/CoordinationView.svelte';
  import EmptyState from './lib/EmptyState.svelte';
  import {
    clampPanelWidth,
    loadPanelPreference,
    savePanelPreference,
    startPanelResize
  } from './lib/panelPreferences';
  import ProcessPanel from './lib/ProcessPanel.svelte';
  import SettingsPanel from './lib/SettingsPanel.svelte';
  import TerminalView from './lib/TerminalView.svelte';
  import TrustReviewDialog from './lib/TrustReview.svelte';
  import {
    DaemonClient,
    type ConnectionStatus,
    type ProcessView,
    type Project,
    type TrustReview
  } from './lib/daemon';
  import {
    workspaceSections,
    type WorkspaceSection,
    type WorkspaceSectionDefinition
  } from './lib/workspace';

  const client = new DaemonClient();
  const projectRailBounds = { min: 176, max: 340 };
  const sectionRailBounds = { min: 160, max: 300 };
  const collapsedProjectRailWidth = 58;
  const collapsedSectionRailWidth = 54;
  let projects = $state<Project[]>([]);
  let processes = $state<ProcessView[]>([]);
  let connection = $state<ConnectionStatus>({
    status: 'connecting',
    message: null,
    port: null
  });
  let busy = $state(false);
  let processBusy = $state(false);
  let processRequest = 0;
  let error = $state<string | null>(null);
  let renameId = $state<number | null>(null);
  let renameValue = $state('');
  let selectedProcessId = $state<number | null>(null);
  let loadedProjectId = $state<number | null>(null);
  let processActionId = $state<number | null>(null);
  let trustReview = $state<TrustReview | null>(null);
  let trustBusy = $state(false);
  let workspaceSection = $state<WorkspaceSection>('terminal');
  let coordinationActionSignal = $state(0);
  let panelRefreshSignal = $state(0);
  let agentSpawnSignal = $state(0);
  let projectRailWidth = $state(238);
  let projectRailCollapsed = $state(false);
  let sectionRailWidth = $state(198);
  let sectionRailCollapsed = $state(false);
  let selectedProject = $derived(projects.find((project) => project.selected) ?? null);
  let selectedProcess = $derived(
    processes.find((process) => process.id === selectedProcessId) ?? null
  );
  let activeSection = $derived(
    workspaceSections.find((section) => section.id === workspaceSection) ?? workspaceSections[0]
  );

  function applyConnectionStatus(status: ConnectionStatus): void {
    const wasConnected = connection.status === 'connected';
    connection = status;
    if (status.status === 'connected' && !wasConnected) {
      void client.subscribeProcessStatuses().catch(reportError);
      void refreshProjects();
    }
  }

  function focusRename(node: HTMLInputElement): void {
    queueMicrotask(() => {
      node.focus();
      node.select();
    });
  }

  $effect(() => {
    const projectId = selectedProject?.id ?? null;
    const connected = connection.status === 'connected';
    if (!connected || projectId === null) {
      processes = [];
      selectedProcessId = null;
      trustReview = null;
      loadedProjectId = null;
      return;
    }
    if (loadedProjectId !== projectId) {
      loadedProjectId = projectId;
      processes = [];
      selectedProcessId = null;
      trustReview = null;
      void loadProcesses(projectId);
    }
  });

  onMount(() => {
    const projectRailPreference = loadPanelPreference(
      'project-rail',
      { collapsed: false, width: projectRailWidth },
      projectRailBounds.min,
      projectRailBounds.max
    );
    projectRailWidth = projectRailPreference.width;
    projectRailCollapsed = projectRailPreference.collapsed;
    const sectionRailPreference = loadPanelPreference(
      'section-rail',
      { collapsed: false, width: sectionRailWidth },
      sectionRailBounds.min,
      sectionRailBounds.max
    );
    sectionRailWidth = sectionRailPreference.width;
    sectionRailCollapsed = sectionRailPreference.collapsed;

    let active = true;
    const stopProcessStatuses = client.onProcessStatuses((next) => {
      if (!active || !selectedProject) return;
      applyProcesses(next.filter((process) => process.project_id === selectedProject?.id));
    });
    const statusRefresh = setInterval(() => {
      if (active && connection.status === 'connected' && !busy) void refreshProjects();
    }, 5000);
    void client
      .start(
        (status) => {
          if (active) applyConnectionStatus(status);
        },
        (message) => {
          if (active) error = message;
        }
      )
      .then((status) => {
        if (active) applyConnectionStatus(status);
      })
      .catch(reportError);

    return () => {
      active = false;
      clearInterval(statusRefresh);
      stopProcessStatuses();
      client.close();
    };
  });

  function handleShortcut(event: KeyboardEvent): void {
    const target = event.target as HTMLElement | null;
    if (
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      target?.isContentEditable
    ) {
      return;
    }
    if (event.metaKey && !event.altKey && event.key.toLowerCase() === 'b') {
      event.preventDefault();
      if (event.shiftKey) {
        if (selectedProject) toggleSectionRail();
      } else {
        toggleProjectRail();
      }
      return;
    }
    if (!event.metaKey || event.altKey || event.shiftKey) return;
    const shortcut = Number(event.key);
    const section = workspaceSections.find((candidate) => candidate.shortcut === shortcut);
    if (!section || !selectedProject) return;
    event.preventDefault();
    workspaceSection = section.id;
  }

  function persistProjectRail(): void {
    savePanelPreference('project-rail', {
      collapsed: projectRailCollapsed,
      width: projectRailWidth
    });
  }

  function persistSectionRail(): void {
    savePanelPreference('section-rail', {
      collapsed: sectionRailCollapsed,
      width: sectionRailWidth
    });
  }

  function toggleProjectRail(): void {
    projectRailCollapsed = !projectRailCollapsed;
    persistProjectRail();
  }

  function toggleSectionRail(): void {
    sectionRailCollapsed = !sectionRailCollapsed;
    persistSectionRail();
  }

  function resizeRailFromKeyboard(
    event: KeyboardEvent,
    rail: 'project' | 'section'
  ): void {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    const delta = event.key === 'ArrowLeft' ? -12 : 12;
    if (rail === 'project') {
      projectRailWidth = clampPanelWidth(
        projectRailWidth + delta,
        projectRailBounds.min,
        projectRailBounds.max
      );
      persistProjectRail();
    } else {
      sectionRailWidth = clampPanelWidth(
        sectionRailWidth + delta,
        sectionRailBounds.min,
        sectionRailBounds.max
      );
      persistSectionRail();
    }
  }

  async function withProjects(operation: () => Promise<Project[]>): Promise<void> {
    busy = true;
    error = null;
    try {
      projects = await operation();
    } catch (cause) {
      reportError(cause);
    } finally {
      busy = false;
    }
  }

  async function refreshProjects(): Promise<void> {
    await withProjects(() => client.projects());
  }

  async function refreshProcesses(projectId: number): Promise<void> {
    const request = ++processRequest;
    processBusy = true;
    try {
      const next = await client.processes(projectId);
      if (request !== processRequest || selectedProject?.id !== projectId) return;
      applyProcesses(next);
    } catch (cause) {
      if (request === processRequest) reportError(cause);
    } finally {
      if (request === processRequest) processBusy = false;
    }
  }

  async function loadProcesses(projectId: number): Promise<void> {
    try {
      await client.syncConfig(projectId);
    } catch (cause) {
      reportError(cause);
    }
    await refreshProcesses(projectId);
  }

  function applyProcesses(next: ProcessView[]): void {
    processes = next;
    if (!next.some((process) => process.id === selectedProcessId)) {
      selectedProcessId =
        next.find((process) => process.status === 'running')?.id ?? next[0]?.id ?? null;
    }
  }

  async function processAction(
    process: ProcessView,
    operation: (processId: number) => Promise<ProcessView>
  ): Promise<void> {
    processActionId = process.id;
    error = null;
    selectedProcessId = process.id;
    try {
      await operation(process.id);
      await refreshProcesses(process.project_id);
    } catch (cause) {
      reportError(cause);
    } finally {
      processActionId = null;
    }
  }

  async function openTrustReview(process: ProcessView): Promise<void> {
    processActionId = process.id;
    error = null;
    try {
      trustReview = await client.trustReview(process.id);
    } catch (cause) {
      reportError(cause);
    } finally {
      processActionId = null;
    }
  }

  async function approveTrust(): Promise<void> {
    if (!trustReview || !selectedProject) return;
    const review = trustReview;
    trustBusy = true;
    error = null;
    try {
      await client.trustYmlProcess(review.process_id, review.expected_hash);
      trustReview = null;
      selectedProcessId = review.process_id;
      await refreshProcesses(selectedProject.id);
    } catch (cause) {
      reportError(cause);
      try {
        trustReview = await client.trustReview(review.process_id);
      } catch {
        trustReview = null;
      }
    } finally {
      trustBusy = false;
    }
  }

  async function spawnTerminal(): Promise<void> {
    if (!selectedProject || processActionId !== null) return;
    processActionId = -1;
    error = null;
    try {
      const process = await client.spawnTerminal(selectedProject.id);
      selectedProcessId = process.id;
      await refreshProcesses(selectedProject.id);
      workspaceSection = 'terminal';
    } catch (cause) {
      reportError(cause);
    } finally {
      processActionId = null;
    }
  }

  async function registerProject(): Promise<void> {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Register a project folder'
    });
    if (typeof selected === 'string') await withProjects(() => client.register(selected));
  }

  async function selectProject(project: Project): Promise<void> {
    if (project.selected || busy) return;
    await withProjects(() => client.select(project.id));
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
    await withProjects(() => client.rename(projectId, name));
  }

  function projectLabel(project: Project): string {
    return project.display_name ?? project.name;
  }

  function reportError(cause: unknown): void {
    error = cause instanceof Error ? cause.message : String(cause);
  }

  function sectionCount(section: WorkspaceSectionDefinition): string | null {
    if (section.id === 'terminal' || section.id === 'processes') return String(processes.length);
    if (section.id === 'agents') {
      return String(processes.filter((process) => process.kind === 'agent').length);
    }
    return null;
  }

  function primaryActionLabel(section: WorkspaceSection): string {
    switch (section) {
      case 'terminal':
      case 'processes':
        return 'New terminal';
      case 'todos':
        return 'New todo';
      case 'scratchpads':
        return 'Refresh notes';
      case 'agents':
        return 'Spawn agent';
      case 'settings':
        return 'Refresh';
    }
  }

  function runPrimaryAction(): void {
    switch (workspaceSection) {
      case 'terminal':
      case 'processes':
        void spawnTerminal();
        break;
      case 'todos':
        coordinationActionSignal += 1;
        break;
      case 'scratchpads':
      case 'settings':
        panelRefreshSignal += 1;
        break;
      case 'agents':
        agentSpawnSignal += 1;
        break;
    }
  }
</script>

<svelte:window onkeydown={handleShortcut} />

<svelte:head>
  <title>{selectedProject ? `${projectLabel(selectedProject)} · ${activeSection.label}` : 'gbuild'}</title>
</svelte:head>

<main
  class="app-shell"
  class:no-project={selectedProject === null}
  style={`--project-rail-width: ${projectRailCollapsed ? collapsedProjectRailWidth : projectRailWidth}px; --section-rail-width: ${sectionRailCollapsed ? collapsedSectionRailWidth : sectionRailWidth}px;`}
>
  <aside class="project-rail" class:collapsed={projectRailCollapsed} aria-label="Projects">
    <header class="brand" data-tauri-drag-region>
      <div class="brand-mark" aria-hidden="true"><span></span><span></span><span></span></div>
      <div class="brand-copy">
        <strong>gbuild</strong>
        <span>local workspaces</span>
      </div>
      <button
        class="rail-toggle"
        type="button"
        aria-label={`${projectRailCollapsed ? 'Expand' : 'Collapse'} project rail`}
        title={`${projectRailCollapsed ? 'Expand' : 'Collapse'} project rail (⌘B)`}
        onclick={toggleProjectRail}
      >{projectRailCollapsed ? '›' : '‹'}</button>
    </header>

    <div class="rail-label">
      <span>Projects</span>
      <small>{projects.length.toString().padStart(2, '0')}</small>
    </div>

    <div class="project-list" aria-live="polite">
      {#if projects.length === 0 && connection.status === 'connected' && !busy}
        <div class="project-empty">
          <span aria-hidden="true">↘</span>
          <strong>No projects yet</strong>
          <p>Register a folder to give its terminals, agents, and notes a home.</p>
          <button type="button" onclick={() => void registerProject()}>Register folder</button>
        </div>
      {/if}

      {#each projects as project (project.id)}
        <article class:active={project.selected} class="project-row">
          {#if renameId === project.id}
            <form
              class="rename-form"
              onsubmit={(event) => {
                event.preventDefault();
                void commitRename();
              }}
            >
              <input
                aria-label="Project name"
                bind:value={renameValue}
                use:focusRename
                onkeydown={(event) => {
                  if (event.key === 'Escape') cancelRename();
                }}
              />
              <button type="submit">Save</button>
            </form>
          {:else}
            <button
              class="project-select"
              type="button"
              aria-current={project.selected ? 'page' : undefined}
              aria-label={`${projectLabel(project)}, ${project.status}`}
              onclick={() => void selectProject(project)}
            >
              <span
                class="status-dot"
                class:error={project.status === 'error'}
                class:running={project.status === 'running'}
                aria-hidden="true"
              ></span>
              <span class="project-glyph" aria-hidden="true">
                {projectLabel(project).slice(0, 1).toUpperCase()}
              </span>
              <span class="project-copy">
                <strong>{projectLabel(project)}</strong>
                <small>{project.path}</small>
              </span>
            </button>
            <button
              class="rename-button"
              type="button"
              aria-label={`Rename ${projectLabel(project)}`}
              title="Rename project"
              onclick={() => beginRename(project)}
            >
              ···
            </button>
          {/if}
        </article>
      {/each}
    </div>

    <footer class="project-footer">
      <button
        class="register-button"
        type="button"
        disabled={connection.status !== 'connected' || busy}
        onclick={() => void registerProject()}
      >
        <span aria-hidden="true">+</span>
        <span class="button-copy">Register project</span>
      </button>
    </footer>
    {#if !projectRailCollapsed}
      <button
        type="button"
        class="resize-handle"
        aria-label="Resize project rail"
        title={`Resize project rail · ${projectRailWidth}px · arrow keys`}
        onkeydown={(event) => resizeRailFromKeyboard(event, 'project')}
        onpointerdown={(event) =>
          startPanelResize(event, {
            current: projectRailWidth,
            min: projectRailBounds.min,
            max: projectRailBounds.max,
            onResize: (width) => (projectRailWidth = width),
            onEnd: persistProjectRail
          })}
      ></button>
    {/if}
  </aside>

  {#if selectedProject}
    <aside
      class="section-rail"
      class:collapsed={sectionRailCollapsed}
      aria-label={`${projectLabel(selectedProject)} sections`}
    >
      <header class="project-context" data-tauri-drag-region>
        <div class="project-context-copy">
          <span>Current project</span>
          <strong>{projectLabel(selectedProject)}</strong>
          <small title={selectedProject.path}>{selectedProject.path}</small>
        </div>
        <button
          class="rail-toggle"
          type="button"
          aria-label={`${sectionRailCollapsed ? 'Expand' : 'Collapse'} section rail`}
          title={`${sectionRailCollapsed ? 'Expand' : 'Collapse'} section rail (⌘⇧B)`}
          onclick={toggleSectionRail}
        >{sectionRailCollapsed ? '›' : '‹'}</button>
      </header>

      <nav class="section-nav" aria-label="Project sections">
        {#each workspaceSections as section}
          <button
            type="button"
            class:active={workspaceSection === section.id}
            aria-current={workspaceSection === section.id ? 'page' : undefined}
            title={`${section.label} · ⌘${section.shortcut}`}
            onclick={() => (workspaceSection = section.id)}
          >
            <span class="section-icon" aria-hidden="true">{section.icon}</span>
            <span class="shortcut">⌘{section.shortcut}</span>
            <span class="section-copy">
              <strong>{section.label}</strong>
              <small>{section.description}</small>
            </span>
            {#if sectionCount(section) !== null}<i>{sectionCount(section)}</i>{/if}
          </button>
        {/each}
      </nav>

      <footer class="daemon-state" title={connection.message ?? undefined}>
        <span class:online={connection.status === 'connected'} aria-hidden="true"></span>
        <div>
          <strong>{connection.status === 'connected' ? 'Daemon online' : connection.status}</strong>
          <small>{connection.port ? `127.0.0.1:${connection.port}` : 'Local control service'}</small>
        </div>
      </footer>
      {#if !sectionRailCollapsed}
        <button
          type="button"
          class="resize-handle"
          aria-label="Resize section rail"
          title={`Resize section rail · ${sectionRailWidth}px · arrow keys`}
          onkeydown={(event) => resizeRailFromKeyboard(event, 'section')}
          onpointerdown={(event) =>
            startPanelResize(event, {
              current: sectionRailWidth,
              min: sectionRailBounds.min,
              max: sectionRailBounds.max,
              onResize: (width) => (sectionRailWidth = width),
              onEnd: persistSectionRail
            })}
        ></button>
      {/if}
    </aside>
  {/if}

  <section class="content-shell" class:empty={selectedProject === null}>
    {#if selectedProject}
      <header class="section-header" data-tauri-drag-region>
        <div>
          <span class="breadcrumb">{projectLabel(selectedProject)} / {activeSection.label}</span>
          <h1>{activeSection.label}</h1>
          <p>{activeSection.description}</p>
        </div>
        <button
          class="primary-action"
          type="button"
          disabled={connection.status !== 'connected'}
          onclick={runPrimaryAction}
        >
          <span aria-hidden="true">+</span>
          {primaryActionLabel(workspaceSection)}
        </button>
      </header>

      {#if error}
        <button class="error-banner" type="button" onclick={() => (error = null)}>
          <span>{error}</span><strong>Dismiss</strong>
        </button>
      {/if}

      <div class="section-stage" class:terminal-stage={workspaceSection === 'terminal'}>
        {#if workspaceSection === 'terminal'}
          {#if processes.length > 0}
            <nav class="session-bar" aria-label="Terminal sessions">
              <span>Session</span>
              <div>
                {#each processes as process (process.id)}
                  <button
                    type="button"
                    class:active={process.id === selectedProcessId}
                    onclick={() => (selectedProcessId = process.id)}
                  >
                    <i
                      class:running={process.status === 'running'}
                      class:error={process.status === 'crashed'}
                      aria-hidden="true"
                    ></i>
                    {process.name}
                  </button>
                {/each}
              </div>
            </nav>
          {/if}
          <div class="terminal-surface">
            {#if selectedProcess}
              {#key selectedProcess.id}
                <TerminalView
                  {client}
                  process={selectedProcess}
                  connected={connection.status === 'connected'}
                  onError={reportError}
                />
              {/key}
            {:else}
              <EmptyState
                eyebrow={processBusy ? 'Reading sessions' : 'Start here'}
                title={processBusy ? 'Loading project sessions' : 'Open a terminal for this project'}
                body="A terminal is the quickest way to explore the repository, run a command, or hand work to an agent."
                actionLabel="New terminal"
                icon="›_"
                disabled={connection.status !== 'connected' || processActionId !== null}
                onAction={() => void spawnTerminal()}
              />
            {/if}
          </div>
        {:else if workspaceSection === 'processes'}
          <div class="scroll-stage">
            <ProcessPanel
              {processes}
              selectedId={selectedProcessId}
              busyId={processActionId}
              connected={connection.status === 'connected'}
              onSelect={(processId) => {
                selectedProcessId = processId;
                workspaceSection = 'terminal';
              }}
              onStart={(process) => void processAction(process, (id) => client.startProcess(id))}
              onStop={(process) => void processAction(process, (id) => client.stopProcess(id))}
              onRestart={(process) => void processAction(process, (id) => client.restartProcess(id))}
              onTrust={(process) => void openTrustReview(process)}
              onSpawnTerminal={() => void spawnTerminal()}
            />
          </div>
        {:else if workspaceSection === 'todos' || workspaceSection === 'scratchpads'}
          <div class="scroll-stage">
            <CoordinationView
              {client}
              projectId={selectedProject.id}
              connected={connection.status === 'connected'}
              view={workspaceSection}
              actionSignal={workspaceSection === 'todos'
                ? coordinationActionSignal
                : panelRefreshSignal}
              onError={reportError}
            />
          </div>
        {:else if workspaceSection === 'agents'}
          <div class="scroll-stage panel-stage">
            <AgentsPanel
              {client}
              project={selectedProject}
              {processes}
              {selectedProcessId}
              spawnSignal={agentSpawnSignal}
              connected={connection.status === 'connected'}
              onSelectProcess={(processId) => (selectedProcessId = processId)}
              onError={reportError}
            />
          </div>
        {:else}
          {#key panelRefreshSignal}
            <div class="scroll-stage panel-stage">
              <SettingsPanel
                {client}
                project={selectedProject}
                {connection}
                onError={reportError}
              />
            </div>
          {/key}
        {/if}
      </div>
    {:else}
      <div class="onboarding">
        <span class="eyebrow">Local orchestration</span>
        <h1>Give your work<br />a control room.</h1>
        <p>
          Register a repository to organize its terminals, processes, todos, scratchpads, and agents
          in one place.
        </p>
        <button
          type="button"
          disabled={connection.status !== 'connected' || busy}
          onclick={() => void registerProject()}
        >
          <span aria-hidden="true">+</span> Register your first project
        </button>
        <small>
          {connection.status === 'connected'
            ? `Daemon ready on ${connection.port}`
            : connection.status === 'connecting'
              ? 'Connecting to the local daemon…'
              : 'The local daemon is unavailable'}
        </small>
      </div>
    {/if}
  </section>
</main>

{#if trustReview}
  <TrustReviewDialog
    review={trustReview}
    busy={trustBusy}
    onApprove={() => void approveTrust()}
    onClose={() => (trustReview = null)}
  />
{/if}

<style>
  .app-shell {
    display: grid;
    width: 100%;
    height: 100%;
    grid-template-columns: var(--project-rail-width) var(--section-rail-width) minmax(0, 1fr);
    background: var(--night);
  }

  .app-shell.no-project {
    grid-template-columns: var(--project-rail-width) minmax(0, 1fr);
  }

  .project-rail,
  .section-rail,
  .content-shell {
    min-width: 0;
    min-height: 0;
  }

  .project-rail {
    position: relative;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    background: #17191c;
  }

  .brand {
    position: relative;
    display: flex;
    min-height: 52px;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    user-select: none;
  }

  .brand-mark {
    display: flex;
    width: 25px;
    height: 25px;
    align-items: flex-end;
    gap: 3px;
    padding: 5px;
    border: 1px solid #454a51;
    background: #202328;
  }

  .brand-mark span { width: 3px; background: #9ca3ad; }
  .brand-mark span:nth-child(1) { height: 7px; }
  .brand-mark span:nth-child(2) { height: 15px; }
  .brand-mark span:nth-child(3) { height: 11px; }
  .brand strong, .brand span { display: block; }
  .brand strong { color: #f3f4f6; font-size: 14px; font-weight: 680; }
  .brand-copy > span { margin-top: 1px; color: #777e87; font-size: 9px; }

  .rail-toggle {
    display: grid;
    width: 24px;
    height: 24px;
    flex: none;
    place-items: center;
    margin-left: auto;
    border: 1px solid #3b4047;
    border-radius: 3px;
    background: #1d2024;
    color: #a3a9b1;
    font: 600 14px/1 'JetBrains Mono Variable', monospace;
    cursor: pointer;
  }
  .rail-toggle:hover { border-color: #656c75; background: #292d32; color: #fff; }

  .resize-handle {
    position: absolute;
    z-index: 8;
    top: 0;
    right: -3px;
    bottom: 0;
    width: 6px;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: col-resize;
    touch-action: none;
  }
  .resize-handle::after {
    position: absolute;
    top: 0;
    right: 2px;
    bottom: 0;
    width: 1px;
    background: transparent;
    content: '';
  }
  .resize-handle:hover::after,
  .resize-handle:focus-visible::after { background: #7a818a; }

  .rail-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 9px 12px 6px;
    border-top: 1px solid var(--border);
    color: #a2a8b0;
    font-size: 9px;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .rail-label small { color: #707780; font-size: 9px; }
  .project-list { min-height: 0; flex: 1; overflow-y: auto; padding: 0 6px 8px; scrollbar-color: #42474f transparent; scrollbar-width: thin; }
  .project-row { position: relative; display: flex; min-height: 44px; margin: 1px 0; border: 1px solid transparent; border-radius: 3px; }
  .project-row:hover { background: #202328; }
  .project-row.active { border-color: #41464d; background: #25282d; box-shadow: inset 2px 0 #777f89; }
  .project-select { display: flex; min-width: 0; flex: 1; align-items: center; gap: 9px; border: 0; padding: 7px 8px; background: transparent; text-align: left; cursor: pointer; }
  .status-dot { width: 7px; height: 7px; flex: none; border-radius: 50%; background: #516a76; }
  .status-dot.running { background: var(--signal); }
  .status-dot.error { background: var(--fault); }
  .project-copy { min-width: 0; }
  .project-glyph { display: none; }
  .project-copy strong, .project-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .project-copy strong { color: #d4d7dc; font-size: 12px; font-weight: 620; }
  .project-row.active .project-copy strong { color: #fff; }
  .project-copy small { max-width: 165px; margin-top: 2px; color: #777e87; font-size: 9px; }
  .rename-button { width: 28px; border: 0; background: transparent; color: transparent; cursor: pointer; }
  .project-row:hover .rename-button, .rename-button:focus-visible { color: #77909b; }
  .rename-form { display: flex; width: 100%; align-items: center; gap: 5px; padding: 8px; }
  .rename-form input { min-width: 0; flex: 1; border: 1px solid #3b5a67; padding: 7px; background: #071820; color: #d9e3e7; font-size: 11px; }
  .rename-form button { border: 0; padding: 8px; background: var(--signal); color: #071820; font: 700 8px 'JetBrains Mono Variable', monospace; text-transform: uppercase; }

  .project-empty { display: grid; justify-items: start; margin: 5px 4px; border: 1px dashed #2b4652; padding: 17px; }
  .project-empty > span { color: var(--signal); font: 17px 'JetBrains Mono Variable', monospace; }
  .project-empty strong { margin-top: 12px; color: #bdcbd1; font-size: 12px; }
  .project-empty p { margin: 6px 0 13px; color: #657e89; font-size: 10px; line-height: 1.5; }
  .project-empty button { border: 1px solid #3f6e68; border-radius: 2px; padding: 7px 9px; background: #12302b; color: #bfe2dc; font-size: 9px; cursor: pointer; }

  .project-footer { padding: 8px; border-top: 1px solid var(--border); }
  .register-button { display: flex; width: 100%; min-height: 32px; align-items: center; justify-content: center; gap: 7px; border: 1px solid #42474f; border-radius: 3px; background: #202328; color: #d1d5db; font-size: 10px; font-weight: 620; cursor: pointer; }
  .register-button span { color: #9da4ad; font: 14px 'JetBrains Mono Variable', monospace; }
  .register-button:hover:not(:disabled) { border-color: #6b727c; background: #292d32; }
  .register-button:disabled { cursor: not-allowed; opacity: 0.45; }

  .section-rail {
    position: relative;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    background: #141619;
  }

  .project-context { display: flex; min-height: 68px; align-items: center; gap: 8px; padding: 10px 9px 9px 12px; border-bottom: 1px solid var(--border); }
  .project-context-copy { min-width: 0; flex: 1; }
  .project-context span, .project-context small { display: block; font-family: 'JetBrains Mono Variable', monospace; }
  .project-context span { color: #777e87; font-size: 8px; font-weight: 650; letter-spacing: 0.05em; text-transform: uppercase; }
  .project-context strong { display: block; overflow: hidden; margin-top: 4px; color: #eceef1; font-size: 13px; text-overflow: ellipsis; white-space: nowrap; }
  .project-context small { overflow: hidden; margin-top: 3px; color: #747b84; font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }

  .section-nav { display: grid; align-content: start; gap: 1px; min-height: 0; flex: 1; padding: 7px 6px; }
  .section-nav button { position: relative; display: grid; min-width: 0; grid-template-columns: 24px minmax(0, 1fr) auto; align-items: center; gap: 6px; min-height: 42px; border: 1px solid transparent; border-radius: 3px; padding: 5px 7px; background: transparent; color: #a0a6ae; text-align: left; cursor: pointer; }
  .section-nav button:hover { border-color: #34383e; background: #1e2125; }
  .section-nav button.active { border-color: #42474f; background: #25282d; color: #f0f1f3; box-shadow: inset 2px 0 #777f89; }
  .shortcut { color: #707780; font: 8px 'JetBrains Mono Variable', monospace; }
  .section-icon { display: none; }
  .section-nav button.active .shortcut { color: #b9bec5; }
  .section-copy { min-width: 0; }
  .section-copy strong, .section-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .section-copy strong { font-size: 11px; font-weight: 650; }
  .section-copy small { margin-top: 1px; color: #777e87; font-size: 9px; }
  .section-nav i { color: #767d86; font: normal 8px 'JetBrains Mono Variable', monospace; }

  .daemon-state { display: flex; align-items: center; gap: 8px; min-height: 45px; padding: 8px 11px; border-top: 1px solid var(--border); }
  .daemon-state > span { width: 7px; height: 7px; border-radius: 50%; background: #536b76; }
  .daemon-state > span.online { background: var(--signal); }
  .daemon-state strong, .daemon-state small { display: block; font-family: 'JetBrains Mono Variable', monospace; }
  .daemon-state strong { color: #a0a6ae; font-size: 8px; text-transform: capitalize; }
  .daemon-state small { margin-top: 2px; color: #737a83; font-size: 8px; }

  .content-shell { position: relative; display: grid; grid-template-rows: auto auto minmax(0, 1fr); overflow: hidden; background: var(--night); }
  .content-shell.empty { display: flex; }
  .section-header { display: flex; min-height: 68px; align-items: center; justify-content: space-between; gap: 16px; padding: 10px 18px; border-bottom: 1px solid var(--border); }
  .breadcrumb { color: #777e87; font: 650 8px 'JetBrains Mono Variable', monospace; letter-spacing: 0.04em; text-transform: uppercase; }
  .section-header h1 { margin: 3px 0 0; color: #f3f4f6; font-size: 23px; font-weight: 650; letter-spacing: -0.02em; line-height: 1.05; }
  .section-header p { margin: 3px 0 0; color: #8d949d; font-size: 11px; }
  .primary-action { display: flex; min-height: 32px; align-items: center; gap: 6px; border: 1px solid #4a4f57; border-radius: 3px; padding: 0 11px; background: #25282d; color: #e3e5e8; font-size: 10px; font-weight: 650; cursor: pointer; }
  .primary-action span { color: #a7adb5; font: 14px 'JetBrains Mono Variable', monospace; }
  .primary-action:hover:not(:disabled) { border-color: #707780; background: #2b2f34; }
  .primary-action:disabled { cursor: not-allowed; opacity: 0.45; }

  .error-banner { display: flex; align-items: center; justify-content: space-between; gap: 12px; border: 0; border-bottom: 1px solid rgb(220 107 107 / 38%); padding: 7px 18px; background: rgb(120 44 44 / 18%); color: #efa5a5; font-size: 10px; text-align: left; cursor: pointer; }
  .error-banner strong { flex: none; color: #b98584; font: 700 7px 'JetBrains Mono Variable', monospace; letter-spacing: 0.08em; text-transform: uppercase; }
  .section-stage { position: relative; display: flex; min-width: 0; min-height: 0; overflow: hidden; }
  .scroll-stage { width: 100%; min-width: 0; min-height: 0; overflow: auto; padding: 12px 16px 24px; scrollbar-color: #41464d transparent; scrollbar-width: thin; }
  .panel-stage { padding: 0; }
  .terminal-stage { flex-direction: column; }

  .session-bar { display: grid; flex: none; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: 10px; min-height: 40px; padding: 5px 14px; border-bottom: 1px solid var(--border); background: #15171a; }
  .session-bar > span { color: #607b87; font: 700 7px 'JetBrains Mono Variable', monospace; letter-spacing: 0.11em; text-transform: uppercase; }
  .session-bar > div { display: flex; min-width: 0; gap: 5px; overflow-x: auto; scrollbar-width: none; }
  .session-bar button { display: flex; flex: none; align-items: center; gap: 6px; border: 1px solid #34383e; border-radius: 3px; padding: 5px 8px; background: #1d2024; color: #9aa0a8; font: 8px 'JetBrains Mono Variable', monospace; cursor: pointer; }
  .session-bar button:hover { border-color: #555b64; }
  .session-bar button.active { border-color: #616872; background: #292d32; color: #f0f1f3; }
  .session-bar i { width: 6px; height: 6px; border-radius: 50%; background: #536b76; }
  .session-bar i.running { background: var(--signal); }
  .session-bar i.error { background: var(--fault); }
  .terminal-surface { display: flex; min-width: 0; min-height: 0; flex: 1; padding: 8px 10px 10px; }
  .terminal-surface > :global(.terminal-frame) { width: 100%; height: 100%; }

  .onboarding { display: grid; width: min(650px, calc(100% - 60px)); place-items: start; align-content: center; margin: auto; }
  .onboarding .eyebrow { color: var(--signal); font: 700 8px 'JetBrains Mono Variable', monospace; letter-spacing: 0.13em; text-transform: uppercase; }
  .onboarding h1 { margin: 9px 0 0; color: #f1f2f4; font-size: clamp(34px, 6vw, 58px); font-weight: 590; letter-spacing: -0.04em; line-height: 0.98; }
  .onboarding p { max-width: 470px; margin: 17px 0; color: #9299a2; font-size: 13px; line-height: 1.55; }
  .onboarding button { display: flex; min-height: 35px; align-items: center; gap: 7px; border: 1px solid #4a4f57; border-radius: 3px; padding: 0 12px; background: #25282d; color: #e5e7eb; font-size: 11px; font-weight: 650; cursor: pointer; }
  .onboarding button span { color: var(--signal); font: 16px 'JetBrains Mono Variable', monospace; }
  .onboarding button:disabled { cursor: not-allowed; opacity: 0.45; }
  .onboarding small { margin-top: 15px; color: #4d6874; font: 8px 'JetBrains Mono Variable', monospace; text-transform: uppercase; }

  .project-rail.collapsed .brand {
    justify-content: flex-start;
    padding-inline: 6px 4px;
  }
  .project-rail.collapsed .brand-mark { width: 24px; height: 24px; }
  .project-rail.collapsed .brand-copy,
  .project-rail.collapsed .rail-label span,
  .project-rail.collapsed .project-copy,
  .project-rail.collapsed .rename-button,
  .project-rail.collapsed .button-copy,
  .project-rail.collapsed .project-empty { display: none; }
  .project-rail.collapsed .rail-toggle { width: 20px; height: 24px; margin-left: 2px; }
  .project-rail.collapsed .rail-label { justify-content: center; padding-inline: 0; }
  .project-rail.collapsed .project-list { padding-inline: 5px; }
  .project-rail.collapsed .project-row { min-height: 42px; }
  .project-rail.collapsed .project-select { position: relative; justify-content: center; padding: 5px; }
  .project-rail.collapsed .project-glyph { display: grid; width: 26px; height: 26px; place-items: center; border: 1px solid #41464d; border-radius: 3px; color: #c5c9ce; background: #202328; font-size: 11px; font-weight: 680; }
  .project-rail.collapsed .status-dot { position: absolute; z-index: 1; right: 7px; bottom: 7px; width: 6px; height: 6px; border: 1px solid #17191c; }
  .project-rail.collapsed .project-footer { padding: 6px; }
  .project-rail.collapsed .register-button { min-height: 30px; }

  .section-rail.collapsed .project-context { justify-content: center; padding-inline: 0; }
  .section-rail.collapsed .project-context-copy { display: none; }
  .section-rail.collapsed .rail-toggle { margin: 0; }
  .section-rail.collapsed .section-nav { padding-inline: 5px; }
  .section-rail.collapsed .section-nav button { display: grid; min-height: 41px; grid-template-columns: 1fr; justify-items: center; padding: 5px; }
  .section-rail.collapsed .section-icon { display: block; color: #a4aab2; font: 600 11px 'JetBrains Mono Variable', monospace; }
  .section-rail.collapsed .shortcut,
  .section-rail.collapsed .section-copy,
  .section-rail.collapsed .section-nav i,
  .section-rail.collapsed .daemon-state div { display: none; }
  .section-rail.collapsed .section-nav button.active .section-icon { color: #f0f1f3; }
  .section-rail.collapsed .daemon-state { justify-content: center; padding-inline: 0; }

  @media (max-width: 900px) {
    .project-copy small, .section-copy small { display: none; }
  }

  @media (max-width: 690px) {
    .section-header { padding-inline: 12px; }
    .section-header p { display: none; }
    .primary-action { padding-inline: 10px; }
  }
</style>
