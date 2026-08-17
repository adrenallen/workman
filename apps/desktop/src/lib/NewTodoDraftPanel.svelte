<script lang="ts">
  import { onMount } from 'svelte';

  import CreationDraftScaffold from './CreationDraftScaffold.svelte';
  import TodoBlockerPicker from './TodoBlockerPicker.svelte';
  import type { TodoSummary } from './coordination';
  import type { TodoCreationDraft } from './creationDrafts';
  import { submitOnEnter } from './formInputConventions';

  interface Props {
    draft: TodoCreationDraft;
    projectName: string;
    todos: TodoSummary[];
    busy?: boolean;
    onChange: (patch: Partial<TodoCreationDraft>) => void;
    onCreate: () => void;
    onDiscard: () => void;
  }

  let {
    draft,
    projectName,
    todos,
    busy = false,
    onChange,
    onCreate,
    onDiscard
  }: Props = $props();
  let titleInput = $state<HTMLInputElement | null>(null);

  onMount(() => requestAnimationFrame(() => titleInput?.focus()));
</script>

<CreationDraftScaffold
  {projectName}
  kindLabel="Todo"
  title={draft.title.trim() || 'New todo'}
  createLabel="Create todo"
  {busy}
  canCreate={Boolean(draft.title.trim())}
  {onCreate}
  {onDiscard}
>
  <section class="todo-fields">
    <label>
      <span>Title</span>
      <input bind:this={titleInput} value={draft.title} placeholder="What needs to happen?" disabled={busy} oninput={(event) => onChange({ title: event.currentTarget.value })} />
    </label>
    <label>
      <span>Notes <small>optional</small></span>
      <textarea value={draft.body} rows="7" placeholder="Outcome, constraints, or context" disabled={busy} use:submitOnEnter oninput={(event) => onChange({ body: event.currentTarget.value })}></textarea>
    </label>
    <div class="todo-row">
      <label>
        <span>Priority</span>
        <select value={draft.priority} disabled={busy} onchange={(event) => onChange({ priority: event.currentTarget.value as TodoCreationDraft['priority'] })}>
          <option value="high">High</option><option value="medium">Medium</option><option value="low">Low</option>
        </select>
      </label>
      <label>
        <span>Tags</span>
        <input value={draft.tags} placeholder="ui, follow-up" disabled={busy} oninput={(event) => onChange({ tags: event.currentTarget.value })} />
      </label>
    </div>
    <div class="blockers-field">
      <TodoBlockerPicker
        {todos}
        selectedIds={draft.blockerIds}
        label="Blocked by · optional"
        description="Create this todo with its prerequisites already linked."
        compact
        onChange={(blockerIds) => onChange({ blockerIds })}
      />
    </div>
  </section>
</CreationDraftScaffold>

<style>
  .todo-fields { display: grid; gap: 13px; }
  label { display: grid; gap: 6px; }
  label > span { color: var(--text-soft); font: 650 var(--font-size-xs) var(--terminal-font-family); letter-spacing: .045em; text-transform: uppercase; }
  label small { color: var(--muted-foreground); font: inherit; text-transform: none; }
  input, textarea, select { width: 100%; min-height: 34px; border: 1px solid var(--input); border-radius: var(--radius); outline: 0; padding: 7px 8px; background: var(--background); color: var(--text); font-size: var(--font-size-sm); }
  textarea { min-height: 150px; resize: vertical; line-height: 1.5; }
  input:focus, textarea:focus, select:focus { border-color: var(--ring); box-shadow: 0 0 0 2px color-mix(in srgb, var(--ring) 18%, transparent); }
  .todo-row { display: grid; grid-template-columns: .45fr 1fr; gap: 10px; }
  .blockers-field { border-top: 1px solid var(--border); padding-top: 13px; }
  @media (max-width: 620px) { .todo-row { grid-template-columns: 1fr; } }
</style>
