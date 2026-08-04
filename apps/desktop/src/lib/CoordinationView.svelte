<script lang="ts">
  import ScratchpadPanel from './ScratchpadPanel.svelte';
  import TodoBoard from './TodoBoard.svelte';
  import type {
    CoordinationClient,
    CoordinationSnapshot,
    ScratchpadRead,
    TodoDetail,
    TodoPriority,
    TodoSummary
  } from './coordination';

  type CoordinationViewMode = 'todos' | 'scratchpads';

  interface Props {
    client: CoordinationClient;
    projectId: number;
    connected: boolean;
    view: CoordinationViewMode;
    actionSignal: number;
    onError: (message: string) => void;
  }

  let { client, projectId, connected, view, actionSignal, onError }: Props = $props();
  let snapshot = $state<CoordinationSnapshot | null>(null);
  let selectedTodoId = $state<number | null>(null);
  let todoDetail = $state<TodoDetail | null>(null);
  let todoLoading = $state(false);
  let selectedScratchpadId = $state<number | null>(null);
  let scratchpadRead = $state<ScratchpadRead | null>(null);
  let scratchpadLoading = $state(false);
  let refreshing = $state(false);
  let mutating = $state(false);
  let createOpen = $state(false);
  let createTitle = $state('');
  let createBody = $state('');
  let createPriority = $state<TodoPriority>('medium');
  let createTags = $state('');
  let sequence = 0;
  let observedActionSignal = $state(0);
  let actionSignalReady = false;

  $effect(() => {
    const activeProject = projectId;
    const isConnected = connected;
    let disposed = false;
    sequence += 1;
    snapshot = null;
    selectedTodoId = null;
    todoDetail = null;
    selectedScratchpadId = null;
    scratchpadRead = null;

    if (!isConnected) return;
    void refresh(activeProject, () => disposed);
    const poll = setInterval(() => void refresh(activeProject, () => disposed), 2000);
    return () => {
      disposed = true;
      clearInterval(poll);
    };
  });

  $effect(() => {
    if (!actionSignalReady) {
      observedActionSignal = actionSignal;
      actionSignalReady = true;
      return;
    }
    if (actionSignal === observedActionSignal) return;
    observedActionSignal = actionSignal;
    if (view === 'todos') createOpen = true;
    else if (connected) void refresh(projectId, () => false);
  });

  function preferredTodo(todos: TodoSummary[]): number | null {
    return (
      todos.find((todo) => todo.status === 'in_progress')?.id ??
      todos.find((todo) => todo.status === 'open')?.id ??
      todos[0]?.id ??
      null
    );
  }

  async function refresh(activeProject: number, disposed: () => boolean): Promise<void> {
    const request = ++sequence;
    refreshing = true;
    try {
      const next = await client.coordinationSnapshot(activeProject);
      if (disposed() || request !== sequence || activeProject !== projectId) return;
      snapshot = next;

      const todoId = next.todos.some((todo) => todo.id === selectedTodoId)
        ? selectedTodoId
        : preferredTodo(next.todos);
      if (todoId !== selectedTodoId) selectedTodoId = todoId;
      if (todoId !== null) void loadTodo(activeProject, todoId, request);
      else todoDetail = null;

      const scratchpadId = next.scratchpads.some(
        (scratchpad) => scratchpad.id === selectedScratchpadId
      )
        ? selectedScratchpadId
        : (next.scratchpads[0]?.id ?? null);
      if (scratchpadId !== selectedScratchpadId) selectedScratchpadId = scratchpadId;
      const selectedSummary = next.scratchpads.find(
        (scratchpad) => scratchpad.id === scratchpadId
      );
      if (
        scratchpadId !== null &&
        (scratchpadRead?.scratchpad.id !== scratchpadId ||
          scratchpadRead.scratchpad.revision !== selectedSummary?.revision)
      ) {
        void loadScratchpad(activeProject, scratchpadId, request);
      } else if (scratchpadId === null) {
        scratchpadRead = null;
      }
    } catch (cause) {
      if (!disposed() && activeProject === projectId) onError(message(cause));
    } finally {
      if (!disposed() && request === sequence) refreshing = false;
    }
  }

  async function loadTodo(
    activeProject: number,
    todoId: number,
    parentSequence?: number
  ): Promise<void> {
    todoLoading = true;
    try {
      const next = await client.coordinationTodo(activeProject, todoId);
      if (
        activeProject !== projectId ||
        todoId !== selectedTodoId ||
        (parentSequence !== undefined && parentSequence !== sequence)
      )
        return;
      todoDetail = next;
    } catch (cause) {
      if (activeProject === projectId && todoId === selectedTodoId) onError(message(cause));
    } finally {
      if (activeProject === projectId && todoId === selectedTodoId) todoLoading = false;
    }
  }

  async function loadScratchpad(
    activeProject: number,
    scratchpadId: number,
    parentSequence?: number
  ): Promise<void> {
    scratchpadLoading = true;
    try {
      const next = await client.coordinationScratchpad(activeProject, scratchpadId);
      if (
        activeProject !== projectId ||
        scratchpadId !== selectedScratchpadId ||
        (parentSequence !== undefined && parentSequence !== sequence)
      )
        return;
      scratchpadRead = next;
    } catch (cause) {
      if (activeProject === projectId && scratchpadId === selectedScratchpadId) {
        onError(message(cause));
      }
    } finally {
      if (activeProject === projectId && scratchpadId === selectedScratchpadId) {
        scratchpadLoading = false;
      }
    }
  }

  async function createTodo(): Promise<void> {
    const title = createTitle.trim();
    if (!title || mutating) return;
    mutating = true;
    try {
      const todo = await client.coordinationTodoCreate(projectId, {
        title,
        body: createBody.trim(),
        priority: createPriority,
        tags: createTags
          .split(',')
          .map((tag) => tag.trim())
          .filter(Boolean)
      });
      selectedTodoId = todo.id;
      createOpen = false;
      createTitle = '';
      createBody = '';
      createPriority = 'medium';
      createTags = '';
      await refresh(projectId, () => false);
      await loadTodo(projectId, todo.id);
    } catch (cause) {
      onError(message(cause));
    } finally {
      mutating = false;
    }
  }

  async function completeTodo(todoId: number, completed: boolean): Promise<void> {
    if (mutating) return;
    mutating = true;
    try {
      await client.coordinationTodoComplete(projectId, todoId, completed);
      selectedTodoId = todoId;
      await refresh(projectId, () => false);
      await loadTodo(projectId, todoId);
    } catch (cause) {
      onError(message(cause));
    } finally {
      mutating = false;
    }
  }

  async function commentTodo(todoId: number, body: string): Promise<void> {
    if (mutating || !body.trim()) return;
    mutating = true;
    try {
      await client.coordinationTodoComment(projectId, todoId, body.trim());
      await loadTodo(projectId, todoId);
      await refresh(projectId, () => false);
    } catch (cause) {
      onError(message(cause));
    } finally {
      mutating = false;
    }
  }

  function selectTodo(todoId: number): void {
    selectedTodoId = todoId;
    void loadTodo(projectId, todoId);
  }

  function selectScratchpad(scratchpadId: number): void {
    selectedScratchpadId = scratchpadId;
    void loadScratchpad(projectId, scratchpadId);
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }

  function focusInput(node: HTMLInputElement): void {
    queueMicrotask(() => node.focus());
  }
