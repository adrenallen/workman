<script lang="ts">
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import TagIcon from '@lucide/svelte/icons/tag';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import * as Collapsible from '$lib/components/ui/collapsible';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';

  import type { ScratchpadRead } from './coordination';
  import DocumentScaffold from './DocumentScaffold.svelte';
  import LiveMarkdownEditor from './LiveMarkdownEditor.svelte';
  import { scratchpadOutline, type ScratchpadOutlineItem } from './scratchpadOutline';

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
    onDelete
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
  let seenFocusRequest = -1;
  let editorFocusRequest = $state(0);
  let outlineScrollKey = $state(0);
  let outlineScrollRequest = $state<{ key: number; line: number } | null>(null);
  let viewportLine = $state(1);
  let titleInput = $state<HTMLInputElement | null>(null);

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
    titleDraft = next;
    updateDraft();
  }

  function handleBodyChange(next: string): void {
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
    clearSaveTimer();
    if (activeId === null || !dirty || conflict || saveState === 'saving' || !titleDraft.trim()) return;
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

  $effect(() => () => clearSaveTimer());
</script>

{#snippet outlineList(closeAfterSelect: boolean)}
  <nav class="outline-list" aria-label="On this page">
    {#each outline as item (item.id)}
      <button
        type="button"
        class:active={activeOutlineId === item.id}
        class:level-three={item.level === 3}
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
        <span>Document outline</span>
        <h2>On this page</h2>
        {@render outlineList(false)}
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

      <input
        class="title"
        bind:this={titleInput}
        value={titleDraft}
        aria-label="Scratchpad title"
        disabled={busy}
        oninput={(event) => handleTitleChange(event.currentTarget.value)}
        onblur={handleTitleBlur}
        onkeydown={handleTitleKeydown}
      />

      <div class="metadata" aria-label="Scratchpad metadata">
        <span class="metadata-chip status-chip">
          <ArchiveIcon size={13} strokeWidth={1.8} aria-hidden="true" />
          {read.scratchpad.archived ? 'Archived' : 'Active'}
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
          scrollRequest={outlineScrollRequest}
          onChange={handleBodyChange}
          onSave={() => void saveDraft()}
          onViewportLineChange={(line) => (viewportLine = line)}
        />
      </section>
    </article>
  </DocumentScaffold>
{:else}
  <div class="state">Scratchpad not found.</div>
{/if}

<style>
  .scratchpad-document { min-width: 0; }
  .title { width: 100%; border: 0; border-radius: var(--radius); outline: 0; padding: 2px 4px 5px; background: transparent; color: var(--foreground); font: 680 clamp(25px, 3.1cqw, 34px)/1.16 var(--ui-font-family); letter-spacing: -0.025em; }
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
  .body-section { height: clamp(420px, 62vh, 720px); margin-top: 22px; overflow: hidden; border: 1px solid var(--border); border-radius: calc(var(--radius) + 1px); background: var(--background); }
  .outline-rail { position: sticky; top: 18px; }
  .outline-rail > span { color: var(--muted-foreground); font: 650 var(--font-size-xs)/1 var(--terminal-font-family); letter-spacing: 0.055em; text-transform: uppercase; }
  .outline-rail h2 { margin: 5px 0 12px; color: var(--foreground); font-size: var(--font-size-base); line-height: 1.2; }
  .outline-list { display: grid; gap: 2px; }
  .outline-list button { width: 100%; min-height: 28px; overflow: hidden; border: 0; border-left: 2px solid var(--border); border-radius: 0 var(--radius) var(--radius) 0; padding: 4px 7px; background: transparent; color: var(--muted-foreground); font-size: var(--font-size-xs); text-align: left; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  .outline-list button.level-three { padding-left: 17px; }
  .outline-list button:hover { background: var(--card); color: var(--text-soft); }
  .outline-list button:focus-visible { outline: 2px solid var(--ring); outline-offset: -2px; }
  .outline-list button.active { border-left-color: var(--foreground); background: var(--accent); color: var(--foreground); font-weight: 630; }
  .outline-list p { margin: 0; border-left: 2px solid var(--border); padding: 3px 8px; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.45; }
  .mobile-outline { display: none; margin-bottom: 14px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--card); }
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
  }

  @container (max-width: 620px) {
    .metadata { gap: 4px; }
    .save-state { width: 100%; margin: 3px 0 0; }
    .tags-editor { grid-template-columns: minmax(0, 1fr) auto auto; }
    .tags-editor label { grid-column: 1 / -1; }
    .body-section { height: 460px; margin-top: 16px; }
    .conflict-banner, .recovery-banner { align-items: stretch; flex-wrap: wrap; }
    .conflict-banner div, .recovery-banner span { width: 100%; flex-basis: 100%; }
  }

  @media (prefers-reduced-motion: reduce) {
    .mobile-outline :global(.outline-trigger svg) { transition: none; }
  }
</style>
