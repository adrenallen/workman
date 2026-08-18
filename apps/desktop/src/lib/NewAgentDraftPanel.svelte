<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import SlidersHorizontalIcon from '@lucide/svelte/icons/sliders-horizontal';
  import XIcon from '@lucide/svelte/icons/x';
  import { convertFileSrc, invoke, isTauri } from '@tauri-apps/api/core';
  import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview';
  import { onDestroy, onMount } from 'svelte';

  import AgentBrandMark from './AgentBrandMark.svelte';
  import { resolveAgentDraftChoice } from './agentDraftChoices';
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
  import * as Collapsible from './components/ui/collapsible';
  import { Input } from './components/ui/input';
  import * as Select from './components/ui/select';
  import { Textarea } from './components/ui/textarea';
  import { primaryModifier, primaryModifierLabel } from './primaryModifier';
  import { pointIsInsideRect } from './terminalInput';

  interface AgentDraftSubmission {
    input: SpawnAgentInput;
    tool: AgentTool;
    template: AgentTemplate | null;
  }

  interface Props {
    draft: AgentCreationDraft;
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

  let advancedOpen = $state(false);
  let previewOpen = $state(true);
  let promptTextarea = $state<HTMLTextAreaElement | null>(null);
  let promptField = $state<HTMLDivElement | null>(null);
  let attachmentSaving = $state(false);
  let attachmentDropActive = $state(false);
  let attachmentPreviews = $state<Record<string, string>>({});
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
  const templateChoice = $derived(
    draft.templateId !== null ? `template:${draft.templateId}` : 'none'
  );
  const agentChoice = $derived(
    draft.agentToolId !== null ? `tool:${draft.agentToolId}` : ''
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

  function selectTemplate(value: string | undefined): void {
    const template = availableTemplates.find((candidate) => value === `template:${candidate.id}`);
    if (!template) {
      onChange({ templateId: null });
      if (selectedTool) rememberChoice({ kind: 'tool', id: selectedTool.id });
      return;
    }
    onChange({ templateId: template.id, agentToolId: template.agent_tool_id });
    rememberChoice({ kind: 'template', id: template.id, agentToolId: template.agent_tool_id });
  }

  function selectAgent(value: string | undefined): void {
    const tool = enabledTools.find((candidate) => value === `tool:${candidate.id}`);
    if (!tool) return;
    onChange({ agentToolId: tool.id });
    rememberChoice(selectedTemplate
      ? { kind: 'template', id: selectedTemplate.id, agentToolId: tool.id }
      : { kind: 'tool', id: tool.id });
  }

  function submit(): void {
    if (!selectedTool || busy || attachmentSaving) return;
    let extraArgs: string[];
    try {
      extraArgs = parseExtraArgs(draft.extraArgs);
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
        extra_args: extraArgs,
        prompt: draft.prompt.trim() || undefined,
        attachments: draft.attachments.length > 0 ? [...draft.attachments] : undefined
      },
      tool: selectedTool,
      template: selectedTemplate
    });
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== 'Enter' || !primaryModifier(event)) return;
    event.preventDefault();
    submit();
  }

  async function attachImages(files: File[]): Promise<void> {
    const available = Math.max(0, 8 - draft.attachments.length);
    const images = files.filter((file) => file.type.startsWith('image/')).slice(0, available);
    if (images.length === 0) {
      if (available === 0) onError('A new-agent draft can have at most 8 image attachments.');
      return;
    }
    attachmentSaving = true;
    const paths: string[] = [];
    const previews: Record<string, string> = {};
    try {
      for (const image of images) {
        const bytes = Array.from(new Uint8Array(await image.arrayBuffer()));
        const path = await invoke<string>('terminal_save_clipboard_image', {
          bytes,
          mimeType: image.type
        });
        paths.push(path);
        previews[path] = URL.createObjectURL(image);
      }
      attachmentPreviews = { ...attachmentPreviews, ...previews };
      onChange({ attachments: [...draft.attachments, ...paths] });
    } catch (cause) {
      for (const preview of Object.values(previews)) URL.revokeObjectURL(preview);
      onError(`Could not attach image: ${cause instanceof Error ? cause.message : String(cause)}`);
    } finally {
      attachmentSaving = false;
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
    void attachImages(images);
  }

  function handlePromptDrop(event: DragEvent): void {
    const files = Array.from(event.dataTransfer?.files ?? []);
    if (!files.some((file) => file.type.startsWith('image/'))) return;
    event.preventDefault();
    event.stopPropagation();
    attachmentDropActive = false;
    void attachImages(files);
  }

  function attachImagePaths(paths: string[]): void {
    const available = Math.max(0, 8 - draft.attachments.length);
    const existing = new Set(draft.attachments);
    const images = paths
      .filter((path) => /\.(?:png|jpe?g|gif|webp|bmp|tiff?)$/iu.test(path))
      .filter((path) => !existing.has(path))
      .slice(0, available);
    if (images.length === 0) {
      if (available === 0) onError('A new-agent draft can have at most 8 image attachments.');
      return;
    }
    onChange({ attachments: [...draft.attachments, ...images] });
  }

  function handleNativePromptDrop(payload: DragDropEvent): void {
    if (payload.type === 'leave') {
      attachmentDropActive = false;
      return;
    }
    if (!promptField) return;
    const inside = pointIsInsideRect(
      payload.position,
      promptField.getBoundingClientRect(),
      window.devicePixelRatio
    );
    if (payload.type === 'enter' || payload.type === 'over') {
      attachmentDropActive = inside;
      return;
    }
    attachmentDropActive = false;
    if (inside) attachImagePaths(payload.paths);
  }

  function removeAttachment(path: string): void {
    const preview = attachmentPreviews[path];
    if (preview) URL.revokeObjectURL(preview);
    const { [path]: _, ...remainingPreviews } = attachmentPreviews;
    attachmentPreviews = remainingPreviews;
    onChange({ attachments: draft.attachments.filter((candidate) => candidate !== path) });
  }

  function attachmentName(path: string): string {
    return path.split('/').at(-1) || 'image';
  }

  function attachmentPreview(path: string): string {
    return attachmentPreviews[path] ?? (isTauri() ? convertFileSrc(path) : '');
  }

  onMount(() => {
    if (!isTauri()) return;
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
  canCreate={!loading && !attachmentSaving && selectedTool !== null}
  onCreate={submit}
  {onDiscard}
>
  {#snippet secondaryAction()}
    {#if onOpenSettings}
      <Button type="button" variant="outline" disabled={busy} onclick={onOpenSettings}>Open Settings</Button>
    {/if}
  {/snippet}
  <section class="agent-fields">
    <div class="choice-grid">
      <label for={`draft-agent-template-${draft.id}`}>
        <span>Template <small>optional</small></span>
        <Select.Root type="single" value={templateChoice} disabled={loading || busy} onValueChange={selectTemplate}>
          <Select.Trigger id={`draft-agent-template-${draft.id}`} class="w-full text-left">
            {selectedTemplate?.name
              ?? (draft.templateId !== null
                ? metadataLoaded ? `Unavailable template #${draft.templateId}` : 'Loading template…'
                : 'None')}
          </Select.Trigger>
          <Select.Content>
            <Select.Item value="none" label="None">None</Select.Item>
            {#each templates as template (template.id)}
              {@const tool = tools.find((candidate) => candidate.id === template.agent_tool_id)}
              {#if tool}
                <Select.Item value={`template:${template.id}`} label={template.name} disabled={!tool.enabled}>
                  <AgentBrandMark {tool} size={16} />
                  <span>{template.name}</span>
                  <span class="text-xs text-muted-foreground">{tool.name}{#if !tool.enabled} · agent disabled{/if}</span>
                </Select.Item>
              {/if}
            {/each}
          </Select.Content>
        </Select.Root>
        {#if choice.missingTemplate}
          <small class="choice-warning">Template #{draft.templateId} is no longer available. Choose another template or None.</small>
        {/if}
      </label>

      <label for={`draft-agent-tool-${draft.id}`}>
        <span>Agent</span>
        <Select.Root type="single" value={agentChoice} disabled={loading || busy} onValueChange={selectAgent}>
          <Select.Trigger id={`draft-agent-tool-${draft.id}`} class="w-full text-left">
            {#if selectedTool}
              <span class="flex min-w-0 items-center gap-1.5"><AgentBrandMark tool={selectedTool} size={16} /><span class="truncate">{selectedTool.name}</span></span>
            {:else if draft.agentToolId !== null}
              {metadataLoaded ? `Unavailable agent #${draft.agentToolId}` : 'Loading agent…'}
            {:else}Select an agent{/if}
          </Select.Trigger>
          <Select.Content>
            {#each enabledTools as tool (tool.id)}
              <Select.Item value={`tool:${tool.id}`} label={tool.name}><AgentBrandMark {tool} size={16} /><span>{tool.name}</span></Select.Item>
            {/each}
          </Select.Content>
        </Select.Root>
        {#if choice.missingTool}
          <small class="choice-warning">Agent #{draft.agentToolId} is no longer available. Choose another agent to create this draft.</small>
        {/if}
        {#if agentOverridden && templateDefaultTool}
          <small>Template default: {templateDefaultTool.name}. Template launch args are skipped for other agents.</small>
        {/if}
      </label>
    </div>

    {#if selectedTemplate}
      <Collapsible.Root bind:open={previewOpen} class="overflow-hidden rounded-md border border-border bg-muted/20">
        <Collapsible.Trigger class="flex min-h-9 w-full items-center gap-2 px-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
          <span class="min-w-0 flex-1"><strong class="block text-sm font-medium">Template prompt</strong><span class="block text-xs text-muted-foreground">Template prompt is prepended</span></span>
          <ChevronDownIcon class={`text-muted-foreground ${previewOpen ? 'rotate-180' : ''}`} size={14} />
        </Collapsible.Trigger>
        <Collapsible.Content><div class="template-preview" aria-label="Template prompt preview">{selectedTemplate.prompt || 'No template prompt'}</div></Collapsible.Content>
      </Collapsible.Root>
    {/if}

    <div
      bind:this={promptField}
      class="prompt-field"
      role="group"
      aria-label="Prompt and image attachments"
      class:attachment-drop-active={attachmentDropActive}
      ondragover={(event) => {
        if (!Array.from(event.dataTransfer?.types ?? []).includes('Files')) return;
        event.preventDefault();
        attachmentDropActive = true;
      }}
      ondragleave={() => { attachmentDropActive = false; }}
      ondrop={handlePromptDrop}
    >
      <label for={`draft-agent-prompt-${draft.id}`}><span>Prompt <small>optional</small></span></label>
      <Textarea
        id={`draft-agent-prompt-${draft.id}`}
        class="min-h-[17rem] resize-y text-sm leading-6"
        bind:ref={promptTextarea}
        value={draft.prompt}
        placeholder="What should this agent work on?"
        disabled={busy}
        oninput={(event) => onChange({ prompt: event.currentTarget.value })}
        onkeydown={handleKeydown}
        onpaste={handlePromptPaste}
      />
      {#if draft.attachments.length > 0}
        <div class="attachment-list" aria-label="Attached images">
          {#each draft.attachments as attachment, index (attachment)}
            <div class="attachment-chip">
              {#if attachmentPreview(attachment)}
                <img src={attachmentPreview(attachment)} alt="" />
              {/if}
              <span>{attachmentName(attachment)}</span>
              <button
                type="button"
                aria-label={`Remove attached image ${index + 1}: ${attachmentName(attachment)}`}
                disabled={busy || attachmentSaving}
                onclick={() => removeAttachment(attachment)}
              ><XIcon size={13} /></button>
            </div>
          {/each}
        </div>
      {/if}
      <small>{primaryModifierLabel}+Enter creates. Shift+Enter adds a line.</small>
    </div>

    <Collapsible.Root bind:open={advancedOpen} class="overflow-hidden rounded-md border border-border">
      <Collapsible.Trigger class="flex min-h-9 w-full items-center gap-2 px-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
        <SlidersHorizontalIcon class="text-muted-foreground" size={14} />
        <span class="min-w-0 flex-1 text-sm font-medium">Advanced</span>
        <ChevronDownIcon class={`text-muted-foreground ${advancedOpen ? 'rotate-180' : ''}`} size={14} />
      </Collapsible.Trigger>
      <Collapsible.Content>
        <div class="advanced-grid">
          <label for={`draft-agent-name-${draft.id}`}><span>Name <small>optional</small></span><Input id={`draft-agent-name-${draft.id}`} value={draft.name} placeholder={`${selectedTool?.name.toLowerCase() ?? 'agent'} worker`} disabled={busy} oninput={(event) => onChange({ name: event.currentTarget.value })} onkeydown={handleKeydown} /></label>
          <label for={`draft-agent-args-${draft.id}`}><span>Extra launch args <small>optional</small></span><Input id={`draft-agent-args-${draft.id}`} value={draft.extraArgs} placeholder='--model "gpt-5"' disabled={busy} autocapitalize="off" autocorrect="off" spellcheck={false} oninput={(event) => onChange({ extraArgs: event.currentTarget.value })} onkeydown={handleKeydown} /></label>
        </div>
      </Collapsible.Content>
    </Collapsible.Root>

    {#if !loading && enabledTools.length === 0}
      <p class="empty-note">No enabled agents. Add or enable one in Settings.</p>
    {/if}
  </section>
</CreationDraftScaffold>

<style>
  .agent-fields { display: grid; gap: 13px; }
  .choice-grid, .advanced-grid { display: grid; gap: 12px; grid-template-columns: repeat(2, minmax(0, 1fr)); }
  label { display: grid; align-content: start; gap: 6px; color: var(--foreground); font-size: var(--font-size-sm); font-weight: 560; }
  label > span { color: var(--text-soft); }
  label small { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 400; line-height: 1.4; }
  .template-preview { max-height: 144px; overflow-y: auto; border-top: 1px solid var(--border); padding: 8px 12px; color: var(--muted-foreground); font: var(--font-size-xs)/1.55 var(--terminal-font-family); white-space: pre-wrap; }
  .advanced-grid { border-top: 1px solid var(--border); padding: 12px; }
  .empty-note { margin: 0; border: 1px solid var(--border); border-radius: var(--radius); padding: 9px 11px; background: color-mix(in srgb, var(--muted) 20%, transparent); color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .choice-warning { color: var(--warning-token); }
  .prompt-field { display: grid; align-content: start; gap: 6px; }
  .prompt-field > small { color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.4; }
  .attachment-drop-active { outline: 2px solid var(--ring); outline-offset: 4px; border-radius: var(--radius); }
  .attachment-list { display: flex; flex-wrap: wrap; gap: 7px; }
  .attachment-chip { display: inline-flex; max-width: 220px; align-items: center; gap: 6px; border: 1px solid var(--border); border-radius: var(--radius); padding: 3px 5px 3px 3px; background: var(--muted); color: var(--foreground); font-size: var(--font-size-xs); font-weight: 500; }
  .attachment-chip img { width: 28px; height: 28px; flex: 0 0 auto; border-radius: calc(var(--radius) - 2px); object-fit: cover; }
  .attachment-chip span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .attachment-chip button { display: inline-grid; width: 22px; height: 22px; flex: 0 0 auto; place-items: center; border: 0; border-radius: 4px; background: transparent; color: var(--muted-foreground); }
  .attachment-chip button:hover:not(:disabled) { background: color-mix(in srgb, var(--foreground) 10%, transparent); color: var(--foreground); }
  @media (max-width: 620px) { .choice-grid, .advanced-grid { grid-template-columns: 1fr; } }
</style>
