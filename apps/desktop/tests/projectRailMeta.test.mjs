import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const appUrl = new URL('../src/App.svelte', import.meta.url);

async function projectRowSource() {
  const app = await readFile(appUrl, 'utf8');
  return app.slice(
    app.indexOf('{#snippet projectRailRow'),
    app.indexOf('<WorktreeOperationRow', app.indexOf('{#snippet projectRailRow'))
  );
}

test('expanded project rows reserve a second-line meta strip in PR, agent, terminal, command order', async () => {
  const [row, app] = await Promise.all([projectRowSource(), readFile(appUrl, 'utf8')]);
  const strip = row.slice(
    row.indexOf('<span class="project-meta-strip"'),
    row.indexOf('</span>', row.indexOf('<span class="project-meta-strip"'))
  );

  assert.match(row, /class="project-copy"><strong>\{rowLabel\}<\/strong><\/span>/);
  assert.match(row, /\{#if !projectRailCollapsed\}\s*<span class="project-meta-strip" data-project-meta-strip>/);
  assert.ok(strip.indexOf('<WorktreeRowMeta') < strip.indexOf('<ProjectKindIndicators'));
  assert.match(strip, /showNoPullRequest=\{false\}/);
  assert.doesNotMatch(row, /project-row-meta/);
  assert.match(app, /\.project-row \{[^}]*min-height: 44px;/);
  assert.match(app, /\.project-meta-strip \{[^}]*height: 20px;/);
});

test('project row tooltip owns the path and keeps worktree branch context', async () => {
  const row = await projectRowSource();

  assert.match(row, /\{@const tooltipLabel = `\$\{fullTitle\} · \$\{project\.path\}/);
  assert.match(row, /\{#snippet content\(\)\}[\s\S]*<strong>\{fullTitle\}<\/strong>[\s\S]*<span>\{project\.path\}<\/span>/);
  assert.match(row, /<GitBranchIcon[^>]*>[\s\S]*Worktree of \{parentLabel\}/);
  assert.doesNotMatch(row, /<small>\{project\.path\}<\/small>/);
  assert.doesNotMatch(row, /class="worktree-parent"/);
});

test('path remains visible in project overview and project settings', async () => {
  const [overview, settings] = await Promise.all([
    readFile(new URL('../src/lib/ProjectOverview.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/ProjectSettingsDialog.svelte', import.meta.url), 'utf8')
  ]);

  assert.match(overview, /description=\{project\.path\}/);
  assert.match(overview, /<dt>Checkout path<\/dt>\s*<dd title=\{project\.path\}>\{project\.path\}<\/dd>/);
  assert.match(settings, /<dt>Path<\/dt><dd title=\{project\.path\}>\{project\.path\}<\/dd>/);
});

test('structured TooltipLabel content remains optional for existing callers', async () => {
  const tooltip = await readFile(
    new URL('../src/lib/components/ds/TooltipLabel.svelte', import.meta.url),
    'utf8'
  );

  assert.match(tooltip, /content\?: Snippet/);
  assert.match(tooltip, /\{#if content\}[\s\S]*\{@render content\(\)\}[\s\S]*\{:else\}\s*\{label\}/);
});
