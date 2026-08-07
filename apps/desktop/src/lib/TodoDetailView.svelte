<script lang="ts">
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import CheckIcon from '@lucide/svelte/icons/check';
  import CirclePlusIcon from '@lucide/svelte/icons/circle-plus';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import LockIcon from '@lucide/svelte/icons/lock';
  import MessageSquareIcon from '@lucide/svelte/icons/message-square';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
  import TagIcon from '@lucide/svelte/icons/tag';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import UnlockIcon from '@lucide/svelte/icons/unlock';
  import UserRoundCheckIcon from '@lucide/svelte/icons/user-round-check';
  import { tick } from 'svelte';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import AgentStatusIndicator from '$lib/components/ds/AgentStatusIndicator.svelte';
  import TodoStatusIndicator from '$lib/components/ds/TodoStatusIndicator.svelte';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import * as Popover from '$lib/components/ui/popover';

  import DocumentScaffold from './DocumentScaffold.svelte';
  import TodoBlockerPicker from './TodoBlockerPicker.svelte';
  import type {
    TodoActivity,
    TodoComment,
    TodoDetail,
    TodoPriority,
    TodoStatus,
    TodoSummary,
    UpdateTodoInput
  } from './coordination';
  import type { ProcessView } from './daemon';
  import { submitOnEnter } from './formInputConventions';
  import LiveMarkdownEditor from './LiveMarkdownEditor.svelte';
  import MarkdownView from './MarkdownView.svelte';
  import { todoClaimLabel, todoClaimState } from './todoPresentation';
  import { resolveTodoClaimant } from './todoClaimant';

  interface ProjectOption {
    id: number;
    name: string;
  }

  interface Props {
    detail: TodoDetail | null;
    loading: boolean;
    busy: boolean;
    projectName?: string;
    todos?: TodoSummary[];
    navigationIds?: number[];
    projectOptions?: ProjectOption[];
    processes?: ProcessView[];
    focusCommentId?: number | null;
    onBack?: () => void;
    onNavigateTodo?: (todoId: number) => void;
    onNavigateClaimant?: (processId: number) => void;
    onUpdate?: (update: UpdateTodoInput) => Promise<void> | void;
    onComplete: (completed: boolean) => void;
    onComment: (body: string) => void;
    onLock?: (locked: boolean) => Promise<void> | void;
    onSetBlockers?: (blockerIds: number[]) => Promise<void> | void;
    onDelete?: () => Promise<void> | void;
    onTransfer?: (projectId: number) => Promise<void> | void;
  }

  let {
    detail,
    loading,
    busy,
    projectName = 'Project',
    todos = [],
    navigationIds = [],
    projectOptions = [],
    processes = [],
    focusCommentId = null,
    onBack,
    onNavigateTodo,
    onNavigateClaimant,
    onUpdate = () => {},
    onComplete,
    onComment,
    onLock,
    onSetBlockers,
    onDelete,
    onTransfer
  }: Props = $props();

  let activeId = $state<number | null>(null);
  let titleDraft = $state('');
  let bodyDraft = $state('');
  let tagsDraft = $state('');
  let tagsOpen = $state(false);
  let commentBody = $state('');
  let titleInput = $state<HTMLInputElement | null>(null);
  let activitySection = $state<HTMLElement | null>(null);
  let bodyFocusRequest = $state(0);
  let bodySaveTimer: ReturnType<typeof setTimeout> | null = null;
  let focusedCommentKey = $state<string | null>(null);

  let currentIndex = $derived(detail ? navigationIds.indexOf(detail.todo.id) : -1);
  let previousId = $derived(currentIndex > 0 ? navigationIds[currentIndex - 1] : null);
  let nextId = $derived(
    currentIndex >= 0 && currentIndex < navigationIds.length - 1
      ? navigationIds[currentIndex + 1]
      : null
  );
  let blockingTodos = $derived(
    detail ? todos.filter((todo) => todo.blocker_ids.includes(detail!.todo.id)) : []
  );
  let claimant = $derived(detail ? resolveTodoClaimant(detail.todo, processes) : null);
  let activityItems = $derived.by(() => {
    if (!detail) return [] as Array<
      | { type: 'event'; timestamp: number; event: TodoActivity }
      | { type: 'comment'; timestamp: number; comment: TodoComment }
    >;
    return [
      ...detail.activity.map((event) => ({ type: 'event' as const, timestamp: event.created_at, event })),
      ...detail.comments.map((comment) => ({ type: 'comment' as const, timestamp: comment.created_at, comment }))
    ].sort((left, right) => left.timestamp - right.timestamp);
  });

  $effect(() => {
    const todo = detail?.todo;
    if (!todo || activeId === todo.id) return;
    activeId = todo.id;
    titleDraft = todo.title;
    bodyDraft = todo.body;
    tagsDraft = todo.tags.join(', ');
    tagsOpen = false;
    commentBody = '';
  });

  $effect(() => {
    const todoId = detail?.todo.id;
    const commentId = focusCommentId;
    if (
      todoId === undefined ||
      commentId === null ||
      !detail?.comments.some((comment) => comment.id === commentId)
    ) return;
    const key = `${todoId}:${commentId}`;
    if (focusedCommentKey === key) return;
    focusedCommentKey = key;
    void tick().then(() => {
      const comment = document.getElementById(`todo-comment-${commentId}`);
      comment?.scrollIntoView({ behavior: 'smooth', block: 'center' });
      comment?.focus({ preventScroll: true });
    });
  });

  $effect(() => () => {
    if (bodySaveTimer !== null) clearTimeout(bodySaveTimer);
  });

  function statusLabel(status: TodoStatus): string {
    if (status === 'in_progress') return 'In progress';
    return status.charAt(0).toUpperCase() + status.slice(1);
  }

  function relativeTime(epochMillis: number): string {
    const seconds = Math.round((epochMillis - Date.now()) / 1_000);
    const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
    if (Math.abs(seconds) < 60) return formatter.format(seconds, 'second');
    const minutes = Math.round(seconds / 60);
    if (Math.abs(minutes) < 60) return formatter.format(minutes, 'minute');
    const hours = Math.round(minutes / 60);
    if (Math.abs(hours) < 24) return formatter.format(hours, 'hour');
    return formatter.format(Math.round(hours / 24), 'day');
  }

  function exactTime(epochMillis: number): string {
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short'
    }).format(new Date(epochMillis));
  }

  function eventCopy(event: TodoActivity): string {
    if (event.kind === 'created') return 'created this todo';
    if (event.kind === 'completed') return 'completed this todo';
    if (event.kind === 'reopened') return 'reopened this todo';
    if (event.kind === 'locked') return 'claimed this todo';
    return 'released the claim';
  }

  function eventIcon(kind: TodoActivity['kind']) {
    if (kind === 'created') return CirclePlusIcon;
    if (kind === 'completed') return CheckIcon;
    if (kind === 'reopened') return RotateCcwIcon;
    if (kind === 'locked') return LockIcon;
    return UnlockIcon;
  }

  function focusEditor(): void {
    bodyFocusRequest += 1;
  }

  function saveTitle(): void {
    if (!detail) return;
    const title = titleDraft.trim();
    if (!title) {
      titleDraft = detail.todo.title;
      return;
    }
    if (title !== detail.todo.title) void onUpdate({ title });
  }

  function handleTitleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      titleInput?.blur();
    } else if (event.key === 'Escape' && detail) {
      event.preventDefault();
      titleDraft = detail.todo.title;
      titleInput?.blur();
    }
  }

  function scheduleBodySave(): void {
    if (bodySaveTimer !== null) clearTimeout(bodySaveTimer);
    bodySaveTimer = setTimeout(() => {
      bodySaveTimer = null;
      saveBody();
    }, 700);
  }

  function changeBody(next: string): void {
    bodyDraft = next;
    scheduleBodySave();
  }

  function saveBody(): void {
    if (!detail || bodyDraft === detail.todo.body) return;
    void onUpdate({ body: bodyDraft });
  }

  function saveTags(): void {
    if (!detail) return;
    const tags = [...new Set(tagsDraft.split(',').map((tag) => tag.trim()).filter(Boolean))];
    tagsDraft = tags.join(', ');
    tagsOpen = false;
    if (tags.join('\u0000') !== detail.todo.tags.join('\u0000')) void onUpdate({ tags });
  }

  function submitComment(): void {
    const body = commentBody.trim();
    if (!body) return;
    commentBody = '';
    onComment(body);
  }

  function navigate(todoId: number | null): void {
    if (todoId !== null) onNavigateTodo?.(todoId);
  }

  function jumpToActivity(): void {
    activitySection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
</script>

{#if loading && !detail}
  <div class="state">Loading todo…</div>
{:else if detail}
  <DocumentScaffold
    ariaLabel={`Todo #${detail.todo.id}`}
    breadcrumbRoot={projectName}
    breadcrumbCurrent={detail.todo.title}
    reference={`#${detail.todo.id}`}
    previousDisabled={previousId === null || busy}
    nextDisabled={nextId === null || busy}
    onBack={onBack}
    onPrevious={() => navigate(previousId)}
    onNext={() => navigate(nextId)}
    onCopyReference={() => void navigator.clipboard.writeText(`#${detail.todo.id}`)}
  >
    {#snippet actions()}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <IconButton {...props} label="Todo actions">
              {#snippet icon()}<EllipsisIcon size={16} strokeWidth={1.8} />{/snippet}
            </IconButton>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" class="w-56">
          <DropdownMenu.Label>Todo #{detail.todo.id}</DropdownMenu.Label>
          <DropdownMenu.Separator />
          <DropdownMenu.Item onclick={() => { titleInput?.focus(); titleInput?.select(); }}>
            <PencilIcon class="size-4" aria-hidden="true" /> Edit title
          </DropdownMenu.Item>
          <DropdownMenu.Item onclick={focusEditor}>
            <PencilIcon class="size-4" aria-hidden="true" /> Edit body
          </DropdownMenu.Item>
          <DropdownMenu.Item disabled={busy} onclick={() => onComplete(!detail!.todo.completed)}>
            {#if detail.todo.completed}<RotateCcwIcon class="size-4" aria-hidden="true" /> Reopen{:else}<CheckIcon class="size-4" aria-hidden="true" /> Complete{/if}
          </DropdownMenu.Item>
          {#if onLock}
            <DropdownMenu.Item disabled={busy || Boolean(detail.todo.locked_by && detail.todo.locked_by !== 'desktop-ui')} onclick={() => void onLock(!detail!.todo.locked_by)}>
              {#if detail.todo.locked_by}<UnlockIcon class="size-4" aria-hidden="true" /> Release claim{:else}<LockIcon class="size-4" aria-hidden="true" /> Claim for desktop{/if}
            </DropdownMenu.Item>
          {/if}
          {#if onTransfer && projectOptions.length > 0}
            <DropdownMenu.Sub>
              <DropdownMenu.SubTrigger><ArchiveIcon class="size-4" aria-hidden="true" /> Transfer to</DropdownMenu.SubTrigger>
              <DropdownMenu.SubContent>
                {#each projectOptions as project (project.id)}
                  <DropdownMenu.Item onclick={() => void onTransfer?.(project.id)}>{project.name}</DropdownMenu.Item>
                {/each}
              </DropdownMenu.SubContent>
            </DropdownMenu.Sub>
          {/if}
          {#if onDelete}
            <DropdownMenu.Separator />
            <DropdownMenu.Item variant="destructive" disabled={busy} onclick={() => void onDelete()}>
              <Trash2Icon class="size-4" aria-hidden="true" /> Delete todo
            </DropdownMenu.Item>
          {/if}
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    {/snippet}

    {#snippet rail()}
      <section class="activity-rail">
        <span>Discussion</span>
        <h2>{activityItems.length} item{activityItems.length === 1 ? '' : 's'}</h2>
        <button type="button" onclick={jumpToActivity}>
          <MessageSquareIcon size={15} strokeWidth={1.8} aria-hidden="true" />
          <span><strong>Activity</strong><small>Jump to comments</small></span>
        </button>
      </section>
    {/snippet}

    <article class="todo-document">
      <input
        class="title"
        bind:this={titleInput}
        bind:value={titleDraft}
        aria-label="Todo title"
        disabled={busy}
        onblur={saveTitle}
        onkeydown={handleTitleKeydown}
      />

      <div class="metadata" aria-label="Todo metadata">
        <label class="metadata-chip status-chip" title={todoClaimLabel(detail.todo)}>
          <TodoStatusIndicator state={todoClaimState(detail.todo)} label={todoClaimLabel(detail.todo)} />
          <select
            aria-label="Todo status"
            value={detail.todo.status}
            disabled={busy}
            onchange={(event) => void onUpdate({ status: event.currentTarget.value as TodoStatus })}
          >
            <option value="backlog">Backlog</option>
            <option value="open">Open</option>
            <option value="in_progress">In progress</option>
            <option value="completed">Completed</option>
          </select>
        </label>

        <label class={`metadata-chip priority-chip ${detail.todo.priority}`}>
          <span aria-hidden="true"></span>
          <select
            aria-label="Todo priority"
            value={detail.todo.priority}
            disabled={busy}
            onchange={(event) => void onUpdate({ priority: event.currentTarget.value as TodoPriority })}
          >
            <option value="high">High priority</option>
            <option value="medium">Medium priority</option>
            <option value="low">Low priority</option>
          </select>
        </label>

        {#if claimant?.process}
          <button
            class="metadata-chip claimant-chip"
            type="button"
            aria-label={`Claimed by ${claimant.name}. Jump to ${claimant.name}`}
            title={`Jump to ${claimant.name}`}
            onclick={() => onNavigateClaimant?.(claimant!.process!.id)}
          >
            <strong>{claimant.name}</strong>
            <AgentStatusIndicator process={claimant.process} showLabel />
          </button>
        {:else if claimant}
          <span
            class="metadata-chip claimant-chip external"
            aria-label={`Claimed by ${claimant.name}`}
            title="This claimant is not attached to a Workman process"
          >
            <LockIcon size={13} strokeWidth={1.8} aria-hidden="true" />
            <strong>{claimant.name}</strong>
          </span>
        {/if}

        {#if detail.todo.assignee === 'user'}
          <span class="metadata-chip assignment-chip" aria-label="Assigned to you" title="Assigned to you by an agent or orchestrator">
            <UserRoundCheckIcon size={13} strokeWidth={1.8} aria-hidden="true" />
            <strong>Assigned to you</strong>
          </span>
        {/if}

        <button class="metadata-chip" type="button" aria-expanded={tagsOpen} onclick={() => (tagsOpen = !tagsOpen)}>
          <TagIcon size={13} strokeWidth={1.8} aria-hidden="true" />
          {detail.todo.tags.length} tag{detail.todo.tags.length === 1 ? '' : 's'}
        </button>

        <Popover.Root>
          <Popover.Trigger>
            {#snippet child({ props })}
              <button {...props} class:blocked={detail.todo.is_blocked} class="metadata-chip" type="button">
                Blocked by {detail.todo.blocker_ids.length}
              </button>
            {/snippet}
          </Popover.Trigger>
          <Popover.Content align="start" class="w-96 gap-1 p-2">
            <TodoBlockerPicker
              {todos}
              selectedIds={detail.todo.blocker_ids}
              currentTodoId={detail.todo.id}
              disabled={busy || !onSetBlockers}
              onChange={(blockerIds) => onSetBlockers?.(blockerIds)}
              onNavigate={(todoId) => navigate(todoId)}
            />
          </Popover.Content>
        </Popover.Root>

        <Popover.Root>
          <Popover.Trigger>
            {#snippet child({ props })}
              <button {...props} class="metadata-chip" type="button" disabled={blockingTodos.length === 0}>Unblocks {blockingTodos.length}</button>
            {/snippet}
          </Popover.Trigger>
          <Popover.Content align="start" class="w-80 gap-1 p-1.5">
            <Popover.Header class="px-2 py-1"><Popover.Title>Unblocks</Popover.Title><Popover.Description>Todos waiting on this one.</Popover.Description></Popover.Header>
            {#each blockingTodos as todo (todo.id)}
              <button class:resolved={todo.completed} class="relation-row" type="button" onclick={() => navigate(todo.id)}>
                <TodoStatusIndicator state={todoClaimState(todo)} label={todoClaimLabel(todo)} />
                <span>#{todo.id}</span><strong>{todo.title}</strong><small>{todo.completed ? 'Resolved' : statusLabel(todo.status)}</small>
              </button>
            {/each}
          </Popover.Content>
        </Popover.Root>
      </div>

      {#if tagsOpen}
        <form class="tags-editor" onsubmit={(event) => { event.preventDefault(); saveTags(); }}>
          <label for="todo-tags">Tags</label>
          <input id="todo-tags" bind:value={tagsDraft} placeholder="feedback, ui, follow-up" />
          <button type="button" onclick={() => { tagsDraft = detail!.todo.tags.join(', '); tagsOpen = false; }}>Cancel</button>
          <button class="primary" type="submit" disabled={busy}>Save tags</button>
        </form>
      {/if}

      <button class="activity-jump-inline" type="button" onclick={jumpToActivity}>
        <MessageSquareIcon size={14} strokeWidth={1.8} aria-hidden="true" />
        Jump to activity
        <small>{activityItems.length}</small>
      </button>

      <section class="body-section" aria-label="Todo body">
        <LiveMarkdownEditor
          value={bodyDraft}
          focusRequest={bodyFocusRequest}
          flow
          onChange={changeBody}
          onSave={saveBody}
        />
      </section>

      <div class="tag-list" aria-label="Todo tags">
        {#each detail.todo.tags as tag (tag)}<button type="button" onclick={() => (tagsOpen = true)}>{tag}</button>{/each}
        {#if detail.todo.tags.length === 0}<button class="add-tag" type="button" onclick={() => (tagsOpen = true)}>+ Add tags</button>{/if}
      </div>

      <section class="activity" bind:this={activitySection} aria-labelledby="activity-title">
        <header>
          <div><span>History and discussion</span><h2 id="activity-title">Activity</h2></div>
          <small>{activityItems.length} item{activityItems.length === 1 ? '' : 's'}</small>
        </header>

        <div class="activity-feed">
          {#each activityItems as item (`${item.type}-${item.type === 'event' ? item.event.id : item.comment.id}`)}
            {#if item.type === 'event'}
              {@const EventIcon = eventIcon(item.event.kind)}
              <article class="event-row">
                <span class="event-icon"><EventIcon size={14} strokeWidth={1.8} aria-hidden="true" /></span>
                <p><strong>{item.event.actor}</strong> {eventCopy(item.event)}</p>
                <time title={exactTime(item.timestamp)}>{relativeTime(item.timestamp)}</time>
              </article>
            {:else}
              <article
                id={`todo-comment-${item.comment.id}`}
                class:comment-focus-target={focusCommentId === item.comment.id}
                class="comment-row"
                tabindex="-1"
              >
                <header><strong>{item.comment.actor}</strong><time title={exactTime(item.timestamp)}>{relativeTime(item.timestamp)}</time></header>
                <MarkdownView source={item.comment.body} />
              </article>
            {/if}
          {:else}
            <p class="empty-activity">No activity yet. Add context for the next person below.</p>
          {/each}
        </div>

        <form class="comment-composer" onsubmit={(event) => { event.preventDefault(); submitComment(); }}>
          <label for="todo-comment">Add to the activity</label>
          <textarea id="todo-comment" bind:value={commentBody} rows="3" placeholder="Write a comment in Markdown…" use:submitOnEnter></textarea>
          <div><small>Enter posts · Shift+Enter adds a line</small><button class="primary" type="submit" disabled={busy || !commentBody.trim()}>Comment</button></div>
        </form>
      </section>
    </article>
  </DocumentScaffold>
{:else}
  <div class="state">Todo not found.</div>
{/if}

<style>
  .todo-document { min-width: 0; min-height: 100%; }
  .title { width: 100%; border: 0; border-radius: var(--radius); outline: 0; padding: 2px 4px 5px; background: transparent; color: var(--foreground); font: 680 clamp(25px, 3.1cqw, 34px)/1.16 var(--ui-font-family); letter-spacing: -0.025em; }
  .title:hover { background: var(--card); }
  .title:focus { background: var(--card); box-shadow: 0 0 0 2px var(--ring); }
  .metadata { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 12px; }
  .metadata-chip { display: inline-flex; min-height: 28px; align-items: center; gap: 6px; border: 1px solid var(--border); border-radius: 999px; padding: 0 8px; background: var(--card); color: var(--text-soft); font-size: var(--font-size-xs); cursor: pointer; }
  .metadata-chip:disabled { cursor: default; opacity: 0.68; }
  .metadata-chip.blocked { border-color: color-mix(in srgb, var(--destructive) 45%, var(--border)); color: var(--destructive); }
  .claimant-chip { border-color: color-mix(in srgb, var(--todo-state-claimed) 38%, var(--border)); }
  .claimant-chip strong { overflow: hidden; max-width: 150px; color: var(--todo-state-claimed); font-size: inherit; font-weight: 680; text-overflow: ellipsis; white-space: nowrap; }
  .claimant-chip.external { cursor: default; }
  .assignment-chip { cursor: default; }
  .assignment-chip strong { font-size: inherit; font-weight: 650; }
  .metadata-chip select { max-width: 132px; border: 0; outline: 0; background: transparent; color: inherit; font: inherit; cursor: pointer; }
  .priority-chip > span { width: 7px; height: 7px; border-radius: 2px; background: var(--muted-foreground); }
  .priority-chip.high { color: var(--destructive); }
  .priority-chip.high > span { background: var(--destructive); }
  .priority-chip.medium { color: var(--warning-token); }
  .priority-chip.medium > span { background: var(--warning-token); }
  .relation-row { display: grid; width: 100%; min-height: 34px; grid-template-columns: 14px 42px minmax(0, 1fr) auto; align-items: center; gap: 6px; border: 0; border-radius: var(--radius); padding: 3px 7px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .relation-row:hover { background: var(--accent); }
  .relation-row.resolved { opacity: 0.65; }
  .relation-row span { color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .relation-row strong { overflow: hidden; font-size: var(--font-size-sm); font-weight: 590; text-overflow: ellipsis; white-space: nowrap; }
  .relation-row small { color: var(--muted-foreground); font-size: 10px; }
  .tags-editor { display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 6px; margin-top: 10px; border: 1px solid var(--border); border-radius: var(--radius); padding: 6px; background: var(--card); }
  .tags-editor label { padding-left: 4px; color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 650; }
  .tags-editor input { min-width: 0; height: 29px; border: 1px solid var(--input); border-radius: var(--radius); outline: 0; padding: 0 8px; background: var(--background); color: var(--foreground); font-size: var(--font-size-sm); }
  .tags-editor button, .comment-composer button { min-height: 29px; border: 1px solid var(--input); border-radius: var(--radius); padding: 0 9px; background: var(--card); color: var(--text-soft); font-size: var(--font-size-sm); cursor: pointer; }
  button.primary { border-color: var(--primary); background: var(--primary); color: var(--primary-foreground); font-weight: 650; }
  .activity-jump-inline { display: none; min-height: 28px; align-items: center; gap: 6px; margin: 12px 0 -5px auto; border: 0; border-radius: var(--radius); padding: 0 5px; background: transparent; color: var(--muted-foreground); font-size: var(--font-size-xs); cursor: pointer; }
  .activity-jump-inline:hover { background: var(--card); color: var(--foreground); }
  .activity-jump-inline small { min-width: 18px; border: 1px solid var(--border); border-radius: 999px; padding: 1px 5px; font: 10px var(--terminal-font-family); text-align: center; }
  .body-section { margin-top: 22px; overflow: visible; border-top: 1px solid var(--border); background: transparent; }
  .tag-list { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 9px; }
  .tag-list button { min-height: 24px; border: 1px solid var(--border); border-radius: 999px; padding: 0 7px; background: var(--card); color: var(--muted-foreground); font-size: var(--font-size-xs); cursor: pointer; }
  .tag-list .add-tag { border-style: dashed; background: transparent; }
  .activity { margin-top: 40px; border-top: 1px solid var(--border); padding-top: 22px; }
  .activity > header { display: flex; align-items: end; justify-content: space-between; gap: var(--space-2); }
  .activity > header span { color: var(--muted-foreground); font: 650 var(--font-size-xs)/1 var(--terminal-font-family); letter-spacing: 0.055em; text-transform: uppercase; }
  .activity h2 { margin: 4px 0 0; color: var(--foreground); font-size: 20px; line-height: 1.2; }
  .activity > header small { color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .activity-feed { display: grid; margin-top: 12px; }
  .event-row { display: grid; min-height: 38px; grid-template-columns: 26px minmax(0, 1fr) auto; align-items: center; gap: 7px; border-bottom: 1px solid var(--border); color: var(--muted-foreground); }
  .event-row p { margin: 0; font-size: var(--font-size-sm); }
  .event-row strong { color: var(--text-soft); font-weight: 620; }
  .event-icon { display: grid; width: 24px; height: 24px; place-items: center; border: 1px solid var(--border); border-radius: 999px; color: var(--muted-foreground); background: var(--card); }
  .event-row time, .comment-row time { color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .comment-row { margin: 0; border-bottom: 1px solid var(--border); padding: 10px 0 11px 32px; background: transparent; }
  .comment-row.comment-focus-target { box-shadow: inset 2px 0 0 var(--ring); }
  .comment-row:focus { outline: 0; }
  .comment-row > header { display: flex; min-height: 24px; align-items: center; justify-content: space-between; gap: 8px; }
  .comment-row > header strong { font-size: var(--font-size-sm); font-weight: 650; }
  .comment-row :global(.markdown) { padding: 4px 0 0; }
  .empty-activity { margin: 0; border: 1px dashed var(--border); border-radius: var(--radius); padding: 13px; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .comment-composer { display: grid; gap: 6px; margin-top: 14px; border: 1px solid var(--border); border-radius: var(--radius); padding: 9px; background: var(--card); }
  .comment-composer label { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 650; }
  .comment-composer textarea { width: 100%; max-height: 156px; resize: none; border: 1px solid var(--input); border-radius: var(--radius); outline: 0; padding: 8px 9px; background: var(--background); color: var(--foreground); font-size: var(--font-size-sm); line-height: 1.45; }
  .comment-composer > div { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .comment-composer small { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .state { display: grid; width: 100%; height: 100%; place-items: center; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .activity-rail { position: sticky; top: 18px; }
  .activity-rail > span { color: var(--muted-foreground); font: 650 var(--font-size-xs)/1 var(--terminal-font-family); letter-spacing: 0.055em; text-transform: uppercase; }
  .activity-rail h2 { margin: 5px 0 12px; color: var(--foreground); font-size: var(--font-size-base); line-height: 1.2; }
  .activity-rail button { display: flex; width: 100%; min-height: 44px; align-items: center; gap: 8px; border: 0; border-left: 2px solid var(--border); border-radius: 0 var(--radius) var(--radius) 0; padding: 5px 8px; background: transparent; color: var(--muted-foreground); text-align: left; cursor: pointer; }
  .activity-rail button:hover { border-left-color: var(--foreground); background: var(--card); color: var(--foreground); }
  .activity-rail button > span { display: grid; gap: 2px; }
  .activity-rail strong { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 630; }
  .activity-rail small { color: var(--muted-foreground); font-size: var(--font-size-xs); }

  @container (max-width: 880px) {
    .activity-jump-inline { display: flex; }
  }

  @container (max-width: 620px) {
    .metadata { gap: 4px; }
    .metadata-chip { max-width: 100%; }
    .tags-editor { grid-template-columns: minmax(0, 1fr) auto auto; }
    .tags-editor label { grid-column: 1 / -1; }
    .body-section { margin-top: 16px; }
    .activity { margin-top: 30px; }
    .comment-row { padding-left: 0; }
  }
</style>
