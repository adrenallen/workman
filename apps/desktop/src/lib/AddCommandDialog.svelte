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
  import { projectDisplayName } from './worktrees';

  export interface CommandInput {
    project_id: number;
    name: string;
    command: string;
    working_dir: string;
    env: Record<string, string>;
    auto_start: boolean;
    auto_restart: boolean;
    restart_when_changed: string[];
  }

  interface ConfigStatus {
    project_id: number;
    path: string;
    exists: boolean;
  }

  interface ValidatedWorkingDirectory {
    absolute: string;
    relative: string;
  }

  export type CommandProcessReceipt = Pick<ProcessView, 'id' | 'project_id' | 'name'>;

  interface Props {
    client: DaemonClient;
    project: Project;
    initialProcess?: ProcessView | null;
    onPending?: (input: CommandInput) => number | null;
    onAdded: (process: CommandProcessReceipt, optimisticId: number | null) => void;
    onFailed?: (cause: unknown, optimisticId: number) => void;
    onClose: () => void;
  }

  let {
    client,
    project,
    initialProcess = null,
    onPending,
    onAdded,
    onFailed,
    onClose
  }: Props = $props();

  let editing = $derived(initialProcess !== null);
  let running = $derived(
    initialProcess?.status === 'running' || initialProcess?.status === 'starting'
  );
  let name = $state(untrack(() => initialProcess?.name ?? ''));
  let command = $state(untrack(() => initialProcess?.command ?? ''));
  let workingDir = $state(untrack(() => initialProcess?.working_dir ?? ''));
  let environment = $state(untrack(() => formatEnvironment(initialProcess?.env ?? {})));
  let restartWhenChanged = $state(
    untrack(() => (initialProcess?.restart_when_changed ?? []).join('\n'))
  );
  let autoStart = $state(untrack(() => initialProcess?.auto_start ?? true));
  let autoRestart = $state(untrack(() => initialProcess?.auto_restart ?? false));
  let saveMode = $state<'yml' | 'local'>(
    untrack(() => initialProcess?.source ?? 'yml')
  );
  let configExists = $state<boolean | null>(null);
  let busy = $state(false);
  let attempted = $state(false);
  let workingDirError = $state<string | null>(null);
  let environmentError = $state<string | null>(null);
  let formError = $state<string | null>(null);
  let nameInput: HTMLInputElement;

  onMount(() => {
    if (!editing) void loadConfigStatus();
    requestAnimationFrame(() => nameInput?.focus());
  });

  function formatEnvironment(env: Record<string, string>): string {
    return Object.entries(env)
      .map(([key, value]) => `${key}=${escapeEnvironmentValue(value)}`)
      .join('\n');
  }

  function escapeEnvironmentValue(value: string): string {
    return value
      .replaceAll('\\', '\\\\')
      .replaceAll('\n', '\\n')
      .replaceAll('\r', '\\r')
      .replaceAll('\t', '\\t');
  }

  function unescapeEnvironmentValue(value: string): string {
    let result = '';
    for (let index = 0; index < value.length; index += 1) {
      const character = value[index];
      if (character !== '\\' || index + 1 >= value.length) {
        result += character;
        continue;
      }
      const escaped = value[index + 1];
      if (escaped === 'n') result += '\n';
      else if (escaped === 'r') result += '\r';
      else if (escaped === 't') result += '\t';
      else if (escaped === '\\') result += '\\';
      else result += `\\${escaped}`;
      index += 1;
    }
    return result;
  }

  function parseEnvironment(value: string): Record<string, string> | null {
    const env: Record<string, string> = {};
    for (const [index, rawLine] of value.split('\n').entries()) {
      if (!rawLine.trim()) continue;
      const separator = rawLine.indexOf('=');
      const key = separator < 0 ? '' : rawLine.slice(0, separator).trim();
      if (!key) {
        environmentError = `Line ${index + 1} must use KEY=value.`;
        return null;
      }
      env[key] = unescapeEnvironmentValue(rawLine.slice(separator + 1));
    }
    return env;
  }

  async function loadConfigStatus(): Promise<void> {
    try {
      const status = await client.control<ConfigStatus>('config.status', {
        project_id: project.id
      });
      configExists = status.exists;
    } catch {
      configExists = null;
    }
  }

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
    const env = parseEnvironment(environment);
    if (!env) return;

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
    const optimisticId = editing ? null : onPending?.(pendingInput) ?? null;
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
      const process = editing
        ? await client.control<CommandProcessReceipt>('config.command_update', {
            process_id: initialProcess!.id,
            ...input
          })
        : saveMode === 'yml'
          ? await client.control<CommandProcessReceipt>('config.command_save', { ...input })
          : await createLocalCommand(input);
      onAdded(process, optimisticId);
    } catch (cause) {
      if (optimisticId !== null && onFailed) {
        onFailed(cause, optimisticId);
      } else if (cause instanceof DaemonRequestError && cause.code === 'invalid_working_directory') {
        workingDirError = `Choose an existing folder inside ${project.path}.`;
      } else {
        formError = cause instanceof Error ? cause.message : String(cause);
      }
    } finally {
      busy = false;
    }
  }

  async function createLocalCommand(input: CommandInput): Promise<CommandProcessReceipt> {
    const process = await client.control<CommandProcessReceipt>('process.create', {
      process: {
        id: 0,
        project_id: input.project_id,
        kind: 'command',
        name: input.name,
        command: input.command,
        working_dir: input.working_dir,
        env: input.env,
        auto_start: input.auto_start,
        auto_restart: input.auto_restart,
        restart_when_changed: input.restart_when_changed,
        source: 'local',
        trust_hash: null,
        status: 'stopped',
        pid: null,
        exit_code: null,
        exit_signal: null,
        exited_at: null,
        agent_tool_id: null
      }
    });
    return input.auto_start ? client.startProcess(process.id) : process;
  }

