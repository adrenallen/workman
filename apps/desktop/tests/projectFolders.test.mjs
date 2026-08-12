import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  applyProjectRailLayout,
  buildProjectRailLayout,
  folderDragId,
  moveProjectRailEntry,
  moveProjectRailEntryFromKeyboard
} from '../src/lib/projectFolders.ts';

function project(id, sortOrder, folderId = null) {
  return { id, sort_order: sortOrder, folder_id: folderId };
}

function folder(id, sortOrder, collapsed = false) {
  return { id, name: `Folder ${id}`, sort_order: sortOrder, collapsed };
}

test('builds one mixed top-level order and stable per-folder project orders', () => {
  assert.deepEqual(
    buildProjectRailLayout(
      [project(1, 2), project(2, 1, 10), project(3, 0, 10), project(4, 0, 99)],
      [folder(10, 1)]
    ),
    [
      { kind: 'project', id: 4 },
      { kind: 'folder', id: 10, project_ids: [3, 2] },
      { kind: 'project', id: 1 }
    ]
  );
});

test('moves projects into, across, and out of folders without disturbing other entries', () => {
  const original = [
    { kind: 'project', id: 1 },
    { kind: 'folder', id: 10, project_ids: [2, 3] },
    { kind: 'folder', id: 11, project_ids: [4] },
    { kind: 'project', id: 5 }
  ];
  const into = moveProjectRailEntry(original, {
    sourceId: 1,
    targetId: folderDragId(10),
    placement: 'after',
    inside: true
  });
  assert.deepEqual(into[0], { kind: 'folder', id: 10, project_ids: [2, 3, 1] });

  const across = moveProjectRailEntry(into, {
    sourceId: 2,
    targetId: 4,
    placement: 'before'
  });
  assert.deepEqual(across[0], { kind: 'folder', id: 10, project_ids: [3, 1] });
  assert.deepEqual(across[1], { kind: 'folder', id: 11, project_ids: [2, 4] });

  const out = moveProjectRailEntry(across, {
    sourceId: 4,
    targetId: folderDragId(11),
    placement: 'after'
  });
  assert.deepEqual(out, [
    { kind: 'folder', id: 10, project_ids: [3, 1] },
    { kind: 'folder', id: 11, project_ids: [2] },
    { kind: 'project', id: 4 },
    { kind: 'project', id: 5 }
  ]);
  assert.deepEqual(original[1].project_ids, [2, 3], 'the original layout stays immutable');
});

test('reorders folders only at top level and rejects folder nesting', () => {
  const layout = [
    { kind: 'folder', id: 10, project_ids: [1] },
    { kind: 'project', id: 2 },
    { kind: 'folder', id: 11, project_ids: [] }
  ];
  assert.deepEqual(
    moveProjectRailEntry(layout, {
      sourceId: folderDragId(11),
      targetId: folderDragId(10),
      placement: 'before'
    }),
    [layout[2], layout[0], layout[1]]
  );
  assert.equal(
    moveProjectRailEntry(layout, {
      sourceId: folderDragId(10),
      targetId: folderDragId(11),
      placement: 'after',
      inside: true
    }),
    layout
  );
});

test('keyboard fallback reorders visual siblings without changing membership', () => {
  const layout = [
    { kind: 'project', id: 1 },
    { kind: 'folder', id: 10, project_ids: [2, 3] },
    { kind: 'project', id: 4 }
  ];
  assert.deepEqual(moveProjectRailEntryFromKeyboard(layout, folderDragId(10), -1), [
    layout[1], layout[0], layout[2]
  ]);
  assert.deepEqual(moveProjectRailEntryFromKeyboard(layout, 3, -1), [
    layout[0],
    { kind: 'folder', id: 10, project_ids: [3, 2] },
    layout[2]
  ]);
});

test('optimistic layout application assigns mixed and scoped sort orders', () => {
  const projects = [project(1, 0), project(2, 1), project(3, 2)];
  const folders = [folder(10, 0)];
  const state = applyProjectRailLayout(projects, folders, [
    { kind: 'project', id: 3 },
    { kind: 'folder', id: 10, project_ids: [2, 1] }
  ]);
  assert.deepEqual(state.folders, [{ ...folders[0], sort_order: 1 }]);
  assert.deepEqual(
    state.projects.map(({ id, folder_id, sort_order }) => ({ id, folder_id, sort_order })),
    [
      { id: 3, folder_id: null, sort_order: 0 },
      { id: 2, folder_id: 10, sort_order: 0 },
      { id: 1, folder_id: 10, sort_order: 1 }
    ]
  );
});

test('folder header keeps collapse, rename, context, pointer, and keyboard paths accessible', async () => {
  const source = await readFile(
    new URL('../src/lib/ProjectFolderHeader.svelte', import.meta.url),
    'utf8'
  );
  assert.match(source, /aria-expanded=\{!folder\.collapsed\}/);
  assert.match(source, /use:reorderItem/);
  assert.match(source, /canDropInside: \(sourceId\) => sourceId > 0/);
  assert.match(source, /event\.shiftKey \|\| event\.key !== 'F10'/);
  assert.match(source, /Project folder name/);
  assert.match(source, /data-reorder-drop='inside'/);

  const menu = await readFile(
    new URL('../src/lib/ProjectFolderMenu.svelte', import.meta.url),
    'utf8'
  );
  assert.match(menu, /Rename folder/);
  assert.match(menu, /Delete folder…/);
  assert.match(menu, /Projects return to top level/);
});
