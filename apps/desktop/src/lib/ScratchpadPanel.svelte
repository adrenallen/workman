<script lang="ts">
  import MarkdownView from './MarkdownView.svelte';
  import type { ScratchpadRead, ScratchpadSummary } from './coordination';

  interface Props {
    scratchpads: ScratchpadSummary[];
    selectedId: number | null;
    read: ScratchpadRead | null;
    loading: boolean;
    onSelect: (scratchpadId: number) => void;
    onRefresh: () => void;
  }

  let { scratchpads, selectedId, read, loading, onSelect, onRefresh }: Props = $props();
  let query = $state('');
  let filtered = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return scratchpads;
    return scratchpads.filter(
      (scratchpad) =>
        scratchpad.name.toLowerCase().includes(needle) ||
        scratchpad.tags.some((tag) => tag.toLowerCase().includes(needle))
    );
  });
</script>

<section class="scratchpads" aria-label="Live scratchpads">
  <aside class="scratchpad-list">
    <header>
      <div>
        <span class="live-dot" aria-hidden="true"></span>
        <strong>Live scratchpads</strong>
      </div>
      <small>{scratchpads.length.toString().padStart(2, '0')}</small>
    </header>
    <label class="search">
      <span aria-hidden="true">⌕</span>
      <input bind:value={query} placeholder="Filter notes or tags" aria-label="Filter scratchpads" />
    </label>
    <div class="list">
      {#each filtered as scratchpad (scratchpad.id)}
        <button
          type="button"
          class:active={selectedId === scratchpad.id}
          aria-pressed={selectedId === scratchpad.id}
          onclick={() => onSelect(scratchpad.id)}
        >
          <span class="note-mark" aria-hidden="true">▤</span>
          <span class="note-copy">
            <strong>{scratchpad.name}</strong>
            <small>
              rev {scratchpad.revision}
              {#if scratchpad.tags.length > 0} · {scratchpad.tags.slice(0, 2).join(' / ')}{/if}
            </small>
          </span>
        </button>
      {:else}
        <div class="empty-list">
          <span>{query ? 'No matching notes.' : 'No shared notes yet.'}</span>
          {#if !query}<button type="button" onclick={onRefresh}>Check again</button>{/if}
        </div>
      {/each}
    </div>
  </aside>

  <article class="viewer" aria-live="polite">
    {#if loading && read?.scratchpad.id !== selectedId}
      <div class="viewer-empty">
        <span class="loader" aria-hidden="true"></span>
        <p>Reading the shared buffer…</p>
      </div>
    {:else if read}
      <header>
        <div>
          <span class="eyebrow">Rendered markdown</span>
          <h3>{read.scratchpad.name}</h3>
        </div>
        <div class="revision" title="This view refreshes whenever the revision changes">
          <span class="live-dot" aria-hidden="true"></span>
          rev {read.scratchpad.revision} · {read.total_lines} lines
        </div>
      </header>
      {#if read.scratchpad.tags.length > 0}
        <div class="tags">
          {#each read.scratchpad.tags as tag}<span>{tag}</span>{/each}
        </div>
      {/if}
      <div class="content">
        {#if read.scratchpad.content.trim()}
          <MarkdownView source={read.scratchpad.content} />
        {:else}
          <p class="empty-note">This scratchpad is empty. Agent edits will appear here.</p>
        {/if}
      </div>
    {:else}
      <div class="viewer-empty">
        <span class="note-glyph" aria-hidden="true">▤</span>
        <span class="eyebrow">Shared project memory</span>
        <h3>{scratchpads.length === 0 ? 'No scratchpads yet' : 'Select a scratchpad'}</h3>
        <p>
          {scratchpads.length === 0
            ? 'Agents create scratchpads to leave durable notes, plans, and handoffs for the team.'
            : 'Agent notes render here and refresh as revisions land.'}
        </p>
        <button type="button" onclick={onRefresh}>Refresh notes</button>
      </div>
    {/if}
  </article>
</section>

<style>
  .scratchpads {
    display: grid;
    min-width: 0;
    min-height: 310px;
    grid-template-columns: minmax(190px, 0.32fr) minmax(0, 1fr);
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
  }

  .scratchpad-list {
    min-width: 0;
    border-right: 1px solid var(--border);
  }

  .scratchpad-list > header,
  .scratchpad-list > header > div,
  .viewer > header,
  .revision,
  .search {
    display: flex;
    align-items: center;
  }

  .scratchpad-list > header {
    justify-content: space-between;
    min-height: 36px;
    border-bottom: 1px solid var(--border);
    padding: 0 9px;
  }

  .scratchpad-list > header > div { gap: 7px; }

  .scratchpad-list > header strong,
  .scratchpad-list > header small,
  .note-copy small,
  .eyebrow,
  .revision,
  .tags,
  .search,
  .empty-list {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  .scratchpad-list > header strong {
    color: #c9cdd2;
    font-size: 10px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .scratchpad-list > header small { color: #7a818a; font-size: 8px; }

  .live-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--signal);
  }

  .search {
    gap: 7px;
    margin: 6px;
    border: 1px solid #3a3f46;
    border-radius: 3px;
    padding: 0 7px;
    background: #111315;
    color: #858c95;
  }

  .search input {
    min-width: 0;
    flex: 1;
    border: 0;
    outline: 0;
    padding: 6px 0;
    background: transparent;
    color: #d1d4d8;
    font: inherit;
    font-size: 8px;
  }

  .search input::placeholder { color: #506872; }

  .list {
    max-height: 355px;
    overflow-y: auto;
    padding: 0 5px 6px;
    scrollbar-color: #29434f transparent;
    scrollbar-width: thin;
  }

  .list button {
    display: grid;
    width: 100%;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 7px;
    border: 0;
    border-bottom: 1px solid #2d3136;
    padding: 7px 6px;
    background: transparent;
    color: #aebfc4;
    text-align: left;
    cursor: pointer;
  }

  .list button:hover { background: #202328; }
  .list button.active { background: #25282d; box-shadow: inset 2px 0 #747b84; }
  .note-mark { color: #777e87; font-size: 13px; }
  .list button.active .note-mark { color: #bdc1c7; }
  .note-copy { min-width: 0; }
  .note-copy strong { display: block; overflow: hidden; font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  .note-copy small { display: block; overflow: hidden; margin-top: 2px; color: #818892; font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .empty-list { display: grid; justify-items: center; gap: 7px; padding: 16px 7px; color: #858c95; font-size: 8px; text-align: center; }
  .empty-list button,
  .viewer-empty button { border: 1px solid #4a4f57; border-radius: 3px; padding: 6px 9px; background: #25282d; color: #e0e2e5; font: 650 9px 'Archivo Variable', sans-serif; cursor: pointer; }
  .empty-list button:hover,
  .viewer-empty button:hover { border-color: #707780; }

  .viewer { display: flex; min-width: 0; flex-direction: column; }

  .viewer > header {
    justify-content: space-between;
    gap: 15px;
    min-height: 46px;
    border-bottom: 1px solid var(--border);
    padding: 0 12px;
  }

  .eyebrow {
    color: #818892;
    font-size: 8px;
    font-weight: 650;
    letter-spacing: 0.11em;
    text-transform: uppercase;
  }

  .viewer h3 { margin: 2px 0 0; color: #eceef0; font-size: 15px; }
  .revision { flex: none; gap: 6px; color: #858c95; font-size: 8px; text-transform: uppercase; }

  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    border-bottom: 1px solid #203640;
    padding: 5px 12px;
  }

  .tags span {
    border: 1px solid #31505a;
    border-radius: 999px;
    padding: 2px 6px;
    color: #a0a6ae;
    font-size: 8px;
  }

  .content {
    min-height: 0;
    flex: 1;
    overflow: auto;
    padding: 14px 16px 24px;
    scrollbar-color: #2b4551 transparent;
    scrollbar-width: thin;
  }

  .viewer-empty {
    display: grid;
    min-height: 300px;
    place-content: center;
    justify-items: center;
    color: #617983;
    text-align: center;
  }

  .viewer-empty h3 { color: #8ca0a8; }
  .viewer-empty p, .empty-note { max-width: 380px; margin: 6px 0 13px; color: #5c737d; font-size: 10px; line-height: 1.55; }
  .note-glyph { color: #46636d; font-size: 28px; }

  .loader {
    width: 15px;
    height: 15px;
    border: 1px solid #33505b;
    border-top-color: var(--signal);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin { to { transform: rotate(360deg); } }

  @media (max-width: 760px) {
    .scratchpads { grid-template-columns: 1fr; }
    .scratchpad-list { border-right: 0; border-bottom: 1px solid #29424d; }
    .list { max-height: 180px; }
  }
</style>
