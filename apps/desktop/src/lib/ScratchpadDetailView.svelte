<script lang="ts">
  import LiveMarkdownEditor from './LiveMarkdownEditor.svelte';
  import MarkdownView from './MarkdownView.svelte';
  import type { ScratchpadRead } from './coordination';

  interface Props {
    read: ScratchpadRead | null;
    loading: boolean;
    focusRequest?: number;
    onRefresh: () => Promise<void> | void;
    onSave: (content: string, expectedRevision: number) => Promise<ScratchpadRead>;
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

  let { read, loading, focusRequest = 0, onRefresh, onSave }: Props = $props();
  let activeId = $state<number | null>(null);
  let baseRevision = $state(0);
  let baseMarkdown = $state('');
  let draft = $state('');
  let dirty = $state(false);
  let editing = $state(false);
  let saveState = $state<SaveState>('saved');
  let conflict = $state<Conflict | null>(null);
  let recovery = $state<RecoveryCopy | null>(null);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let seenFocusRequest = -1;
  let editorFocusRequest = $state(0);

  function fullMarkdown(next: ScratchpadRead): string {
    const body = next.scratchpad.content;
    return body ? `# ${next.scratchpad.name}\n\n${body}` : `# ${next.scratchpad.name}\n`;
  }

  function recoveryKey(scratchpadId: number): string {
    return `awm.scratchpad-recovery.${scratchpadId}`;
  }

  function rememberRecovery(copy: RecoveryCopy): void {
    recovery = copy;
    if (activeId !== null) {
      localStorage.setItem(recoveryKey(activeId), JSON.stringify(copy));
    }
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
    if (!dirty || conflict) return;
    saveTimer = setTimeout(() => void saveDraft(), 800);
  }

  function handleChange(next: string): void {
    draft = next;
    dirty = draft !== baseMarkdown;
    saveState = conflict ? 'conflict' : dirty ? 'unsaved' : 'saved';
    scheduleSave();
  }

  function currentConflict(): Conflict | null {
    return conflict;
  }

  async function saveDraft(force = false): Promise<void> {
    clearSaveTimer();
    if (activeId === null || !dirty || conflict || (saveState === 'saving' && !force)) return;
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
      if (draft === savingMarkdown) draft = canonical;
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
    draft = conflict.remoteMarkdown;
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
    draft = restored;
    dirty = draft !== baseMarkdown;
    saveState = dirty ? 'unsaved' : 'saved';
    editing = true;
    editorFocusRequest += 1;
    scheduleSave();
  }

  async function finishEditing(): Promise<void> {
    await saveDraft(true);
    if (!conflict) editing = false;
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
      draft = nextMarkdown;
      dirty = false;
      conflict = null;
      recovery = loadRecovery(nextId);
      saveState = 'saved';
      return;
    }
    if (next.scratchpad.revision <= baseRevision) return;
    if (saveState === 'saving' && nextMarkdown === draft) {
      baseRevision = next.scratchpad.revision;
      baseMarkdown = nextMarkdown;
      draft = nextMarkdown;
      dirty = false;
      conflict = null;
      saveState = 'saved';
      return;
    }
    if (!dirty && saveState !== 'saving') {
      baseRevision = next.scratchpad.revision;
      baseMarkdown = nextMarkdown;
      draft = nextMarkdown;
      conflict = null;
      saveState = 'saved';
      return;
    }
    if (!conflict || next.scratchpad.revision > conflict.remoteRevision) {
      clearSaveTimer();
      conflict = {
        remoteMarkdown: nextMarkdown,
        remoteRevision: next.scratchpad.revision
      };
      rememberRecovery({ label: 'Your draft before the conflict', markdown: draft });
      saveState = 'conflict';
    }
  });

  $effect(() => {
    const request = focusRequest;
    if (request <= seenFocusRequest) return;
    seenFocusRequest = request;
    if (request > 0) {
      editing = true;
      editorFocusRequest += 1;
    }
  });

  $effect(() => () => clearSaveTimer());
</script>

