import assert from 'node:assert/strict';
import test from 'node:test';

import {
  agentModelSuggestions,
  agentSupportsEffort,
  configuredAgentLaunchOptions,
  splitAgentLaunchOptions,
  withAgentLaunchOptions
} from '../src/lib/agentLaunchOptions.ts';

test('Claude model and effort flags round-trip without duplicate launch arguments', () => {
  const split = splitAgentLaunchOptions(
    ['--keep', '--model=fable', '--effort', 'high', '--model', 'opus'],
    'claude_code'
  );
  assert.deepEqual(split, {
    model: 'opus',
    effort: 'high',
    extraArgs: ['--keep']
  });
  assert.deepEqual(
    withAgentLaunchOptions(
      ['--model', 'old', '--effort=low', '--keep'],
      'claude',
      'fable',
      'xhigh'
    ),
    ['--keep', '--model', 'fable', '--effort', 'xhigh']
  );
  assert.deepEqual(agentModelSuggestions('claude-code'), ['fable', 'opus', 'sonnet', 'haiku']);
});

test('Codex effort uses its config override while preserving unrelated config', () => {
  const split = splitAgentLaunchOptions(
    [
      '-c',
      'sandbox_mode="danger-full-access"',
      '--config=model_reasoning_effort="high"',
      '-m',
      'gpt-5.6-sol'
    ],
    'codex'
  );
  assert.deepEqual(split, {
    model: 'gpt-5.6-sol',
    effort: 'high',
    extraArgs: ['-c', 'sandbox_mode="danger-full-access"']
  });
  assert.deepEqual(
    withAgentLaunchOptions(split.extraArgs, 'codex', null, 'xhigh'),
    ['-c', 'sandbox_mode="danger-full-access"', '-c', 'model_reasoning_effort="xhigh"']
  );
});

test('configured template settings override readable agent-command defaults', () => {
  const tool = {
    command: 'claude --model sonnet --effort medium --dangerously-skip-permissions',
    tool_type: 'claude_code'
  };
  assert.deepEqual(configuredAgentLaunchOptions(tool, []), {
    model: 'sonnet',
    effort: 'medium'
  });
  assert.deepEqual(
    configuredAgentLaunchOptions(tool, ['--model', 'fable', '--effort=xhigh']),
    { model: 'fable', effort: 'xhigh' }
  );
  assert.equal(agentSupportsEffort('gemini'), false);
});

test('unsupported agents retain raw model and effort-like arguments unchanged', () => {
  assert.deepEqual(
    splitAgentLaunchOptions(['--model', 'custom', '--effort', 'max'], 'custom'),
    {
      model: null,
      effort: null,
      extraArgs: ['--model', 'custom', '--effort', 'max']
    }
  );
});

test('invalid or incomplete structured flags remain visible as raw launch arguments', () => {
  assert.deepEqual(
    splitAgentLaunchOptions(['--model=', '--effort', 'turbo', '--model'], 'claude'),
    {
      model: null,
      effort: null,
      extraArgs: ['--model=', '--effort', 'turbo', '--model']
    }
  );
});
