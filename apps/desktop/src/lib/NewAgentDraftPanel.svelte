<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import CpuIcon from '@lucide/svelte/icons/cpu';
  import LayoutTemplateIcon from '@lucide/svelte/icons/layout-template';
  import FileImageIcon from '@lucide/svelte/icons/file-image';
  import FileTextIcon from '@lucide/svelte/icons/file-text';
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
  import * as Tabs from './components/ui/tabs';
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
  const launchMode = $derived(draft.templateId !== null ? 'template' : 'tool');
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
      promptTextarea?.focus({ preventScroll: true });
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

  function switchLaunchMode(mode: string): void {
    if (loading || busy || mode === launchMode) return;
    if (mode === 'template' && availableTemplates[0]) {
      selectTemplate(availableTemplates[0]);
    } else if (mode === 'tool') {
      const tool = selectedTool ?? enabledTools[0];
      if (tool) selectStandaloneAgent(tool);
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
  {#snippet heading()}
    <div class="agent-heading">
      <div>
        <h1>{draft.name.trim() || 'New agent'}</h1>
        <p>Choose a starting point, then give your agent a task.</p>
      </div>
      <AgentPromptHistory entries={promptHistory} {busy} onRestore={onRestorePrompt} onClear={onClearPromptHistory} {onError} />
    </div>
  {/snippet}
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
    <fieldset
      class="launch-fieldset"
      disabled={loading || busy}
      aria-busy={loading}
      aria-describedby={`draft-agent-choice-help-${draft.id}`}
    >
      <legend>How would you like to start?</legend>
      <p id={`draft-agent-choice-help-${draft.id}`} class="selection-help">Pick one. A template includes instructions; a model starts fresh.</p>
      <Tabs.Root value={launchMode} onValueChange={switchLaunchMode} class="launch-tabs">
        <Tabs.List class="launch-modes" aria-label="Agent starting point">
          <Tabs.Trigger value="template" class="launch-mode" disabled={loading || busy || availableTemplates.length === 0}>
            <LayoutTemplateIcon size={20} strokeWidth={1.8} />
            <span><strong>Use a template</strong><small>Saved instructions &amp; setup</small></span>
          </Tabs.Trigger>
          <Tabs.Trigger value="tool" class="launch-mode" disabled={loading || busy || enabledTools.length === 0}>
            <CpuIcon size={20} strokeWidth={1.8} />
            <span><strong>Choose a model</strong><small>Start with your own instructions</small></span>
          </Tabs.Trigger>
        </Tabs.List>
      <div class="launch-roster" class:roster-loading={loading}>
        {#if loading}
          <div class="loading-choice" role="status">Loading launch choices…</div>
        {:else}
          <Tabs.Content value="template">
            <section class="roster-group" aria-labelledby={`draft-agent-templates-${draft.id}`}>
              <h2 id={`draft-agent-templates-${draft.id}`} class="roster-heading">Choose a template</h2>
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
                      <AgentBrandMark {tool} size={22} />
                      <span class="choice-copy">
                        <strong>{template.name}</strong>
                        <small>Uses {tool.name}{#if !tool.enabled} · disabled{/if}</small>
                      </span>
                      <span class="choice-indicator" aria-hidden="true"></span>
                    </span>
                  </label>
                {/each}
              </div>
            </section>
          </Tabs.Content>

          <Tabs.Content value="tool">
          <section class="roster-group" aria-labelledby={`draft-agent-tools-${draft.id}`}>
            <h2 id={`draft-agent-tools-${draft.id}`} class="roster-heading">Choose a model or tool</h2>
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
                    <span class="choice-copy"><strong>{tool.name}</strong></span>
                    <span class="choice-indicator" aria-hidden="true"></span>
                  </span>
                </label>
              {/each}
            </div>
          </section>
          </Tabs.Content>
        {/if}
      </div>
      </Tabs.Root>
      {#if !loading && availableTemplates.length === 0 && !choice.missingTemplate}
        <small class="selection-help">No templates available yet. You can add them in Settings.</small>
      {/if}
      {#if choice.missingTemplate}
        <small class="choice-warning">Template #{draft.templateId} is no longer available. Choose another template or a standalone agent.</small>
      {/if}
      {#if choice.missingTool}
        <small class="choice-warning">Agent #{draft.agentToolId} is no longer available. Choose another agent to create this draft.</small>
      {/if}
    </fieldset>

    <section class="launch-configuration" aria-label="Agent setup">
    {#if selectedTemplate}
      <section class="template-options" aria-label="Template setup">

        <div class="template-detail">
          <Collapsible.Root bind:open={templateInstructionsOpen}>
            <Collapsible.Trigger class="template-detail-trigger">
              <FileTextIcon class="text-muted-foreground" size={16} strokeWidth={1.8} />
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
              <CpuIcon class="text-muted-foreground" size={16} strokeWidth={1.8} />
              <span class="template-detail-copy">
                <strong>Runs with</strong>
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

    <Collapsible.Root bind:open={modelSettingsOpen} class={selectedTemplate ? 'model-settings with-template' : 'model-settings'}>
      <Collapsible.Trigger class="template-detail-trigger">
        <SlidersHorizontalIcon class="text-muted-foreground" size={16} strokeWidth={1.8} />
        <span class="template-detail-copy"><strong>Model settings</strong><small>Model, effort, name &amp; launch options</small></span>
        <ChevronDownIcon class={`template-detail-chevron ${modelSettingsOpen ? 'open' : ''}`} size={14} aria-hidden="true" />
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
    </section>

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
        <small>{selectedTemplate ? `Add a task or extra context. ${selectedTemplate.name}'s instructions are already included.` : 'Tell this agent what to do.'}</small>
      </label>
      <div class="prompt-composer">
        <Textarea
          id={`draft-agent-prompt-${draft.id}`}
          class="prompt-textarea min-h-[10rem] resize-y text-base leading-6"
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
          <Button
            type="submit"
            class="min-h-9 px-5"
            disabled={busy || !canCreate}
            aria-busy={busy}
            aria-describedby={`draft-agent-create-help-${draft.id}`}
          >{busy ? 'Creating…' : 'Create agent'}</Button>
        </div>
      </div>
      <p class="composer-help" id={`draft-agent-create-help-${draft.id}`} aria-live="polite">
        {attachmentSaving ? 'Saving image…' : 'Paste or drop images to attach'}
        <span>{hotkeyDisplayLabel($hotkeyPreferences['submit-focused-form']) || 'No hotkey'} creates · Shift+Enter adds a line</span>
      </p>
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
  .agent-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 20px; }
  .agent-heading h1 { margin: 0; color: var(--foreground); font: 650 26px/1.2 'Archivo Variable', var(--ui-font-family); letter-spacing: -.025em; overflow-wrap: anywhere; }
  .agent-heading p { margin: 8px 0 0; color: var(--muted-foreground); font-size: var(--font-size-sm); line-height: 1.5; }
  .agent-heading > div { min-width: 0; }
  .agent-fields { display: grid; gap: 24px; }
  .field-label { display: grid; align-content: start; gap: 8px; color: var(--foreground); font-size: var(--font-size-sm); font-weight: 560; }
  .field-label small { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 400; line-height: 1.5; }
  .launch-fieldset, .override-fieldset { min-width: 0; margin: 0; border: 0; padding: 0; }
  .launch-fieldset > legend { padding: 0; color: var(--foreground); font-size: 16px; font-weight: 620; }
  .selection-help { display: block; margin: 6px 0 16px; color: var(--muted-foreground); font-size: var(--font-size-sm); line-height: 1.5; }
  .agent-fields :global(.launch-tabs) { gap: 20px; }
  .agent-fields :global(.launch-modes) { display: grid; width: 100%; height: auto; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px; padding: 4px; border: 1px solid var(--border); border-radius: 8px; background: var(--background); }
  .agent-fields :global(.launch-mode) { height: auto; min-height: 72px; justify-content: flex-start; gap: 12px; padding: 12px 16px; border-radius: 5px; white-space: normal; text-align: left; box-shadow: none; color: var(--muted-foreground); transition: background-color 120ms ease, color 120ms ease; }
  .agent-fields :global(.launch-mode[data-state='active']) { border-color: var(--border-strong); background: var(--accent); color: var(--foreground); box-shadow: none; }
  .agent-fields :global(.launch-mode:hover:not(:disabled)) { color: var(--foreground); }
  .agent-fields :global(.launch-mode:focus-visible) { outline: 2px solid var(--ring); outline-offset: 2px; }
  .agent-fields :global(.launch-mode > span) { display: grid; gap: 4px; }
  .agent-fields :global(.launch-mode strong) { font-size: var(--font-size-base); font-weight: 620; line-height: 1.3; }
  .agent-fields :global(.launch-mode small) { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 400; line-height: 1.4; }
  .launch-roster { min-width: 0; }
  .loading-choice { display: grid; min-height: 80px; place-items: center; padding: 16px; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .roster-heading { margin: 0 0 10px; color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 560; }
  .roster-options { display: grid; max-height: 264px; overflow-y: auto; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; padding: 2px; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .launch-choice, .override-choice { position: relative; min-width: 0; cursor: pointer; }
  .launch-choice:has(input:disabled), .override-choice:has(input:disabled) { cursor: not-allowed; opacity: .52; }
  .choice-radio { position: absolute; width: 1px; height: 1px; opacity: 0; }
  .choice-card { display: flex; min-width: 0; min-height: 60px; align-items: center; gap: 12px; border: 1px solid var(--border); border-radius: 6px; padding: 12px 14px; background: var(--background); color: var(--foreground); transition: background-color 120ms ease, border-color 120ms ease; }
  .choice-card.compact { min-height: 48px; padding: 8px 10px; }
  .choice-copy { display: grid; min-width: 0; flex: 1; gap: 4px; }
  .choice-copy strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 590; overflow-wrap: anywhere; }
  .choice-copy small { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 400; overflow-wrap: anywhere; }
  .choice-indicator { width: 15px; height: 15px; flex: 0 0 auto; border: 1px solid var(--border-strong); border-radius: 50%; background: var(--background); box-shadow: inset 0 0 0 3px var(--background); }
  .choice-radio:checked + .choice-card { border-color: var(--text-soft); background: var(--card); }
  .choice-radio:checked + .choice-card .choice-indicator { border-color: var(--primary); background: var(--primary); }
  .choice-radio:focus-visible + .choice-card { outline: 2px solid var(--ring); outline-offset: 0; }
  .launch-choice:hover .choice-card, .override-choice:hover .choice-card { background: var(--card); border-color: var(--text-soft); }
  .choice-warning { display: block; margin-top: 12px; color: var(--warning-token); font-size: var(--font-size-sm); line-height: 1.5; }
  .launch-configuration { overflow: hidden; border: 1px solid var(--border); border-radius: 6px; background: var(--card); }
  .template-detail + .template-detail, .launch-configuration :global(.model-settings.with-template) { border-top: 1px solid var(--border); }
  .launch-configuration :global(.template-detail-trigger) { display: flex; width: 100%; min-height: 52px; align-items: center; gap: 12px; border: 0; padding: 12px 16px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .launch-configuration :global(.template-detail-trigger:hover) { background: var(--accent); }
  .launch-configuration :global(.template-detail-trigger:focus-visible) { outline: 2px solid var(--ring); outline-offset: -2px; }
  .launch-configuration :global(.template-detail-chevron) { flex: none; color: var(--muted-foreground); transition: transform 120ms ease; }
  .launch-configuration :global(.template-detail-chevron.open) { transform: rotate(180deg); }
  .template-detail-copy { display: grid; min-width: 0; flex: 1; grid-template-columns: 140px minmax(0, 1fr); align-items: baseline; gap: 12px; }
  .template-detail-copy strong { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 590; }
  .template-detail-copy small { overflow: hidden; color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 400; text-overflow: ellipsis; white-space: nowrap; }
  .override-fieldset { padding: 0 16px 16px; }
  .override-options { display: grid; max-height: 260px; overflow-y: auto; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; padding: 2px; scrollbar-width: thin; }
  .override-note { display: block; margin-top: 10px; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.5; }
  .template-preview { max-height: 280px; overflow-y: auto; margin: 0 16px 16px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--background); color: var(--text-soft); font: var(--font-size-sm)/1.6 var(--ui-font-family); white-space: pre-wrap; overflow-wrap: anywhere; }
  .model-settings-grid { display: grid; gap: 16px; grid-template-columns: repeat(2, minmax(0, 1fr)); border-top: 1px solid var(--border); padding: 16px; }
  .launch-tuning { display: grid; grid-column: 1 / -1; gap: 12px; border-top: 1px solid var(--border); padding-top: 16px; }
  .launch-tuning-heading { display: grid; gap: 4px; }
  .launch-tuning-heading > span { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 590; }
  .launch-tuning-heading small { color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.5; }
  .launch-tuning-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; }
  .prompt-field { display: grid; min-width: 0; gap: 12px; }
  .prompt-composer { overflow: hidden; border: 1px solid var(--border-strong); border-radius: 6px; background: var(--card); }
  .prompt-composer:focus-within { border-color: var(--ring); outline: 2px solid var(--ring); outline-offset: 1px; }
  .prompt-composer :global(.prompt-textarea) { padding: 14px 16px; border: 0; border-radius: 0; background: transparent; box-shadow: none; }
  .prompt-composer :global(.prompt-textarea:focus-visible) { outline: none; border-color: transparent; box-shadow: none; }
  .instruction-label { gap: 6px; }
  .instruction-label > span { font-size: 16px; font-weight: 620; }
  .instruction-label > span small { margin-left: 6px; }
  .instruction-label > small { font-size: var(--font-size-sm); }
  .prompt-actions { display: flex; min-height: 52px; align-items: center; justify-content: space-between; gap: 12px; border-top: 1px solid var(--border); padding: 8px 12px; }
  .prompt-actions :global(button[type='submit']) { margin-left: auto; }
  .composer-help { display: flex; justify-content: space-between; flex-wrap: wrap; gap: 4px 16px; margin: 0; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.5; }
  .attachment-drop-active { outline: 2px solid var(--ring); outline-offset: 4px; border-radius: var(--radius); }
  .attachment-list { display: flex; flex-wrap: wrap; gap: 7px; border-top: 1px solid var(--border); padding: 8px 10px; }
  .attachment-chip { display: inline-flex; max-width: 220px; align-items: center; gap: 6px; border: 1px solid var(--border); border-radius: var(--radius); padding: 3px 5px 3px 3px; background: var(--muted); color: var(--foreground); font-size: var(--font-size-xs); font-weight: 500; }
  .attachment-chip img { width: 28px; height: 28px; flex: 0 0 auto; border-radius: calc(var(--radius) - 2px); object-fit: cover; }
  .attachment-copy { display: grid; min-width: 0; line-height: 1.2; }
  .attachment-copy strong, .attachment-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .attachment-copy strong { color: var(--signal); font: 600 var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .attachment-copy small { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  @container agent-draft (max-width: 600px) {
    .agent-heading { flex-direction: column; gap: 12px; }
    .agent-fields :global(.launch-mode) { padding: 12px; gap: 8px; align-items: flex-start; }
    .agent-fields :global(.launch-mode > svg) { display: none; }
    .template-detail-copy { grid-template-columns: 1fr; gap: 4px; }
    .template-detail-copy small { white-space: normal; }
  }
  @container agent-draft (max-width: 420px) {
    .model-settings-grid, .launch-tuning-fields, .roster-options, .override-options { grid-template-columns: 1fr; }
    .agent-fields :global(.launch-mode small) { font-size: var(--font-size-xs); }
  }
  @media (prefers-reduced-motion: reduce) {
    .choice-card, .agent-fields :global(.launch-mode), .launch-configuration :global(.template-detail-chevron) { transition: none; }
  }
  @media (forced-colors: active) {
    .launch-choice, .override-choice { display: flex; align-items: center; }
    .choice-radio { position: static; width: 16px; height: 16px; flex: none; margin-left: 9px; opacity: 1; }
    .choice-card { flex: 1; }
    .choice-indicator { display: none; }
    .choice-radio:checked + .choice-card { outline: 2px solid Highlight; outline-offset: -2px; background: Canvas; box-shadow: none; }
  }
</style>
