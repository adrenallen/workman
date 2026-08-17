import type { CommandCreationDraft } from './creationDrafts';
import type { ProcessView } from './daemon';

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

export type CommandProcessReceipt = Pick<ProcessView, 'id' | 'project_id' | 'name'>;

interface CommandCreationClient {
  control<T>(method: string, params?: Record<string, unknown>): Promise<T>;
  startProcess(processId: number): Promise<ProcessView>;
}

interface ValidatedWorkingDirectory {
  absolute: string;
  relative: string;
}

export class CommandEnvironmentError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CommandEnvironmentError';
  }
}

export function formatCommandEnvironment(env: Record<string, string>): string {
  return Object.entries(env)
    .map(([key, value]) => `${key}=${escapeEnvironmentValue(value)}`)
    .join('\n');
}

export function parseCommandEnvironment(value: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [index, rawLine] of value.split('\n').entries()) {
    if (!rawLine.trim()) continue;
    const separator = rawLine.indexOf('=');
    const key = separator < 0 ? '' : rawLine.slice(0, separator).trim();
    if (!key) throw new CommandEnvironmentError(`Line ${index + 1} must use KEY=value.`);
    env[key] = unescapeEnvironmentValue(rawLine.slice(separator + 1));
  }
  return env;
}

/**
 * Run new-command creation from an immutable snapshot. The snapshot is captured before
 * onPending can replace the selected draft with its optimistic process row.
 */
export async function submitCommandCreation(
  client: CommandCreationClient,
  projectId: number,
  draft: CommandCreationDraft,
  onPending?: (input: CommandInput) => number | null
): Promise<{ process: CommandProcessReceipt; optimisticId: number | null }> {
  const saveMode = draft.saveMode;
  const rawWorkingDir = draft.workingDir;
  const input: CommandInput = {
    project_id: projectId,
    name: draft.name.trim(),
    command: draft.command.trim(),
    working_dir: rawWorkingDir,
    env: parseCommandEnvironment(draft.environment),
    auto_start: draft.autoStart,
    auto_restart: draft.autoRestart,
    restart_when_changed: draft.restartWhenChanged
      .split('\n')
      .map((pattern) => pattern.trim())
      .filter(Boolean)
  };

  const optimisticId = onPending?.(input) ?? null;
  const validated = await client.control<ValidatedWorkingDirectory>('config.validate_working_dir', {
    project_id: projectId,
    working_dir: rawWorkingDir
  });
  const validatedInput: CommandInput = {
    ...input,
    working_dir: saveMode === 'yml' ? validated.relative : validated.absolute
  };
  const process = saveMode === 'yml'
    ? await client.control<CommandProcessReceipt>('config.command_save', { ...validatedInput })
    : await createLocalCommand(client, validatedInput);
  return { process, optimisticId };
}

export async function createLocalCommand(
  client: CommandCreationClient,
  input: CommandInput
): Promise<CommandProcessReceipt> {
  const process = await client.control<ProcessView>('process.create', {
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
