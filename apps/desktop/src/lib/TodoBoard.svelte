<script lang="ts">
  import MarkdownView from './MarkdownView.svelte';
  import type { TodoDetail, TodoStatus, TodoSummary } from './coordination';

  interface Props {
    todos: TodoSummary[];
    selectedId: number | null;
    detail: TodoDetail | null;
    detailLoading: boolean;
    busy: boolean;
    onSelect: (todoId: number) => void;
    onCreate: () => void;
    onComplete: (todoId: number, completed: boolean) => void;
    onComment: (todoId: number, body: string) => void;
  }

  const columns: { status: TodoStatus; label: string; marker: string }[] = [
    { status: 'backlog', label: 'Backlog', marker: '◇' },
    { status: 'open', label: 'Ready', marker: '○' },
    { status: 'in_progress', label: 'In motion', marker: '◒' },
    { status: 'completed', label: 'Done', marker: '●' }
  ];

  let {
    todos,
    selectedId,
    detail,
    detailLoading,
    busy,
    onSelect,
    onCreate,
    onComplete,
    onComment
  }: Props = $props();
  let commentBody = $state('');

  $effect(() => {
    selectedId;
    commentBody = '';
  });

  function inColumn(status: TodoStatus): TodoSummary[] {
    return todos.filter((todo) => todo.status === status);
  }

  function shortActor(actor: string): string {
    const parts = actor.split('-');
    return parts.length > 2 ? `${parts[0]}-${parts.at(-1)}` : actor;
  }

  function formatTime(epochMillis: number): string {
    return new Intl.DateTimeFormat(undefined, {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit'
    }).format(new Date(epochMillis));
  }
</script>

