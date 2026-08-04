<script lang="ts">
  import MarkdownView from './MarkdownView.svelte';
  import type { ScratchpadRead } from './coordination';

  interface Props {
    read: ScratchpadRead | null;
    loading: boolean;
    onRefresh: () => void;
  }

  let { read, loading, onRefresh }: Props = $props();
</script>

{#if loading && !read}
  <div class="state">Loading scratchpad…</div>
{:else if read}
  <article class="scratchpad-document">
    <header>
      <div class="tags">
        {#each read.scratchpad.tags as tag}<span>{tag}</span>{/each}
      </div>
      <div class="revision">rev {read.scratchpad.revision} · {read.total_lines} lines</div>
      <button type="button" onclick={onRefresh}>Refresh</button>
    </header>
    <div class="content">
      {#if read.scratchpad.content.trim()}
        <MarkdownView source={read.scratchpad.content} />
      {:else}
        <p>This scratchpad is empty.</p>
      {/if}
    </div>
  </article>
{:else}
  <div class="state">Scratchpad not found.</div>
{/if}

<style>
  .scratchpad-document { display: grid; width: 100%; height: 100%; min-width: 0; grid-template-rows: auto minmax(0, 1fr); }
  header { display: flex; min-height: 34px; align-items: center; gap: 6px; border-bottom: 1px solid var(--border); padding: 4px 8px; }
  .tags { display: flex; min-width: 0; flex: 1; gap: 4px; overflow-x: auto; }
  .tags span { flex: none; border: 1px solid #3a3f46; border-radius: 3px; padding: 2px 5px; color: #9ba1aa; background: #1d2024; font: 7px 'JetBrains Mono Variable', monospace; }
  .revision { flex: none; color: var(--muted); font: 8px 'JetBrains Mono Variable', monospace; }
  button { flex: none; border: 1px solid #444950; border-radius: 3px; padding: 4px 7px; background: #24272b; color: #c8ccd1; font-size: 9px; cursor: pointer; }
  .content { min-height: 0; overflow: auto; padding: 14px 18px 28px; scrollbar-color: #41464d transparent; scrollbar-width: thin; }
  .content > :global(.markdown) { max-width: 880px; }
  .content > p { color: var(--muted); font-size: 11px; }
  .state { display: grid; width: 100%; height: 100%; place-items: center; color: var(--muted); font-size: 10px; }
</style>
