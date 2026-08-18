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
  assert.match(strip, /openPopoverKey=\{projectRailPopoverKey\}/);
  assert.match(strip, /onOpenPopoverChange=\{\(key\) => \(projectRailPopoverKey = key\)\}/);
  assert.match(strip, /onSelect=\{\(process\) => openProjectRailProcess\(project, process\)\}/);
  assert.match(strip, /onShowAll=\{\(kind\) => openProjectRailOverview\(project, kind\)\}/);
  assert.doesNotMatch(row, /project-row-meta/);
  assert.match(app, /\.project-row \{[^}]*min-height: 44px;/);
  assert.match(app, /\.project-content \{[^}]*grid-column: 1;[^}]*grid-row: 1;/);
  assert.match(app, /\.project-meta-strip \{[^}]*height: 20px;/);
  assert.match(app, /\.project-select \{[^}]*position: relative;[^}]*grid-template-rows: minmax\(20px, auto\) 20px;/);
  assert.match(app, /\.project-meta-strip \{[^}]*grid-column: 1;[^}]*overflow: visible;[^}]*pointer-events: none;/);
  assert.match(app, /\.project-meta-strip :global\(\.worktree-meta\)[^}]*pointer-events: auto;/);
  assert.doesNotMatch(app, /\.project-row\.has-unread \.project-meta-strip/);
});

test('project tooltip is icon-only, delayed, non-blocking, and keeps worktree context', async () => {
  const [row, app, tooltip, timing] = await Promise.all([
    projectRowSource(),
    readFile(appUrl, 'utf8'),
    readFile(new URL('../src/lib/components/ds/TooltipLabel.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/projectRailTooltip.ts', import.meta.url), 'utf8')
  ]);

  assert.match(row, /\{@const tooltipLabel = `\$\{fullTitle\} · \$\{project\.path\}/);
  assert.match(row, /aria-label=\{`\$\{tooltipLabel\} · \$\{projectKind\} · \$\{activityLabel\}/);
  assert.match(row, /\{:else\}\s*<span class="project-content">\s*<button/);
  assert.match(row, /<span class="project-icon-anchor">\s*<TooltipLabel/);
  assert.match(row, /delayDuration=\{PROJECT_RAIL_TOOLTIP_DELAY_MS\}/);
  assert.match(row, /side=\{projectRailCollapsed \? 'right' : 'top'\}/);
  assert.match(row, /disableHoverableContent=\{true\}/);
  assert.match(row, /skipDelayDuration=\{0\}/);
  assert.match(row, /contentClass="project-rail-tooltip"/);
  assert.match(row, /worktreeTooltip=\{false\}/);
  assert.match(row, /\{#snippet content\(\)\}[\s\S]*<strong>\{fullTitle\}<\/strong>[\s\S]*<span>\{project\.path\}<\/span>/);
  assert.match(row, /<GitBranchIcon[^>]*>[\s\S]*Worktree of \{parentLabel\}/);
  assert.match(app, /:global\(\.project-rail-tooltip\) \{ pointer-events: none; \}/);
  assert.match(tooltip, /delayDuration\?: number/);
  assert.match(tooltip, /<Tooltip\.Provider \{delayDuration\} \{disableHoverableContent\} \{skipDelayDuration\}>/);
  assert.match(timing, /PROJECT_RAIL_TOOLTIP_DELAY_MS = 800/);
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

test('kind indicators expose click-only idle rosters and shared popover state', async () => {
  const indicators = await readFile(
    new URL('../src/lib/ProjectKindIndicators.svelte', import.meta.url),
    'utf8'
  );

  assert.match(indicators, /data-tone=\{detail\.tone\}/);
  assert.match(indicators, /\{#each kinds as kind \(kind\)\}/);
  assert.match(indicators, /activity\[kind\]\.processIds/);
  assert.match(indicators, /No \{kindTitle\(kind\)\.toLocaleLowerCase\(\)\} in this project/);
  assert.match(indicators, /return count > 99 \? '99\+' : String\(count\)/);
  assert.match(indicators, /openPopoverKey === popoverKey\(kind\)/);
  assert.match(indicators, /onclick=\{\(event\) => togglePopover\(kind, event\)\}/);
  assert.doesNotMatch(indicators, /onpointer(?:enter|leave)/);
  assert.doesNotMatch(indicators, /PopoverHoverIntent/);
  assert.doesNotMatch(indicators, /Date\.now\(\)/);
});

test('pull request icons always open a coordinated click-only list popover', async () => {
  const pullRequests = await readFile(
    new URL('../src/lib/WorktreeRowMeta.svelte', import.meta.url),
    'utf8'
  );

  assert.match(pullRequests, /open=\{popoverOpen\} onOpenChange=\{changeOpen\}/);
  assert.match(pullRequests, /onclick=\{togglePopover\}/);
  assert.match(pullRequests, /Show \$\{pullRequests\.length\} pull request/);
  assert.match(pullRequests, /<PullRequestList[\s\S]*onChoose=/);
  assert.doesNotMatch(pullRequests, /onpointer(?:enter|leave)/);
  assert.doesNotMatch(pullRequests, /pullRequestMode|PopoverHoverIntent/);
});

test('rail badges overflow upward and worktrees mark the project icon corner', async () => {
  const [row, icon, indicators, pullRequests] = await Promise.all([
    projectRowSource(),
    readFile(new URL('../src/lib/ProjectIcon.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/ProjectKindIndicators.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/WorktreeRowMeta.svelte', import.meta.url), 'utf8')
  ]);

  assert.match(row, /fallback=\{project\.repository_id !== null \? 'repository' : 'project'\}/);
  assert.match(row, /worktree=\{parentLabel !== null\}/);
  assert.match(icon, /worktree\?: boolean/);
  assert.match(icon, /worktreeTooltip\?: boolean/);
  assert.match(icon, /data-project-worktree-badge/);
  assert.match(icon, /label="Worktree"/);
  assert.match(icon, /\.project-icon > :global\(\.tooltip-anchor\) \{[^}]*top: -4px;[^}]*left: -4px;/);
  assert.match(icon, /\.project-icon > \.worktree-badge \{[^}]*top: -4px;[^}]*left: -4px;/);
  assert.match(indicators, /\.kind-glyph small \{[^}]*top: -5px;[^}]*right: -5px;/);
  assert.doesNotMatch(indicators, /\.kind-glyph small \{[^}]*bottom:/);
  assert.match(pullRequests, /\.multi-pr-icon > span:last-child \{[^}]*top: -6px;[^}]*right: -7px;/);
  assert.doesNotMatch(pullRequests, /\.multi-pr-icon > span:last-child \{[^}]*bottom:/);
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
