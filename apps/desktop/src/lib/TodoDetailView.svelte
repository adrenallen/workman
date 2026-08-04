<script lang="ts">
  import { onMount } from 'svelte';

  import MarkdownView from './MarkdownView.svelte';
  import type { TodoDetail } from './coordination';
  import {
    clampPanelWidth,
    loadPanelPreference,
    savePanelPreference,
    startPanelResize
  } from './panelPreferences';

  interface Props {
    detail: TodoDetail | null;
    loading: boolean;
    busy: boolean;
    onComplete: (completed: boolean) => void;
    onComment: (body: string) => void;
  }

  let { detail, loading, busy, onComplete, onComment }: Props = $props();
  const inspectorBounds = { min: 220, max: 460 };
  const collapsedInspectorWidth = 42;
  let inspectorWidth = $state(280);
  let inspectorCollapsed = $state(false);
  let commentBody = $state('');

  $effect(() => {
    detail?.todo.id;
    commentBody = '';
  });

  onMount(() => {
    const preference = loadPanelPreference(
      'todo-comments-inspector',
      { collapsed: false, width: inspectorWidth },
      inspectorBounds.min,
      inspectorBounds.max
    );
    inspectorWidth = preference.width;
    inspectorCollapsed = preference.collapsed;
  });

  function persistInspector(): void {
    savePanelPreference('todo-comments-inspector', {
      collapsed: inspectorCollapsed,
      width: inspectorWidth
    });
  }

  function toggleInspector(): void {
    inspectorCollapsed = !inspectorCollapsed;
    persistInspector();
  }

  function handleShortcut(event: KeyboardEvent): void {
    const target = event.target as HTMLElement | null;
    if (
      !event.metaKey || !event.shiftKey || event.altKey || event.key.toLowerCase() !== 'i' ||
      target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target?.isContentEditable
    ) return;
    event.preventDefault();
    toggleInspector();
  }

  function resizeFromKeyboard(event: KeyboardEvent): void {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    inspectorWidth = clampPanelWidth(
      inspectorWidth + (event.key === 'ArrowLeft' ? 12 : -12),
      inspectorBounds.min,
      inspectorBounds.max
    );
    persistInspector();
  }

  function submitComment(): void {
    const body = commentBody.trim();
    if (!body) return;
    commentBody = '';
    onComment(body);
  }

  function formatTime(epochMillis: number): string {
    return new Intl.DateTimeFormat(undefined, {
      month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit'
    }).format(new Date(epochMillis));
  }
</script>

<svelte:window onkeydown={handleShortcut} />

