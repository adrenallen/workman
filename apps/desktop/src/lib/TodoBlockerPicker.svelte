<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import SearchIcon from '@lucide/svelte/icons/search';
  import XIcon from '@lucide/svelte/icons/x';

  import TodoStatusIndicator from './components/ds/TodoStatusIndicator.svelte';
  import type { TodoSummary } from './coordination';
  import { todoClaimLabel, todoClaimState } from './todoPresentation';

  interface Props {
    todos: TodoSummary[];
    selectedIds: number[];
    currentTodoId?: number | null;
    label?: string;
    description?: string;
    disabled?: boolean;
    compact?: boolean;
    onChange: (blockerIds: number[]) => Promise<void> | void;
    onNavigate?: (todoId: number) => void;
  }

  let {
    todos,
    selectedIds,
    currentTodoId = null,
    label = 'Blocked by',
    description = 'Add a todo by #id or title.',
    disabled = false,
    compact = false,
    onChange,
    onNavigate
  }: Props = $props();

  let query = $state('');
  let open = $state(false);
  let saving = $state(false);
  let localError = $state<string | null>(null);

  let results = $derived.by(() => {
    const available = todos.filter(
      (todo) => todo.id !== currentTodoId && !selectedIds.includes(todo.id)
    );
    const needle = query.trim().toLowerCase();
    if (!needle) {
      return [...available]
        .sort((left, right) => Number(left.completed) - Number(right.completed) || right.id - left.id)
        .slice(0, 6);
    }
    const exactId = Number(needle.replace(/^#/, ''));
    const matches = available
      .map((todo) => ({ todo, score: candidateScore(todo, needle, exactId) }))
      .filter((candidate) => candidate.score >= 0);
    return matches
      .sort((left, right) => left.score - right.score || right.todo.id - left.todo.id)
      .slice(0, 6)
      .map((candidate) => candidate.todo);
  });

  function fuzzyIndex(haystack: string, needle: string): number {
    let cursor = 0;
    let first = -1;
    for (const character of needle) {
      cursor = haystack.indexOf(character, cursor);
      if (cursor < 0) return -1;
      if (first < 0) first = cursor;
      cursor += 1;
    }
    return first;
  }

  function candidateScore(todo: TodoSummary, needle: string, exactId: number): number {
    if (Number.isInteger(exactId) && todo.id === exactId) return 0;
    const haystack = `#${todo.id} ${todo.title}`.toLowerCase();
    if (haystack.startsWith(needle)) return 10;
    const direct = haystack.indexOf(needle);
    if (direct >= 0) return 20 + direct;
    const tokens = needle.split(/\s+/).filter(Boolean);
    let score = 50;
    for (const token of tokens) {
      const index = fuzzyIndex(haystack, token);
      if (index < 0) return -1;
      score += index;
    }
    return score;
  }

  function errorMessage(cause: unknown): string {
    const detail = cause instanceof Error ? cause.message : String(cause);
    return detail.includes('cycle')
      ? `Cycle blocked: ${detail}`
      : `Could not update blockers: ${detail}`;
  }

  async function commit(nextIds: number[]): Promise<void> {
    if (saving || disabled) return;
    saving = true;
    localError = null;
    try {
      await onChange(nextIds);
      query = '';
    } catch (cause) {
      localError = errorMessage(cause);
    } finally {
      saving = false;
    }
  }

  function submitQuery(): void {
    const needle = query.trim();
    const exactId = Number(needle.replace(/^#/, ''));
    if (currentTodoId !== null && Number.isInteger(exactId) && exactId === currentTodoId) {
      localError = 'A todo cannot block itself.';
      return;
    }
    const match = results[0];
    if (!match) {
      localError = /^#?\d+$/.test(needle)
        ? `No todo ${needle.startsWith('#') ? needle : `#${needle}`} exists in this project.`
        : 'No todo matches that title.';
      return;
    }
    void commit([...selectedIds, match.id]);
  }
</script>

<section class:compact class="blocker-picker" aria-label={label}>
  <header>
    <div><strong>{label}</strong><small>{description}</small></div>
    {#if saving}<span>Saving…</span>{/if}
  </header>

  {#if selectedIds.length > 0}
    <div class="selected-list">
      {#each selectedIds as blockerId (blockerId)}
        {@const todo = todos.find((candidate) => candidate.id === blockerId)}
        <div class:resolved={todo?.completed} class="selected-row">
          {#if todo}
            <TodoStatusIndicator state={todoClaimState(todo)} label={todoClaimLabel(todo)} />
            <button class="relation" type="button" disabled={!onNavigate} onclick={() => onNavigate?.(todo.id)}>
              <span>#{todo.id}</span><strong>{todo.title}</strong>
              <small>{todo.completed ? 'Resolved' : todo.status === 'in_progress' ? 'In progress' : todo.status}</small>
            </button>
            {#if todo.completed}<span class="resolved-check"><CheckIcon size={14} aria-label="Resolved blocker" /></span>{/if}
          {:else}
            <span class="missing-id">#{blockerId}</span><strong class="missing-title">Todo not loaded</strong>
          {/if}
          <button class="remove" type="button" disabled={disabled || saving} aria-label={`Remove blocker #${blockerId}`} onclick={() => void commit(selectedIds.filter((id) => id !== blockerId))}>
            <XIcon size={13} aria-hidden="true" />
          </button>
        </div>
      {/each}
    </div>
  {/if}

  <form class="search" onsubmit={(event) => { event.preventDefault(); submitQuery(); }}>
    <SearchIcon size={14} aria-hidden="true" />
    <input
      bind:value={query}
      aria-label="Add blocker by todo ID or title"
      placeholder="#123 or search titles"
      disabled={disabled || saving}
      onfocus={() => (open = true)}
      oninput={() => { open = true; localError = null; }}
      onblur={() => setTimeout(() => (open = false), 120)}
    />
    <button type="submit" disabled={disabled || saving || !query.trim()}>Add</button>
  </form>

  {#if open && results.length > 0}
    <div class="results" aria-label="Matching todos">
      {#each results as todo (todo.id)}
        <button type="button" onmousedown={(event) => event.preventDefault()} onclick={() => void commit([...selectedIds, todo.id])}>
          <TodoStatusIndicator state={todoClaimState(todo)} label={todoClaimLabel(todo)} />
          <span>#{todo.id}</span><strong>{todo.title}</strong><small>{todo.completed ? 'Resolved' : todo.status.replace('_', ' ')}</small>
        </button>
      {/each}
    </div>
  {/if}

  {#if localError}<p class="error" role="alert"><strong>Blocker error</strong> {localError}</p>{/if}
</section>

<style>
  .blocker-picker { display: grid; min-width: 0; gap: 7px; }
  header { display: flex; align-items: start; justify-content: space-between; gap: 8px; padding: 2px 2px 0; }
  header div { display: grid; gap: 2px; }
  header strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 650; }
  header small, header > span { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .selected-list { display: grid; gap: 3px; }
  .selected-row { display: grid; min-width: 0; min-height: 35px; grid-template-columns: 14px minmax(0, 1fr) auto auto; align-items: center; gap: 7px; border: 1px solid var(--border); border-radius: var(--radius); padding: 3px 4px 3px 8px; background: var(--background); }
  .selected-row.resolved { opacity: 0.68; }
  .relation { display: grid; min-width: 0; grid-template-columns: auto minmax(0, 1fr) auto; align-items: baseline; gap: 6px; border: 0; padding: 0; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .relation:disabled { cursor: default; }
  .relation span, .missing-id { color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .relation strong, .missing-title { overflow: hidden; font-size: var(--font-size-sm); font-weight: 590; text-overflow: ellipsis; white-space: nowrap; }
  .relation small { color: var(--muted-foreground); font-size: 10px; text-transform: capitalize; }
  .resolved-check { color: var(--todo-state-completed); }
  .remove { display: grid; width: 25px; height: 25px; place-items: center; border: 0; border-radius: var(--radius); padding: 0; background: transparent; color: var(--muted-foreground); cursor: pointer; }
  .remove:hover { background: var(--accent); color: var(--foreground); }
  .search { display: grid; min-width: 0; height: 32px; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 6px; border: 1px solid var(--input); border-radius: var(--radius); padding: 0 4px 0 8px; background: var(--background); color: var(--muted-foreground); }
  .search:focus-within { border-color: var(--ring); box-shadow: 0 0 0 1px var(--ring); }
  .search input { min-width: 0; border: 0 !important; outline: 0; padding: 0 !important; background: transparent !important; color: var(--foreground); font-size: var(--font-size-sm); }
  .search button { min-height: 24px; border: 1px solid var(--input); border-radius: var(--radius); padding: 0 8px; background: var(--card); color: var(--text-soft); font-size: var(--font-size-xs); cursor: pointer; }
  .search button:disabled { cursor: default; opacity: 0.45; }
  .results { display: grid; max-height: 190px; overflow-y: auto; border: 1px solid var(--border); border-radius: var(--radius); padding: 3px; background: var(--popover); box-shadow: var(--shadow-md); }
  .results button { display: grid; min-width: 0; min-height: 31px; grid-template-columns: 14px 38px minmax(0, 1fr) auto; align-items: center; gap: 6px; border: 0; border-radius: var(--radius); padding: 3px 6px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .results button:hover { background: var(--accent); }
  .results span { color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .results strong { overflow: hidden; font-size: var(--font-size-sm); font-weight: 590; text-overflow: ellipsis; white-space: nowrap; }
  .results small { color: var(--muted-foreground); font-size: 10px; text-transform: capitalize; }
  .error { margin: 0; border-left: 2px solid var(--destructive); padding: 5px 7px; background: color-mix(in srgb, var(--destructive) 7%, transparent); color: var(--destructive); font-size: var(--font-size-xs); line-height: 1.4; }
  .error strong { margin-right: 3px; }
  .compact header small { display: none; }
  .compact .selected-row { min-height: 31px; }
</style>
