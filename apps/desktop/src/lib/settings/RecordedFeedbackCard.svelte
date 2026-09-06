<script lang="ts">
  import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
  import SaveIcon from '@lucide/svelte/icons/save';

  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Textarea } from '$lib/components/ui/textarea';
  import { Switch } from '$lib/components/ui/switch';
  import {
    recordedFeedbackPreferences,
    setRecordedFeedbackAgentPrompt,
    setRecordedFeedbackAutoArchive
  } from '../recordedFeedbackAvailability';
  import {
    defaultRecordedFeedbackAgentPrompt,
    feedbackContentToken,
    feedbackTitleToken
  } from '../recordedFeedbackPrompt';

  let prompt = $state($recordedFeedbackPreferences.agentPrompt);
  let error = $state<string | null>(null);
  let dirty = $derived(prompt !== $recordedFeedbackPreferences.agentPrompt);

  function save(): void {
    try {
      setRecordedFeedbackAgentPrompt(prompt);
      error = null;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function reset(): void {
    prompt = defaultRecordedFeedbackAgentPrompt;
    try {
      setRecordedFeedbackAgentPrompt(defaultRecordedFeedbackAgentPrompt);
      error = null;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }
</script>

<section class="feedback-card" aria-labelledby="feedback-settings-title">
  <header>
    <div>
      <h2 id="feedback-settings-title">Recorded Feedback</h2>
      <p>Choose how feedback is sent and organized afterward.</p>
    </div>
    <span class="saved"><StatusIndicator tone="success" label="Feedback preferences saved locally" />Saved locally</span>
  </header>

  <div class="archive-preference">
    <label for="feedback-auto-archive">
      <strong>Automatically archive after sending</strong>
      <small>Move feedback to Archived after a successful send to an agent or scratchpad. Copying keeps it active.</small>
    </label>
    <Switch id="feedback-auto-archive" checked={$recordedFeedbackPreferences.autoArchiveAfterSend} onCheckedChange={setRecordedFeedbackAutoArchive} />
  </div>

  <div class="editor">
    <div class="field-heading">
      <label for="feedback-agent-prompt">Agent delivery prompt</label>
      <span>The feedback itself can contain the task or requested action.</span>
    </div>
    <Textarea
      id="feedback-agent-prompt"
      bind:value={prompt}
      rows={9}
      spellcheck={true}
      aria-describedby="feedback-agent-prompt-help"
    />
    <div id="feedback-agent-prompt-help" class="prompt-help">
      <p><code>{feedbackContentToken}</code> inserts the ordered transcript and screenshots. <code>{feedbackTitleToken}</code> inserts its title. If the feedback marker is omitted, the feedback is appended.</p>
      <div class="actions">
        <Button variant="ghost" size="sm" disabled={prompt === defaultRecordedFeedbackAgentPrompt} onclick={reset}>
          <RotateCcwIcon size={14} />Reset default
        </Button>
        <Button size="sm" disabled={!dirty} onclick={save}>
          <SaveIcon size={14} />{dirty ? 'Save prompt' : 'Saved'}
        </Button>
      </div>
    </div>
    {#if error}<button class="error" type="button" onclick={() => (error = null)}>{error}<span>Dismiss</span></button>{/if}
  </div>
</section>

<style>
  .archive-preference { display: flex; align-items: center; justify-content: space-between; gap: 20px; border-top: 1px solid var(--border); padding: 16px 12px; }
  .archive-preference label { display: grid; gap: 5px; }
  .archive-preference strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 600; }
  .archive-preference small { max-width: 620px; color: var(--muted-foreground); font-size: var(--font-size-sm); line-height: 1.5; }
  .feedback-card { overflow: hidden; border: 1px solid var(--border); border-radius: 4px; background: var(--surface); }
  header { display: flex; min-height: 68px; align-items: flex-start; justify-content: space-between; gap: 16px; padding: 11px 12px 10px; }
  h2 { margin: 0; color: var(--text); font-size: 16px; line-height: 1.15; }
  header p { margin: 5px 0 0; color: var(--muted); font-size: var(--font-size-sm); }
  .saved { display: flex; align-items: center; gap: 6px; border: 1px solid var(--border); border-radius: 3px; padding: 5px 7px; background: var(--night); color: var(--muted); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .editor { display: grid; gap: 9px; border-top: 1px solid var(--border); padding: 13px 12px 12px; }
  .field-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 16px; }
  .field-heading label { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 660; }
  .field-heading span { color: var(--muted); font-size: var(--font-size-xs); text-align: right; }
  .editor :global(textarea) { min-height: 176px; resize: vertical; background: var(--night); font-family: 'JetBrains Mono Variable', monospace; font-size: var(--font-size-sm); line-height: 1.55; }
  .prompt-help { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .prompt-help p { max-width: 670px; margin: 0; color: var(--muted); font-size: var(--font-size-xs); line-height: 1.5; }
  code { border: 1px solid var(--border); border-radius: 3px; padding: 1px 4px; background: var(--night); color: var(--signal); font-family: 'JetBrains Mono Variable', monospace; }
  .actions { display: flex; flex: none; gap: 6px; }
  .error { display: flex; justify-content: space-between; border: 1px solid color-mix(in srgb, var(--destructive) 45%, var(--border)); border-radius: 4px; padding: 8px 10px; background: color-mix(in srgb, var(--destructive) 7%, var(--surface)); color: var(--destructive); font-size: var(--font-size-xs); text-align: left; }
  .error span { font-weight: 700; }
  @media (max-width: 700px) { .field-heading, .prompt-help { align-items: stretch; flex-direction: column; } .field-heading span { text-align: left; } .actions { justify-content: flex-end; } }
</style>
