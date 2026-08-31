import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const sourceRoot = new URL('../src/', import.meta.url);

async function source(relativePath) {
  return readFile(new URL(relativePath, sourceRoot), 'utf8');
}

test('context menus prioritize lifecycle actions and distinguish stop, force stop, and remove', async () => {
  const [model, menu] = await Promise.all([
    source('lib/contextMenu.ts'),
    source('lib/ContextMenu.svelte')
  ]);

  for (const label of ['Create', 'Worktrees', 'Commands', 'More']) {
    assert.match(model, new RegExp(`label: '${label}'`));
  }
  assert.match(model, /id: 'stop',[\s\S]*label: 'Stop',[\s\S]*Graceful · stays in the sidebar[\s\S]*tone: 'warning'/);
  assert.match(model, /id: 'kill',[\s\S]*label: 'Force stop…',[\s\S]*Use if Stop is not responding[\s\S]*tone: 'danger'/);
  assert.match(model, /id: 'close',[\s\S]*label: `Remove \$\{process\.kind\}…`/);
  assert.match(menu, /data-tone=\{itemTone\(item\)\}/);
  assert.match(menu, /action-row\[data-tone='positive'\]/);
  assert.match(menu, /action-row\[data-tone='warning'\]/);
  assert.match(menu, /action-row\[data-tone='info'\]/);
  assert.match(menu, /action-row\[data-tone='danger'\]/);
});

test('settings identify their application-wide scope and use platform-neutral local copy', async () => {
  const [app, panel, status, appearance, sidebar, openers, navigation, quickJump] = await Promise.all([
    source('App.svelte'),
    source('lib/SettingsPanel.svelte'),
    source('lib/settings/SettingsStatusStrip.svelte'),
    source('lib/settings/AppearanceCard.svelte'),
    source('lib/settings/SidebarCard.svelte'),
    source('lib/settings/OpenersCard.svelte'),
    source('lib/navigation.ts'),
    source('lib/QuickJumpPalette.svelte')
  ]);
  const settings = [panel, status, appearance, sidebar, openers].join('\n');

  assert.match(panel, /Application preferences/);
  assert.match(status, /<strong>Application settings<\/strong>/);
  assert.match(status, /Preferences saved locally/);
  assert.doesNotMatch(status, /projectDisplayName|project: Project/);
  assert.match(navigation, /\| \{ type: 'settings' \}/);
  assert.match(navigation, /return 'settings'/);
  assert.match(quickJump, /label: 'Open Settings',[\s\S]*projectName: null,[\s\S]*target: \{ type: 'settings' \}/);
  assert.match(app, /settingsOpen[\s\S]*\? 'Settings — Workman'/);
  assert.doesNotMatch(settings, /on this Mac|macOS default|macOS Terminal/);
});

test('flush modal footers reset shared negative margins and retain edge padding', async () => {
  const dialogs = [
    'lib/ProfileSwitchDialog.svelte',
    'lib/ConfirmationDialog.svelte',
    'lib/AgentCascadeDialog.svelte',
    'lib/WorktreeImportDialog.svelte',
    'lib/WorktreeDialog.svelte',
    'lib/RegisterProjectDialog.svelte',
    'lib/WorktreeRemoveDialog.svelte',
    'lib/ProjectSettingsDialog.svelte',
    'lib/ProjectFolderSettingsDialog.svelte'
  ];

  for (const dialog of dialogs) {
    const contents = await source(dialog);
    const footer = contents.match(/<(?:AlertDialog|Dialog)\.Footer\b[^>]*>/s)?.[0] ?? '';
    assert.match(footer, /mx-0/, `${dialog} resets horizontal footer bleed`);
    assert.match(footer, /mb-0/, `${dialog} resets bottom footer bleed`);
    assert.match(footer, /px-4/, `${dialog} keeps horizontal button padding`);
    assert.match(footer, /py-3/, `${dialog} keeps vertical button padding`);
    assert.match(footer, /flex-wrap/, `${dialog} keeps buttons reachable at narrow widths`);
  }
});

test('project names can use a persisted sidebar identity color without changing defaults', async () => {
  const [app, dialog, appearance, daemon] = await Promise.all([
    source('App.svelte'),
    source('lib/ProjectSettingsDialog.svelte'),
    source('lib/projectAppearance.ts'),
    source('lib/daemon.ts')
  ]);

  assert.match(dialog, /Project name color/);
  assert.match(dialog, /normalizeSidebarIdentityColor\(project\.name_color\)/);
  assert.match(dialog, /nameColor/);
  assert.match(app, /sidebarIdentityColorValue\(project\.name_color\)/);
  assert.match(appearance, /return normalized \? `var\(--project-icon-\$\{normalized\}\)` : undefined/);
  assert.match(daemon, /name_color: string \| null/);
  assert.match(daemon, /name_color: nameColor/);
});
