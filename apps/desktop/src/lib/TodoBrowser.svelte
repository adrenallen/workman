<script lang="ts">
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
  import SearchIcon from '@lucide/svelte/icons/search';
  import SlidersHorizontalIcon from '@lucide/svelte/icons/sliders-horizontal';
  import XIcon from '@lucide/svelte/icons/x';

  import TodoStatusIndicator from './components/ds/TodoStatusIndicator.svelte';
  import SectionOverview from './SectionOverview.svelte';
  import type { Project } from './daemon';
  import type { TodoPriority, TodoStatus, TodoSummary } from './coordination';
  import {
    shortTodoActor,
    todoClaimLabel,
    todoClaimState
  } from './todoPresentation';

  interface Props {
    todos: TodoSummary[];
    onSelect: (todo: TodoSummary, navigationIds: number[]) => void;
    onCreate: () => void;
    project?: Project | null;
  }

  type StatusFilter = 'active' | TodoStatus | 'all';
  type ClaimFilter = 'all' | 'claimed' | 'unclaimed' | `actor:${string}`;
  type BlockedFilter = 'all' | 'blocked' | 'unblocked';
  type PriorityFilter = 'all' | TodoPriority;
  type SortMode = 'state' | 'priority' | 'newest' | 'title';

  let { todos, onSelect, onCreate, project = null }: Props = $props();
  let query = $state('');
  let status = $state<StatusFilter>('active');
  let claim = $state<ClaimFilter>('all');
  let blocked = $state<BlockedFilter>('all');
  let tag = $state('all');
  let priority = $state<PriorityFilter>('all');
  let sort = $state<SortMode>('state');

  const priorityRank: Record<TodoPriority, number> = { high: 0, medium: 1, low: 2 };
  const stateRank = { blocked: 0, claimed: 1, open: 2, completed: 3 } as const;

  let actors = $derived(
    [...new Set(todos.flatMap((todo) => (todo.locked_by ? [todo.locked_by] : [])))].sort()
  );
  let tags = $derived([...new Set(todos.flatMap((todo) => todo.tags))].sort());
  let filteredTodos = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    return todos
      .filter((todo) => {
        if (status === 'active' && todo.completed) return false;
        if (status !== 'all' && status !== 'active' && todo.status !== status) return false;
        if (claim === 'claimed' && !todo.locked_by) return false;
        if (claim === 'unclaimed' && todo.locked_by) return false;
        if (claim.startsWith('actor:') && todo.locked_by !== claim.slice(6)) return false;
        if (blocked === 'blocked' && !todo.is_blocked) return false;
        if (blocked === 'unblocked' && todo.is_blocked) return false;
        if (tag !== 'all' && !todo.tags.includes(tag)) return false;
        if (priority !== 'all' && todo.priority !== priority) return false;
        if (
          needle &&
          !`${todo.id} ${todo.title} ${todo.tags.join(' ')} ${todo.locked_by ?? ''}`
            .toLowerCase()
            .includes(needle)
        ) return false;
        return true;
      })
      .sort(compareTodos);
  });
  let activeFilterCount = $derived(
    Number(status !== 'active') +
      Number(claim !== 'all') +
      Number(blocked !== 'all') +
      Number(tag !== 'all') +
      Number(priority !== 'all')
  );

  function compareTodos(left: TodoSummary, right: TodoSummary): number {
    if (sort === 'title') return left.title.localeCompare(right.title);
    if (sort === 'newest') return right.id - left.id;
    if (sort === 'priority') {
      return priorityRank[left.priority] - priorityRank[right.priority] || right.id - left.id;
    }
    return (
      stateRank[todoClaimState(left)] - stateRank[todoClaimState(right)] ||
      priorityRank[left.priority] - priorityRank[right.priority] ||
      right.id - left.id
    );
  }

  function resetFilters(): void {
    query = '';
    status = 'active';
    claim = 'all';
    blocked = 'all';
    tag = 'all';
    priority = 'all';
    sort = 'state';
  }

  function statusCopy(todo: TodoSummary): string {
    if (todo.status === 'in_progress') return 'in progress';
    return todo.status;
  }

  function blockerCopy(todo: TodoSummary): string {
    const visible = todo.blocker_ids.slice(0, 2).map((id) => `#${id}`).join(', ');
    const overflow = todo.blocker_ids.length > 2 ? ` +${todo.blocker_ids.length - 2}` : '';
    return `${todo.is_blocked ? 'Blocked by' : 'Resolved'} ${visible}${overflow}`;
  }
</script>

<SectionOverview
  ariaLabel="Todos browser"
  eyebrow="Shared work queue"
  title="Todos"
  description="Browse every task, claim state, dependency, and handoff."
  summaryLayout="split"
  {project}
