<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';

  import AgentsPanel from './lib/AgentsPanel.svelte';
  import CoordinationView from './lib/CoordinationView.svelte';
  import EmptyState from './lib/EmptyState.svelte';
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
    if (!event.metaKey || event.altKey || event.shiftKey) return;
    const target = event.target as HTMLElement | null;
    if (
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      target?.isContentEditable
    ) {
      return;
    }
    const shortcut = Number(event.key);
    const section = workspaceSections.find((candidate) => candidate.shortcut === shortcut);
    if (!section || !selectedProject) return;
    event.preventDefault();
    workspaceSection = section.id;
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

<main class="app-shell">
  <aside class="project-rail" aria-label="Projects">
    <header class="brand" data-tauri-drag-region>
      <div class="brand-mark" aria-hidden="true"><span></span><span></span><span></span></div>
      <div>
        <strong>gbuild</strong>
        <span>local workspaces</span>
      </div>
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
        Register project
      </button>
    </footer>
  </aside>

  {#if selectedProject}
    <aside class="section-rail" aria-label={`${projectLabel(selectedProject)} sections`}>
      <header class="project-context" data-tauri-drag-region>
        <span>Current project</span>
        <strong>{projectLabel(selectedProject)}</strong>
        <small title={selectedProject.path}>{selectedProject.path}</small>
      </header>

      <nav class="section-nav" aria-label="Project sections">
        {#each workspaceSections as section}
          <button
            type="button"
            class:active={workspaceSection === section.id}
            aria-current={workspaceSection === section.id ? 'page' : undefined}
            onclick={() => (workspaceSection = section.id)}
          >
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
    </aside>
  {/if}

  <section class="content-shell">
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
    grid-template-columns: 238px 198px minmax(0, 1fr);
    background: var(--night);
  }

  .project-rail,
  .section-rail,
  .content-shell {
    min-width: 0;
    min-height: 0;
  }

  .project-rail {
    display: flex;
    flex-direction: column;
    border-right: 1px solid #243a47;
    background: #0c1b25;
  }

  .brand {
    display: flex;
    min-height: 74px;
    align-items: center;
    gap: 12px;
    padding: 20px 18px 13px;
    user-select: none;
  }

  .brand-mark {
    display: flex;
    width: 27px;
    height: 27px;
    align-items: flex-end;
    gap: 3px;
    padding: 5px;
    border: 1px solid #365363;
    background: #102630;
  }

  .brand-mark span { width: 3px; background: var(--signal); }
  .brand-mark span:nth-child(1) { height: 7px; }
  .brand-mark span:nth-child(2) { height: 15px; }
  .brand-mark span:nth-child(3) { height: 11px; }
  .brand strong, .brand span { display: block; }
  .brand strong { color: #eff5f7; font-size: 15px; font-weight: 690; letter-spacing: 0.08em; }
  .brand > div:last-child > span { margin-top: 2px; color: #647e8b; font: 8px 'JetBrains Mono Variable', monospace; letter-spacing: 0.1em; text-transform: uppercase; }

  .rail-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 16px 9px;
    border-top: 1px solid #203642;
    color: #8197a2;
    font: 700 8px 'JetBrains Mono Variable', monospace;
    letter-spacing: 0.11em;
    text-transform: uppercase;
  }

  .rail-label small { color: #506a77; font-size: 8px; }
  .project-list { min-height: 0; flex: 1; overflow-y: auto; padding: 0 8px 12px; scrollbar-color: #2c4753 transparent; scrollbar-width: thin; }
  .project-row { position: relative; display: flex; min-height: 60px; margin: 2px 0; border: 1px solid transparent; border-radius: 3px; }
  .project-row:hover { background: rgb(112 150 165 / 6%); }
  .project-row.active { border-color: #31535c; background: linear-gradient(105deg, rgb(99 215 197 / 11%), rgb(99 215 197 / 2%)); box-shadow: inset 2px 0 var(--signal); }
  .project-select { display: flex; min-width: 0; flex: 1; align-items: center; gap: 10px; border: 0; padding: 10px; background: transparent; text-align: left; cursor: pointer; }
  .status-dot { width: 7px; height: 7px; flex: none; border-radius: 50%; background: #516a76; }
  .status-dot.running { background: var(--signal); box-shadow: 0 0 0 3px rgb(99 215 197 / 9%); }
  .status-dot.error { background: var(--fault); box-shadow: 0 0 0 3px rgb(239 125 117 / 9%); }
  .project-copy { min-width: 0; }
  .project-copy strong, .project-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .project-copy strong { color: #c3d1d7; font-size: 12px; font-weight: 620; }
  .project-row.active .project-copy strong { color: #eff7f6; }
  .project-copy small { max-width: 150px; margin-top: 4px; color: #58727e; font: 7px 'JetBrains Mono Variable', monospace; }
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

  .project-footer { padding: 13px; border-top: 1px solid #203642; }
  .register-button { display: flex; width: 100%; min-height: 37px; align-items: center; justify-content: center; gap: 8px; border: 1px solid #355360; border-radius: 3px; background: #102630; color: #c0cfd5; font-size: 10px; font-weight: 620; cursor: pointer; }
  .register-button span { color: var(--signal); font: 15px 'JetBrains Mono Variable', monospace; }
  .register-button:hover:not(:disabled) { border-color: var(--signal); }
  .register-button:disabled { cursor: not-allowed; opacity: 0.45; }

  .section-rail {
    display: flex;
    flex-direction: column;
    border-right: 1px solid #223844;
    background: #091720;
  }

  .project-context { min-height: 106px; padding: 28px 16px 16px; border-bottom: 1px solid #203541; }
  .project-context span, .project-context small { display: block; font-family: 'JetBrains Mono Variable', monospace; }
  .project-context span { color: #5e7884; font-size: 7px; font-weight: 700; letter-spacing: 0.12em; text-transform: uppercase; }
  .project-context strong { display: block; overflow: hidden; margin-top: 7px; color: #e0eaed; font-size: 14px; text-overflow: ellipsis; white-space: nowrap; }
  .project-context small { overflow: hidden; margin-top: 6px; color: #4e6874; font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }

  .section-nav { display: grid; align-content: start; gap: 2px; min-height: 0; flex: 1; padding: 12px 8px; }
  .section-nav button { position: relative; display: grid; min-width: 0; grid-template-columns: 28px minmax(0, 1fr) auto; align-items: center; gap: 7px; min-height: 54px; border: 1px solid transparent; border-radius: 3px; padding: 7px 8px; background: transparent; color: #78919c; text-align: left; cursor: pointer; }
  .section-nav button:hover { border-color: #263f4a; background: rgb(85 119 131 / 6%); }
  .section-nav button.active { border-color: #31535c; background: #10252e; color: #dce7ea; box-shadow: inset 2px 0 var(--signal); }
  .shortcut { color: #4f6975; font: 7px 'JetBrains Mono Variable', monospace; }
  .section-nav button.active .shortcut { color: var(--signal); }
  .section-copy { min-width: 0; }
  .section-copy strong, .section-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .section-copy strong { font-size: 11px; font-weight: 650; }
  .section-copy small { margin-top: 3px; color: #506975; font-size: 8px; }
  .section-nav i { color: #58717c; font: normal 7px 'JetBrains Mono Variable', monospace; }

  .daemon-state { display: flex; align-items: center; gap: 9px; min-height: 60px; padding: 12px 15px; border-top: 1px solid #203541; }
  .daemon-state > span { width: 7px; height: 7px; border-radius: 50%; background: #536b76; }
  .daemon-state > span.online { background: var(--signal); box-shadow: 0 0 0 3px rgb(99 215 197 / 8%); }
  .daemon-state strong, .daemon-state small { display: block; font-family: 'JetBrains Mono Variable', monospace; }
  .daemon-state strong { color: #8399a2; font-size: 8px; text-transform: capitalize; }
  .daemon-state small { margin-top: 3px; color: #4d6671; font-size: 7px; }

  .content-shell { position: relative; display: grid; grid-template-rows: auto auto minmax(0, 1fr); overflow: hidden; background: radial-gradient(circle at 80% 6%, rgb(79 125 143 / 9%), transparent 28%), repeating-linear-gradient(0deg, transparent 0 43px, rgb(118 144 160 / 3%) 44px), var(--night); }
  .section-header { display: flex; min-height: 118px; align-items: flex-end; justify-content: space-between; gap: 24px; padding: 28px clamp(24px, 3.5vw, 48px) 19px; border-bottom: 1px solid #243a46; }
  .breadcrumb { color: #5f7884; font: 700 7px 'JetBrains Mono Variable', monospace; letter-spacing: 0.1em; text-transform: uppercase; }
  .section-header h1 { margin: 7px 0 0; color: #edf4f6; font-size: clamp(25px, 3.4vw, 38px); font-weight: 540; letter-spacing: -0.035em; line-height: 1; }
  .section-header p { margin: 8px 0 0; color: #667f8b; font-size: 11px; }
  .primary-action { display: flex; min-height: 39px; align-items: center; gap: 8px; border: 1px solid #4b8179; border-radius: 3px; padding: 0 15px; background: #17362f; color: #def0ec; font-size: 10px; font-weight: 680; cursor: pointer; }
  .primary-action span { color: var(--signal); font: 15px 'JetBrains Mono Variable', monospace; }
  .primary-action:hover:not(:disabled) { border-color: var(--signal); background: #1b4137; }
  .primary-action:disabled { cursor: not-allowed; opacity: 0.45; }

  .error-banner { display: flex; align-items: center; justify-content: space-between; gap: 15px; border: 0; border-bottom: 1px solid rgb(239 125 117 / 28%); padding: 9px clamp(24px, 3.5vw, 48px); background: rgb(130 54 56 / 18%); color: #e7aaa6; font-size: 10px; text-align: left; cursor: pointer; }
  .error-banner strong { flex: none; color: #b98584; font: 700 7px 'JetBrains Mono Variable', monospace; letter-spacing: 0.08em; text-transform: uppercase; }
  .section-stage { position: relative; display: flex; min-width: 0; min-height: 0; overflow: hidden; }
  .scroll-stage { width: 100%; min-width: 0; min-height: 0; overflow: auto; padding: 22px clamp(22px, 3.5vw, 48px) 40px; scrollbar-color: #2b4551 transparent; scrollbar-width: thin; }
  .panel-stage { padding: 0; }
  .terminal-stage { flex-direction: column; }

  .session-bar { display: grid; flex: none; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: 14px; min-height: 49px; padding: 7px clamp(20px, 3vw, 40px); border-bottom: 1px solid #223944; background: rgb(8 20 28 / 68%); }
  .session-bar > span { color: #607b87; font: 700 7px 'JetBrains Mono Variable', monospace; letter-spacing: 0.11em; text-transform: uppercase; }
  .session-bar > div { display: flex; min-width: 0; gap: 5px; overflow-x: auto; scrollbar-width: none; }
  .session-bar button { display: flex; flex: none; align-items: center; gap: 7px; border: 1px solid #29434e; border-radius: 3px; padding: 7px 10px; background: #0c2029; color: #718a94; font: 8px 'JetBrains Mono Variable', monospace; cursor: pointer; }
  .session-bar button:hover { border-color: #42606b; }
  .session-bar button.active { border-color: #4c7f78; background: #14302e; color: #d5e4e3; }
  .session-bar i { width: 6px; height: 6px; border-radius: 50%; background: #536b76; }
  .session-bar i.running { background: var(--signal); }
  .session-bar i.error { background: var(--fault); }
  .terminal-surface { display: flex; min-width: 0; min-height: 0; flex: 1; padding: 14px clamp(16px, 2.4vw, 32px) clamp(16px, 2.4vw, 28px); }
  .terminal-surface > :global(.terminal-frame) { width: 100%; height: 100%; }

  .onboarding { display: grid; width: min(650px, calc(100% - 60px)); place-items: start; align-content: center; margin: auto; }
  .onboarding .eyebrow { color: var(--signal); font: 700 8px 'JetBrains Mono Variable', monospace; letter-spacing: 0.13em; text-transform: uppercase; }
  .onboarding h1 { margin: 13px 0 0; color: #eef4f6; font-size: clamp(42px, 7vw, 78px); font-weight: 480; letter-spacing: -0.055em; line-height: 0.93; }
  .onboarding p { max-width: 470px; margin: 24px 0 23px; color: #6e8793; font-size: 14px; line-height: 1.65; }
  .onboarding button { display: flex; min-height: 42px; align-items: center; gap: 9px; border: 1px solid #4c8179; border-radius: 3px; padding: 0 16px; background: #17372f; color: #e1f1ed; font-size: 11px; font-weight: 660; cursor: pointer; }
  .onboarding button span { color: var(--signal); font: 16px 'JetBrains Mono Variable', monospace; }
  .onboarding button:disabled { cursor: not-allowed; opacity: 0.45; }
  .onboarding small { margin-top: 15px; color: #4d6874; font: 8px 'JetBrains Mono Variable', monospace; text-transform: uppercase; }

  @media (max-width: 900px) {
    .app-shell { grid-template-columns: 190px 172px minmax(0, 1fr); }
    .project-copy small, .section-copy small { display: none; }
    .section-header { min-height: 104px; }
  }

  @media (max-width: 690px) {
    .app-shell { grid-template-columns: 72px 154px minmax(0, 1fr); }
    .brand > div:last-child, .rail-label span, .project-copy, .rename-button, .project-footer .register-button { display: none; }
    .brand { justify-content: center; padding-inline: 0; }
    .rail-label { justify-content: center; }
    .project-row { min-height: 48px; }
    .project-select { justify-content: center; }
    .project-empty { display: none; }
    .section-header { padding-inline: 20px; }
    .section-header p { display: none; }
    .primary-action { padding-inline: 10px; }
  }
</style>
