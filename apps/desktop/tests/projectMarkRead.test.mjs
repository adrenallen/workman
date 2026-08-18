import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('project menu enables mark as read only when that project has unread state', async () => {
  const menu = await readFile(new URL('../src/lib/contextMenu.ts', import.meta.url), 'utf8');

  assert.match(menu, /hasUnread\?: boolean/);
  assert.match(menu, /id: 'mark-read',[\s\S]*label: 'Mark as read',[\s\S]*disabled: !target\.hasUnread/);
});

test('project mark as read uses one bulk RPC and clears only project-local UI state', async () => {
  const [app, daemon] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/daemon.ts', import.meta.url), 'utf8')
  ]);

  assert.match(daemon, /markProjectRead\(projectId: number\)[\s\S]*this\.request\('projects\.mark_read', \{ project_id: projectId \}\)/);
  assert.match(app, /case 'mark-read':\s*await markProjectRead\(project\.id\);/);
  assert.match(app, /notification\.project_id === projectId && notification\.read_at === null/);
  assert.match(app, /process\.project_id === projectId && process\.kind === 'agent' && process\.agent_state\.unread/);
  assert.match(app, /await waitForNotificationIdle\(\);/);
  assert.match(app, /for \(const processId of pendingProcessIds\) markReadPending\.add\(processId\);/);
  assert.match(app, /for \(const processId of pendingProcessIds\) markReadPending\.delete\(processId\);/);
  assert.match(app, /await client\.markProjectRead\(projectId\);/);
  assert.match(app, /notifications = notifications\.map\(\(notification\) =>[\s\S]*unreadNotificationIds\.has\(notification\.id\)[\s\S]*read_at: null/);
  assert.match(app, /Promise\.all\(\[refreshNotifications\(\), refreshProcesses\(projectId\)\]\)/);
});
