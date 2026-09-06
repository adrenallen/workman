<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import FileImageIcon from '@lucide/svelte/icons/file-image';
  import MessageSquareMoreIcon from '@lucide/svelte/icons/message-square-more';
  import SlidersHorizontalIcon from '@lucide/svelte/icons/sliders-horizontal';
  import XIcon from '@lucide/svelte/icons/x';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview';
  import { onDestroy, onMount } from 'svelte';

  import AgentBrandMark from './AgentBrandMark.svelte';
  import AgentPromptHistory from './AgentPromptHistory.svelte';
  import VoiceInputButton from './VoiceInputButton.svelte';
  import type { AgentPromptHistoryEntry } from './agentPromptHistory';
  import {
    agentDraftImageToken,
    attachmentName,
    attachImagePaths as selectAttachmentPaths,
    handleNativePromptDrop as resolveNativePromptDrop,
    insertAgentDraftImageTokens,
    removeAgentDraftAttachment,
    maxAgentDraftAttachments
  } from './agentAttachmentDrafts.ts';
  import {
    agentTemplateSelectionChange,
    agentTemplateRosterChoices,
    isStandaloneAgentSelected,
    resolveAgentDraftChoice
  } from './agentDraftChoices';
  import {
    AGENT_EFFORT_LEVELS,
    agentModelSuggestions,
    agentSupportsEffort,
    agentSupportsModel,
    configuredAgentLaunchOptions,
    splitAgentLaunchOptions,
    withAgentLaunchOptions
  } from './agentLaunchOptions';
  import CreationDraftScaffold from './CreationDraftScaffold.svelte';
  import { parseExtraArgs, type AgentTool, type SpawnAgentInput } from './agentTools';
  import {
    choiceValue,
    lastAgentChoiceStorageKey,
    type AgentChoice,
    type AgentTemplate
  } from './agentTemplates';
  import type { AgentCreationDraft } from './creationDrafts';
  import { Button } from './components/ui/button';
  import IconButton from './components/ds/IconButton.svelte';
  import * as Collapsible from './components/ui/collapsible';
  import { Input } from './components/ui/input';
  import * as Select from './components/ui/select';
  import { Textarea } from './components/ui/textarea';
  import {
    hotkeyDisplayLabel,
    hotkeyPreferences
  } from './hotkeys';

  interface AttachmentImageRead {
    bytes: number[];
    mime_type: string;
  }

  interface AgentDraftSubmission {
    input: SpawnAgentInput;
    tool: AgentTool;
    template: AgentTemplate | null;
  }

  interface PromptSelection {
    start: number;
    end: number;
  }

  interface Props {
    draft: AgentCreationDraft;
    promptHistory?: AgentPromptHistoryEntry[];
    onRestorePrompt?: (entry: AgentPromptHistoryEntry) => void;
    onClearPromptHistory?: () => void;
    projectName: string;
    tools: AgentTool[];
    templates: AgentTemplate[];
    loading?: boolean;
    metadataLoaded?: boolean;
    focusOnMount?: boolean;
    busy?: boolean;
    onChange: (patch: Partial<AgentCreationDraft>) => void;
    onInitialize: (patch: Partial<AgentCreationDraft>) => void;
    onCreate: (submission: AgentDraftSubmission) => void | Promise<void>;
    onDiscard: () => void;
    onInitialFocusHandled?: () => void;
    onOpenSettings?: () => void;
    onError?: (message: string) => void;
  }

  let {
    draft,
    promptHistory = [],
    onRestorePrompt = () => undefined,
    onClearPromptHistory = () => undefined,
    projectName,
    tools,
    templates,
    loading = false,
    metadataLoaded = false,
    focusOnMount = false,
    busy = false,
    onChange,
    onInitialize,
    onCreate,
    onDiscard,
    onInitialFocusHandled = () => undefined,
    onOpenSettings,
    onError = () => undefined
  }: Props = $props();

  let modelSettingsOpen = $state(false);
  let templateInstructionsOpen = $state(false);
  let templateAgentOpen = $state(false);
  let promptTextarea = $state<HTMLTextAreaElement | null>(null);
  let promptField = $state<HTMLDivElement | null>(null);
  let attachmentSaving = $state(false);
  let dictationBusy = $state(false);
  let attachmentDropActive = $state(false);
  let attachmentPreviews = $state<Record<string, string>>({});
  let failedAttachmentPreviews = $state<Record<string, true>>({});
  let removeNativeDropListener: (() => void) | null = null;
  let destroyed = false;

  const choice = $derived(resolveAgentDraftChoice(
    draft,
    tools,
    templates,
    metadataLoaded,
    readLastChoice()
  ));
  const enabledTools = $derived(choice.enabledTools);
  const availableTemplates = $derived(choice.availableTemplates);
  const templateChoices = $derived(agentTemplateRosterChoices(templates, tools));
  const selectedTemplate = $derived(choice.selectedTemplate);
  const selectedTool = $derived(choice.selectedTool);
  const templateDefaultTool = $derived(
    selectedTemplate
      ? tools.find((tool) => tool.id === selectedTemplate.agent_tool_id) ?? null
      : null
  );
  const agentOverridden = $derived(
    selectedTemplate !== null
      && selectedTool !== null
      && selectedTool.id !== selectedTemplate.agent_tool_id
  );
  const inheritedLaunchOptions = $derived(configuredAgentLaunchOptions(
    selectedTool,
    selectedTemplate && !agentOverridden ? selectedTemplate.extra_args : []
  ));
  const modelSupported = $derived(agentSupportsModel(selectedTool?.tool_type));
  const effortSupported = $derived(agentSupportsEffort(selectedTool?.tool_type));
  const modelSuggestions = $derived(agentModelSuggestions(selectedTool?.tool_type));
  const canCreate = $derived(
    !loading
      && !attachmentSaving
      && !dictationBusy
      && selectedTool !== null
      && !choice.missingTemplate
      && !choice.missingTool
  );
  $effect(() => {
    const initial = choice.initialChoice;
    if (!initial) return;
    const templateId = initial.kind === 'template' ? initial.id : null;
    const agentToolId = initial.kind === 'template'
      ? initial.agentToolId ?? availableTemplates.find((template) => template.id === initial.id)?.agent_tool_id ?? null
      : initial.id;
    if (draft.templateId !== templateId || draft.agentToolId !== agentToolId) {
      onInitialize({ templateId, agentToolId });
    }
  });

  $effect(() => {
    if (!focusOnMount) return;
    requestAnimationFrame(() => {
      promptTextarea?.focus();
      onInitialFocusHandled();
    });
  });

  function readLastChoice(): string | null {
    try {
      return localStorage.getItem(lastAgentChoiceStorageKey);
    } catch {
      return null;
    }
  }

  function rememberChoice(choice: AgentChoice): void {
    try {
      localStorage.setItem(lastAgentChoiceStorageKey, choiceValue(choice));
    } catch {
      // Draft editing remains available if webview storage is unavailable.
    }
  }

  function selectTemplate(template: AgentTemplate): void {
    const selection = agentTemplateSelectionChange(selectedTemplate, template);
    if (!selection) return;
    templateInstructionsOpen = false;
    templateAgentOpen = false;
    onChange({
      templateId: selection.id,
      agentToolId: selection.agentToolId,
      ...(draft.agentToolId !== selection.agentToolId ? { model: '', effort: '' } : {})
    });
    rememberChoice(selection);
  }

  function selectStandaloneAgent(tool: AgentTool): void {
    templateInstructionsOpen = false;
    templateAgentOpen = false;
    onChange({
      templateId: null,
      agentToolId: tool.id,
      ...(draft.agentToolId !== tool.id ? { model: '', effort: '' } : {})
    });
    rememberChoice({ kind: 'tool', id: tool.id });
  }

  function selectTemplateAgent(tool: AgentTool): void {
    if (!selectedTemplate) return;
    onChange({
      agentToolId: tool.id,
      ...(draft.agentToolId !== tool.id ? { model: '', effort: '' } : {})
    });
    rememberChoice({ kind: 'template', id: selectedTemplate.id, agentToolId: tool.id });
    templateAgentOpen = false;
  }

  function templateInstructionsSummary(prompt: string): string {
    const summary = prompt.replace(/\s+/g, ' ').trim();
    if (!summary) return 'No template instructions';
    const characters = Array.from(summary);
    if (characters.length <= 92) return summary;
    const excerpt = characters.slice(0, 92).join('').trimEnd();
    const lastWordBoundary = excerpt.lastIndexOf(' ');
    return `${(lastWordBoundary > 0 ? excerpt.slice(0, lastWordBoundary) : excerpt).trimEnd()}…`;
  }

  function submit(): void {
    if (!canCreate || !selectedTool || busy) return;
    let extraArgs: string[];
    let requestedModel: string | null;
    try {
      const parsed = splitAgentLaunchOptions(
        parseExtraArgs(draft.extraArgs),
        selectedTool.tool_type
      );
      requestedModel = draft.model.trim() || parsed.model;
      extraArgs = withAgentLaunchOptions(
        parsed.extraArgs,
        selectedTool.tool_type,
        null,
        draft.effort || parsed.effort
      );
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
      return;
    }
    rememberChoice(selectedTemplate
      ? { kind: 'template', id: selectedTemplate.id, agentToolId: selectedTool.id }
      : { kind: 'tool', id: selectedTool.id });
    void onCreate({
      input: {
        project_id: draft.projectId,
        ...(selectedTemplate
          ? { agent_template_id: selectedTemplate.id, agent_tool_id: selectedTool.id }
          : { agent_tool_id: selectedTool.id }),
        name: draft.name.trim() || undefined,
        model: requestedModel || undefined,
        extra_args: extraArgs,
        prompt: draft.prompt.trim() || undefined,
        attachments: draft.attachments.length > 0 ? [...draft.attachments] : undefined
      },
      tool: selectedTool,
      template: selectedTemplate
    });
  }

  function currentPromptSelection(): PromptSelection {
    return {
      start: promptTextarea?.selectionStart ?? draft.prompt.length,
      end: promptTextarea?.selectionEnd ?? draft.prompt.length
    };
  }

  async function attachImages(
    files: File[],
    insertion = currentPromptSelection()
  ): Promise<void> {
    if (attachmentSaving) {
      onError('Image attachments are already being saved.');
      return;
    }
    const available = Math.max(0, maxAgentDraftAttachments - draft.attachments.length);
    const images = files.filter((file) => file.type.startsWith('image/')).slice(0, available);
    if (images.length === 0) {
      if (available === 0) onError(`A new-agent draft can have at most ${maxAgentDraftAttachments} image attachments.`);
      return;
    }
    attachmentSaving = true;
    const paths: string[] = [];
    const previews: Record<string, string> = {};
    try {
      for (const image of images) {
        const bytes = Array.from(new Uint8Array(await image.arrayBuffer()));
        const path = await invoke<string>('terminal_save_draft_image', {
          bytes,
          mimeType: image.type
        });
        paths.push(path);
        previews[path] = URL.createObjectURL(image);
      }
      commitAttachmentPaths(paths, previews, insertion);
    } catch (cause) {
      for (const preview of Object.values(previews)) URL.revokeObjectURL(preview);
      onError(`Could not attach image: ${cause instanceof Error ? cause.message : String(cause)}`);
    } finally {
      attachmentSaving = false;
    }
  }

  async function importAttachmentPaths(
    paths: string[],
    insertion = currentPromptSelection()
  ): Promise<void> {
    if (attachmentSaving) {
      onError('Image attachments are already being saved.');
      return;
    }
    const selection = selectAttachmentPaths(draft.attachments, paths);
    if (selection.added.length === 0) {
      if (selection.capReached) {
        onError(`A new-agent draft can have at most ${maxAgentDraftAttachments} image attachments.`);
      }
      return;
    }
    attachmentSaving = true;
    const imported: string[] = [];
    try {
      for (const path of selection.added) {
        imported.push(await invoke<string>('terminal_import_draft_image', { path }));
      }
      commitAttachmentPaths(imported, {}, insertion);
      await loadAttachmentPreviews(imported, false);
    } catch (cause) {
      onError(`Could not attach image: ${cause instanceof Error ? cause.message : String(cause)}`);
    } finally {
      attachmentSaving = false;
    }
  }

  function commitAttachmentPaths(
    paths: string[],
    previews: Record<string, string> = {},
    insertion = currentPromptSelection()
  ): void {
    const current = draft.attachments;
    const available = Math.max(0, maxAgentDraftAttachments - current.length);
    const existing = new Set(current);
    const committed = paths.filter((path) => !existing.has(path)).slice(0, available);
    const committedSet = new Set(committed);
    for (const [path, preview] of Object.entries(previews)) {
      if (!committedSet.has(path)) URL.revokeObjectURL(preview);
    }
    attachmentPreviews = {
      ...attachmentPreviews,
      ...Object.fromEntries(Object.entries(previews).filter(([path]) => committedSet.has(path)))
    };
    if (committed.length > 0) {
      const nextPrompt = insertAgentDraftImageTokens(
        draft.prompt,
        insertion.start,
        insertion.end,
        current.length,
        committed.length
      );
      onChange({
        attachments: [...current, ...committed],
        prompt: nextPrompt.prompt
      });
      requestAnimationFrame(() => {
        promptTextarea?.focus();
        promptTextarea?.setSelectionRange(nextPrompt.caret, nextPrompt.caret);
      });
    }
    if (committed.length < paths.length) {
      onError(`A new-agent draft can have at most ${maxAgentDraftAttachments} image attachments.`);
    }
  }

  function handlePromptPaste(event: ClipboardEvent): void {
    const images = Array.from(event.clipboardData?.items ?? [])
      .filter((item) => item.kind === 'file' && item.type.startsWith('image/'))
      .map((item) => item.getAsFile())
      .filter((file): file is File => file !== null);
    if (images.length === 0) return;
    event.preventDefault();
    event.stopPropagation();
    void attachImages(images, currentPromptSelection());
  }

  function handlePromptDrop(event: DragEvent): void {
    const files = Array.from(event.dataTransfer?.files ?? []);
    if (!files.some((file) => file.type.startsWith('image/'))) return;
    event.preventDefault();
    event.stopPropagation();
    attachmentDropActive = false;
    void attachImages(files, currentPromptSelection());
  }

  function handleNativePromptDrop(payload: DragDropEvent): void {
    const result = resolveNativePromptDrop(
      payload,
      promptField?.getBoundingClientRect() ?? null,
      window.devicePixelRatio,
      draft.attachments
    );
    attachmentDropActive = result.dropActive;
    if (result.selection?.added.length) {
      void importAttachmentPaths(result.selection.added, currentPromptSelection());
    } else if (result.selection?.capReached) {
      onError(`A new-agent draft can have at most ${maxAgentDraftAttachments} image attachments.`);
    }
  }

  function removeAttachment(path: string): void {
    const preview = attachmentPreviews[path];
    if (preview) URL.revokeObjectURL(preview);
    const { [path]: _, ...remainingPreviews } = attachmentPreviews;
    attachmentPreviews = remainingPreviews;
    const { [path]: __, ...remainingFailures } = failedAttachmentPreviews;
    failedAttachmentPreviews = remainingFailures;
    onChange(removeAgentDraftAttachment(draft.prompt, draft.attachments, path));
  }

  function attachmentPreview(path: string): string {
    return failedAttachmentPreviews[path] ? '' : attachmentPreviews[path] ?? '';
  }

  function handleAttachmentPreviewError(path: string): void {
    const preview = attachmentPreviews[path];
    if (preview) URL.revokeObjectURL(preview);
    failedAttachmentPreviews = { ...failedAttachmentPreviews, [path]: true };
  }

  async function loadAttachmentPreviews(paths: string[], dropDead: boolean): Promise<void> {
    if (!isTauri()) return;
    const loaded: Record<string, string> = {};
    const dead: string[] = [];
    for (const path of paths) {
      if (attachmentPreviews[path]) continue;
      try {
        const image = await invoke<AttachmentImageRead>('terminal_read_attachment_image', { path });
        const preview = URL.createObjectURL(new Blob(
          [new Uint8Array(image.bytes)],
          { type: image.mime_type }
        ));
        if (destroyed) URL.revokeObjectURL(preview);
        else loaded[path] = preview;
      } catch {
        if (dropDead) dead.push(path);
      }
    }
    if (destroyed) return;
    attachmentPreviews = { ...attachmentPreviews, ...loaded };
    if (dead.length > 0) {
      const next = dead.reduce(
        (current, path) => removeAgentDraftAttachment(current.prompt, current.attachments, path),
        { prompt: draft.prompt, attachments: [...draft.attachments] }
      );
      onChange(next);
      onError(`Removed ${dead.length} missing or unreadable image attachment${dead.length === 1 ? '' : 's'} from this draft.`);
    }
  }

  onMount(() => {
    if (!isTauri()) return;
    void loadAttachmentPreviews([...draft.attachments], true);
    void getCurrentWebview()
      .onDragDropEvent((event) => handleNativePromptDrop(event.payload))
      .then((unlisten) => {
        if (destroyed) unlisten();
        else removeNativeDropListener = unlisten;
      })
      .catch((cause) => {
        if (!destroyed) onError(`Could not listen for image drops: ${cause instanceof Error ? cause.message : String(cause)}`);
      });
  });

  onDestroy(() => {
    destroyed = true;
    removeNativeDropListener?.();
    for (const preview of Object.values(attachmentPreviews)) URL.revokeObjectURL(preview);
  });