<section class="todo-board" aria-label="Todo board">
  <header class="board-heading">
    <div>
      <span class="eyebrow">Coordination graph</span>
      <h3>Todo board</h3>
    </div>
    <div class="board-actions">
      <span class="count">{todos.length.toString().padStart(2, '0')} tasks</span>
      <button type="button" disabled={busy} onclick={onCreate}><span>+</span> New todo</button>
    </div>
  </header>

  {#if todos.length === 0}
    <div class="empty-board">
      <span class="empty-mark" aria-hidden="true">◇</span>
      <span class="eyebrow">A shared work queue</span>
      <h4>Turn the next outcome into a todo</h4>
      <p>Todos give people and agents one place to claim work, record decisions, and mark progress.</p>
      <button type="button" disabled={busy} onclick={onCreate}><span>+</span> Create the first todo</button>
    </div>
  {:else}
    <div class="columns">
      {#each columns as column}
        {@const columnTodos = inColumn(column.status)}
        <section class="column" aria-label={column.label}>
          <header>
            <span aria-hidden="true">{column.marker}</span>
            <strong>{column.label}</strong>
            <small>{columnTodos.length}</small>
          </header>
          <div class="cards">
            {#each columnTodos as todo (todo.id)}
              <button
                class="todo-card"
                class:selected={selectedId === todo.id}
                class:blocked={todo.is_blocked}
                type="button"
                aria-pressed={selectedId === todo.id}
                onclick={() => onSelect(todo.id)}
              >
                <span class="card-topline">
                  <span class:high={todo.priority === 'high'} class="priority">{todo.priority}</span>
                  <span class="todo-id">#{todo.id}</span>
                </span>
                <strong>{todo.title}</strong>
                {#if todo.tags.length > 0}
                  <span class="tags">
                    {#each todo.tags.slice(0, 3) as tag}<i>{tag}</i>{/each}
                    {#if todo.tags.length > 3}<i>+{todo.tags.length - 3}</i>{/if}
                  </span>
                {/if}
                <span class="signals">
                  {#if todo.is_blocked}
                    <span class="blocker" title={`Blocked by ${todo.unresolved_blocker_count} open task(s)`}>
                      ⛓ {todo.unresolved_blocker_count}
                    </span>
                  {:else if todo.blocker_ids.length > 0}
                    <span title="All blockers resolved">✓ deps</span>
                  {/if}
                  {#if todo.comment_count > 0}<span title="Comments">◌ {todo.comment_count}</span>{/if}
                  {#if todo.locked_by}
                    <span class="lock" title={`Locked by ${todo.locked_by}`}>▣ {shortActor(todo.locked_by)}</span>
                  {/if}
                </span>
              </button>
            {:else}
              <div class="empty-column"><span>·</span> Clear</div>
            {/each}
          </div>
        </section>
      {/each}
    </div>

    {#if selectedId !== null}
      <aside class="detail" aria-live="polite">
        {#if detailLoading && detail?.todo.id !== selectedId}
          <div class="detail-empty">Reading task #{selectedId}…</div>
        {:else if detail}
          <div class="detail-copy">
            <header>
              <div>
                <span class="eyebrow">Task #{detail.todo.id}</span>
                <h4>{detail.todo.title}</h4>
              </div>
              <div class="detail-actions">
                <div class="detail-badges">
                  <span class={`priority ${detail.todo.priority === 'high' ? 'high' : ''}`}>
                    {detail.todo.priority}
                  </span>
                  {#if detail.todo.locked_by}<span class="lock">▣ {detail.todo.locked_by}</span>{/if}
                </div>
                <button
                  class:completed={detail.todo.completed}
                  type="button"
                  disabled={busy}
                  onclick={() => onComplete(detail.todo.id, !detail.todo.completed)}
                >
                  {detail.todo.completed ? 'Reopen' : 'Complete'}
                </button>
              </div>
            </header>
            {#if detail.todo.is_blocked}
              <p class="blocked-note">
                Dependencies {detail.todo.blocker_ids.map((id) => `#${id}`).join(', ')} ·
                {detail.todo.unresolved_blocker_count} unresolved
              </p>
            {/if}
            <div class="todo-body">
              {#if detail.todo.body.trim()}
                <MarkdownView source={detail.todo.body} />
              {:else}
                <p class="muted">No task notes.</p>
              {/if}
            </div>
          </div>
          <section class="comments" aria-label="Todo comments">
            <header>
              <span>Thread</span>
              <small>{detail.comment_total_count}</small>
            </header>
            <div class="comment-list">
              {#each detail.comments as comment (comment.id)}
                <article>
                  <header>
                    <strong>{comment.actor}</strong>
                    <time datetime={new Date(comment.created_at).toISOString()}>{formatTime(comment.created_at)}</time>
                  </header>
                  <p>{comment.body}</p>
                </article>
              {:else}
                <p class="muted">No comments yet. Add context for whoever picks this up next.</p>
              {/each}
            </div>
            <form
              class="comment-form"
              onsubmit={(event) => {
                event.preventDefault();
                const body = commentBody.trim();
                if (!body) return;
                commentBody = '';
                onComment(detail.todo.id, body);
              }}
            >
              <textarea bind:value={commentBody} rows="2" placeholder="Add a comment" aria-label="Add a todo comment"></textarea>
              <button type="submit" disabled={busy || !commentBody.trim()}>Comment</button>
            </form>
          </section>
        {/if}
      </aside>
    {/if}
  {/if}
</section>

<style>
  .todo-board {
    min-width: 0;
  }

  .board-heading,
  .board-heading > div,
  .column > header,
  .card-topline,
  .signals,
  .detail > .detail-copy > header,
  .comments > header,
  .comments article header {
    display: flex;
    align-items: center;
  }

  .board-heading {
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .board-heading > div { gap: 11px; }

  .board-actions { justify-content: flex-end; }

  .board-actions button,
  .empty-board button,
  .detail-actions > button,
  .comment-form button {
    border: 1px solid #4a4f57;
    border-radius: 3px;
    background: #25282d;
    color: #e1e3e6;
    font-size: 9px;
    font-weight: 650;
    cursor: pointer;
  }

  .board-actions button { display: flex; align-items: center; gap: 5px; padding: 5px 8px; }
  .board-actions button span, .empty-board button span { color: #a6acb4; font: 12px 'JetBrains Mono Variable', monospace; }
  .board-actions button:disabled,
  .empty-board button:disabled,
  .detail-actions > button:disabled,
  .comment-form button:disabled { cursor: not-allowed; opacity: 0.48; }

  .board-heading h3 {
    margin: 0;
    color: #eceef0;
    font-size: 17px;
    font-weight: 620;
  }

  .eyebrow,
  .count,
  .column > header,
  .priority,
  .todo-id,
  .tags,
  .signals,
  .detail-badges,
  .comments > header,
  .comments article header {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  .eyebrow {
    color: #858c95;
    font-size: 8px;
    font-weight: 650;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .count {
    color: #818892;
    font-size: 9px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .empty-board {
    display: grid;
    min-height: 240px;
    place-content: center;
    justify-items: center;
    border: 1px dashed #3b4047;
    border-radius: 4px;
    padding: 22px;
    background: #15171a;
    text-align: center;
  }

  .empty-mark { color: #9ca2aa; font-size: 24px; }
  .empty-board h4 { margin: 7px 0 0; color: #e6e8ea; font-size: 18px; }
  .empty-board p { max-width: 420px; margin: 7px 0 13px; color: #9198a1; font-size: 11px; line-height: 1.5; }
  .empty-board button { display: flex; align-items: center; gap: 6px; padding: 6px 9px; }

  .columns {
    display: grid;
    grid-template-columns: repeat(4, minmax(155px, 1fr));
    gap: 8px;
    overflow-x: auto;
    padding-bottom: 5px;
  }

  .column {
    min-width: 155px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: #17191c;
  }

  .column > header {
    gap: 7px;
    min-height: 30px;
    border-bottom: 1px solid var(--border);
    padding: 0 8px;
    color: #a0a6ae;
    font-size: 8px;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .column > header > span { color: #aab0b8; font-size: 11px; }
  .column > header strong { font-weight: 620; }
  .column > header small { margin-left: auto; color: #516873; }

  .cards {
    display: grid;
    align-content: start;
    gap: 4px;
    min-height: 115px;
    max-height: 250px;
    overflow-y: auto;
    padding: 5px;
    scrollbar-color: #2c4652 transparent;
    scrollbar-width: thin;
  }

  .todo-card {
    display: grid;
    gap: 5px;
    border: 1px solid #34383e;
    border-radius: 3px;
    padding: 7px;
    background: #1d2024;
    color: #c9cdd2;
    text-align: left;
    cursor: pointer;
  }

  .todo-card:hover { border-color: #555b63; }
  .todo-card.selected { border-color: #666d76; box-shadow: inset 2px 0 #7a818a; }
  .todo-card.blocked { background: rgb(112 48 48 / 18%); }
  .todo-card > strong { font-size: 11px; line-height: 1.32; }
  .card-topline { justify-content: space-between; }

  .priority,
  .todo-id {
    color: #76909a;
    font-size: 7px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .priority.high { color: #f18b80; }

  .tags,
  .signals {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .tags i {
    border: 1px solid #304a56;
    border-radius: 999px;
    padding: 2px 5px;
    color: #89a0a9;
    font-size: 6px;
    font-style: normal;
    letter-spacing: 0.04em;
  }

  .signals {
    min-height: 11px;
    color: #69828d;
    font-size: 7px;
  }

  .signals .blocker { color: #f18b80; }
  .signals .lock, .lock { color: #e0ad5d; }

  .empty-column {
    display: grid;
    place-items: center;
    min-height: 90px;
    color: #3f5863;
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 8px;
    text-transform: uppercase;
  }

  .empty-column span { color: #55717b; font-size: 18px; }

  .detail {
    display: grid;
    grid-template-columns: minmax(0, 1.6fr) minmax(220px, 0.8fr);
    margin-top: 10px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: #17191c;
  }

  .detail-copy { min-width: 0; padding: 11px 13px; }
  .detail > .detail-copy > header { justify-content: space-between; gap: 15px; }
  .detail-copy h4 { margin: 2px 0 0; color: #eceef0; font-size: 15px; }
  .detail-badges { display: flex; flex-wrap: wrap; gap: 8px; font-size: 7px; }
  .detail-actions { display: flex; align-items: center; gap: 9px; }
  .detail-actions > button { padding: 7px 9px; }
  .detail-actions > button.completed { border-color: #35515d; background: #10242d; color: #8da2aa; }

  .todo-body { margin-top: 10px; }

  .blocked-note {
    margin: 13px 0 0;
    border-left: 2px solid #d26863;
    padding: 6px 10px;
    background: rgb(115 45 49 / 16%);
    color: #e5a4a0;
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 9px;
  }

  .comments {
    min-width: 0;
    border-left: 1px solid var(--border);
  }

  .comments > header {
    justify-content: space-between;
    min-height: 34px;
    border-bottom: 1px solid var(--border);
    padding: 0 9px;
    color: #9aa0a8;
    font-size: 8px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .comment-list { max-height: 210px; overflow-y: auto; }
  .comments article { border-bottom: 1px solid #2d3136; padding: 8px 9px; }
  .comments article:last-child { border-bottom: 0; }
  .comments article header { justify-content: space-between; gap: 8px; font-size: 7px; }
  .comments article strong { overflow: hidden; color: #88d8cc; text-overflow: ellipsis; }
  .comments article time { flex: none; color: #536b76; }
  .comments article p { margin: 6px 0 0; color: #aebfc5; font-size: 10px; line-height: 1.45; white-space: pre-wrap; }
  .comment-list > .muted { padding: 12px; line-height: 1.5; }
  .comment-form { display: grid; gap: 6px; border-top: 1px solid var(--border); padding: 7px; }
  .comment-form textarea { width: 100%; resize: vertical; border: 1px solid #304b56; border-radius: 3px; padding: 8px; background: #081820; color: #d7e1e4; font-size: 10px; line-height: 1.4; outline: 0; }
  .comment-form textarea:focus { border-color: var(--signal); }
  .comment-form button { justify-self: end; padding: 7px 10px; }
  .muted, .detail-empty { color: #607680; font-size: 10px; }
  .detail-empty { grid-column: 1 / -1; padding: 16px; font-family: 'JetBrains Mono Variable', monospace; }

  @media (max-width: 980px) {
    .columns { grid-template-columns: repeat(4, minmax(190px, 1fr)); }
    .detail { grid-template-columns: 1fr; }
    .comments { border-top: 1px solid #29434e; border-left: 0; }
  }
</style>