>
  {#snippet icon()}<CircleCheckIcon strokeWidth={1.8} />{/snippet}
  {#snippet action()}
    <button class="primary-action" type="button" onclick={onCreate}>+ New todo</button>
  {/snippet}

  {#snippet controls()}
    <div class="filter-panel">
      <label class="search-field">
        <SearchIcon size={14} aria-hidden="true" />
        <input bind:value={query} aria-label="Search todos" placeholder="Search title, tag, actor, or ID" />
        {#if query}
          <button type="button" aria-label="Clear todo search" title="Clear search" onclick={() => (query = '')}>
            <XIcon size={13} aria-hidden="true" />
          </button>
        {/if}
      </label>
      <label><span>Status</span><select bind:value={status} aria-label="Filter todos by status"><option value="active">Active</option><option value="all">All</option><option value="backlog">Backlog</option><option value="open">Open</option><option value="in_progress">In progress</option><option value="completed">Done</option></select></label>
      <label><span>Claimed by</span><select bind:value={claim} aria-label="Filter todos by claimant"><option value="all">Anyone</option><option value="claimed">Claimed</option><option value="unclaimed">Unclaimed</option>{#each actors as actor}<option value={`actor:${actor}`}>{shortTodoActor(actor)}</option>{/each}</select></label>
      <label><span>Dependency</span><select bind:value={blocked} aria-label="Filter blocked todos"><option value="all">Any</option><option value="blocked">Blocked</option><option value="unblocked">Not blocked</option></select></label>
      <label><span>Tag</span><select bind:value={tag} aria-label="Filter todos by tag"><option value="all">Any tag</option>{#each tags as item}<option value={item}>{item}</option>{/each}</select></label>
      <label><span>Priority</span><select bind:value={priority} aria-label="Filter todos by priority"><option value="all">Any</option><option value="high">High</option><option value="medium">Medium</option><option value="low">Low</option></select></label>
      <label><span>Sort</span><select bind:value={sort} aria-label="Sort todos"><option value="state">Claim state</option><option value="priority">Priority</option><option value="newest">Newest ID</option><option value="title">Title</option></select></label>
    </div>
  {/snippet}

  {#snippet summary()}
    <span class="summary-count"><SlidersHorizontalIcon size={13} aria-hidden="true" /> {filteredTodos.length} of {todos.length}</span>
    {#if activeFilterCount > 0 || query}<button class="summary-reset" type="button" onclick={resetFilters}>Reset {activeFilterCount + Number(Boolean(query))} filter{activeFilterCount + Number(Boolean(query)) === 1 ? '' : 's'}</button>{/if}
  {/snippet}

  <div class="todo-ledger" aria-live="polite">
    {#each filteredTodos as todo (todo.id)}
      <button
        type="button"
        class="todo-row"
        data-state={todoClaimState(todo)}
        title={`${todoClaimLabel(todo)} · ${statusCopy(todo)} · ${todo.priority} priority`}
        onclick={() => onSelect(todo, filteredTodos.map((candidate) => candidate.id))}
      >
        <span class="state-rail" aria-hidden="true"></span>
        <TodoStatusIndicator state={todoClaimState(todo)} label={todoClaimLabel(todo)} />
        <span class="todo-id">#{todo.id}</span>
        <span class="todo-copy">
          <strong>{todo.title}</strong>
          <small>
            <span>{statusCopy(todo)}</span>
            {#if todo.tags.length > 0}<span>{todo.tags.join(' · ')}</span>{/if}
            {#if todo.blocker_ids.length > 0}<span class:resolved={!todo.is_blocked} class="blocker-hint">{blockerCopy(todo)}</span>{/if}
          </small>
        </span>
        <span class="claim-copy">
          <strong>{todoClaimState(todo)}</strong>
          <small>{todo.locked_by ? shortTodoActor(todo.locked_by) : todo.is_blocked ? `${todo.unresolved_blocker_count} blockers` : '—'}</small>
        </span>
        <span class={`priority ${todo.priority}`}>{todo.priority}</span>
        <span class="signals">{#if todo.comment_count > 0}<span title={`${todo.comment_count} comments`}>◌ {todo.comment_count}</span>{/if}{#if todo.blocker_ids.length > 0}<span title={`Depends on ${todo.blocker_ids.map((id) => `#${id}`).join(', ')}`}>↳ {todo.blocker_ids.length}</span>{/if}</span>
      </button>
    {:else}
      <div class="empty-results">
        <strong>No todos match these filters</strong>
        <p>Reset the filters or search for a different title, tag, actor, or ID.</p>
        <button type="button" onclick={resetFilters}>Reset filters</button>
      </div>
    {/each}
  </div>
</SectionOverview>

<style>
  .filter-panel label > span, .summary-count, .summary-reset, .todo-id, .claim-copy, .priority, .signals { font-family: var(--terminal-font-family); }
  .primary-action { min-height: 30px; border: 1px solid var(--input); border-radius: var(--radius); padding: 0 10px; background: var(--primary); color: var(--primary-foreground); font-size: var(--font-size-sm); font-weight: 650; cursor: pointer; }
  .primary-action:active { transform: translateY(1px); }

  .filter-panel { display: grid; grid-template-columns: minmax(190px, 1.7fr) repeat(6, minmax(92px, 0.72fr)); gap: 6px; padding: 8px 10px; }
  .filter-panel label { display: grid; min-width: 0; gap: 3px; }
  .filter-panel label > span { color: var(--muted-foreground); font-size: 10px; letter-spacing: 0.06em; text-transform: uppercase; }
  .filter-panel select, .search-field { width: 100%; height: 28px; border: 1px solid var(--input); border-radius: var(--radius); background: var(--background); color: var(--text-soft); font-size: var(--font-size-xs); outline: 0; }
  .filter-panel select { padding: 0 6px; }
  .search-field { display: flex !important; align-self: end; grid-template-columns: none !important; flex-direction: row; align-items: center; gap: 5px !important; padding: 0 7px; color: var(--muted-foreground); }
  .search-field input { min-width: 0; flex: 1; border: 0; outline: 0; padding: 0; background: transparent; color: var(--foreground); font-size: var(--font-size-sm); }
  .search-field button { display: grid; width: 20px; height: 20px; place-items: center; border: 0; padding: 0; background: transparent; color: var(--muted-foreground); cursor: pointer; }

  .summary-count { display: flex; align-items: center; gap: 5px; }
  .summary-reset { border: 0; padding: 3px 0; background: transparent; color: var(--ring); font-size: inherit; cursor: pointer; }

  .todo-ledger { min-height: 0; overflow-y: auto; padding: 4px 7px 10px; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .todo-row { position: relative; display: grid; width: 100%; min-height: 38px; grid-template-columns: 3px 15px 45px minmax(180px, 1fr) minmax(94px, 0.28fr) 62px 58px; align-items: center; gap: 7px; border: 0; border-bottom: 1px solid var(--border); padding: 3px 8px 3px 0; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .todo-row:hover { background: var(--popover); }
  .todo-row:focus-visible { z-index: 1; }
  .state-rail { align-self: stretch; background: var(--todo-state-open); opacity: 0.55; }
  .todo-row[data-state='claimed'] .state-rail { background: var(--todo-state-claimed); opacity: 1; }
  .todo-row[data-state='blocked'] .state-rail { background: var(--todo-state-blocked); opacity: 1; }
  .todo-row[data-state='completed'] .state-rail { background: var(--todo-state-completed); opacity: 0.45; }
  .todo-row[data-state='completed'] .todo-copy strong { color: var(--muted-foreground); text-decoration: line-through; }
  .todo-id { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .todo-copy { min-width: 0; }
  .todo-copy strong, .todo-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .todo-copy strong { font-size: var(--font-size-sm); font-weight: 590; }
  .todo-copy small { display: flex; gap: 8px; margin-top: 1px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .todo-copy .blocker-hint { color: var(--todo-state-blocked); }
  .todo-copy .blocker-hint.resolved { color: var(--todo-state-completed); }
  .claim-copy { min-width: 0; font-size: var(--font-size-xs); }
  .claim-copy strong, .claim-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .claim-copy strong { color: var(--text-soft); font-weight: 650; text-transform: uppercase; }
  .claim-copy small { margin-top: 1px; color: var(--muted-foreground); }
  .todo-row[data-state='claimed'] .claim-copy strong { color: var(--todo-state-claimed); }
  .todo-row[data-state='blocked'] .claim-copy strong { color: var(--todo-state-blocked); }
  .priority { justify-self: start; border: 1px solid var(--border-strong); border-radius: 999px; padding: 2px 6px; color: var(--muted-foreground); font-size: 10px; text-transform: uppercase; }
  .priority.high { border-color: color-mix(in srgb, var(--destructive) 42%, var(--border)); color: var(--destructive); }
  .signals { display: flex; justify-content: flex-end; gap: 6px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .empty-results { display: grid; min-height: 180px; place-content: center; justify-items: center; text-align: center; }
  .empty-results strong { font-size: var(--font-size-base); }
  .empty-results p { max-width: 380px; margin: 5px 0 9px; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .empty-results button { border: 1px solid var(--input); border-radius: var(--radius); padding: 5px 8px; background: var(--card); color: var(--text-soft); font-size: var(--font-size-sm); cursor: pointer; }

  @container (max-width: 980px) {
    .filter-panel { grid-template-columns: minmax(190px, 2fr) repeat(3, minmax(100px, 1fr)); }
    .todo-row { grid-template-columns: 3px 15px 42px minmax(150px, 1fr) 92px 58px; }
    .signals { display: none; }
  }
</style>
