import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { navigationTargetKey } from '../src/lib/navigation.ts';

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
  const pullRequestIndex = strip.indexOf('<WorktreeRowMeta');
  const processKindsIndex = strip.indexOf('<ProjectKindIndicators');
  assert.notEqual(pullRequestIndex, -1);
  assert.notEqual(processKindsIndex, -1);
  assert.ok(pullRequestIndex < processKindsIndex);
  assert.match(strip, /showNoPullRequest=\{false\}/);
  assert.match(strip, /onSelect=\{\(process\) => openProjectRailProcess\(project, process\)\}/);
  assert.match(strip, /onShowAll=\{\(kind\) => openProjectRailOverview\(project, kind\)\}/);
  assert.doesNotMatch(row, /project-row-meta/);
  assert.match(app, /\.project-row \{[^}]*min-height: 44px;/);
  assert.match(app, /\.project-meta-strip \{[^}]*height: 20px;/);
  assert.match(app, /\.project-select \{[^}]*position: relative;[^}]*grid-template-rows: minmax\(20px, auto\) 20px;/);
  assert.match(app, /\.project-meta-strip \{[^}]*grid-column: 1;[^}]*overflow: hidden;[^}]*pointer-events: none;/);
  assert.match(app, /\.project-meta-strip :global\(\.worktree-meta\)[^}]*pointer-events: auto;/);
  assert.doesNotMatch(app, /\.project-row\.has-unread \.project-meta-strip/);
});

test('project row tooltip owns the path and keeps worktree branch context', async () => {
  const row = await projectRowSource();

  assert.match(row, /\{@const tooltipLabel = `\$\{fullTitle\} · \$\{project\.path\}/);
  assert.match(row, /aria-label=\{`\$\{tooltipLabel\} · \$\{projectKind\} · \$\{activityLabel\}/);
  assert.match(row, /\{#snippet content\(\)\}[\s\S]*<strong>\{fullTitle\}<\/strong>[\s\S]*<span>\{project\.path\}<\/span>/);
  assert.match(row, /<GitBranchIcon[^>]*>[\s\S]*Worktree of \{parentLabel\}/);
  assert.doesNotMatch(row, /<small>\{project\.path\}<\/small>/);
  assert.doesNotMatch(row, /class="worktree-parent"/);
});

test('project errors stay in the accessible row summary without adding an idle indicator', async () => {
  const app = await readFile(appUrl, 'utf8');

  assert.match(app, /project\.status === 'error' \? `project error · \$\{activitySummary\}`/);
  assert.doesNotMatch(app, /project\.status === 'error'[\s\S]{0,180}<ProjectKindIndicators/);
});

test('show-all process navigation is serialized and contributes the project to recents', async () => {
  const app = await readFile(appUrl, 'utf8');

  assert.equal(
    navigationTargetKey({ type: 'processes', projectId: 42, kind: 'terminal' }),
    'project:42'
  );
  assert.match(app, /function openProjectRailOverview[\s\S]*appNavigation\.navigate\([\s\S]*type: 'processes'/);
  assert.match(app, /case 'processes':\s*openProcessOverview\(target\.kind\);/);
});

test('kind indicators consume helper tones, bound counts, and restore row focus', async () => {
  const indicators = await readFile(
    new URL('../src/lib/ProjectKindIndicators.svelte', import.meta.url),
    'utf8'
  );

  assert.match(indicators, /data-tone=\{detail\.tone\}/);
  assert.match(indicators, /return count > 99 \? '99\+' : String\(count\)/);
  assert.match(indicators, /querySelector<HTMLButtonElement>\('\.project-select'\)/);
  assert.match(indicators, /tick\(\)\.then\(\(\) => projectButton\.focus\(\)\)/);
  assert.doesNotMatch(indicators, /Date\.now\(\)/);
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
