import type { AgentTool } from './agentTools';
import { normalizeAgentToolType } from './agentToolType.ts';
import { parseExtraArgs } from './extraArgs.ts';

export const AGENT_EFFORT_LEVELS = ['low', 'medium', 'high', 'xhigh', 'max'] as const;
export type AgentEffort = (typeof AGENT_EFFORT_LEVELS)[number];

export interface AgentLaunchOptions {
  model: string | null;
  effort: AgentEffort | null;
}

export interface SplitAgentLaunchOptions extends AgentLaunchOptions {
  extraArgs: string[];
}

const modelToolTypes = new Set([
  'claude',
  'claude_code',
  'codex',
  'gemini',
  'gemini_cli',
  'grok',
  'grok_cli',
  'grok_build',
  'kimi',
  'kimi_code',
  'opencode',
  'open_code'
]);

export function agentSupportsModel(toolType: string | null | undefined): boolean {
  return modelToolTypes.has(normalizeAgentToolType(toolType));
}

export function agentSupportsEffort(toolType: string | null | undefined): boolean {
  return effortDialect(toolType) !== null;
}

export function agentModelSuggestions(toolType: string | null | undefined): string[] {
  const normalized = normalizeAgentToolType(toolType);
  if (normalized === 'claude' || normalized === 'claude_code') {
    return ['fable', 'opus', 'sonnet', 'haiku'];
  }
  return [];
}

export function splitAgentLaunchOptions(
  args: readonly string[],
  toolType: string | null | undefined
): SplitAgentLaunchOptions {
  const modelSupported = agentSupportsModel(toolType);
  const dialect = effortDialect(toolType);
  const extraArgs: string[] = [];
  let model: string | null = null;
  let effort: AgentEffort | null = null;
  let index = 0;

  while (index < args.length) {
    const argument = args[index];
    if (argument === '--') {
      extraArgs.push(...args.slice(index));
      break;
    }
    if (modelSupported && (argument === '--model' || argument === '-m')) {
      const value = normalizeValue(args[index + 1]);
      if (value !== null) {
        model = value;
        index += 2;
        continue;
      }
    }
    if (modelSupported) {
      const attachedModel = attachedOption(argument, '--model', '-m');
      if (attachedModel !== null) {
        const value = normalizeValue(attachedModel);
        if (value !== null) {
          model = value;
          index += 1;
          continue;
        }
      }
    }
    if (dialect === 'flag' && argument === '--effort') {
      const value = normalizeEffort(args[index + 1]);
      if (value !== null) {
        effort = value;
        index += 2;
        continue;
      }
    }
    if (dialect === 'flag' && argument.startsWith('--effort=')) {
      const value = normalizeEffort(argument.slice('--effort='.length));
      if (value !== null) {
        effort = value;
        index += 1;
        continue;
      }
    }
    if (dialect === 'codex-config' && (argument === '-c' || argument === '--config')) {
      const configuredEffort = codexConfiguredEffort(args[index + 1]);
      if (configuredEffort !== null) {
        effort = configuredEffort;
        index += 2;
        continue;
      }
    }
    if (dialect === 'codex-config' && argument.startsWith('--config=')) {
      const configuredEffort = codexConfiguredEffort(argument.slice('--config='.length));
      if (configuredEffort !== null) {
        effort = configuredEffort;
        index += 1;
        continue;
      }
    }
    extraArgs.push(argument);
    index += 1;
  }

  return { model, effort, extraArgs };
}

export function withAgentLaunchOptions(
  args: readonly string[],
  toolType: string | null | undefined,
  model: string | null | undefined,
  effort: string | null | undefined
): string[] {
  const split = splitAgentLaunchOptions(args, toolType);
  const configured = [...split.extraArgs];
  const normalizedModel = normalizeValue(model);
  const normalizedEffort = normalizeEffort(effort);
  if (agentSupportsModel(toolType) && normalizedModel) {
    configured.push('--model', normalizedModel);
  }
  const dialect = effortDialect(toolType);
  if (dialect === 'flag' && normalizedEffort) {
    configured.push('--effort', normalizedEffort);
  } else if (dialect === 'codex-config' && normalizedEffort) {
    configured.push('-c', `model_reasoning_effort="${normalizedEffort}"`);
  }
  return configured;
}

export function configuredAgentLaunchOptions(
  tool: AgentTool | null | undefined,
  templateArgs: readonly string[] = []
): AgentLaunchOptions {
  if (!tool) return { model: null, effort: null };
  let commandArgs: string[] = [];
  try {
    commandArgs = parseExtraArgs(tool.command);
  } catch {
    // A shell-composed command remains launchable; it simply has no readable UI defaults.
  }
  const command = splitAgentLaunchOptions(commandArgs, tool.tool_type);
  const template = splitAgentLaunchOptions(templateArgs, tool.tool_type);
  return {
    model: template.model ?? command.model,
    effort: template.effort ?? command.effort
  };
}

function effortDialect(toolType: string | null | undefined): 'flag' | 'codex-config' | null {
  const normalized = normalizeAgentToolType(toolType);
  if (normalized === 'claude' || normalized === 'claude_code') return 'flag';
  if (normalized === 'codex') return 'codex-config';
  return null;
}

function attachedOption(argument: string, long: string, short: string): string | null {
  if (argument.startsWith(`${long}=`)) return argument.slice(long.length + 1);
  if (argument.startsWith(short) && argument.length > short.length) {
    return argument.slice(short.length).replace(/^=/u, '');
  }
  return null;
}

function codexConfiguredEffort(value: string | null | undefined): AgentEffort | null {
  const normalized = normalizeValue(value);
  if (!normalized) return null;
  const separator = normalized.indexOf('=');
  if (separator < 0 || normalized.slice(0, separator).trim() !== 'model_reasoning_effort') {
    return null;
  }
  return normalizeEffort(normalized.slice(separator + 1).replace(/^["']|["']$/gu, ''));
}

function normalizeEffort(value: string | null | undefined): AgentEffort | null {
  const normalized = normalizeValue(value)?.toLowerCase();
  return AGENT_EFFORT_LEVELS.find((effort) => effort === normalized) ?? null;
}

function normalizeValue(value: string | null | undefined): string | null {
  const normalized = value?.trim();
  return normalized ? normalized : null;
}