{#if loading && !read}
  <div class="state">Loading scratchpad…</div>
{:else if read}
  <article
    class="scratchpad-document"
    class:editing
    class:has-notice={conflict !== null || recovery !== null}
  >
    <header>
      <div class="tags">
        {#each read.scratchpad.tags as tag}<span>{tag}</span>{/each}
      </div>
      <div class:attention={saveState === 'conflict' || saveState === 'error'} class="save-state">
        {#if saveState === 'saving'}<i></i> Saving…
        {:else if saveState === 'unsaved'}Unsaved
        {:else if saveState === 'conflict'}Conflict
        {:else if saveState === 'error'}Save failed
        {:else}Saved · rev {baseRevision}{/if}
      </div>
      {#if editing}
        <button type="button" onclick={() => void finishEditing()}>Done</button>
      {:else}
        <button type="button" onclick={() => { editing = true; editorFocusRequest += 1; }}>Edit</button>
      {/if}
    </header>

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
        <button type="button" aria-label="Dismiss recovery copy" onclick={dismissRecovery}>×</button>
      </div>
    {/if}

    <div class="content">
      {#if editing}
        <LiveMarkdownEditor
          value={draft}
          focusRequest={editorFocusRequest}
          onChange={handleChange}
          onSave={() => void saveDraft(true)}
        />
      {:else}
        <div class="read-document">
          <h1>{read.scratchpad.name}</h1>
          {#if read.scratchpad.content.trim()}
            <MarkdownView source={read.scratchpad.content} />
          {:else}
            <button class="empty-note" type="button" onclick={() => { editing = true; editorFocusRequest += 1; }}>
              This scratchpad is empty. Start writing.
            </button>
          {/if}
        </div>
      {/if}
    </div>
  </article>
{:else}
  <div class="state">Scratchpad not found.</div>
{/if}

<style>
  .scratchpad-document { display: grid; width: 100%; height: 100%; min-width: 0; grid-template-rows: auto minmax(0, 1fr); background: #0c1419; }
  .scratchpad-document.has-notice { grid-template-rows: auto auto minmax(0, 1fr); }
  .scratchpad-document:not(.editing) { background: linear-gradient(135deg, #0c1419 0%, #0b1115 68%); }
  header { display: flex; min-height: 38px; align-items: center; gap: 7px; border-bottom: 1px solid var(--border); padding: 4px 9px; background: #10191e; }
  .tags { display: flex; min-width: 0; flex: 1; gap: 4px; overflow-x: auto; }
  .tags span { flex: none; border: 1px solid #34434a; border-radius: 999px; padding: 2px 6px; color: #9fb0b7; background: #152128; font: 7px 'JetBrains Mono Variable', monospace; }
  .save-state { display: flex; flex: none; align-items: center; gap: 5px; color: #71858d; font: 8px 'JetBrains Mono Variable', monospace; }
  .save-state.attention { color: #e3a671; }
  .save-state i { width: 5px; height: 5px; border-radius: 50%; background: var(--signal); box-shadow: 0 0 8px color-mix(in srgb, var(--signal) 70%, transparent); animation: pulse 1s ease-in-out infinite; }
  button { flex: none; border: 1px solid #3a484e; border-radius: 4px; padding: 4px 8px; background: #1b252a; color: #cbd5d9; font-size: 9px; cursor: pointer; }
  button:hover { border-color: #537078; background: #243239; }
  button.primary { border-color: #327a72; background: #173c39; color: #8ce2d5; }
  .conflict-banner, .recovery-banner { display: flex; align-items: center; gap: 7px; border-bottom: 1px solid #594332; padding: 7px 10px; background: #2a211a; color: #dbb48e; }
  .conflict-banner div { display: grid; min-width: 0; flex: 1; gap: 2px; }
  .conflict-banner strong { color: #f2cfaa; font-size: 10px; }
  .conflict-banner span, .recovery-banner span { font-size: 9px; }
  .recovery-banner { border-color: #27444a; background: #11252a; color: #9ebec5; }
  .recovery-banner span { min-width: 0; flex: 1; }
  .content { min-height: 0; overflow: hidden; }
  .read-document { height: 100%; max-width: 900px; overflow: auto; padding: 25px 30px 70px; scrollbar-color: #41464d transparent; scrollbar-width: thin; }
  .read-document > h1 { margin: 0 0 20px; color: #edf2f3; font: 680 28px/1.15 'Archivo Variable', sans-serif; letter-spacing: -.02em; }
  .read-document > :global(.markdown) { max-width: 820px; }
  .empty-note { border: 1px dashed #304149; padding: 16px 18px; background: #101a1f; color: #71858d; }
  .state { display: grid; width: 100%; height: 100%; place-items: center; color: var(--muted); font-size: 10px; }
  @keyframes pulse { 50% { opacity: .35; transform: scale(.8); } }
</style>