</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="command-dialog w-[min(560px,calc(100vw-32px))] max-w-none gap-0 rounded-lg border border-border bg-popover p-0 shadow-2xl"
    showCloseButton={false}
    aria-label={editing ? 'Edit command' : 'Add command'}
    aria-describedby="command-dialog-description"
  >
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
    <header>
      <div>
        <span>Project command</span>
        <h2>{editing ? 'Edit command' : 'Add command'}</h2>
      </div>
      <IconButton label={editing ? 'Close edit command' : 'Close add command'} disabled={busy} onclick={onClose}>
        {#snippet icon()}<XIcon size={15} />{/snippet}
      </IconButton>
    </header>

    <div class="command-body">
      <p id="command-dialog-description" class="description">
        {editing ? 'Update' : 'Add'} a repeatable process {editing ? 'in' : 'to'} <strong>{projectDisplayName(project)}</strong>.
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

      {#if !editing}<fieldset>
        <legend>Where to save this command</legend>
        <label class:chosen={saveMode === 'yml'} class="save-choice">
          <input type="radio" bind:group={saveMode} value="yml" disabled={busy} />
          <span><strong>Save to workman.yml</strong>
            {#if configExists === false}<small>No workman.yml found — we'll create one for you</small>{:else}<small>Share this command with the project</small>{/if}
          </span>
        </label>
        <label class:chosen={saveMode === 'local'} class="save-choice">
          <input type="radio" bind:group={saveMode} value="local" disabled={busy} />
          <span><strong>Store locally only</strong><small>Keep this command just for yourself on this machine</small></span>
        </label>
      </fieldset>{:else}
        <p class="storage-note">
          Stored {saveMode === 'yml' ? 'in workman.yml' : 'locally on this machine'}.
        </p>
      {/if}

      {#if formError}<p class="form-error" role="alert">{formError}</p>{/if}
      </div>
    </div>

      <footer>
        <Button variant="outline" type="button" disabled={busy} onclick={onClose}>Cancel</Button>
        <Button type="submit" disabled={busy}>
          {busy ? (editing ? 'Saving…' : 'Adding…') : (editing ? 'Save changes' : 'Add command')}
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
  legend,
  small {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  header span,
  label > span,
  legend {
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
  .check input, .save-choice input { accent-color: #61a0ae; }
  .check span { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 540; letter-spacing: 0; text-transform: none; }
  fieldset { display: grid; gap: 7px; margin: 0; border: 0; padding: 0; }
  legend { margin-bottom: 7px; padding: 0; }
  .save-choice {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: start;
    gap: 10px;
    border: 1px solid #2f4650;
    border-radius: 3px;
    padding: 10px 11px;
    background: #0c1920;
    cursor: pointer;
  }
  .save-choice.chosen { border-color: #537783; background: #10232b; }
  .save-choice input { margin: 2px 0 0; }
  .save-choice span { display: grid; gap: 4px; }
  .save-choice strong { color: #c4d0d4; font-size: var(--font-size-sm); font-weight: 610; }
  .save-choice small { color: #718891; font-size: var(--font-size-xs); }
  .form-error { margin: 0; border-left: 2px solid #b96c62; padding: 7px 9px; background: rgb(185 108 98 / 9%); color: #e2a097; font-size: var(--font-size-sm); line-height: 1.4; }
  .running-note, .storage-note { margin: 12px 19px 0; border-left: 2px solid #b99758; padding: 8px 10px; background: rgb(185 151 88 / 8%); color: #b9aa89; font-size: var(--font-size-sm); line-height: 1.45; }
  .storage-note { margin: 0; border-left-color: #537783; background: #10232b; color: #80949d; }
  footer { display: flex; justify-content: flex-end; gap: 8px; border-top: 1px solid #2c4049; padding: 12px 19px; }
</style>
