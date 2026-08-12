import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { AGENT_BRANDS, monogram, resolveAgentBrand } from '../src/lib/agentBrands.ts';

function tool(name, toolType, command = name.toLowerCase()) {
  return { name, tool_type: toolType, command };
}

test('maps every bundled preset brand from declarative preset metadata', () => {
  assert.deepEqual(
    [
      tool('Claude', 'claude_code'),
      tool('Codex', 'codex'),
      tool('Gemini', 'gemini_cli'),
      tool('Kimi', 'kimi_code'),
      tool('DeepSeek', 'deepseek'),
      tool('OpenCode', 'open_code')
    ].map((candidate) => resolveAgentBrand(candidate).id),
    ['anthropic', 'openai', 'google', 'moonshot', 'deepseek', 'opencode']
  );
  assert.deepEqual(
    AGENT_BRANDS.filter((brand) => brand.glyph).map((brand) => brand.id),
    ['anthropic', 'openai']
  );
  assert.deepEqual(
    AGENT_BRANDS.filter((brand) => brand.asset).map((brand) => [brand.id, brand.asset]),
    [['deepseek', 'deepseek'], ['moonshot', 'kimi']]
  );
});

test('provider identity outranks a generic host tool type', () => {
  const brand = resolveAgentBrand(
    tool('DeepSeek v4 flash', 'opencode', 'opencode --model deepseek/deepseek-v4-flash')
  );
  assert.equal(brand.id, 'deepseek');
  assert.equal(brand.monogram, 'DS');
});

test('unmapped presets receive a stable neutral two-letter monogram', () => {
  assert.equal(resolveAgentBrand(tool('Local Pilot', 'custom')).id, 'custom');
  assert.equal(resolveAgentBrand(tool('Local Pilot', 'custom')).monogram, 'LP');
  assert.equal(monogram('Ollama'), 'OL');
  assert.equal(monogram('my_agent'), 'MA');
});

test('tree renders the brand mark before the subprocess count without changing state indicators', async () => {
  const source = await readFile(new URL('../src/lib/ProjectTree.svelte', import.meta.url), 'utf8');
  const mark = source.indexOf('<AgentBrandMark');
  const subprocessCount = source.indexOf('<CountBadge prefix="+"', mark);
  assert.ok(mark >= 0 && subprocessCount > mark);
  assert.match(source, /<AgentStatusIndicator \{process\} \/>/);
});
