<script lang="ts">
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import MessageSquareIcon from '@lucide/svelte/icons/message-square';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import TagIcon from '@lucide/svelte/icons/tag';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import { Button } from '$lib/components/ui/button';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import * as Collapsible from '$lib/components/ui/collapsible';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { Textarea } from '$lib/components/ui/textarea';

  import type {
    NewScratchpadCommentInput,
    ScratchpadComment,
    ScratchpadRead
  } from './coordination';
  import DocumentScaffold from './DocumentScaffold.svelte';
  import CountBadge from './CountBadge.svelte';
  import LiveMarkdownEditor from './LiveMarkdownEditor.svelte';
  import { scratchpadOutline, type ScratchpadOutlineItem } from './scratchpadOutline';
  import {
    mapScratchpadSelectionAnchor,
    resolveScratchpadAnchor,
    selectionAnchor,
    type PositionMapper,
    type ScratchpadSelectionAnchor
  } from './scratchpadAnchors';
  import { autoGrowTextarea, singleLineTitle } from './wrappingTitle';

  interface Props {
    read: ScratchpadRead | null;
    loading: boolean;
    busy?: boolean;
    projectName?: string;
    navigationIds?: number[];
    focusRequest?: number;
    onBack?: () => void;
    onNavigateScratchpad?: (scratchpadId: number) => void;
    onRefresh: () => Promise<void> | void;
    onSave: (content: string, expectedRevision: number) => Promise<ScratchpadRead>;
    onSetTags?: (tags: string[], expectedRevision: number) => Promise<void> | void;
    onArchive?: (expectedRevision: number) => Promise<void> | void;
    onDelete?: (expectedRevision: number) => Promise<void> | void;
    onCreateComment?: (input: NewScratchpadCommentInput) => Promise<void> | void;
    onUpdateComment?: (commentId: number, body: string) => Promise<void> | void;
    onResolveComment?: (commentId: number, resolved: boolean) => Promise<void> | void;
    onDeleteComment?: (commentId: number) => Promise<void> | void;
  }

  interface Conflict {
    remoteMarkdown: string;
    remoteRevision: number;
  }

  interface RecoveryCopy {
    label: string;
    markdown: string;
  }

  type SaveState = 'saved' | 'unsaved' | 'saving' | 'conflict' | 'error';

  let {
    read,
    loading,
    busy = false,
    projectName = 'Project',
    navigationIds = [],
    focusRequest = 0,
    onBack,
    onNavigateScratchpad,
    onRefresh,
    onSave,
    onSetTags,
    onArchive,
    onDelete,
    onCreateComment,
    onUpdateComment,
    onResolveComment,
    onDeleteComment
  }: Props = $props();

  let activeId = $state<number | null>(null);
  let baseRevision = $state(0);
  let baseMarkdown = $state('');
  let draft = $state('');
  let titleDraft = $state('');
  let bodyDraft = $state('');
  let dirty = $state(false);
  let saveState = $state<SaveState>('saved');
  let conflict = $state<Conflict | null>(null);
  let recovery = $state<RecoveryCopy | null>(null);
  let tagsOpen = $state(false);
  let tagsDraft = $state('');
  let metadataBusy = $state(false);
  let mobileOutlineOpen = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let saveInFlight: Promise<void> | null = null;
  let seenFocusRequest = -1;
  let editorFocusRequest = $state(0);
  let outlineScrollKey = $state(0);
  let outlineScrollRequest = $state<{ key: number; line: number } | null>(null);
  let viewportLine = $state(1);
  let titleInput = $state<HTMLTextAreaElement | null>(null);
  let commentsOpen = $state(true);
  let mobileCommentsOpen = $state(false);
  let showResolvedComments = $state(false);
  let composerAnchor = $state<ScratchpadSelectionAnchor | null | undefined>(undefined);
  let commentDraft = $state('');
  let commentBusy = $state(false);
  let focusedCommentId = $state<number | null>(null);
  let editingCommentId = $state<number | null>(null);
  let editingCommentDraft = $state('');
  let commentScrollKey = $state(0);
  let commentScrollRequest = $state<{
    key: number;
    commentId: number;
    fallbackLine?: number | null;
  } | null>(null);
  let pendingDeleteComment = $state<ScratchpadComment | null>(null);

  let outline = $derived(scratchpadOutline(bodyDraft));
  let activeOutlineId = $derived.by(() => {
    let active = outline[0]?.id ?? null;
    for (const item of outline) {
      if (item.line > viewportLine + 1) break;
      active = item.id;
    }
    return active;
  });
  let currentIndex = $derived(read ? navigationIds.indexOf(read.scratchpad.id) : -1);
  let previousId = $derived(currentIndex > 0 ? navigationIds[currentIndex - 1] : null);
  let nextId = $derived(
    currentIndex >= 0 && currentIndex < navigationIds.length - 1
      ? navigationIds[currentIndex + 1]
      : null
  );
  let metadataDisabled = $derived(
    busy || metadataBusy || dirty || saveState === 'saving' || saveState === 'conflict'
  );
  let locallyResolvedComments = $derived(
    (read?.comments ?? []).map((comment) => ({
      ...comment,
      ...resolveScratchpadAnchor(bodyDraft, comment)
    }))
  );
  let visibleComments = $derived(
    locallyResolvedComments.filter((comment) => showResolvedComments || !comment.resolved)
  );

  function visibleElement<T extends HTMLElement>(selector: string): T | null {
    return [...document.querySelectorAll<T>(selector)].find((element) => element.offsetParent !== null) ?? null;
  }

  function beginComment(anchor: ScratchpadSelectionAnchor | null): void {
    commentsOpen = true;
    mobileCommentsOpen = true;
    composerAnchor = anchor;
    commentDraft = '';
    queueMicrotask(() => visibleElement<HTMLTextAreaElement>('[data-scratchpad-comment-composer]')?.focus());
  }

  function cancelComment(): void {
    composerAnchor = undefined;
    commentDraft = '';
  }

  async function saveComment(): Promise<void> {
    if (!onCreateComment || composerAnchor === undefined || !commentDraft.trim() || commentBusy) return;
    commentBusy = true;
    try {
      await saveDraft();
      if (dirty || saveState !== 'saved' || conflict) return;
      const localResolution = composerAnchor
        ? resolveScratchpadAnchor(bodyDraft, composerAnchor)
        : null;
      const anchor = composerAnchor &&
        localResolution?.anchor_state === 'anchored' &&
        localResolution.current_start !== null &&
        localResolution.current_end !== null
        ? selectionAnchor(
            bodyDraft,
            localResolution.current_start,
            localResolution.current_end
          )
        : composerAnchor
          ? {
              quote: composerAnchor.quote,
              anchor_prefix: composerAnchor.anchor_prefix,
              anchor_suffix: composerAnchor.anchor_suffix
            }
          : {};
      await onCreateComment({
        body: commentDraft.trim(),
        ...anchor,
        expected_revision: baseRevision,
        allow_unanchored: composerAnchor !== null
      });
      cancelComment();
      await onRefresh();
    } catch {
      // The parent reports daemon errors; preserve the draft for retry.
    } finally {
      commentBusy = false;
    }
  }

  function composerKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      cancelComment();
    } else if (event.key === 'Enter' && event.metaKey) {
      event.preventDefault();
      void saveComment();
    }
  }

  function focusComment(commentId: number): void {
    focusedCommentId = commentId;
    commentsOpen = true;
    mobileCommentsOpen = true;
    queueMicrotask(() => {
      const row = visibleElement<HTMLElement>(`[data-scratchpad-comment-row="${commentId}"]`);
      row?.scrollIntoView({ block: 'nearest' });
    });
  }

  function jumpToComment(comment: ScratchpadComment): void {
    if (comment.anchor_state !== 'anchored') return;
    focusedCommentId = comment.id;
    commentScrollKey += 1;
    commentScrollRequest = {
      key: commentScrollKey,
      commentId: comment.id,
      fallbackLine: comment.current_start_line
    };
  }

  function beginEditComment(comment: ScratchpadComment): void {
    editingCommentId = comment.id;
    editingCommentDraft = comment.body;
    queueMicrotask(() => visibleElement<HTMLTextAreaElement>(`[data-scratchpad-comment-edit="${comment.id}"]`)?.focus());
  }

  async function saveEditedComment(commentId: number): Promise<void> {
    if (!onUpdateComment || !editingCommentDraft.trim() || commentBusy) return;
    commentBusy = true;
    try {
      await onUpdateComment(commentId, editingCommentDraft.trim());
      editingCommentId = null;
      await onRefresh();
    } catch {
      // Preserve the edit for retry.
    } finally {
      commentBusy = false;
    }
  }

  async function toggleCommentResolved(comment: ScratchpadComment): Promise<void> {
    if (!onResolveComment || commentBusy) return;
    commentBusy = true;
    try {
      await onResolveComment(comment.id, !comment.resolved);
      await onRefresh();
    } catch {
      // The parent reports daemon errors.
    } finally {
      commentBusy = false;
    }
  }

  async function deleteComment(commentId: number): Promise<void> {
    if (!onDeleteComment || commentBusy) return;
    commentBusy = true;
    try {
      await onDeleteComment(commentId);
      pendingDeleteComment = null;
      if (focusedCommentId === commentId) focusedCommentId = null;
      await onRefresh();
    } catch {
      // The parent reports daemon errors.
    } finally {
      commentBusy = false;
    }
  }

  function relativeTime(timestamp: number): string {
    const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1000));
    if (seconds < 60) return 'now';
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}h`;
    return `${Math.floor(hours / 24)}d`;
  }

  function fullMarkdown(next: ScratchpadRead): string {
    const body = next.scratchpad.content;
    return body ? `# ${next.scratchpad.name}\n\n${body}` : `# ${next.scratchpad.name}\n`;
  }

  function splitMarkdown(markdown: string): { title: string; body: string } {
    const normalized = markdown.replaceAll('\r\n', '\n');
    const heading = /^#\s+([^\n]+)(?:\n+|$)/.exec(normalized);
    if (!heading) return { title: read?.scratchpad.name ?? 'Untitled scratchpad', body: normalized };
    return { title: heading[1].trim(), body: normalized.slice(heading[0].length) };
  }

  function composeMarkdown(title = titleDraft, body = bodyDraft): string {
    const heading = title.trim();
    return body ? `# ${heading}\n\n${body}` : `# ${heading}\n`;
  }

  function applyMarkdown(markdown: string): void {
    const parts = splitMarkdown(markdown);
    if (composerAnchor) {
      const resolved = resolveScratchpadAnchor(parts.body, composerAnchor);
      if (
        resolved.anchor_state === 'anchored' &&
        resolved.current_start !== null &&
        resolved.current_end !== null
      ) {
        composerAnchor = selectionAnchor(
          parts.body,
          resolved.current_start,
          resolved.current_end
        );
      }
    }
    draft = markdown;
    titleDraft = parts.title;
    bodyDraft = parts.body;
  }

  function recoveryKey(scratchpadId: number): string {
    return `workman.scratchpad-recovery.${scratchpadId}`;
  }

  function rememberRecovery(copy: RecoveryCopy): void {
    recovery = copy;
    if (activeId !== null) localStorage.setItem(recoveryKey(activeId), JSON.stringify(copy));
  }

  function loadRecovery(scratchpadId: number): RecoveryCopy | null {
    try {
      const raw = localStorage.getItem(recoveryKey(scratchpadId));
      if (!raw) return null;
      const stored = JSON.parse(raw) as Partial<RecoveryCopy>;
      if (typeof stored.label !== 'string' || typeof stored.markdown !== 'string') return null;
      return { label: stored.label, markdown: stored.markdown };
    } catch {
      return null;
    }
  }

  function dismissRecovery(): void {
    if (activeId !== null) localStorage.removeItem(recoveryKey(activeId));
    recovery = null;
  }

  function clearSaveTimer(): void {
    if (saveTimer !== null) clearTimeout(saveTimer);
    saveTimer = null;
  }

  function scheduleSave(): void {
    clearSaveTimer();
    if (!dirty || conflict || !titleDraft.trim()) return;
    saveTimer = setTimeout(() => void saveDraft(), 800);
  }

  function updateDraft(): void {
    draft = composeMarkdown();
    dirty = draft !== baseMarkdown;
    saveState = conflict ? 'conflict' : dirty ? 'unsaved' : 'saved';
    scheduleSave();
  }

  function handleTitleChange(next: string): void {
    titleDraft = singleLineTitle(next);
    updateDraft();
  }

  function handleBodyChange(next: string, changes: PositionMapper): void {
    if (composerAnchor) {
      composerAnchor = mapScratchpadSelectionAnchor(composerAnchor, next, changes);
    }
    bodyDraft = next;
    updateDraft();
  }

  function handleTitleBlur(): void {
    if (!titleDraft.trim()) {
      titleDraft = splitMarkdown(baseMarkdown).title;
      updateDraft();
      return;
    }
    void saveDraft();
  }

  function handleTitleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      titleInput?.blur();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      titleDraft = splitMarkdown(baseMarkdown).title;
      updateDraft();
      titleInput?.blur();
    }
  }

  function currentConflict(): Conflict | null {
    return conflict;
  }

  async function saveDraft(): Promise<void> {
    if (saveInFlight) return saveInFlight;
    clearSaveTimer();
    if (activeId === null || !dirty || conflict || saveState === 'saving' || !titleDraft.trim()) return;
    saveInFlight = (async () => {
      const savingMarkdown = draft;
      const expectedRevision = baseRevision;
      saveState = 'saving';
      try {
        const saved = await onSave(savingMarkdown, expectedRevision);
        const canonical = fullMarkdown(saved);
        const latestConflict = currentConflict();
        if (
          latestConflict &&
          latestConflict.remoteRevision === saved.scratchpad.revision &&
          latestConflict.remoteMarkdown === canonical
        ) {
          conflict = null;
        }
        baseRevision = saved.scratchpad.revision;
        baseMarkdown = canonical;
        if (draft === savingMarkdown) applyMarkdown(canonical);
        dirty = draft !== baseMarkdown;
        saveState = dirty ? 'unsaved' : 'saved';
        if (dirty) scheduleSave();
      } catch {
        saveState = 'error';
        await onRefresh();
      }
    })();
    try {
      await saveInFlight;
    } finally {
      saveInFlight = null;
    }
  }

  function useTheirs(): void {
    if (!conflict) return;
    rememberRecovery({ label: 'Your draft before the conflict', markdown: draft });
    applyMarkdown(conflict.remoteMarkdown);
    baseMarkdown = conflict.remoteMarkdown;
    baseRevision = conflict.remoteRevision;
    dirty = false;
    conflict = null;
    saveState = 'saved';
  }

  function keepEditing(): void {
    if (!conflict) return;
    rememberRecovery({ label: `Agent revision ${conflict.remoteRevision}`, markdown: conflict.remoteMarkdown });
    baseMarkdown = conflict.remoteMarkdown;
    baseRevision = conflict.remoteRevision;
    conflict = null;
    dirty = draft !== baseMarkdown;
    saveState = dirty ? 'unsaved' : 'saved';
    scheduleSave();
  }

  function restoreRecovery(): void {
    if (!recovery) return;
    const current = draft;
    const restored = recovery.markdown;
    rememberRecovery({ label: 'Draft replaced by the recovery copy', markdown: current });
    applyMarkdown(restored);
    dirty = draft !== baseMarkdown;
    saveState = dirty ? 'unsaved' : 'saved';
    editorFocusRequest += 1;
    scheduleSave();
  }

  function selectOutline(item: ScratchpadOutlineItem, closeAfterSelect: boolean): void {
    viewportLine = item.line;
    outlineScrollKey += 1;
    outlineScrollRequest = { key: outlineScrollKey, line: item.line };
    if (closeAfterSelect) mobileOutlineOpen = false;
  }

  function navigate(scratchpadId: number | null): void {
    if (scratchpadId !== null) onNavigateScratchpad?.(scratchpadId);
  }

  async function saveTags(): Promise<void> {
    if (!onSetTags || metadataDisabled) return;
    const tags = [...new Set(tagsDraft.split(',').map((tag) => tag.trim()).filter(Boolean))];
    metadataBusy = true;
    try {
      await onSetTags(tags, baseRevision);
      tagsOpen = false;
      await onRefresh();
    } catch {
      saveState = 'error';
      await onRefresh();
    } finally {
      metadataBusy = false;
    }
  }

  async function archiveScratchpad(): Promise<void> {
    if (!onArchive || metadataDisabled) return;
    metadataBusy = true;
    try {
      await onArchive(baseRevision);
    } finally {
      metadataBusy = false;
    }
  }

  async function deleteScratchpad(): Promise<void> {
    if (!onDelete || metadataDisabled) return;
    metadataBusy = true;
    try {
      await onDelete(baseRevision);
    } finally {
      metadataBusy = false;
    }
  }

  $effect(() => {
    const next = read;
    if (!next) return;
    const nextId = next.scratchpad.id;
    const nextMarkdown = fullMarkdown(next);
    if (activeId !== nextId) {
      clearSaveTimer();
      activeId = nextId;
      baseRevision = next.scratchpad.revision;
      baseMarkdown = nextMarkdown;
      applyMarkdown(nextMarkdown);
      dirty = false;
      conflict = null;
      recovery = loadRecovery(nextId);
      tagsDraft = next.scratchpad.tags.join(', ');
      tagsOpen = false;
      saveState = 'saved';
      viewportLine = 1;
      composerAnchor = undefined;
      focusedCommentId = null;
      editingCommentId = null;
      return;
    }
    if (next.scratchpad.revision <= baseRevision) return;
    tagsDraft = next.scratchpad.tags.join(', ');
    if (saveState === 'saving' && nextMarkdown === draft) {
      baseRevision = next.scratchpad.revision;
      baseMarkdown = nextMarkdown;
      applyMarkdown(nextMarkdown);
      dirty = false;
      conflict = null;
      saveState = 'saved';
      return;
    }
    if (!dirty && saveState !== 'saving') {
      baseRevision = next.scratchpad.revision;
      baseMarkdown = nextMarkdown;
      applyMarkdown(nextMarkdown);
      conflict = null;
      saveState = 'saved';
      return;
    }
    if (!conflict || next.scratchpad.revision > conflict.remoteRevision) {
      clearSaveTimer();
      conflict = { remoteMarkdown: nextMarkdown, remoteRevision: next.scratchpad.revision };
      rememberRecovery({ label: 'Your draft before the conflict', markdown: draft });
      saveState = 'conflict';
    }
  });

  $effect(() => {
    const request = focusRequest;
    if (request <= seenFocusRequest) return;
    seenFocusRequest = request;
    if (request > 0) editorFocusRequest += 1;
  });

  $effect(() => {
    const activeId = activeOutlineId;
    if (activeId === null) return;
    queueMicrotask(() => {
      const list = document.querySelector<HTMLElement>('[data-scratchpad-outline="desktop"]');
      const active = [...(list?.querySelectorAll<HTMLElement>('[data-outline-id]') ?? [])]
        .find((item) => item.dataset.outlineId === activeId);
      active?.scrollIntoView({ block: 'nearest' });
    });
  });

  $effect(() => () => clearSaveTimer());
