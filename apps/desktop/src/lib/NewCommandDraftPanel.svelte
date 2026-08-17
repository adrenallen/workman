<script lang="ts">
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';

  import { Button } from '$lib/components/ui/button';
  import CreationDraftScaffold from './CreationDraftScaffold.svelte';
  import type { CommandInput, CommandProcessReceipt } from './AddCommandDialog.svelte';
  import type { CommandCreationDraft } from './creationDrafts';
  import { DaemonRequestError, type DaemonClient, type Project } from './daemon';
  import { projectDisplayName } from './worktrees';

  interface ConfigStatus {
    project_id: number;
    path: string;
    exists: boolean;
  }

  interface ValidatedWorkingDirectory {
    absolute: string;
    relative: string;
  }

  interface Props {
    client: DaemonClient;
    project: Project;
    draft: CommandCreationDraft;
    onChange: (patch: Partial<CommandCreationDraft>) => void;
    onPending?: (input: CommandInput) => number | null;
    onAdded: (process: CommandProcessReceipt, optimisticId: number | null) => void;
    onFailed?: (cause: unknown, optimisticId: number) => void;
    onDiscard: () => void;
  }

  let { client, project, draft, onChange, onPending, onAdded, onFailed, onDiscard }: Props = $props();
  let configExists = $state<boolean | null>(null);
  let busy = $state(false);
  let attempted = $state(false);
  let workingDirError = $state<string | null>(null);
  let environmentError = $state<string | null>(null);
  let formError = $state<string | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);

  onMount(() => {
    void loadConfigStatus();
    requestAnimationFrame(() => nameInput?.focus());
  });

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
      const status = await client.control<ConfigStatus>('config.status', { project_id: project.id });
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
      defaultPath: draft.workingDir.trim() || project.path
    });
    if (typeof selected === 'string') {
      onChange({ workingDir: selected });
      workingDirError = null;
    }
  }

  async function submit(): Promise<void> {
    attempted = true;
    formError = null;
    workingDirError = null;
    environmentError = null;
    if (!draft.name.trim() || !draft.command.trim()) return;
    const env = parseEnvironment(draft.environment);
    if (!env) return;

    busy = true;
    const pendingInput: CommandInput = {
      project_id: project.id,
      name: draft.name.trim(),
      command: draft.command.trim(),
      working_dir: draft.workingDir,
      env,
      auto_start: draft.autoStart,
      auto_restart: draft.autoRestart,
      restart_when_changed: draft.restartWhenChanged.split('\n').map((pattern) => pattern.trim()).filter(Boolean)
    };
    const optimisticId = onPending?.(pendingInput) ?? null;
    try {
      const validated = await client.control<ValidatedWorkingDirectory>('config.validate_working_dir', {
        project_id: project.id,
        working_dir: draft.workingDir
      });
      const input: CommandInput = {
        ...pendingInput,
        working_dir: draft.saveMode === 'yml' ? validated.relative : validated.absolute
      };
      const process = draft.saveMode === 'yml'
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

<CreationDraftScaffold
  projectName={projectDisplayName(project)}
  kindLabel="Command"
  title={draft.name.trim() || 'New command'}
  createLabel="Create command"
  {busy}
  onCreate={() => void submit()}
  {onDiscard}
>
  <section class="command-fields">
    <p class="description">Add a repeatable process to <strong>{projectDisplayName(project)}</strong>.</p>
    <label>
      <span>Command name</span>
      <input bind:this={nameInput} aria-invalid={attempted && !draft.name.trim()} value={draft.name} disabled={busy} placeholder="e.g. Vite, Queue, Logs" oninput={(event) => onChange({ name: event.currentTarget.value })} />
      {#if attempted && !draft.name.trim()}<small class="error">Command name is required.</small>{/if}
    </label>
    <label>
      <span>Command</span>
      <input aria-invalid={attempted && !draft.command.trim()} value={draft.command} disabled={busy} placeholder="e.g. npm run dev" autocapitalize="off" spellcheck={false} oninput={(event) => onChange({ command: event.currentTarget.value })} />
      {#if attempted && !draft.command.trim()}<small class="error">Command is required.</small>{/if}
    </label>
    <label>
      <span>Working directory</span>
      <div class="browse-row">
        <input aria-invalid={workingDirError !== null} value={draft.workingDir} disabled={busy} placeholder={project.path} autocapitalize="off" autocorrect="off" spellcheck={false} oninput={(event) => { workingDirError = null; onChange({ workingDir: event.currentTarget.value }); }} />
        <Button variant="outline" size="sm" disabled={busy} onclick={() => void browse()}><FolderOpenIcon size={14} />Browse</Button>
      </div>
      {#if workingDirError}<small class="error">{workingDirError}</small>{:else}<small>Leave empty to use project root</small>{/if}
    </label>
    <label>
      <span>Environment <small>optional · one KEY=value per line</small></span>
      <textarea rows="4" aria-invalid={environmentError !== null} value={draft.environment} disabled={busy} placeholder={'NODE_ENV=development\nPORT=3000'} autocapitalize="off" spellcheck={false} oninput={(event) => { environmentError = null; onChange({ environment: event.currentTarget.value }); }}></textarea>
      {#if environmentError}<small class="error">{environmentError}</small>{/if}
    </label>
    <label>
      <span>Restart when files change <small>optional · one pattern per line</small></span>
      <textarea rows="3" value={draft.restartWhenChanged} disabled={busy} placeholder={'src/**\nconfig/*.json'} autocapitalize="off" spellcheck={false} oninput={(event) => onChange({ restartWhenChanged: event.currentTarget.value })}></textarea>
    </label>
    <div class="switches" aria-label="Command behavior">
      <label class="check"><input type="checkbox" checked={draft.autoStart} disabled={busy} onchange={(event) => onChange({ autoStart: event.currentTarget.checked })} /><span>Auto-start when project starts</span></label>
      <label class="check"><input type="checkbox" checked={draft.autoRestart} disabled={busy} onchange={(event) => onChange({ autoRestart: event.currentTarget.checked })} /><span>Auto-restart if command exits</span></label>
    </div>
    <fieldset>
      <legend>Where to save this command</legend>
      <label class:chosen={draft.saveMode === 'yml'} class="save-choice"><input type="radio" name={`command-save-${draft.id}`} checked={draft.saveMode === 'yml'} disabled={busy} onchange={() => onChange({ saveMode: 'yml' })} /><span><strong>Save to workman.yml</strong>{#if configExists === false}<small>No workman.yml found — we'll create one for you</small>{:else}<small>Share this command with the project</small>{/if}</span></label>
      <label class:chosen={draft.saveMode === 'local'} class="save-choice"><input type="radio" name={`command-save-${draft.id}`} checked={draft.saveMode === 'local'} disabled={busy} onchange={() => onChange({ saveMode: 'local' })} /><span><strong>Store locally only</strong><small>Keep this command just for yourself on this machine</small></span></label>
    </fieldset>
    {#if formError}<p class="form-error" role="alert">{formError}</p>{/if}
  </section>
</CreationDraftScaffold>

<style>
  .command-fields { display: grid; gap: 13px; }
  .description { margin: 0 0 2px; color: var(--muted-foreground); font-size: var(--font-size-sm); }
  .description strong { color: var(--text-soft); font-weight: 570; }
  label { display: grid; gap: 6px; }
  label > span, legend { color: var(--muted-foreground); font: 650 var(--font-size-xs) var(--terminal-font-family); letter-spacing: .045em; text-transform: uppercase; }
  input:not([type]), .browse-row input, textarea { width: 100%; min-height: 34px; border: 1px solid var(--input); border-radius: var(--radius); padding: 0 9px; outline: 0; background: var(--background); color: var(--text); font: var(--font-size-sm) var(--terminal-font-family); }
  textarea { min-height: 58px; resize: vertical; padding-block: 8px; line-height: 1.45; }
  input:focus, textarea:focus { border-color: var(--ring); box-shadow: 0 0 0 2px color-mix(in srgb, var(--ring) 18%, transparent); }
  input[aria-invalid='true'], textarea[aria-invalid='true'] { border-color: var(--destructive); }
  small { color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.4; }
  small.error { color: var(--destructive); }
  .browse-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 7px; }
  .switches { display: flex; flex-wrap: wrap; gap: 14px 20px; padding: 2px 0; }
  .check { display: flex; align-items: center; gap: 7px; }
  .check span { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 540; letter-spacing: 0; text-transform: none; }
  fieldset { display: grid; gap: 7px; margin: 0; border: 0; padding: 0; }
  legend { margin-bottom: 3px; padding: 0; }
  .save-choice { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: start; gap: 10px; border: 1px solid var(--border); border-radius: var(--radius); padding: 10px 11px; background: var(--card); cursor: pointer; }
  .save-choice.chosen { border-color: var(--border-strong); background: var(--accent); }
  .save-choice input { margin-top: 2px; }
  .save-choice span { display: grid; gap: 3px; }
  .save-choice strong { color: var(--text-soft); font-size: var(--font-size-sm); font-weight: 610; }
  .save-choice small { color: var(--muted-foreground); text-transform: none; }
  .form-error { margin: 0; border-left: 2px solid var(--destructive); padding: 7px 9px; background: color-mix(in srgb, var(--destructive) 8%, transparent); color: var(--destructive); font-size: var(--font-size-sm); line-height: 1.4; }
</style>
