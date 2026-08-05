<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';

  import {
    DaemonRequestError,
    type DaemonClient,
    type ProcessView,
    type Project
  } from './daemon';

  interface CommandInput {
    project_id: number;
    name: string;
    command: string;
    working_dir: string;
    auto_start: boolean;
    auto_restart: boolean;
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

  type CommandProcessReceipt = Pick<ProcessView, 'id' | 'project_id' | 'name'>;

  interface Props {
    client: DaemonClient;
    project: Project;
    onAdded: (process: CommandProcessReceipt) => void;
    onClose: () => void;
  }

  let { client, project, onAdded, onClose }: Props = $props();

  let name = $state('');
  let command = $state('');
  let workingDir = $state('');
  let autoStart = $state(true);
  let autoRestart = $state(false);
  let saveMode = $state<'yml' | 'local'>('yml');
  let configExists = $state<boolean | null>(null);
  let busy = $state(false);
  let attempted = $state(false);
  let workingDirError = $state<string | null>(null);
  let formError = $state<string | null>(null);
  let nameInput: HTMLInputElement;

  onMount(() => {
    void loadConfigStatus();
    requestAnimationFrame(() => nameInput?.focus());
  });

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
    if (!name.trim() || !command.trim()) return;

    busy = true;
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
        auto_start: autoStart,
        auto_restart: autoRestart
      };
      const process = saveMode === 'yml'
        ? await client.control<CommandProcessReceipt>('config.command_save', { ...input })
        : await createLocalCommand(input);
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

  async function createLocalCommand(input: CommandInput): Promise<CommandProcessReceipt> {
    const process = await client.control<CommandProcessReceipt>('process.create', {
      process: {
        id: 0,
        project_id: input.project_id,
        kind: 'command',
        name: input.name,
        command: input.command,
        working_dir: input.working_dir,
        env: {},
        auto_start: input.auto_start,
        auto_restart: input.auto_restart,
        restart_when_changed: [],
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

  function closeOnEscape(event: KeyboardEvent): void {
    if (event.key === 'Escape' && !busy) onClose();
  }
</script>

<svelte:window onkeydown={closeOnEscape} />

<div
  class="backdrop"
  role="presentation"
  onclick={(event) => {
    if (event.target === event.currentTarget && !busy) onClose();
  }}
>
  <dialog
    open
    class="dialog"
    aria-modal="true"
    aria-label="Add command"
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
        <h2>Add command</h2>
      </div>
      <button class="close" type="button" aria-label="Close add command" disabled={busy} onclick={onClose}>×</button>
    </header>

    <p id="command-dialog-description" class="description">
      Add a repeatable process to <strong>{project.name}</strong>.
    </p>

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
            oninput={() => (workingDirError = null)}
          />
          <button type="button" disabled={busy} onclick={() => void browse()}>Browse</button>
        </div>
        {#if workingDirError}
          <small id="working-dir-error" class="error">{workingDirError}</small>
        {:else}
          <small id="working-dir-help">Leave empty to use project root</small>
        {/if}
      </label>

      <div class="switches" aria-label="Command behavior">
        <label class="check"><input type="checkbox" bind:checked={autoStart} disabled={busy} /><span>Auto-start when project starts</span></label>
        <label class="check"><input type="checkbox" bind:checked={autoRestart} disabled={busy} /><span>Auto-restart if command exits</span></label>
      </div>

      <fieldset>
        <legend>Where to save this command</legend>
        <label class:chosen={saveMode === 'yml'} class="save-choice">
          <input type="radio" bind:group={saveMode} value="yml" disabled={busy} />
          <span><strong>Save to awm.yml</strong>
            {#if configExists === false}<small>No awm.yml found — we'll create one for you</small>{:else}<small>Share this command with the project</small>{/if}
          </span>
        </label>
        <label class:chosen={saveMode === 'local'} class="save-choice">
          <input type="radio" bind:group={saveMode} value="local" disabled={busy} />
          <span><strong>Store locally only</strong><small>Keep this command just for yourself on this machine</small></span>
        </label>
      </fieldset>

      {#if formError}<p class="form-error" role="alert">{formError}</p>{/if}
    </div>

      <footer>
        <button type="button" disabled={busy} onclick={onClose}>Cancel</button>
        <button class="primary" type="submit" disabled={busy}>{busy ? 'Adding…' : 'Add command'}</button>
      </footer>
    </form>
  </dialog>
</div>

<style>
  .backdrop {
    position: fixed;
    z-index: 35;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 22px;
    background: rgb(2 8 12 / 76%);
    backdrop-filter: blur(6px);
  }

  .dialog {
    position: relative;
    width: min(560px, 100%);
    max-height: calc(100vh - 44px);
    overflow: auto;
    margin: 0;
    border: 1px solid #435761;
    border-radius: 5px;
    padding: 0;
    background: #101d25;
    box-shadow: 0 28px 90px rgb(0 0 0 / 50%);
  }

  form { display: contents; }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 17px 19px 14px;
    border-bottom: 1px solid #2c4049;
    background: linear-gradient(105deg, rgb(67 137 153 / 9%), transparent 68%);
  }

  header span,
  label > span,
  legend,
  button,
  small {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  header span,
  label > span,
  legend {
    color: #78909a;
    font-size: 8px;
    font-weight: 650;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h2 { margin: 4px 0 0; color: #eef3f4; font-size: 20px; font-weight: 540; }
  .close { width: 30px; height: 30px; border: 0; background: transparent; color: #78909a; font-size: 21px; }
  .description { margin: 0; padding: 13px 19px 0; color: #80949d; font-size: 11px; }
  .description strong { color: #acbbc1; font-weight: 570; }
  .fields { display: grid; gap: 13px; padding: 16px 19px 18px; }
  label { display: grid; gap: 6px; }
  input:not([type]), .browse-row input {
    width: 100%;
    min-height: 34px;
    border: 1px solid #344a54;
    border-radius: 3px;
    padding: 0 10px;
    outline: 0;
    background: #0b171e;
    color: #dce5e8;
    font: 11px 'JetBrains Mono Variable', monospace;
  }
  input:focus { border-color: #5d8994; box-shadow: 0 0 0 2px rgb(93 137 148 / 13%); }
  input[aria-invalid='true'] { border-color: #b96c62; }
  input::placeholder { color: #526770; }
  small { color: #687f89; font-size: 8px; line-height: 1.4; }
  small.error { color: #e28e82; }
  .browse-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 7px; }
  button { border: 1px solid #3d535d; border-radius: 3px; padding: 0 12px; background: #172832; color: #aebdc3; font-size: 9px; cursor: pointer; }
  button:hover:not(:disabled) { border-color: #58727d; background: #1d303a; color: #e3eaec; }
  button:disabled { cursor: default; opacity: 0.55; }
  .switches { display: flex; gap: 20px; padding: 1px 0; }
  .check { display: flex; align-items: center; gap: 7px; }
  .check input, .save-choice input { accent-color: #61a0ae; }
  .check span { color: #b6c4c9; font-size: 10px; font-weight: 540; letter-spacing: 0; text-transform: none; }
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
  .save-choice strong { color: #c4d0d4; font-size: 10px; font-weight: 610; }
  .save-choice small { color: #718891; font-size: 8px; }
  .form-error { margin: 0; border-left: 2px solid #b96c62; padding: 7px 9px; background: rgb(185 108 98 / 9%); color: #e2a097; font-size: 10px; line-height: 1.4; }
  footer { display: flex; justify-content: flex-end; gap: 8px; border-top: 1px solid #2c4049; padding: 12px 19px; }
  footer button { min-height: 31px; }
  footer .primary { border-color: #56818c; background: #31616d; color: #f0f7f8; }
  footer .primary:hover:not(:disabled) { border-color: #70a1ac; background: #39717e; }
</style>