</script>

{#snippet outlineList(closeAfterSelect: boolean)}
  <nav
    class="outline-list"
    aria-label="On this page"
    data-scratchpad-outline={closeAfterSelect ? 'mobile' : 'desktop'}
  >
    {#each outline as item (item.id)}
      <button
        type="button"
        class:active={activeOutlineId === item.id}
        class:level-three={item.level === 3}
        data-outline-id={item.id}
        aria-current={activeOutlineId === item.id ? 'location' : undefined}
        onclick={() => selectOutline(item, closeAfterSelect)}
      >
        {item.label}
      </button>
    {:else}
      <p>Add H2 or H3 headings to build this outline.</p>
    {/each}
  </nav>
{/snippet}

{#snippet commentsPanelContent()}
  <div class="comments-controls">
    <label title="Show resolved scratchpad comments">
      <Checkbox bind:checked={showResolvedComments} aria-label="Show resolved scratchpad comments" /> Resolved
    </label>
    <Button size="xs" variant="outline" disabled={!onCreateComment || commentBusy} onclick={() => beginComment(null)}>Comment</Button>
  </div>
  {#if composerAnchor !== undefined}
    <form class="comment-composer" onsubmit={(event) => { event.preventDefault(); void saveComment(); }}>
      <small>{composerAnchor ? `On “${composerAnchor.quote.slice(0, 52)}${composerAnchor.quote.length > 52 ? '…' : ''}”` : 'Whole document'}</small>
      <Textarea
        data-scratchpad-comment-composer
        bind:value={commentDraft}
        rows={3}
        aria-label="Scratchpad comment"
        placeholder="Add review feedback…"
        onkeydown={composerKeydown}
      ></Textarea>
      <div><Button size="xs" variant="outline" onclick={cancelComment}>Cancel</Button><Button size="xs" type="submit" disabled={!commentDraft.trim() || commentBusy}>Save <kbd>⌘↵</kbd></Button></div>
    </form>
  {/if}
  <div class="comment-list" aria-label="Scratchpad comments">
    {#each visibleComments as comment (comment.id)}
      <article
        class:focused={focusedCommentId === comment.id}
        class:resolved={comment.resolved}
        class="comment-row"
        data-scratchpad-comment-row={comment.id}
      >
        <header><strong>{comment.actor}</strong><time datetime={new Date(comment.created_at).toISOString()} title={new Date(comment.created_at).toLocaleString()}>{relativeTime(comment.created_at)}</time></header>
        {#if comment.quote}
          <Button class="comment-quote" size="xs" variant="ghost" disabled={comment.anchor_state !== 'anchored'} onclick={() => jumpToComment(comment)}>
            “{comment.quote.slice(0, 88)}{comment.quote.length > 88 ? '…' : ''}”
          </Button>
        {/if}
        {#if comment.anchor_state === 'orphaned'}<small class="anchor-note">Text no longer found</small>
        {:else if comment.anchor_state === 'unanchored'}<small class="anchor-note">Whole document</small>{/if}
        {#if editingCommentId === comment.id}
          <Textarea data-scratchpad-comment-edit={comment.id} bind:value={editingCommentDraft} rows={3} aria-label="Edit scratchpad comment" onkeydown={(event) => {
            if (event.key === 'Escape') { event.preventDefault(); editingCommentId = null; }
            else if (event.key === 'Enter' && event.metaKey) { event.preventDefault(); void saveEditedComment(comment.id); }
          }}></Textarea>
        {:else}
          <p>{comment.body}</p>
        {/if}
        <footer>
          {#if comment.anchor_state === 'anchored'}<Button size="xs" variant="outline" onclick={() => jumpToComment(comment)}>Jump</Button>{/if}
          {#if comment.can_resolve}<Button size="xs" variant="outline" disabled={commentBusy} onclick={() => void toggleCommentResolved(comment)}>{comment.resolved ? 'Reopen' : 'Resolve'}</Button>{/if}
          {#if comment.can_edit}
            {#if editingCommentId === comment.id}
              <Button size="xs" variant="outline" onclick={() => (editingCommentId = null)}>Cancel</Button>
              <Button size="xs" disabled={!editingCommentDraft.trim() || commentBusy} onclick={() => void saveEditedComment(comment.id)}>Save</Button>
            {:else}
              <Button size="xs" variant="outline" onclick={() => beginEditComment(comment)}>Edit</Button>
            {/if}
          {/if}
          {#if comment.can_delete}<Button size="xs" variant="destructive" disabled={commentBusy} onclick={() => (pendingDeleteComment = comment)}>Delete</Button>{/if}
        </footer>
      </article>
    {:else}
      <p class="comments-empty">{read?.comment_total_count ? 'No comments in this view.' : 'Select text to leave anchored feedback, or comment on the whole document.'}</p>
    {/each}
  </div>
{/snippet}

{#if loading && !read}
  <div class="state">Loading scratchpad…</div>
{:else if read}
  <DocumentScaffold
    ariaLabel={`Scratchpad #${read.scratchpad.id}`}
    breadcrumbRoot={projectName}
    breadcrumbCurrent={read.scratchpad.name}
    reference={`#${read.scratchpad.id}`}
    previousDisabled={previousId === null || busy}
    nextDisabled={nextId === null || busy}
    onBack={onBack}
    onPrevious={() => navigate(previousId)}
    onNext={() => navigate(nextId)}
    onCopyReference={() => void navigator.clipboard.writeText(`#${read!.scratchpad.id}`)}
  >
    {#snippet actions()}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <IconButton {...props} label="Scratchpad actions">
              {#snippet icon()}<EllipsisIcon size={16} strokeWidth={1.8} />{/snippet}
            </IconButton>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" class="w-56">
          <DropdownMenu.Label>Scratchpad #{read.scratchpad.id}</DropdownMenu.Label>
          <DropdownMenu.Separator />
          <DropdownMenu.Item onclick={() => { titleInput?.focus(); titleInput?.select(); }}>
            <PencilIcon class="size-4" aria-hidden="true" /> Edit title
          </DropdownMenu.Item>
          <DropdownMenu.Item onclick={() => (editorFocusRequest += 1)}>
            <PencilIcon class="size-4" aria-hidden="true" /> Edit document
          </DropdownMenu.Item>
          <DropdownMenu.Item disabled={!onCreateComment} onclick={() => beginComment(null)}>
            <MessageSquareIcon class="size-4" aria-hidden="true" /> Comment on document
          </DropdownMenu.Item>
          {#if !read.scratchpad.archived && onArchive}
            <DropdownMenu.Item disabled={metadataDisabled} onclick={() => void archiveScratchpad()}>
              <ArchiveIcon class="size-4" aria-hidden="true" /> Archive scratchpad
            </DropdownMenu.Item>
          {/if}
          {#if onDelete}
            <DropdownMenu.Separator />
            <DropdownMenu.Item variant="destructive" disabled={metadataDisabled} onclick={() => void deleteScratchpad()}>
              <Trash2Icon class="size-4" aria-hidden="true" /> Delete scratchpad
            </DropdownMenu.Item>
          {/if}
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    {/snippet}

    {#snippet rail()}
      <section class="outline-rail">
        <div class="outline-section">
          <span>Document outline</span>
          <h2>On this page</h2>
          {@render outlineList(false)}
        </div>
        <div class="comments-section">
          <Collapsible.Root bind:open={commentsOpen}>
            <Collapsible.Trigger class="comments-trigger">
              <span><MessageSquareIcon size={14} strokeWidth={1.8} aria-hidden="true" />Comments</span>
              <CountBadge value={read.unresolved_comment_count} title={`${read.unresolved_comment_count} unresolved scratchpad comments`} />
              <ChevronDownIcon class={commentsOpen ? 'open' : ''} size={14} strokeWidth={1.8} aria-hidden="true" />
            </Collapsible.Trigger>
            <Collapsible.Content><div class="comments-panel">{@render commentsPanelContent()}</div></Collapsible.Content>
          </Collapsible.Root>
        </div>
      </section>
    {/snippet}

    <article class="scratchpad-document" class:has-notice={conflict !== null || recovery !== null}>
      <div class="mobile-outline">
        <Collapsible.Root bind:open={mobileOutlineOpen}>
          <Collapsible.Trigger class="outline-trigger">
            <span><strong>On this page</strong><small>{outline.length} section{outline.length === 1 ? '' : 's'}</small></span>
            <ChevronDownIcon class={mobileOutlineOpen ? 'open' : ''} size={15} strokeWidth={1.8} aria-hidden="true" />
          </Collapsible.Trigger>
          <Collapsible.Content><div class="outline-mobile-content">{@render outlineList(true)}</div></Collapsible.Content>
        </Collapsible.Root>
      </div>
      <div class="mobile-comments">
        <Collapsible.Root bind:open={mobileCommentsOpen}>
          <Collapsible.Trigger class="comments-trigger">
            <span><MessageSquareIcon size={14} strokeWidth={1.8} aria-hidden="true" />Comments</span>
            <CountBadge value={read.unresolved_comment_count} title={`${read.unresolved_comment_count} unresolved scratchpad comments`} />
            <ChevronDownIcon class={mobileCommentsOpen ? 'open' : ''} size={14} strokeWidth={1.8} aria-hidden="true" />
          </Collapsible.Trigger>
          <Collapsible.Content><div class="comments-panel">{@render commentsPanelContent()}</div></Collapsible.Content>
        </Collapsible.Root>
      </div>

      <textarea
        class="title"
        bind:this={titleInput}
        value={titleDraft}
        aria-label="Scratchpad title"
        disabled={busy}
        rows="1"
        wrap="soft"
        use:autoGrowTextarea={titleDraft}
        oninput={(event) => handleTitleChange(event.currentTarget.value)}
        onblur={handleTitleBlur}
        onkeydown={handleTitleKeydown}
      ></textarea>

      <div class="metadata" aria-label="Scratchpad metadata">
        <span class="metadata-chip status-chip">
          <ArchiveIcon size={13} strokeWidth={1.8} aria-hidden="true" />
          {read.scratchpad.archived ? 'Archived' : 'Active'}
        </span>
        <span class="metadata-chip" title={`Created by ${read.scratchpad.created_by}`}>
          Updated by {read.scratchpad.updated_by}
        </span>
        {#each read.scratchpad.tags as tag (tag)}
          <button class="metadata-chip tag-chip" type="button" disabled={metadataDisabled} onclick={() => (tagsOpen = true)}>{tag}</button>
        {/each}
        <button class="metadata-chip add-tag" type="button" disabled={metadataDisabled} onclick={() => (tagsOpen = true)}>
          <TagIcon size={13} strokeWidth={1.8} aria-hidden="true" /> Add tag
        </button>
        <span
          class:attention={saveState === 'conflict' || saveState === 'error'}
          class="save-state"
          title={`Scratchpad save state · ${saveState}`}
        >
          {#if saveState === 'saving'}Saving…
          {:else if saveState === 'unsaved'}Unsaved
          {:else if saveState === 'conflict'}Conflict
          {:else if saveState === 'error'}Save failed
          {:else}Saved · rev {baseRevision}{/if}
        </span>
      </div>

      {#if tagsOpen}
        <form class="tags-editor" onsubmit={(event) => { event.preventDefault(); void saveTags(); }}>
          <label for="scratchpad-tags">Tags</label>
          <input id="scratchpad-tags" bind:value={tagsDraft} placeholder="handoff, research, notes" />
          <button type="button" onclick={() => { tagsDraft = read!.scratchpad.tags.join(', '); tagsOpen = false; }}>Cancel</button>
          <button class="primary" type="submit" disabled={metadataDisabled}>Save tags</button>
        </form>
      {/if}

      {#if conflict}
        <div class="conflict-banner" role="alert">
          <div><strong>An agent changed this scratchpad.</strong><span>Your draft and revision {conflict.remoteRevision} are both preserved.</span></div>
          <button type="button" onclick={useTheirs}>Use theirs</button>
          <button class="primary" type="button" onclick={keepEditing}>Keep editing</button>
        </div>
      {:else if recovery}
        <div class="recovery-banner">
          <span>{recovery.label} is kept as a recovery copy.</span>
          <button type="button" onclick={restoreRecovery}>Restore</button>
          <button type="button" aria-label="Dismiss recovery copy" onclick={dismissRecovery}>Dismiss</button>
        </div>
      {/if}

      <section class="body-section" aria-label="Scratchpad document">
        <LiveMarkdownEditor
          value={bodyDraft}
          focusRequest={editorFocusRequest}
          flow
          scrollRequest={outlineScrollRequest}
          {commentScrollRequest}
          comments={locallyResolvedComments}
          {showResolvedComments}
          {focusedCommentId}
          onChange={handleBodyChange}
          onSave={() => void saveDraft()}
          onViewportLineChange={(line) => (viewportLine = line)}
          onCommentSelection={(anchor) => beginComment(anchor)}
          onCommentClick={focusComment}
        />
      </section>
    </article>
  </DocumentScaffold>
{:else}
  <div class="state">Scratchpad not found.</div>
{/if}

<AlertDialog.Root open={pendingDeleteComment !== null} onOpenChange={(open) => { if (!open && !commentBusy) pendingDeleteComment = null; }}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>Delete this comment?</AlertDialog.Title>
      <AlertDialog.Description>This permanently removes the comment and its anchor.</AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel disabled={commentBusy}>Cancel</AlertDialog.Cancel>
      <AlertDialog.Action variant="destructive" disabled={commentBusy} onclick={() => {
        if (pendingDeleteComment) void deleteComment(pendingDeleteComment.id);
      }}>Delete comment</AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

<style>
  .scratchpad-document { min-width: 0; min-height: 100%; }
  .title { display: block; width: 100%; min-height: calc(1.16em + 7px); overflow: hidden; resize: none; overflow-wrap: anywhere; word-break: break-word; white-space: pre-wrap; border: 0; border-radius: var(--radius); outline: 0; padding: 2px 4px 5px; background: transparent; color: var(--foreground); font: 680 clamp(25px, 3.1cqw, 34px)/1.16 var(--ui-font-family); letter-spacing: -0.025em; }
  .title:hover { background: var(--card); }
  .title:focus { background: var(--card); box-shadow: 0 0 0 2px var(--ring); }
  .metadata { display: flex; flex-wrap: wrap; align-items: center; gap: 5px; margin-top: 12px; }
  .metadata-chip { display: inline-flex; min-height: 28px; align-items: center; gap: 6px; border: 1px solid var(--border); border-radius: 999px; padding: 0 8px; background: var(--card); color: var(--text-soft); font-size: var(--font-size-xs); }
  button.metadata-chip { cursor: pointer; }
  button.metadata-chip:disabled { cursor: default; opacity: 0.62; }
  .status-chip { font-weight: 630; }
  .tag-chip { color: var(--muted-foreground); }
  .add-tag { border-style: dashed; color: var(--muted-foreground); }
  .save-state { margin-left: auto; color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .save-state.attention { color: var(--warning-token); }
  .tags-editor { display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 6px; margin-top: 10px; border: 1px solid var(--border); border-radius: var(--radius); padding: 6px; background: var(--card); }
  .tags-editor label { padding-left: 4px; color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 650; }
  .tags-editor input { min-width: 0; height: 29px; border: 1px solid var(--input); border-radius: var(--radius); outline: 0; padding: 0 8px; background: var(--background); color: var(--foreground); font-size: var(--font-size-sm); }
  .tags-editor input:focus { border-color: var(--ring); box-shadow: 0 0 0 1px var(--ring); }
  .tags-editor button, .conflict-banner button, .recovery-banner button { min-height: 29px; border: 1px solid var(--input); border-radius: var(--radius); padding: 0 9px; background: var(--card); color: var(--text-soft); font-size: var(--font-size-sm); cursor: pointer; }
  button.primary { border-color: var(--primary); background: var(--primary); color: var(--primary-foreground); font-weight: 650; }
  .conflict-banner, .recovery-banner { display: flex; align-items: center; gap: 7px; margin-top: 10px; border: 1px solid color-mix(in srgb, var(--warning-token) 42%, var(--border)); border-radius: var(--radius); padding: 7px; background: color-mix(in srgb, var(--warning-token) 8%, var(--card)); color: var(--warning-token); }
  .conflict-banner div { display: grid; min-width: 0; flex: 1; gap: 2px; }
  .conflict-banner strong { color: var(--foreground); font-size: var(--font-size-sm); }
  .conflict-banner span, .recovery-banner span { font-size: var(--font-size-sm); }
  .recovery-banner { border-color: var(--border); background: var(--card); color: var(--muted-foreground); }
  .recovery-banner span { min-width: 0; flex: 1; }
  .body-section { margin-top: 22px; overflow: visible; border-top: 1px solid var(--border); background: transparent; }
  .outline-rail { position: sticky; top: 18px; display: grid; max-height: calc(100vh - 36px); min-height: 0; grid-template-rows: minmax(112px, 1fr) auto; gap: 16px; overflow: hidden; }
  .outline-section { display: grid; min-height: 0; grid-template-rows: auto auto minmax(0, 1fr); overflow: hidden; }
  .outline-section > span { color: var(--muted-foreground); font: 650 var(--font-size-xs)/1 var(--terminal-font-family); letter-spacing: 0.055em; text-transform: uppercase; }
  .outline-section h2 { margin: 5px 0 12px; color: var(--foreground); font-size: var(--font-size-base); line-height: 1.2; }
  .outline-list { display: grid; gap: 2px; }
  .outline-list[data-scratchpad-outline='desktop'] { min-height: 0; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable; }
  .outline-list button { width: 100%; min-height: 28px; overflow: hidden; border: 0; border-left: 2px solid var(--border); border-radius: 0 var(--radius) var(--radius) 0; padding: 4px 7px; background: transparent; color: var(--muted-foreground); font-size: var(--font-size-xs); text-align: left; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  .outline-list button.level-three { padding-left: 17px; }
  .outline-list button:hover { background: var(--card); color: var(--text-soft); }
  .outline-list button:focus-visible { outline: 2px solid var(--ring); outline-offset: -2px; }
  .outline-list button.active { border-left-color: var(--foreground); background: var(--accent); color: var(--foreground); font-weight: 630; }
  .outline-list p { margin: 0; border-left: 2px solid var(--border); padding: 3px 8px; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.45; }
  :global(.comments-trigger) { display: grid; width: 100%; min-height: 34px; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: 6px; border: 1px solid var(--border); border-radius: var(--radius); padding: 0 7px; background: var(--card); color: var(--foreground); cursor: pointer; }
  :global(.comments-trigger > span) { display: inline-flex; align-items: center; gap: 6px; font-size: var(--font-size-sm); font-weight: 650; }
  :global(.comments-trigger > svg) { color: var(--muted-foreground); transition: transform 150ms ease; }
  :global(.comments-trigger > svg.open) { transform: rotate(180deg); }
  .comments-section { min-height: 0; overflow: hidden; }
  .comments-panel { max-height: min(52vh, 520px); overflow-y: auto; border: 1px solid var(--border); border-top: 0; border-radius: 0 0 var(--radius) var(--radius); background: var(--card); scrollbar-gutter: stable; }
  .outline-list[data-scratchpad-outline='desktop'], .comments-panel { scrollbar-width: thin; scrollbar-color: var(--border-strong) transparent; }
  .outline-list[data-scratchpad-outline='desktop']::-webkit-scrollbar, .comments-panel::-webkit-scrollbar { width: 6px; }
  .outline-list[data-scratchpad-outline='desktop']::-webkit-scrollbar-thumb, .comments-panel::-webkit-scrollbar-thumb { border-radius: var(--radius); background: var(--border-strong); }
  .comments-controls { display: flex; min-height: 34px; align-items: center; gap: 5px; border-bottom: 1px solid var(--border); padding: 4px 6px; }
  .comments-controls label { display: inline-flex; min-width: 0; flex: 1; align-items: center; gap: 5px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .comment-composer { display: grid; gap: 5px; border-bottom: 1px solid var(--border); padding: 7px; }
  .comment-composer small, .anchor-note { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .comment-composer > div { display: flex; justify-content: flex-end; gap: 5px; }
  .comment-composer kbd { margin-left: 3px; color: inherit; font: var(--font-size-xs) var(--terminal-font-family); }
  .comment-list { display: grid; }
  .comment-row { display: grid; gap: 5px; border-bottom: 1px solid var(--border); outline: 0; padding: 8px 7px; }
  .comment-row:last-child { border-bottom: 0; }
  .comment-row.focused { box-shadow: inset 2px 0 0 var(--notification-unread); background: color-mix(in srgb, var(--notification-unread) 8%, var(--accent)); }
  .comment-row.resolved { opacity: 0.72; }
  .comment-row header { display: flex; min-width: 0; align-items: baseline; gap: 6px; }
  .comment-row header strong { min-width: 0; flex: 1; overflow: hidden; color: var(--text-soft); font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }
  .comment-row time { color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .comment-row p { margin: 0; color: var(--foreground); font-size: var(--font-size-sm); line-height: 1.42; white-space: pre-wrap; }
  :global(.comment-quote) { overflow: hidden; justify-content: flex-start; border: 0; border-left: 2px solid var(--border-strong); border-radius: 0; padding: 2px 5px; background: transparent; color: var(--muted-foreground); font-size: var(--font-size-xs); font-style: italic; line-height: 1.35; text-align: left; text-overflow: ellipsis; cursor: pointer; white-space: normal; }
  :global(.comment-quote:disabled) { cursor: default; }
  .comment-row footer { display: flex; flex-wrap: wrap; gap: 4px; }
  .comments-empty { margin: 0; padding: 10px 8px; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.45; }
  .mobile-outline { display: none; margin-bottom: 14px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--card); }
  .mobile-comments { display: none; margin-bottom: 14px; }
  .mobile-outline :global(.outline-trigger) { display: flex; width: 100%; min-height: 38px; align-items: center; gap: 8px; border: 0; padding: 5px 8px 5px 10px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .mobile-outline :global(.outline-trigger > span) { display: flex; min-width: 0; flex: 1; align-items: baseline; gap: 7px; }
  .mobile-outline :global(.outline-trigger strong) { font-size: var(--font-size-sm); font-weight: 630; }
  .mobile-outline :global(.outline-trigger small) { color: var(--muted-foreground); font: var(--font-size-xs) var(--terminal-font-family); }
  .mobile-outline :global(.outline-trigger svg) { color: var(--muted-foreground); transition: transform 150ms ease; }
  .mobile-outline :global(.outline-trigger svg.open) { transform: rotate(180deg); }
  .outline-mobile-content { max-height: 220px; overflow-y: auto; border-top: 1px solid var(--border); padding: 5px; }
  .state { display: grid; width: 100%; height: 100%; place-items: center; color: var(--muted-foreground); font-size: var(--font-size-sm); }

  @container (max-width: 880px) {
    .mobile-outline { display: block; }
    .mobile-comments { display: block; }
    .mobile-comments .comments-panel { max-height: 340px; }
  }

  @container (max-width: 620px) {
    .metadata { gap: 4px; }
    .save-state { width: 100%; margin: 3px 0 0; }
    .tags-editor { grid-template-columns: minmax(0, 1fr) auto auto; }
    .tags-editor label { grid-column: 1 / -1; }
    .body-section { margin-top: 16px; }
    .conflict-banner, .recovery-banner { align-items: stretch; flex-wrap: wrap; }
    .conflict-banner div, .recovery-banner span { width: 100%; flex-basis: 100%; }
  }

  @media (prefers-reduced-motion: reduce) {
    .mobile-outline :global(.outline-trigger svg) { transition: none; }
    :global(.comments-trigger > svg) { transition: none; }
  }
</style>