</script>

<CreationDraftScaffold
  {projectName}
  kindLabel="Agent"
  title={draft.name.trim() || 'New agent'}
  createLabel="Create agent"
  {busy}
  {canCreate}
  showFooterCreate={false}
  onCreate={submit}
  {onDiscard}
>
  {#if draft.feedbackId !== null}
    <div class="feedback-handoff" role="note">
      <MessageSquareMoreIcon size={17} strokeWidth={1.8} />
      <span><strong>Feedback attached</strong>The transcript and screenshots will be sent automatically when this agent is ready.</span>
    </div>
  {/if}
  {#snippet secondaryAction()}
    {#if onOpenSettings}
      <Button type="button" variant="outline" disabled={busy} onclick={onOpenSettings}>Open Settings</Button>
    {/if}
  {/snippet}
  <section class="agent-fields">
    <AgentPromptHistory entries={promptHistory} {busy} onRestore={onRestorePrompt} onClear={onClearPromptHistory} {onError} />
    <fieldset
      class="launch-fieldset"
      disabled={loading || busy}
      aria-busy={loading}
      aria-describedby={`draft-agent-choice-help-${draft.id}`}
    >
      <legend>Start from</legend>
      <p id={`draft-agent-choice-help-${draft.id}`} class="selection-help">
        Choose a template, or launch a model or tool directly.
      </p>
      <div class="launch-roster" class:roster-loading={loading}>
        {#if loading}
          <div class="loading-choice" role="status">Loading launch choices…</div>
        {:else}
          {#if templateChoices.length > 0}
            <section class="roster-group" aria-labelledby={`draft-agent-templates-${draft.id}`}>
              <div class="roster-heading">
                <h2 id={`draft-agent-templates-${draft.id}`}>Templates</h2><span>Prompt and setup included</span>
              </div>
              <div class="roster-options">
                {#each templateChoices as templateChoice (templateChoice.template.id)}
                  {@const template = templateChoice.template}
                  {@const tool = templateChoice.tool}
                  <label class="launch-choice">
                    <input
                      class="choice-radio"
                      type="radio"
                      name={`draft-agent-launch-${draft.id}`}
                      checked={selectedTemplate?.id === template.id}
                      disabled={!tool.enabled}
                      onclick={() => selectTemplate(template)}
                    />
                    <span class="choice-card">
                      <AgentBrandMark {tool} size={18} />
                      <span class="choice-copy">
                        <strong>{template.name}</strong>
                        <small>{tool.name}{#if !tool.enabled} · agent disabled{/if}</small>
                      </span>
                      <span class="choice-indicator" aria-hidden="true"></span>
                    </span>
                  </label>
                {/each}
              </div>
            </section>
          {/if}

          <section class="roster-group" aria-labelledby={`draft-agent-tools-${draft.id}`}>
            <div class="roster-heading">
              <h2 id={`draft-agent-tools-${draft.id}`}>Models &amp; tools</h2><span>Launch directly</span>
            </div>
            <div class="roster-options">
              {#each enabledTools as tool (tool.id)}
                <label class="launch-choice">
                  <input
                    class="choice-radio"
                    type="radio"
                    name={`draft-agent-launch-${draft.id}`}
                    checked={isStandaloneAgentSelected(choice, tool.id)}
                    onclick={() => selectStandaloneAgent(tool)}
                  />
                  <span class="choice-card">
                    <AgentBrandMark {tool} size={16} />
                    <span class="choice-copy"><span>{tool.name}</span><small>Standalone agent</small></span>
                    <span class="choice-indicator" aria-hidden="true"></span>
                  </span>
                </label>
              {/each}
            </div>
          </section>
        {/if}
      </div>
      {#if choice.missingTemplate}
        <small class="choice-warning">Template #{draft.templateId} is no longer available. Choose another template or a standalone agent.</small>
      {/if}
      {#if choice.missingTool}
        <small class="choice-warning">Agent #{draft.agentToolId} is no longer available. Choose another agent to create this draft.</small>
      {/if}
    </fieldset>

    {#if selectedTemplate}
      <section class="template-options" aria-labelledby={`draft-template-options-${draft.id}`}>
        <div class="template-options-heading">
          <span>Selected template</span>
          <h2 id={`draft-template-options-${draft.id}`}>{selectedTemplate.name}</h2>
        </div>

        <div class="template-detail">
          <Collapsible.Root bind:open={templateInstructionsOpen}>
            <Collapsible.Trigger class="template-detail-trigger">
              <span class="template-detail-copy">
                <strong>Template instructions</strong>
                <small>{templateInstructionsSummary(selectedTemplate.prompt)}</small>
              </span>
              <ChevronDownIcon class={`template-detail-chevron ${templateInstructionsOpen ? 'open' : ''}`} size={14} aria-hidden="true" />
            </Collapsible.Trigger>
            <Collapsible.Content>
              <div class="template-preview" aria-label="Template instructions preview">{selectedTemplate.prompt || 'No template instructions'}</div>
            </Collapsible.Content>
          </Collapsible.Root>
        </div>

        <div class="template-detail">
          <Collapsible.Root bind:open={templateAgentOpen}>
            <Collapsible.Trigger class="template-detail-trigger">
              <span class="template-detail-copy">
                <strong>Template agent</strong>
                <small>{choice.missingTool ? 'Agent unavailable · choose another' : `${selectedTool?.name ?? 'Choose an agent'} · ${agentOverridden ? 'Override' : 'Template default'}`}</small>
              </span>
              <ChevronDownIcon class={`template-detail-chevron ${templateAgentOpen ? 'open' : ''}`} size={14} aria-hidden="true" />
            </Collapsible.Trigger>
            <Collapsible.Content>
              <fieldset class="override-fieldset" disabled={loading || busy}>
                <legend class="sr-only">Choose a template agent override</legend>
                <div class="override-options">
                  {#each enabledTools as tool (tool.id)}
                    <label class="override-choice">
                      <input
                        class="choice-radio"
                        type="radio"
                        name={`draft-agent-override-${draft.id}`}
                        checked={selectedTool?.id === tool.id}
                        onclick={() => selectTemplateAgent(tool)}
                      />
                      <span class="choice-card compact">
                        <AgentBrandMark {tool} size={16} />
                        <span class="choice-copy">
                          <strong>{tool.name}</strong>
                          <small>{tool.id === selectedTemplate.agent_tool_id ? 'Template default' : 'Override'}</small>
                        </span>
                        <span class="choice-indicator" aria-hidden="true"></span>
                      </span>
                    </label>
                  {/each}
                </div>
                {#if agentOverridden && templateDefaultTool}
                  <small class="override-note">Template launch args are skipped when using {selectedTool?.name} instead of {templateDefaultTool.name}.</small>
                {/if}
              </fieldset>
            </Collapsible.Content>
          </Collapsible.Root>
        </div>
      </section>
    {/if}

    <Collapsible.Root bind:open={modelSettingsOpen} class="overflow-hidden rounded-md border border-border">
      <Collapsible.Trigger class="flex min-h-9 w-full items-center gap-2 px-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
        <SlidersHorizontalIcon class="text-muted-foreground" size={14} />
        <span class="min-w-0 flex-1 text-sm font-medium">Model settings</span>
        <ChevronDownIcon class={`text-muted-foreground ${modelSettingsOpen ? 'rotate-180' : ''}`} size={14} />
      </Collapsible.Trigger>
      <Collapsible.Content>
        <div class="model-settings-grid">
          <label class="field-label" for={`draft-agent-name-${draft.id}`}><span>Name <small>optional</small></span><Input id={`draft-agent-name-${draft.id}`} value={draft.name} placeholder={`${selectedTool?.name.toLowerCase() ?? 'agent'} worker`} disabled={busy} oninput={(event) => onChange({ name: event.currentTarget.value })} /></label>
          <label class="field-label" for={`draft-agent-args-${draft.id}`}><span>Other launch args <small>optional</small></span><Input id={`draft-agent-args-${draft.id}`} value={draft.extraArgs} placeholder="--permission-mode plan" disabled={busy} autocapitalize="off" autocorrect="off" spellcheck={false} oninput={(event) => onChange({ extraArgs: event.currentTarget.value })} /></label>
          {#if modelSupported || effortSupported}
            <section class="launch-tuning" aria-labelledby={`draft-agent-tuning-${draft.id}`}>
              <div class="launch-tuning-heading">
                <span id={`draft-agent-tuning-${draft.id}`}>Model &amp; effort</span>
                <small>
                  {#if inheritedLaunchOptions.model || inheritedLaunchOptions.effort}
                    Inherits {inheritedLaunchOptions.model ?? 'the configured model'}{#if inheritedLaunchOptions.effort} · {inheritedLaunchOptions.effort} effort{/if}
                  {:else}
                    Uses the selected agent's defaults
                  {/if}
                </small>
              </div>
              <div class="launch-tuning-fields">
                {#if modelSupported}
                  <label class="field-label" for={`draft-agent-model-${draft.id}`}>
                    <span>Model <small>optional override</small></span>
                    <Input
                      id={`draft-agent-model-${draft.id}`}
                      value={draft.model}
                      list={modelSuggestions.length > 0 ? `draft-agent-models-${draft.id}` : undefined}
                      placeholder={inheritedLaunchOptions.model ?? 'Agent default'}
                      disabled={busy}
                      autocapitalize="off"
                      autocorrect="off"
                      spellcheck={false}
                      oninput={(event) => onChange({ model: event.currentTarget.value })}
                    />
                    {#if modelSuggestions.length > 0}
                      <datalist id={`draft-agent-models-${draft.id}`}>
                        {#each modelSuggestions as model}<option value={model}></option>{/each}
                      </datalist>
                    {/if}
                  </label>
                {/if}
                {#if effortSupported}
                  <label class="field-label" for={`draft-agent-effort-${draft.id}`}>
                    <span>Effort <small>optional override</small></span>
                    <Select.Root
                      type="single"
                      value={draft.effort || 'inherit'}
                      disabled={busy}
                      onValueChange={(value) => onChange({ effort: value === 'inherit' ? '' : value })}
                    >
                      <Select.Trigger id={`draft-agent-effort-${draft.id}`} class="w-full">
                        {draft.effort || (inheritedLaunchOptions.effort ? `Default · ${inheritedLaunchOptions.effort}` : 'Agent default')}
                      </Select.Trigger>
                      <Select.Content>
                        <Select.Item value="inherit" label={inheritedLaunchOptions.effort ? `Default · ${inheritedLaunchOptions.effort}` : 'Agent default'} />
                        {#each AGENT_EFFORT_LEVELS as effort}
                          <Select.Item value={effort} label={effort} />
                        {/each}
                      </Select.Content>
                    </Select.Root>
                  </label>
                {/if}
              </div>
            </section>
          {/if}
        </div>
      </Collapsible.Content>
    </Collapsible.Root>

    <div
      bind:this={promptField}
      class="prompt-field"
      role="group"
      aria-label={selectedTemplate ? 'Additional instructions and image attachments' : 'Instructions and image attachments'}
      class:attachment-drop-active={attachmentDropActive}
      ondragover={(event) => {
        if (!Array.from(event.dataTransfer?.types ?? []).includes('Files')) return;
        event.preventDefault();
        attachmentDropActive = true;
      }}
      ondragleave={() => { attachmentDropActive = false; }}
      ondrop={handlePromptDrop}
    >
      <label class="field-label instruction-label" for={`draft-agent-prompt-${draft.id}`}>
        <span>{selectedTemplate ? 'Additional instructions' : 'Instructions'} <small>optional</small></span>
        <small>{selectedTemplate ? `Combined with ${selectedTemplate.name}'s instructions in one starting prompt.` : 'Tell this agent what to do.'}</small>
      </label>
      <div class="prompt-composer">
        <Textarea
          id={`draft-agent-prompt-${draft.id}`}
          class="prompt-textarea min-h-[8rem] resize-y text-sm leading-6"
          bind:ref={promptTextarea}
          value={draft.prompt}
          placeholder={selectedTemplate ? 'Add anything this agent should do beyond the template.' : 'What should this agent do?'}
          disabled={busy || attachmentSaving || dictationBusy}
          oninput={(event) => onChange({ prompt: event.currentTarget.value })}
          onpaste={handlePromptPaste}
        />
        {#if draft.attachments.length > 0}
          <div class="attachment-list" role="group" aria-label="Attached images">
            {#each draft.attachments as attachment, index (attachment)}
              <div class="attachment-chip">
                {#if attachmentPreview(attachment)}
                  <img
                    src={attachmentPreview(attachment)}
                    alt=""
                    onerror={() => handleAttachmentPreviewError(attachment)}
                  />
                {:else}
                  <FileImageIcon class="size-7 shrink-0 p-1.5 text-muted-foreground" size={16} strokeWidth={1.8} aria-hidden="true" />
                {/if}
                <span class="attachment-copy">
                  <strong>{agentDraftImageToken(index)}</strong>
                  <small>{attachmentName(attachment)}</small>
                </span>
                <IconButton
                  class="size-6 rounded-sm"
                  label={`Remove attached image ${index + 1}: ${attachmentName(attachment)}`}
                  tooltip={false}
                  disabled={busy || attachmentSaving}
                  onclick={() => removeAttachment(attachment)}
                >{#snippet icon()}<XIcon size={14} strokeWidth={1.8} />{/snippet}</IconButton>
              </div>
            {/each}
          </div>
        {/if}
        <div class="prompt-actions">
          <VoiceInputButton textarea={promptTextarea} disabled={busy || attachmentSaving} onText={(prompt) => onChange({ prompt })} onBusyChange={(value) => { dictationBusy = value; }} />
          <small id={`draft-agent-create-help-${draft.id}`} aria-live="polite">
            {attachmentSaving ? 'Saving image…' : 'Paste images to place them at the cursor'} · {hotkeyDisplayLabel($hotkeyPreferences['submit-focused-form']) || 'No hotkey'} creates · Shift+Enter adds a line
          </small>
          <Button
            type="submit"
            disabled={busy || !canCreate}
            aria-busy={busy}
            aria-describedby={`draft-agent-create-help-${draft.id}`}
          >{busy ? 'Creating…' : 'Create agent'}</Button>
        </div>
      </div>
    </div>

    {#if !loading && enabledTools.length === 0}
      <p class="empty-note">No enabled agents. Add or enable one in Settings.</p>
    {/if}
  </section>
</CreationDraftScaffold>

<style>
  .feedback-handoff { display: grid; grid-template-columns: 22px minmax(0, 1fr); align-items: center; gap: 8px; border: 1px solid color-mix(in srgb, var(--signal) 34%, var(--border)); border-radius: 5px; padding: 8px 10px; background: color-mix(in srgb, var(--signal) 7%, var(--surface)); color: var(--signal); }
  .feedback-handoff span, .feedback-handoff strong { display: block; }
  .feedback-handoff strong { margin-bottom: 2px; color: var(--text-soft); font-size: var(--font-size-sm); }
  .feedback-handoff span { color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.4; }
  .agent-fields { display: grid; gap: 11px; }
  .model-settings-grid { display: grid; gap: 12px; grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .field-label { display: grid; align-content: start; gap: 6px; color: var(--foreground); font-size: var(--font-size-sm); font-weight: 560; }
  .field-label > span { color: var(--text-soft); }
  .field-label small { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 400; line-height: 1.4; }
  .launch-fieldset, .override-fieldset { min-width: 0; margin: 0; border: 0; padding: 0; }
  .launch-fieldset > legend, .override-fieldset > legend { padding: 0; color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 590; }
  .selection-help { margin: 3px 0 7px; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.45; }
  .launch-roster { max-height: min(27vh, 230px); overflow-y: auto; overscroll-behavior: auto; border: 1px solid var(--border); border-radius: var(--radius); background: var(--background); scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .launch-roster.roster-loading { min-height: 94px; }
  .loading-choice { display: grid; min-height: 92px; place-items: center; padding: 16px; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .roster-group + .roster-group { border-top: 1px solid var(--border); }
  .roster-heading { position: sticky; z-index: 1; top: 0; display: flex; align-items: baseline; justify-content: space-between; gap: 12px; border-bottom: 1px solid var(--border); padding: 5px 9px; background: var(--card); }
  .roster-heading h2 { margin: 0; color: var(--text-soft); font-size: var(--font-size-xs); font-weight: 650; letter-spacing: .025em; text-transform: uppercase; }
  .roster-heading span { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .roster-options { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1px; background: var(--border); }
  .roster-options > .launch-choice:last-child:nth-child(odd),
  .override-options > .override-choice:last-child:nth-child(odd) { grid-column: 1 / -1; }
  .launch-choice, .override-choice { position: relative; min-width: 0; cursor: pointer; background: var(--background); }
  .launch-choice:has(input:disabled), .override-choice:has(input:disabled) { cursor: not-allowed; opacity: .52; }
  .choice-radio { position: absolute; width: 1px; height: 1px; opacity: 0; }
  .choice-card { display: flex; min-width: 0; min-height: 50px; align-items: center; gap: 9px; padding: 7px 10px; color: var(--foreground); transition: background-color 120ms ease, box-shadow 120ms ease; }
  .choice-card.compact { min-height: 42px; padding: 5px 9px; }
  .choice-copy { display: grid; min-width: 0; flex: 1; gap: 1px; }
  .choice-copy strong, .choice-copy > span, .choice-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .choice-copy strong, .choice-copy > span { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 590; }
  .choice-copy small { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 400; }
  .choice-indicator { width: 13px; height: 13px; flex: 0 0 auto; border: 1px solid var(--border-strong); border-radius: 999px; background: var(--background); box-shadow: inset 0 0 0 3px var(--background); }
  .choice-radio:checked + .choice-card { background: color-mix(in srgb, var(--primary) 7%, var(--background)); box-shadow: inset 2px 0 var(--primary); }
  .choice-radio:checked + .choice-card .choice-indicator { border-color: var(--primary); background: var(--primary); }
  .choice-radio:focus-visible + .choice-card { position: relative; z-index: 2; outline: 2px solid var(--ring); outline-offset: -2px; }
  .launch-choice:hover .choice-card, .override-choice:hover .choice-card { background: color-mix(in srgb, var(--muted) 38%, var(--background)); }
  .choice-radio:checked + .choice-card:hover { background: color-mix(in srgb, var(--primary) 10%, var(--background)); }
  .choice-warning { display: block; margin-top: 6px; color: var(--warning-token); font-size: var(--font-size-xs); line-height: 1.4; }
  .template-options { overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius); background: var(--background); }
  .template-options-heading { display: flex; min-height: 36px; align-items: baseline; gap: 8px; padding: 7px 10px; background: var(--card); }
  .template-options-heading > span { flex: none; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .template-options-heading h2 { min-width: 0; overflow: hidden; margin: 0; color: var(--foreground); font-size: var(--font-size-sm); font-weight: 620; text-overflow: ellipsis; white-space: nowrap; }
  .template-detail { border-top: 1px solid var(--border); }
  :global(.template-detail-trigger) { display: flex; width: 100%; min-height: 43px; align-items: center; gap: 10px; border: 0; padding: 6px 9px 6px 10px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  :global(.template-detail-trigger:hover) { background: color-mix(in srgb, var(--muted) 38%, var(--background)); }
  :global(.template-detail-trigger:focus-visible) { outline: 2px solid var(--ring); outline-offset: -2px; }
  :global(.template-detail-trigger > .template-detail-chevron) { flex: none; color: var(--muted-foreground); transition: transform 120ms ease; }
  :global(.template-detail-trigger > .template-detail-chevron.open) { transform: rotate(180deg); }
  .template-detail-copy { display: grid; min-width: 0; flex: 1; grid-template-columns: 132px minmax(0, 1fr); align-items: baseline; gap: 9px; }
  .template-detail-copy strong { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 590; }
  .template-detail-copy small { overflow: hidden; color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 400; text-overflow: ellipsis; white-space: nowrap; }
  .override-fieldset { padding: 8px 10px 10px; background: var(--card); }
  .override-options { display: grid; max-height: 260px; overflow-y: auto; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1px; border: 1px solid var(--border); border-radius: calc(var(--radius) - 1px); background: var(--border); scrollbar-width: thin; }
  .override-note { display: block; margin-top: 7px; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.4; }
  .template-preview { border-top: 1px solid var(--border); padding: 8px 10px 9px; background: var(--card); color: var(--muted-foreground); font: var(--font-size-xs)/1.55 var(--terminal-font-family); white-space: pre-wrap; }
  .model-settings-grid { border-top: 1px solid var(--border); padding: 12px; }
  .launch-tuning { display: grid; grid-column: 1 / -1; gap: 9px; border-top: 1px solid var(--border); padding-top: 11px; }
  .launch-tuning-heading { display: flex; min-width: 0; align-items: baseline; justify-content: space-between; gap: 12px; }
  .launch-tuning-heading > span { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 590; }
  .launch-tuning-heading small { overflow: hidden; color: var(--muted-foreground); font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }
  .launch-tuning-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .empty-note { margin: 0; border: 1px solid var(--border); border-radius: var(--radius); padding: 9px 11px; background: color-mix(in srgb, var(--muted) 20%, transparent); color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .prompt-field { display: grid; align-content: start; gap: 6px; }
  .prompt-composer { overflow: hidden; border: 1px solid var(--input); border-radius: var(--radius); background: var(--background); transition: border-color 120ms ease, box-shadow 120ms ease; }
  .prompt-composer:focus-within { border-color: var(--ring); box-shadow: 0 0 0 3px color-mix(in srgb, var(--ring) 50%, transparent); }
  .prompt-composer :global(.prompt-textarea) { border: 0; border-radius: 0; background: transparent; box-shadow: none; }
  .prompt-composer :global(.prompt-textarea:focus-visible) { border-color: transparent; box-shadow: none; }
  .instruction-label { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
  .instruction-label > small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .prompt-actions { display: flex; min-height: 46px; align-items: center; justify-content: space-between; gap: 12px; border-top: 1px solid var(--border); padding: 6px 7px 6px 10px; background: var(--card); }
  .prompt-actions small { color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.4; }
  .attachment-drop-active { outline: 2px solid var(--ring); outline-offset: 4px; border-radius: var(--radius); }
  .attachment-list { display: flex; flex-wrap: wrap; gap: 7px; border-top: 1px solid var(--border); padding: 8px 10px; }
  .attachment-chip { display: inline-flex; max-width: 220px; align-items: center; gap: 6px; border: 1px solid var(--border); border-radius: var(--radius); padding: 3px 5px 3px 3px; background: var(--muted); color: var(--foreground); font-size: var(--font-size-xs); font-weight: 500; }
  .attachment-chip img { width: 28px; height: 28px; flex: 0 0 auto; border-radius: calc(var(--radius) - 2px); object-fit: cover; }
  .attachment-copy { display: grid; min-width: 0; line-height: 1.2; }
  .attachment-copy strong, .attachment-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .attachment-copy strong { color: var(--signal); font: 600 var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .attachment-copy small { color: var(--muted-foreground); font-size: 10px; }
  @media (max-width: 620px) {
    .model-settings-grid, .launch-tuning-fields, .roster-options, .override-options { grid-template-columns: 1fr; }
    .template-detail-copy { grid-template-columns: 1fr; gap: 1px; }
    .instruction-label { align-items: flex-start; flex-direction: column; gap: 2px; }
    .prompt-actions { align-items: stretch; flex-direction: column; }
    .prompt-actions :global(button) { width: 100%; }
  }
  @media (prefers-reduced-motion: reduce) {
    .choice-card, .prompt-composer, :global(.template-detail-trigger > .template-detail-chevron) { transition: none; }
  }
  @media (forced-colors: active) {
    .launch-choice, .override-choice { display: flex; align-items: center; }
    .choice-radio { position: static; width: 16px; height: 16px; flex: none; margin-left: 9px; opacity: 1; }
    .choice-card { flex: 1; }
    .choice-indicator { display: none; }
    .choice-radio:checked + .choice-card { outline: 2px solid Highlight; outline-offset: -2px; background: Canvas; box-shadow: none; }
  }
</style>
