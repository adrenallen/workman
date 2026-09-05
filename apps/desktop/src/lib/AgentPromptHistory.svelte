<script lang="ts">
  import HistoryIcon from '@lucide/svelte/icons/history';
  import { Button } from './components/ui/button';
  import type { AgentPromptHistoryEntry } from './agentPromptHistory';
  import { writeTerminalClipboardText } from './terminalTransfers';

  let { entries, busy = false, onRestore, onClear, onError }: {
    entries: AgentPromptHistoryEntry[];
    busy?: boolean;
    onRestore: (entry: AgentPromptHistoryEntry) => void;
    onClear: () => void;
    onError: (message: string) => void;
  } = $props();
  let copied = $state<string | null>(null);

  async function copy(entry: AgentPromptHistoryEntry): Promise<void> {
    try {
      await writeTerminalClipboardText(entry.draft.prompt);
      copied = entry.id;
    } catch (cause) {
      onError(`Could not copy prompt: ${String(cause)}`);
    }
  }
</script>

<details class="history">
  <summary><HistoryIcon size={14} />Prompt history <span>{entries.length}</span></summary>
  <div class="history-content">
    <div class="history-heading">
      <small>Recent launches in this project, saved on this computer. Reuse opens a new draft with the saved instructions and settings.</small>
      {#if entries.length}<Button type="button" variant="ghost" size="sm" disabled={busy} onclick={onClear}>Clear history</Button>{/if}
    </div>
    {#each entries as entry (entry.id)}
      <details class="history-entry">
        <summary><strong>{entry.label}</strong><time datetime={new Date(entry.createdAt).toISOString()}>{new Date(entry.createdAt).toLocaleString()}</time></summary>
        <pre>{entry.draft.prompt || 'No written instructions'}</pre>
        <div class="entry-actions">
          <small>{entry.draft.attachments.length ? `${entry.draft.attachments.length} image(s) attached` : ''}{entry.draft.feedbackId !== null ? ' · Recorded feedback attached' : ''}</small>
          <Button type="button" variant="ghost" size="sm" disabled={!entry.draft.prompt} onclick={() => void copy(entry)}>{copied === entry.id ? 'Copied' : 'Copy instructions'}</Button>
          <Button type="button" variant="outline" size="sm" disabled={busy} onclick={() => onRestore(entry)}>Use in new draft</Button>
        </div>
      </details>
    {:else}
      <p>Your next agent launch will be saved here, including its template instructions.</p>
    {/each}
  </div>
</details>

<style>
  .history { border: 1px solid var(--border); border-radius: var(--radius); font-size: var(--font-size-sm); }
  summary { cursor: pointer; padding: 9px 10px; }
  .history > summary { display: flex; align-items: center; gap: 8px; font-weight: 550; }
  .history > summary span { margin-left: auto; color: var(--muted-foreground); }
  summary:focus-visible { outline: 2px solid var(--ring); outline-offset: -2px; }
  .history-content { max-height: 360px; overflow-y: auto; border-top: 1px solid var(--border); }
  .history-heading, .entry-actions { display: flex; align-items: center; gap: 8px; padding: 8px 10px; flex-wrap: wrap; }
  .history-heading small, .entry-actions small { flex: 1; color: var(--muted-foreground); }
  .history-entry { border-top: 1px solid var(--border); }
  .history-entry strong { overflow-wrap: anywhere; }
  time { margin-left: 8px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  pre { max-height: 200px; overflow: auto; margin: 0; padding: 8px 10px; background: var(--card); font: var(--font-size-xs)/1.5 var(--terminal-font-family); white-space: pre-wrap; overflow-wrap: anywhere; }
  p { padding: 0 10px 10px; color: var(--muted-foreground); }
</style>
