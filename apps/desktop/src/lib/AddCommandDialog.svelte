<script lang="ts">
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import XIcon from '@lucide/svelte/icons/x';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount, untrack } from 'svelte';

  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import IconButton from '$lib/components/ds/IconButton.svelte';
  import {
    DaemonRequestError,
    type DaemonClient,
    type ProcessView,
    type Project
  } from './daemon';
  import {
    CommandEnvironmentError,
    formatCommandEnvironment,
    parseCommandEnvironment,
    type CommandInput,
    type CommandProcessReceipt
  } from './commandCreation';
  import { hotkeyPreferences, matchesHotkeyAction } from './hotkeys';
  import { projectDisplayName } from './worktrees';

  interface ValidatedWorkingDirectory {
    absolute: string;
    relative: string;
  }

  interface Props {
    client: DaemonClient;
    project: Project;
    initialProcess: ProcessView;
    onAdded: (process: CommandProcessReceipt) => void;
    onClose: () => void;
  }

  let {
    client,
    project,
    initialProcess,
    onAdded,
    onClose
  }: Props = $props();

  let running = $derived(
    initialProcess.status === 'running' || initialProcess.status === 'starting'
  );
  let name = $state(untrack(() => initialProcess.name));
  let command = $state(untrack(() => initialProcess.command ?? ''));
  let formElement: HTMLFormElement;

  function handleKeydown(event: KeyboardEvent): void {
    if (!(event.target instanceof Node) || !formElement?.contains(event.target)) return;
    if (!matchesHotkeyAction(event, 'submit-focused-form', $hotkeyPreferences)) return;
    event.preventDefault();
    event.stopPropagation();
    void submit();
  }
  let workingDir = $state(untrack(() => initialProcess.working_dir));
  let environment = $state(untrack(() => formatCommandEnvironment(initialProcess.env)));
  let restartWhenChanged = $state(
    untrack(() => initialProcess.restart_when_changed.join('\n'))
  );
  let autoStart = $state(untrack(() => initialProcess.auto_start));
  let autoRestart = $state(untrack(() => initialProcess.auto_restart));
  const saveMode = untrack(() => initialProcess.source);
  let busy = $state(false);
  let attempted = $state(false);
  let workingDirError = $state<string | null>(null);
  let environmentError = $state<string | null>(null);
  let formError = $state<string | null>(null);
  let nameInput: HTMLInputElement;

  onMount(() => {
    requestAnimationFrame(() => nameInput?.focus());
  });

  async function browse(): Promise<void> {
    if (busy) return;
    const selected = await open({
      title: 'Choose command working directory',
      directory: true,
      multiple: false,
      defaultPath: workingDir.trim() || project.path
    });
    if (typeof selected === 'string') {
      workingDir = selected;
      workingDirError = null;
    }
  }

  async function submit(): Promise<void> {
    attempted = true;
    formError = null;
    workingDirError = null;
    environmentError = null;
    if (!name.trim() || !command.trim()) return;
    let env: Record<string, string>;
    try {
      env = parseCommandEnvironment(environment);
    } catch (cause) {
      environmentError = cause instanceof CommandEnvironmentError
        ? cause.message
        : 'Environment values could not be parsed.';
      return;
    }

    busy = true;
    const pendingInput: CommandInput = {
      project_id: project.id,
      name: name.trim(),
      command: command.trim(),
      working_dir: workingDir,
      env,
      auto_start: autoStart,
      auto_restart: autoRestart,
      restart_when_changed: restartWhenChanged
        .split('\n')
        .map((pattern) => pattern.trim())
        .filter(Boolean)
    };
    try {
      const validated = await client.control<ValidatedWorkingDirectory>(
        'config.validate_working_dir',
        { project_id: project.id, working_dir: workingDir }
      );
      const input: CommandInput = {
        project_id: project.id,
        name: name.trim(),
        command: command.trim(),
        working_dir: saveMode === 'yml' ? validated.relative : validated.absolute,
        env,
        auto_start: autoStart,
        auto_restart: autoRestart,
        restart_when_changed: pendingInput.restart_when_changed
      };
      const process = await client.control<CommandProcessReceipt>('config.command_update', {
        process_id: initialProcess.id,
        ...input
      });
      onAdded(process);
    } catch (cause) {
      if (cause instanceof DaemonRequestError && cause.code === 'invalid_working_directory') {
        workingDirError = `Choose an existing folder inside ${project.path}.`;
      } else {
        formError = cause instanceof Error ? cause.message : String(cause);
      }
    } finally {
      busy = false;
    }
  }

