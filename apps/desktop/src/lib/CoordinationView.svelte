<script lang="ts">
  import TodoBoard from './TodoBoard.svelte';
  import ScratchpadPanel from './ScratchpadPanel.svelte';
  import type {
    CoordinationClient,
    CoordinationSnapshot,
    ScratchpadRead,
    TodoDetail,
    TodoSummary
  } from './coordination';

  interface Props {
    client: CoordinationClient;
    projectId: number;
    connected: boolean;
    onError: (message: string) => void;
  }

  let { client, projectId, connected, onError }: Props = $props();
  let snapshot = $state<CoordinationSnapshot | null>(null);
  let selectedTodoId = $state<number | null>(null);
  let todoDetail = $state<TodoDetail | null>(null);
  let todoLoading = $state(false);
  let selectedScratchpadId = $state<number | null>(null);
  let scratchpadRead = $state<ScratchpadRead | null>(null);
  let scratchpadLoading = $state(false);
  let refreshing = $state(false);
  let sequence = 0;

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
      }
    } catch (cause) {
      if (!disposed() && activeProject === projectId) onError(message(cause));
    } finally {
      if (!disposed() && request === sequence) refreshing = false;
    }
  }

  async function loadTodo(activeProject: number, todoId: number, parentSequence?: number): Promise<void> {
    todoLoading = true;
    try {
      const next = await client.coordinationTodo(activeProject, todoId);
      if (
        activeProject !== projectId ||
        todoId !== selectedTodoId ||
        (parentSequence !== undefined && parentSequence !== sequence)
      ) return;
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
      ) return;
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
</script>

<div class="coordination-view">
  <header class="coordination-heading">
    <div>
      <span class="eyebrow">Project pulse</span>
      <h2>Coordination</h2>
      <p>Work graph and shared agent notes</p>
    </div>
    <span class="sync" class:refreshing>
      <i aria-hidden="true"></i>
      {connected ? 'live · 2s' : 'offline'}
    </span>
  </header>

  {#if !connected}
    <div class="offline">Reconnect to the daemon to load coordination state.</div>
  {:else if snapshot === null}
    <div class="loading"><span aria-hidden="true"></span> Loading coordination graph…</div>
  {:else}
    <TodoBoard
      todos={snapshot.todos}
      selectedId={selectedTodoId}
      detail={todoDetail}
      detailLoading={todoLoading}
      onSelect={selectTodo}
    />
    <ScratchpadPanel
      scratchpads={snapshot.scratchpads}
      selectedId={selectedScratchpadId}
      read={scratchpadRead}
      loading={scratchpadLoading}
      onSelect={selectScratchpad}
    />
  {/if}
</div>

<style>
  .coordination-view {
    display: grid;
    min-width: 0;
    gap: 20px;
    padding: 20px clamp(20px, 3.6vw, 52px) 42px;
  }

  .coordination-heading,
  .coordination-heading > div,
  .sync,
  .loading {
    display: flex;
    align-items: center;
  }

  .coordination-heading { justify-content: space-between; gap: 20px; }
  .coordination-heading > div { flex-wrap: wrap; gap: 9px 13px; }

  .coordination-heading h2 {
    margin: 0;
    color: #e0e9ec;
    font-size: 22px;
    line-height: 1;
  }

  .coordination-heading p {
    width: 100%;
    margin: 0;
    color: #607985;
    font-size: 10px;
  }

  .eyebrow,
  .sync,
  .loading,
  .offline {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  .eyebrow {
    color: var(--signal);
    font-size: 8px;
    font-weight: 650;
    letter-spacing: 0.11em;
    text-transform: uppercase;
  }

  .sync {
    flex: none;
    gap: 7px;
    border: 1px solid #29434e;
    border-radius: 999px;
    padding: 6px 9px;
    color: #69828c;
    font-size: 7px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .sync i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--signal);
    box-shadow: 0 0 8px rgb(99 215 197 / 55%);
  }

  .sync.refreshing i { animation: pulse 0.8s ease-in-out infinite alternate; }

  .loading,
  .offline {
    min-height: 220px;
    place-content: center;
    justify-content: center;
    gap: 9px;
    color: #66808a;
    font-size: 9px;
  }

  .loading span {
    width: 12px;
    height: 12px;
    border: 1px solid #36535e;
    border-top-color: var(--signal);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes pulse { to { opacity: 0.35; } }
</style>
