<script lang="ts">
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import ArchiveRestoreIcon from '@lucide/svelte/icons/archive-restore';
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
  import BotIcon from '@lucide/svelte/icons/bot';
  import CheckIcon from '@lucide/svelte/icons/check';
  import ClipboardIcon from '@lucide/svelte/icons/clipboard';
  import ImageIcon from '@lucide/svelte/icons/image';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import NotebookTextIcon from '@lucide/svelte/icons/notebook-text';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import { invoke } from '@tauri-apps/api/core';

  import { Button } from '$lib/components/ui/button';
  import type { ProcessView } from './daemon';
  import {
    agentCanReceiveFeedback,
    agentFeedbackAvailability,
    feedbackDuration,
    feedbackStatusLabel,
    type RecordedFeedback,
    type RecordedFeedbackBlock
  } from './recordedFeedback';
  import { moveFeedbackBlock, removeFeedbackBlock, replaceFeedbackText } from './recordedFeedbackTimeline';

  interface Props {
    feedback: RecordedFeedback | null;
    loading: boolean;
    busy: boolean;
    processes: ProcessView[];
    onSave: (title: string, blocks: RecordedFeedbackBlock[], captions: Array<{ snapshot_id: number; caption: string }>) => Promise<void>;
    onSendAgent: (processId: number) => Promise<void>;
    onSendNewAgent: () => Promise<void>;
    onSendScratchpad: () => Promise<void>;
    onCopy: () => Promise<void>;
    onArchive: () => Promise<void>;
    onDelete: () => void;
  }

  let { feedback, loading, busy, processes, onSave, onSendAgent, onSendNewAgent,
    onSendScratchpad, onCopy, onArchive, onDelete }: Props = $props();

  let title = $state('');
  let blocks = $state<RecordedFeedbackBlock[]>([]);
  let captions = $state<Record<number, string>>({});
  let images = $state<Record<number, string>>({});
  let selectedAgentId = $state<number | null>(null);
  let localBusy = $state(false);
  let localError = $state<string | null>(null);
  let syncedKey = '';

  let agents = $derived(processes.filter((process) => process.kind === 'agent'));
  let eligibleAgents = $derived(agents.filter(agentCanReceiveFeedback));
  let dirty = $derived(Boolean(feedback && (
    title !== feedback.title
    || JSON.stringify(blocks) !== JSON.stringify(feedback.blocks)
    || feedback.snapshots.some((snapshot) => (captions[snapshot.id] ?? '') !== snapshot.caption)
  )));

  $effect(() => {
    if (!feedback) return;
    const key = `${feedback.id}:${feedback.revision}`;
    if (key === syncedKey) return;
    syncedKey = key;
    title = feedback.title;
    blocks = feedback.blocks.map((block) => ({ ...block }));
    captions = Object.fromEntries(feedback.snapshots.map((snapshot) => [snapshot.id, snapshot.caption]));
    for (const snapshot of feedback.snapshots) {
      if (!images[snapshot.id]) void loadImage(feedback.id, snapshot.id, snapshot.image_path);
    }
  });

  $effect(() => {
    const available = eligibleAgents;
    if (!selectedAgentId || !available.some((agent) => agent.id === selectedAgentId)) {
      selectedAgentId = available[0]?.id ?? null;
    }
  });

  async function loadImage(feedbackId: number, snapshotId: number, path: string): Promise<void> {
    try {
      const source = await invoke<string>('feedback_read_image', { feedbackId, path });
      if (feedback?.id === feedbackId) images = { ...images, [snapshotId]: source };
    } catch (cause) { localError = messageFor(cause); }
  }

  async function save(): Promise<void> {
    if (!feedback || !dirty) return;
    await run(async () => {
      await onSave(title, blocks, feedback.snapshots.map((snapshot) => ({
        snapshot_id: snapshot.id,
        caption: captions[snapshot.id] ?? ''
      })));
    });
  }

  async function afterSave(action: () => Promise<void>): Promise<void> {
    if (!feedback || feedback.status !== 'ready') return;
    await run(async () => {
      if (dirty) await onSave(title, blocks, feedback.snapshots.map((snapshot) => ({
        snapshot_id: snapshot.id,
        caption: captions[snapshot.id] ?? ''
      })));
      await action();
    });
  }

  async function archive(): Promise<void> {
    if (!feedback) return;
    if (feedback.status === 'ready') await afterSave(onArchive);
    else await run(onArchive);
  }

  async function run(action: () => Promise<void>): Promise<void> {
    if (localBusy || busy) return;
    localBusy = true;
    localError = null;
    try { await action(); }
    catch (cause) { localError = messageFor(cause); }
    finally { localBusy = false; }
  }

  function addText(): void {
    blocks = [...blocks, { kind: 'text', text: '', start_ms: feedback?.duration_ms ?? 0, end_ms: feedback?.duration_ms ?? 0 }];
  }

  function messageFor(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

{#if loading && !feedback}
  <section class="loading" aria-live="polite"><LoaderCircleIcon size={18} /><span>Loading recorded feedback…</span></section>
{:else if feedback}
  <section class="feedback-detail" aria-labelledby="feedback-title">
    <header>
      <div class="title-row">
        <span class:recording={feedback.status === 'recording'} class:failed={feedback.status === 'failed'} class="status-dot"></span>
        <input id="feedback-title" aria-label="Feedback title" bind:value={title} disabled={feedback.status !== 'ready' || localBusy || busy} />
        <span class="duration">{feedbackDuration(feedback.duration_ms)}</span>
      </div>
      <div class="metadata">
        {#if feedback.archived}<span>Archived</span>{/if}<span>{feedbackStatusLabel(feedback.status)}</span><span>{feedback.snapshots.length} snapshot{feedback.snapshots.length === 1 ? '' : 's'}</span><span>Revision {feedback.revision}</span>
      </div>
    </header>

    {#if feedback.status === 'transcribing'}
      <div class="state-card"><LoaderCircleIcon class="spin" size={20} /><div><strong>Creating your local transcript</strong><span>The recording is ready; Whisper is processing it entirely on this computer.</span></div></div>
    {:else if feedback.status === 'recording'}
      <div class="state-card recording-card"><span class="live-dot"></span><div><strong>Recording in progress</strong><span>Use the floating control to snap a region or display, then Finish.</span></div></div>
    {:else if feedback.status === 'failed'}
      <div class="state-card failed-card"><div><strong>This recording needs attention</strong><span>{feedback.error_code ?? 'The recording could not be completed.'} Start a new recording when you’re ready.</span></div></div>
    {:else}
      <div class="document" aria-label="Editable feedback transcript">
        {#each blocks as block, index (`${block.kind}-${block.kind === 'image' ? block.snapshot_id : index}`)}
          <article class:image-block={block.kind === 'image'} class="block">
            <div class="block-tools">
              <button type="button" aria-label="Move block up" disabled={index === 0 || localBusy} onclick={() => (blocks = moveFeedbackBlock(blocks, index, index - 1))}><ArrowUpIcon size={13} /></button>
              <button type="button" aria-label="Move block down" disabled={index === blocks.length - 1 || localBusy} onclick={() => (blocks = moveFeedbackBlock(blocks, index, index + 1))}><ArrowDownIcon size={13} /></button>
              <button type="button" aria-label="Remove block" disabled={localBusy} onclick={() => (blocks = removeFeedbackBlock(blocks, index))}><Trash2Icon size={13} /></button>
            </div>
            {#if block.kind === 'text'}
              <textarea aria-label={`Transcript block ${index + 1}`} value={block.text} rows={Math.max(3, Math.ceil(block.text.length / 95))} oninput={(event) => (blocks = replaceFeedbackText(blocks, index, event.currentTarget.value))}></textarea>
            {:else}
              {@const snapshot = feedback.snapshots.find((candidate) => candidate.id === block.snapshot_id)}
              {#if snapshot}
                <div class="image-frame">
                  {#if images[snapshot.id]}<img src={images[snapshot.id]} alt={captions[snapshot.id] || `Feedback snapshot ${snapshot.ordinal + 1}`} />{:else}<ImageIcon size={24} />{/if}
                </div>
                <div class="caption-row"><span>#{snapshot.ordinal + 1} · {feedbackDuration(snapshot.anchor_ms)}</span><input aria-label={`Caption for snapshot ${snapshot.ordinal + 1}`} placeholder="Add a caption…" value={captions[snapshot.id] ?? ''} oninput={(event) => (captions = { ...captions, [snapshot.id]: event.currentTarget.value })} /></div>
              {/if}
            {/if}
          </article>
        {/each}
        <button class="add-text" type="button" onclick={addText}><PlusIcon size={14} /> Add text</button>
      </div>
    {/if}

    {#if localError}<button class="inline-error" type="button" onclick={() => (localError = null)}>{localError}<span>Dismiss</span></button>{/if}

    <footer>
      <div class="manage-actions">
        {#if feedback.status === 'ready'}<Button variant="outline" size="sm" disabled={!dirty || localBusy || busy} onclick={() => void save()}>{dirty ? 'Save changes' : 'Saved'}{#if !dirty}<CheckIcon size={13} />{/if}</Button>{/if}
        <Button variant="ghost" size="sm" disabled={localBusy || busy || feedback.status === 'recording'} onclick={() => void archive()}>
          {#if feedback.archived}<ArchiveRestoreIcon size={14} />Restore{:else}<ArchiveIcon size={14} />Archive{/if}
        </Button>
        <Button variant="ghost" size="sm" class="delete" disabled={localBusy || busy || feedback.status === 'recording'} onclick={onDelete}><Trash2Icon size={14} />Delete</Button>
      </div>
      {#if feedback.status === 'ready'}
        <div class="send-actions">
          <select aria-label="Agent target" bind:value={selectedAgentId} disabled={localBusy || busy || eligibleAgents.length === 0}>
            {#if eligibleAgents.length === 0}<option value={null}>No ready agents</option>{/if}
            {#each agents as agent}<option value={agent.id} disabled={!agentCanReceiveFeedback(agent)}>{agent.name} · {agentFeedbackAvailability(agent)}</option>{/each}
          </select>
          <Button size="sm" disabled={!selectedAgentId || localBusy || busy} onclick={() => void afterSave(() => onSendAgent(selectedAgentId!))}><BotIcon size={14} />Send</Button>
          <Button variant="outline" size="sm" disabled={localBusy || busy} onclick={() => void afterSave(onSendNewAgent)}><PlusIcon size={14} />New agent</Button>
          <Button variant="outline" size="sm" disabled={localBusy || busy} onclick={() => void afterSave(onSendScratchpad)}><NotebookTextIcon size={14} />Scratchpad</Button>
          <Button variant="ghost" size="sm" disabled={localBusy || busy} aria-label="Copy packet prompt" title="Copy a prompt that points to the immutable local packet" onclick={() => void afterSave(onCopy)}><ClipboardIcon size={14} />Copy</Button>
        </div>
      {/if}
    </footer>
  </section>
{/if}

<style>
  .feedback-detail { display: grid; width: min(980px, 100%); height: 100%; min-height: 0; margin: 0 auto; grid-template-rows: auto minmax(0, 1fr) auto; background: var(--background); color: var(--foreground); }
  header { border-bottom: 1px solid var(--border); padding: 18px 22px 13px; }
  .title-row { display: grid; grid-template-columns: 10px minmax(0, 1fr) auto; align-items: center; gap: 10px; }
  .title-row input { min-width: 0; border: 0; border-bottom: 1px solid transparent; outline: 0; padding: 2px 0; background: transparent; color: var(--foreground); font-size: 20px; font-weight: 720; }
  .title-row input:focus { border-bottom-color: var(--ring); }
  .status-dot, .live-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--success); }
  .status-dot.recording, .live-dot { background: #ff4d5e; box-shadow: 0 0 0 3px rgb(255 77 94 / 14%); }
  .status-dot.failed { background: var(--destructive); }
  .duration, .metadata { color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .metadata { display: flex; gap: 9px; margin: 7px 0 0 20px; }
  .metadata span + span::before { margin-right: 9px; content: '·'; }
  .document { overflow: auto; min-height: 0; padding: 18px 22px max(32px, env(safe-area-inset-bottom)); }
  .block { position: relative; margin: 0 auto 12px; border: 1px solid transparent; border-radius: 6px; padding: 8px 40px 8px 8px; }
  .block:hover, .block:focus-within { border-color: var(--border); background: color-mix(in srgb, var(--card) 75%, transparent); }
  .block-tools { position: absolute; top: 7px; right: 6px; display: grid; opacity: 0; }
  .block:hover .block-tools, .block:focus-within .block-tools { opacity: 1; }
  .block-tools button { display: grid; width: 25px; height: 25px; place-items: center; border: 0; border-radius: 3px; background: transparent; color: var(--muted-foreground); }
  .block-tools button:hover:not(:disabled) { background: var(--muted); color: var(--foreground); }
  textarea { box-sizing: border-box; width: 100%; resize: vertical; border: 0; outline: 0; padding: 5px 7px; background: transparent; color: var(--foreground); font: 15px/1.58 'Inter Variable', sans-serif; }
  .image-frame { display: grid; overflow: hidden; min-height: 140px; place-items: center; border: 1px solid var(--border); border-radius: 5px; background: var(--night); color: var(--muted-foreground); }
  .image-frame img { display: block; width: 100%; max-height: 560px; object-fit: contain; }
  .caption-row { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: 10px; padding: 8px 3px 1px; }
  .caption-row span { color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .caption-row input { min-width: 0; border: 0; border-bottom: 1px solid var(--border); outline: 0; padding: 4px 2px; background: transparent; color: var(--foreground); font-size: var(--font-size-sm); }
  .caption-row input:focus { border-bottom-color: var(--ring); }
  .add-text { display: flex; align-items: center; gap: 6px; margin: 6px 8px; border: 0; padding: 7px 9px; background: transparent; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .add-text:hover { color: var(--foreground); }
  .state-card { display: flex; align-self: start; gap: 12px; margin: 22px; border: 1px solid var(--border); border-radius: 7px; padding: 16px; background: var(--card); }
  .state-card strong, .state-card span { display: block; }
  .state-card span { margin-top: 4px; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .recording-card { border-color: rgb(255 77 94 / 35%); }
  .failed-card { border-color: color-mix(in srgb, var(--destructive) 40%, var(--border)); }
  :global(.spin) { animation: spin 1s linear infinite; }
  footer { position: relative; display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 9px; border-top: 1px solid var(--border); padding: 10px 14px max(10px, env(safe-area-inset-bottom)); background: color-mix(in srgb, var(--card) 94%, transparent); backdrop-filter: blur(12px); }
  .manage-actions, .send-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 5px; }
  .send-actions select { max-width: 210px; height: 32px; border: 1px solid var(--border-strong); border-radius: 4px; padding: 0 7px; background: var(--background); color: var(--foreground); font-size: var(--font-size-xs); }
  :global(.delete) { color: var(--destructive); }
  .inline-error { display: flex; justify-content: space-between; margin: 0 22px 12px; border: 1px solid color-mix(in srgb, var(--destructive) 45%, var(--border)); border-radius: 5px; padding: 8px 10px; background: color-mix(in srgb, var(--destructive) 7%, var(--surface)); color: var(--destructive); text-align: left; }
  .inline-error span { font-weight: 700; }
  .loading { display: flex; min-height: 180px; align-items: center; justify-content: center; gap: 8px; color: var(--muted-foreground); }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 800px) { footer { align-items: stretch; } .send-actions { width: 100%; } .send-actions select { max-width: none; flex: 1; } }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
</style>