{#if loading && !detail}
  <div class="loading">Loading todo…</div>
{:else if detail}
  <section
    class="todo-detail"
    style={`--todo-inspector-width: ${inspectorCollapsed ? collapsedInspectorWidth : inspectorWidth}px;`}
  >
    <article class="todo-document">
      <header class="todo-actions">
        <div class="badges">
          <span class:high={detail.todo.priority === 'high'}>{detail.todo.priority}</span>
          <span>{detail.todo.status.replace('_', ' ')}</span>
          {#if detail.todo.locked_by}<span>locked · {detail.todo.locked_by}</span>{/if}
        </div>
        <button
          type="button"
          disabled={busy}
          onclick={() => onComplete(!detail.todo.completed)}
        >{detail.todo.completed ? 'Reopen' : 'Complete'}</button>
      </header>

      {#if detail.todo.blocker_ids.length > 0}
        <div class:blocked={detail.todo.is_blocked} class="blockers">
          <strong>{detail.todo.is_blocked ? 'Blocked by' : 'Dependencies'}</strong>
          {detail.todo.blocker_ids.map((id) => `#${id}`).join(', ')}
          {#if detail.todo.is_blocked} · {detail.todo.unresolved_blocker_count} open{/if}
        </div>
      {/if}

      <div class="todo-body">
        {#if detail.todo.body.trim()}
          <MarkdownView source={detail.todo.body} />
        {:else}
          <p>No notes for this todo.</p>
        {/if}
      </div>
    </article>

    <aside class="comments" class:collapsed={inspectorCollapsed} aria-label="Todo comments">
      <header>
        <span>Comments</span>
        <div>
          <small>{detail.comment_total_count}</small>
          <button
            type="button"
            aria-label={`${inspectorCollapsed ? 'Expand' : 'Collapse'} comments`}
            title={`${inspectorCollapsed ? 'Expand' : 'Collapse'} comments (⌘⇧I)`}
            onclick={toggleInspector}
          >{inspectorCollapsed ? '‹' : '›'}</button>
        </div>
      </header>
      <div class="comment-list">
        {#each detail.comments as comment (comment.id)}
          <article>
            <header><strong>{comment.actor}</strong><time>{formatTime(comment.created_at)}</time></header>
            <p>{comment.body}</p>
          </article>
        {:else}
          <p class="muted">No comments yet.</p>
        {/each}
      </div>
      <form onsubmit={(event) => { event.preventDefault(); submitComment(); }}>
        <textarea bind:value={commentBody} rows="2" placeholder="Add a comment" aria-label="Add a todo comment"></textarea>
        <button type="submit" disabled={busy || !commentBody.trim()}>Comment</button>
      </form>
      {#if !inspectorCollapsed}
        <button
          type="button"
          class="resize-handle"
          aria-label="Resize comments"
          title={`Resize comments · ${inspectorWidth}px · arrow keys`}
          onkeydown={resizeFromKeyboard}
          onpointerdown={(event) =>
            startPanelResize(event, {
              current: inspectorWidth,
              direction: -1,
              min: inspectorBounds.min,
              max: inspectorBounds.max,
              onResize: (width) => (inspectorWidth = width),
              onEnd: persistInspector
            })}
        ></button>
      {/if}
    </aside>
  </section>
{:else}
  <div class="loading">Todo not found.</div>
{/if}

<style>
  .todo-detail { display: grid; width: 100%; height: 100%; min-width: 0; grid-template-columns: minmax(0, 1fr) var(--todo-inspector-width); background: var(--night); }
  .todo-document { min-width: 0; overflow: auto; scrollbar-color: #41464d transparent; scrollbar-width: thin; }
  .todo-actions { display: flex; min-height: 38px; align-items: center; justify-content: space-between; gap: 10px; border-bottom: 1px solid var(--border); padding: 5px 10px; }
  .badges { display: flex; min-width: 0; flex-wrap: wrap; gap: 5px; }
  .badges span { border: 1px solid #3a3f46; border-radius: 3px; padding: 2px 5px; color: #9ba1aa; background: #1d2024; font: 7px 'JetBrains Mono Variable', monospace; text-transform: uppercase; }
  .badges span.high { border-color: rgb(220 107 107 / 52%); color: #dc8a8a; }
  .todo-actions > button, form > button { border: 1px solid #4a4f57; border-radius: 3px; padding: 5px 8px; background: #25282d; color: #e1e3e6; font-size: 9px; font-weight: 650; cursor: pointer; }
  button:disabled { cursor: not-allowed; opacity: 0.45; }
  .blockers { margin: 9px 12px 0; border-left: 2px solid #6e747d; padding: 6px 8px; background: #1a1d21; color: #a8adb5; font-size: 10px; }
  .blockers.blocked { border-color: var(--warning); color: #d9bd8c; }
  .blockers strong { margin-right: 5px; }
  .todo-body { max-width: 840px; padding: 14px 18px 28px; }
  .todo-body > p { color: var(--muted); font-size: 11px; }

  .comments { position: relative; min-width: 0; border-left: 1px solid var(--border); background: #15171a; }
  .comments > header { display: flex; min-height: 34px; align-items: center; justify-content: space-between; border-bottom: 1px solid var(--border); padding: 4px 6px 4px 9px; color: #a5abb3; font-size: 9px; font-weight: 650; }
  .comments > header > div { display: flex; align-items: center; gap: 5px; }
  .comments > header small { color: var(--muted); font: 8px 'JetBrains Mono Variable', monospace; }
  .comments > header button { display: grid; width: 22px; height: 22px; place-items: center; border: 1px solid #3b4047; border-radius: 3px; background: #1d2024; color: #a3a9b1; cursor: pointer; }
  .comment-list { max-height: calc(100% - 118px); overflow-y: auto; }
  .comment-list article { border-bottom: 1px solid #292d32; padding: 8px 9px; }
  .comment-list article header { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 8px; }
  .comment-list article strong { overflow: hidden; color: #c8ccd1; text-overflow: ellipsis; white-space: nowrap; }
  .comment-list article time { flex: none; color: var(--muted); font: 7px 'JetBrains Mono Variable', monospace; }
  .comment-list article p, .muted { margin: 4px 0 0; color: #aeb3ba; font-size: 10px; line-height: 1.45; white-space: pre-wrap; }
  .muted { padding: 9px; }
  form { position: absolute; right: 0; bottom: 0; left: 0; display: grid; gap: 5px; border-top: 1px solid var(--border); padding: 6px; background: #15171a; }
  textarea { width: 100%; resize: none; border: 1px solid #3b4047; border-radius: 3px; outline: 0; padding: 6px 7px; background: #111315; color: var(--text); font-size: 10px; line-height: 1.35; }
  form > button { justify-self: end; }
  .resize-handle { position: absolute; z-index: 5; top: 0; bottom: 0; left: -3px; width: 6px; border: 0; padding: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .resize-handle::after { position: absolute; top: 0; bottom: 0; left: 2px; width: 1px; background: transparent; content: ''; }
  .resize-handle:hover::after, .resize-handle:focus-visible::after { background: #7a818a; }
  .comments.collapsed > header { height: 100%; min-height: 120px; align-items: center; flex-direction: column; justify-content: flex-start; gap: 8px; padding: 8px 0; }
  .comments.collapsed > header > span, .comments.collapsed .comment-list, .comments.collapsed form { display: none; }
  .comments.collapsed > header > div { flex-direction: column-reverse; }
  .loading { display: grid; width: 100%; height: 100%; place-items: center; color: var(--muted); font-size: 10px; }
</style>