</script>

<div class="coordination-view">
  <div class="live-strip">
    <span class:refreshing><i aria-hidden="true"></i>{connected ? 'Live project data · 2s' : 'Offline'}</span>
    {#if snapshot}
      <small>
        {view === 'todos'
          ? `${snapshot.todo_total_count} todo${snapshot.todo_total_count === 1 ? '' : 's'}`
          : `${snapshot.scratchpad_total_count} note${snapshot.scratchpad_total_count === 1 ? '' : 's'}`}
      </small>
    {/if}
  </div>

  {#if !connected}
    <div class="offline">Reconnect to the daemon to load this project.</div>
  {:else if snapshot === null}
    <div class="loading"><span aria-hidden="true"></span> Loading project data…</div>
  {:else if view === 'todos'}
    <TodoBoard
      todos={snapshot.todos}
      selectedId={selectedTodoId}
      detail={todoDetail}
      detailLoading={todoLoading}
      busy={mutating}
      onSelect={selectTodo}
      onCreate={() => (createOpen = true)}
      onComplete={(todoId, completed) => void completeTodo(todoId, completed)}
      onComment={(todoId, body) => void commentTodo(todoId, body)}
    />
  {:else}
    <ScratchpadPanel
      scratchpads={snapshot.scratchpads}
      selectedId={selectedScratchpadId}
      read={scratchpadRead}
      loading={scratchpadLoading}
      onSelect={selectScratchpad}
      onRefresh={() => void refresh(projectId, () => false)}
    />
  {/if}
</div>

{#if createOpen}
  <div
    class="dialog-backdrop"
    role="presentation"
    onclick={(event) => {
      if (event.target === event.currentTarget && !mutating) createOpen = false;
    }}
  >
    <form
      class="create-dialog"
      aria-label="Create todo"
      onsubmit={(event) => {
        event.preventDefault();
        void createTodo();
      }}
    >
      <header>
        <div>
          <span>New project todo</span>
          <h2>What needs to happen?</h2>
        </div>
        <button type="button" aria-label="Close" disabled={mutating} onclick={() => (createOpen = false)}>×</button>
      </header>
      <label>
        <span>Title</span>
        <input bind:value={createTitle} use:focusInput placeholder="Ship the first useful slice" required />
      </label>
      <label>
        <span>Notes <small>optional</small></span>
        <textarea bind:value={createBody} rows="4" placeholder="Outcome, constraints, or context"></textarea>
      </label>
      <div class="form-row">
        <label>
          <span>Priority</span>
          <select bind:value={createPriority}>
            <option value="high">High</option>
            <option value="medium">Medium</option>
            <option value="low">Low</option>
          </select>
        </label>
        <label>
          <span>Tags <small>comma separated</small></span>
          <input bind:value={createTags} placeholder="ui, follow-up" />
        </label>
      </div>
      <footer>
        <button type="button" disabled={mutating} onclick={() => (createOpen = false)}>Cancel</button>
        <button class="submit" type="submit" disabled={mutating || !createTitle.trim()}>
          {mutating ? 'Creating…' : 'Create todo'}
        </button>
      </footer>
    </form>
  </div>
{/if}

<style>
  .coordination-view { display: grid; min-width: 0; gap: 9px; }
  .live-strip { display: flex; align-items: center; justify-content: space-between; min-height: 25px; border-bottom: 1px solid var(--border); color: #858c95; font-family: 'JetBrains Mono Variable', monospace; font-size: 8px; letter-spacing: 0.04em; text-transform: uppercase; }
  .live-strip > span { display: flex; align-items: center; gap: 7px; }
  .live-strip i { width: 6px; height: 6px; border-radius: 50%; background: var(--signal); }
  .live-strip .refreshing i { animation: pulse 0.8s ease-in-out infinite alternate; }
  .live-strip small { color: #777e87; font: inherit; }
  .loading, .offline { display: flex; min-height: 200px; align-items: center; justify-content: center; gap: 8px; color: #90969f; font: 9px 'JetBrains Mono Variable', monospace; }
  .loading span { width: 12px; height: 12px; border: 1px solid #36535e; border-top-color: var(--signal); border-radius: 50%; animation: spin 0.8s linear infinite; }

  .dialog-backdrop { position: fixed; z-index: 40; inset: 0; display: grid; place-items: center; padding: 16px; background: rgb(3 4 5 / 74%); }
  .create-dialog { width: min(500px, 100%); border: 1px solid #4a4f57; border-radius: 4px; padding: 0; background: #1c1f23; color: #e2e4e6; box-shadow: 0 18px 55px rgb(0 0 0 / 42%); }
  .create-dialog header { display: flex; align-items: start; justify-content: space-between; border-bottom: 1px solid var(--border); padding: 13px 15px 11px; }
  .create-dialog header span, label > span { color: #9299a2; font: 700 8px 'JetBrains Mono Variable', monospace; letter-spacing: 0.06em; text-transform: uppercase; }
  .create-dialog h2 { margin: 4px 0 0; color: #f0f1f3; font-size: 19px; font-weight: 630; }
  .create-dialog header button { border: 0; background: transparent; color: #718892; font-size: 22px; cursor: pointer; }
  .create-dialog > label, .form-row { margin: 12px 15px 0; }
  .create-dialog label { display: grid; gap: 5px; }
  label small { color: #506974; font: inherit; }
  input, textarea, select { width: 100%; border: 1px solid #41464d; border-radius: 3px; padding: 8px 9px; background: #111315; color: #e0e2e5; font-size: 11px; outline: 0; }
  input:focus, textarea:focus, select:focus { border-color: #757c85; }
  textarea { resize: vertical; line-height: 1.5; }
  .form-row { display: grid; grid-template-columns: minmax(120px, 0.4fr) minmax(0, 1fr); gap: 12px; }
  .create-dialog footer { display: flex; justify-content: flex-end; gap: 7px; margin-top: 15px; border-top: 1px solid var(--border); padding: 10px 15px; }
  .create-dialog footer button { min-height: 31px; border: 1px solid #484d54; border-radius: 3px; padding: 0 10px; background: #25282d; color: #c4c8cd; font-size: 10px; cursor: pointer; }
  .create-dialog footer .submit { border-color: #666d76; background: #30343a; color: #f0f1f3; font-weight: 650; }
  .create-dialog button:disabled { cursor: not-allowed; opacity: 0.5; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes pulse { to { opacity: 0.35; } }
</style>