</script>

<svelte:window onkeydown={handleKeydown} />

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="command-dialog w-[min(560px,calc(100vw-32px))] max-w-none gap-0 rounded-lg border border-border bg-popover p-0 shadow-2xl"
    showCloseButton={false}
    aria-label="Edit command"
    aria-describedby="command-dialog-description"
  >
    <form
      bind:this={formElement}
      onsubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
    <header>
      <div>
        <span>Project command</span>
        <h2>Edit command</h2>
      </div>
      <IconButton label="Close edit command" disabled={busy} onclick={onClose}>
        {#snippet icon()}<XIcon size={15} />{/snippet}
      </IconButton>
    </header>

    <div class="command-body">
      <p id="command-dialog-description" class="description">
        Update a repeatable process in <strong>{projectDisplayName(project)}</strong>.
      </p>

      {#if running}
        <p class="running-note" role="note">
          This command is running. Saved changes apply the next time it starts; the current run is unchanged.
        </p>
      {/if}

      <div class="fields">
      <label>
        <span>Command name</span>
        <input
          aria-invalid={attempted && !name.trim()}
          aria-describedby={attempted && !name.trim() ? 'command-name-error' : undefined}
          bind:this={nameInput}
          bind:value={name}
          disabled={busy}
          placeholder="e.g. Vite, Queue, Logs"
        />
        {#if attempted && !name.trim()}<small id="command-name-error" class="error">Command name is required.</small>{/if}
      </label>

      <label>
        <span>Command</span>
        <input
          aria-invalid={attempted && !command.trim()}
          aria-describedby={attempted && !command.trim() ? 'command-value-error' : undefined}
          bind:value={command}
          disabled={busy}
          placeholder="e.g. npm run dev"
          autocapitalize="off"
          spellcheck={false}
        />
        {#if attempted && !command.trim()}<small id="command-value-error" class="error">Command is required.</small>{/if}
      </label>

      <label>
        <span>Working directory</span>
        <div class="browse-row">
          <input
            aria-invalid={workingDirError !== null}
            aria-describedby={workingDirError ? 'working-dir-error' : 'working-dir-help'}
            bind:value={workingDir}
            disabled={busy}
            placeholder={project.path}
            autocapitalize="off"
            autocorrect="off"
            spellcheck={false}
            oninput={() => (workingDirError = null)}
          />
          <Button variant="outline" size="sm" disabled={busy} onclick={() => void browse()}>
            <FolderOpenIcon size={14} />Browse
          </Button>
        </div>
        {#if workingDirError}
          <small id="working-dir-error" class="error">{workingDirError}</small>
        {:else}
          <small id="working-dir-help">Leave empty to use project root</small>
        {/if}
      </label>

      <label>
        <span>Environment <small>optional · one KEY=value per line</small></span>
        <textarea
          rows="3"
          aria-invalid={environmentError !== null}
          aria-describedby={environmentError ? 'command-environment-error' : undefined}
          bind:value={environment}
          disabled={busy}
          placeholder={'NODE_ENV=development\nPORT=3000'}
          autocapitalize="off"
          spellcheck={false}
          oninput={() => (environmentError = null)}
        ></textarea>
        {#if environmentError}<small id="command-environment-error" class="error">{environmentError}</small>{/if}
      </label>

      <label>
        <span>Restart when files change <small>optional · one pattern per line</small></span>
        <textarea
          rows="2"
          bind:value={restartWhenChanged}
          disabled={busy}
          placeholder={'src/**\nconfig/*.json'}
          autocapitalize="off"
          spellcheck={false}
        ></textarea>
      </label>

      <div class="switches" aria-label="Command behavior">
        <label class="check"><input type="checkbox" bind:checked={autoStart} disabled={busy} /><span>Auto-start when project starts</span></label>
        <label class="check"><input type="checkbox" bind:checked={autoRestart} disabled={busy} /><span>Auto-restart if command exits</span></label>
      </div>

      <p class="storage-note">
        Stored {saveMode === 'yml' ? 'in workman.yml' : 'locally on this machine'}.
      </p>

      {#if formError}<p class="form-error" role="alert">{formError}</p>{/if}
      </div>
    </div>

      <footer>
        <Button variant="outline" type="button" disabled={busy} onclick={onClose}>Cancel</Button>
        <Button type="submit" disabled={busy}>
          {busy ? 'Saving…' : 'Save changes'}
        </Button>
      </footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<style>
  form { display: grid; min-height: 0; max-height: calc(100dvh - 2rem); grid-template-rows: auto minmax(0, 1fr) auto; }
  .command-body { min-height: 0; overflow-y: auto; overscroll-behavior: contain; }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 17px 19px 14px;
    border-bottom: 1px solid #2c4049;
    background: var(--card);
  }

  header span,
  label > span,
  small {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  header span,
  label > span {
    color: #78909a;
    font-size: var(--font-size-xs);
    font-weight: 650;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h2 { margin: 4px 0 0; color: #eef3f4; font-size: 20px; font-weight: 540; }
  .description { margin: 0; padding: 13px 19px 0; color: #80949d; font-size: var(--font-size-sm); }
  .description strong { color: #acbbc1; font-weight: 570; }
  .fields { display: grid; gap: 13px; padding: 16px 19px 18px; }
  label { display: grid; gap: 6px; }
  input:not([type]), .browse-row input, textarea {
    width: 100%;
    min-height: 34px;
    border: 1px solid #344a54;
    border-radius: 3px;
    padding: 0 10px;
    outline: 0;
    background: #0b171e;
    color: #dce5e8;
    font: var(--font-size-sm) 'JetBrains Mono Variable', monospace;
  }
  textarea { min-height: 54px; resize: vertical; padding-block: 8px; line-height: 1.45; }
  input:focus, textarea:focus { border-color: #5d8994; box-shadow: 0 0 0 2px rgb(93 137 148 / 13%); }
  input[aria-invalid='true'], textarea[aria-invalid='true'] { border-color: #b96c62; }
  input::placeholder { color: #526770; }
  small { color: #687f89; font-size: var(--font-size-xs); line-height: 1.4; }
  small.error { color: #e28e82; }
  .browse-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 7px; }
  .switches { display: flex; gap: 20px; padding: 1px 0; }
  .check { display: flex; align-items: center; gap: 7px; }
  .check input { accent-color: #61a0ae; }
  .check span { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 540; letter-spacing: 0; text-transform: none; }
  .form-error { margin: 0; border-left: 2px solid #b96c62; padding: 7px 9px; background: rgb(185 108 98 / 9%); color: #e2a097; font-size: var(--font-size-sm); line-height: 1.4; }
  .running-note, .storage-note { margin: 12px 19px 0; border-left: 2px solid #b99758; padding: 8px 10px; background: rgb(185 151 88 / 8%); color: #b9aa89; font-size: var(--font-size-sm); line-height: 1.45; }
  .storage-note { margin: 0; border-left-color: #537783; background: #10232b; color: #80949d; }
  footer { display: flex; justify-content: flex-end; gap: 8px; border-top: 1px solid #2c4049; padding: 12px 19px; }
</style>
