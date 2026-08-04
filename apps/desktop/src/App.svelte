<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';

  import {
    DaemonClient,
    type ConnectionStatus,
    type ProcessView,
    type Project
  } from './lib/daemon';
  import TerminalView from './lib/TerminalView.svelte';

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
  let selectedProject = $derived(projects.find((project) => project.selected) ?? null);
  let selectedProcess = $derived(
    processes.find((process) => process.id === selectedProcessId) ?? null
  );

  function applyConnectionStatus(status: ConnectionStatus): void {
    const wasConnected = connection.status === 'connected';
    connection = status;
    if (status.status === 'connected' && !wasConnected) void refreshProjects();
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
      loadedProjectId = null;
      return;
    }
    if (loadedProjectId !== projectId) {
      loadedProjectId = projectId;
      processes = [];
      selectedProcessId = null;
      void refreshProcesses(projectId);
    }
  });

  onMount(() => {
    let active = true;
    const statusRefresh = setInterval(() => {
      if (active && connection.status === 'connected' && !busy) {
        void refreshProjects();
        if (selectedProject) void refreshProcesses(selectedProject.id);
      }
    }, 5000);
    void client
      .start(
        (status) => {
          if (!active) return;
          applyConnectionStatus(status);
        },
        (message) => {
          if (active) error = message;
        }
      )
      .then((status) => {
        if (!active) return;
        applyConnectionStatus(status);
      })
      .catch((cause) => {
        if (active) error = String(cause);
      });

    return () => {
      active = false;
      clearInterval(statusRefresh);
      client.close();
    };
  });

  async function withProjects(operation: () => Promise<Project[]>): Promise<void> {
    busy = true;
    error = null;
    try {
      projects = await operation();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
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
      processes = next;
      const selectedStillExists = next.some((process) => process.id === selectedProcessId);
      if (!selectedStillExists) {
        selectedProcessId =
          next.find((process) => process.status === 'running')?.id ?? next[0]?.id ?? null;
      }
    } catch (cause) {
      if (request === processRequest) {
        error = cause instanceof Error ? cause.message : String(cause);
      }
    } finally {
      if (request === processRequest) processBusy = false;
    }
  }

  async function registerProject(): Promise<void> {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Register a project folder'
    });
    if (typeof selected === 'string') {
      await withProjects(() => client.register(selected));
    }
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

  function reportTerminalError(message: string): void {
    error = message;
  }
</script>

<svelte:head>
  <title>{selectedProject ? `${projectLabel(selectedProject)} — gbuild` : 'gbuild'}</title>
</svelte:head>

<main class="shell">
  <aside class="rail" aria-label="Projects">
    <header class="brand" data-tauri-drag-region>
      <div class="brand-mark" aria-hidden="true"><span></span><span></span><span></span></div>
      <div>
        <strong>gbuild</strong>
        <span>agent workbench</span>
      </div>
    </header>

    <div
      class="connection"
      class:is-connected={connection.status === 'connected'}
      title={connection.message ?? undefined}
    >
      <span class="connection-light" aria-hidden="true"></span>
      <span>
        {connection.status === 'connected'
          ? `Daemon · ${connection.port}`
          : connection.status === 'connecting'
            ? 'Connecting to daemon'
            : 'Daemon unavailable'}
      </span>
    </div>

    <section class="project-section">
      <div class="section-heading">
        <h1>Projects</h1>
        <span>{projects.length.toString().padStart(2, '0')}</span>
      </div>

      <div class="project-list" aria-live="polite">
        {#if projects.length === 0 && connection.status === 'connected' && !busy}
          <div class="empty-projects">
            <span class="empty-glyph">↳</span>
            <p>Register a folder to start a workspace.</p>
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
                <button type="submit" aria-label="Save project name">Save</button>
              </form>
            {:else}
              <button
                class="project-select"
                type="button"
                aria-current={project.selected ? 'page' : undefined}
                aria-label={`${projectLabel(project)}, ${project.status}`}
                onclick={() => void selectProject(project)}
              >
                <span class:status-error={project.status === 'error'} class:status-running={project.status === 'running'} class="status-dot"></span>
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
                Aa
              </button>
            {/if}
          </article>
        {/each}
      </div>
    </section>

    <div class="rail-footer">
      <button
        class="register-button"
        type="button"
        disabled={connection.status !== 'connected' || busy}
        onclick={() => void registerProject()}
      >
        <span aria-hidden="true">+</span>
        Register folder
      </button>
      {#if error}
        <button class="error-message" type="button" onclick={() => (error = null)}>
          {error}<span>Dismiss</span>
        </button>
      {/if}
    </div>
  </aside>

  <section class="workspace">
    {#if selectedProject}
      <header class="workspace-heading">
        <div>
          <span class="eyebrow">Active workspace</span>
          <h2>{projectLabel(selectedProject)}</h2>
        </div>
        <span class="workspace-path">{selectedProject.path}</span>
      </header>
      <div class="status-rule" class:running={selectedProject.status === 'running'} class:fault={selectedProject.status === 'error'}>
        <span></span>
        <small>{selectedProject.status}</small>
      </div>
      {#if processes.length > 0}
        <nav class="process-strip" aria-label="Project processes">
          {#each processes as process (process.id)}
            <button
              type="button"
              class:active={process.id === selectedProcessId}
              aria-current={process.id === selectedProcessId ? 'true' : undefined}
              onclick={() => (selectedProcessId = process.id)}
            >
              <span
                class="process-light"
                class:is-running={process.status === 'running'}
                class:is-crashed={process.status === 'crashed'}
              ></span>
              <span><strong>{process.name}</strong><small>{process.kind}</small></span>
            </button>
          {/each}
        </nav>
      {/if}
      <div class="workspace-body">
        {#if selectedProcess}
          {#key selectedProcess.id}
            <TerminalView
              {client}
              process={selectedProcess}
              connected={connection.status === 'connected'}
              onError={reportTerminalError}
            />
          {/key}
        {:else}
          <div class="workspace-empty">
            <div class="terminal-prompt" aria-hidden="true"><span>›</span><i></i></div>
            <h3>{processBusy ? 'Loading processes' : 'No process selected'}</h3>
            <p>{processBusy ? 'Reading the project process registry.' : 'Start or register a process to open its terminal stream.'}</p>
          </div>
        {/if}
      </div>
    {:else}
      <div class="welcome">
        <span class="eyebrow">Local orchestration</span>
        <h2>One daemon.<br />Every workspace.</h2>
        <p>Choose a registered project, or add a folder from the rail.</p>
      </div>
    {/if}
  </section>
</main>
