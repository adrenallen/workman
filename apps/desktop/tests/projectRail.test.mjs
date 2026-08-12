import assert from 'node:assert/strict';
import test from 'node:test';

import { moveOrderedId, reorderItem } from '../src/lib/reorder.ts';
import { initialFlatProjectOrder, worktreeParentLabel } from '../src/lib/worktrees.ts';

function project(id, parentProjectId = null, name = `project-${id}`, displayName = null) {
  return { id, parent_project_id: parentProjectId, name, display_name: displayName };
}

test('seeds a flat rail with each parent followed by its existing worktrees once', () => {
  assert.deepEqual(
    initialFlatProjectOrder([
      project(1),
      project(2),
      project(3, 1),
      project(4, 1),
      project(5, 2)
    ]),
    [1, 3, 4, 2, 5]
  );
});

test('keeps orphaned worktrees visible and preserves stable sibling order', () => {
  assert.deepEqual(
    initialFlatProjectOrder([
      project(8, 99),
      project(1),
      project(4, 1),
      project(3, 1),
      project(2)
    ]),
    [8, 1, 4, 3, 2]
  );
});

test('is idempotent once the initial parent-followed order has been seeded', () => {
  assert.deepEqual(
    initialFlatProjectOrder([project(1), project(3, 1), project(4, 1), project(2), project(5, 2)]),
    [1, 3, 4, 2, 5]
  );
});

test('flat reordering can separate a worktree from its parent without moving a block', () => {
  assert.deepEqual(moveOrderedId([1, 3, 4, 2, 5], 3, 2, 'after'), [1, 4, 2, 3, 5]);
});

test('labels a flat worktree with its parent and keeps an orphan fallback', () => {
  const parent = project(1, null, 'repository', 'Client site');
  assert.equal(worktreeParentLabel(project(2, 1, 'repository: topic'), [parent]), 'Client site');
  assert.equal(worktreeParentLabel(project(3, 99, 'repository: orphan'), [], 'Repository'), 'Repository');
  assert.equal(worktreeParentLabel(parent, [parent]), null);
});

test('shared row action uses a pointer threshold, reorders, and never turns a drag into a click', () => {
  class FakeRow {
    dataset = {};
    listeners = new Map();
    draggable = false;
    title = '';
    capturedPointer = null;

    constructor(top) {
      this.top = top;
    }

    addEventListener(type, listener, capture = false) {
      const listeners = this.listeners.get(type) ?? [];
      listeners.push({ listener, capture: capture === true });
      this.listeners.set(type, listeners);
    }
    removeEventListener(type, listener, capture = false) {
      const listeners = this.listeners.get(type) ?? [];
      this.listeners.set(
        type,
        listeners.filter((entry) => entry.listener !== listener || entry.capture !== (capture === true))
      );
    }
    setAttribute() {}
    removeAttribute(name) {
      if (name === 'data-reorder-dragging') delete this.dataset.reorderDragging;
      if (name === 'data-reorder-drop') delete this.dataset.reorderDrop;
    }
    contains() {
      return false;
    }
    setPointerCapture(pointerId) {
      this.capturedPointer = pointerId;
    }
    hasPointerCapture(pointerId) {
      return this.capturedPointer === pointerId;
    }
    releasePointerCapture(pointerId) {
      if (this.capturedPointer === pointerId) this.capturedPointer = null;
    }
    getBoundingClientRect() {
      return { top: this.top, bottom: this.top + 24, left: 0, right: 200, height: 24 };
    }
    dispatch(type, event) {
      const listeners = this.listeners.get(type) ?? [];
      for (const { listener } of [...listeners].sort((left, right) => Number(right.capture) - Number(left.capture))) {
        listener(event);
        if (event.immediatePropagationStopped) break;
      }
    }
  }

  const source = new FakeRow(0);
  const target = new FakeRow(24);
  const dropped = [];
  const options = (id) => ({
    id,
    group: 'scratchpad:1',
    label: `Scratchpad ${id}`,
    canDropInside: id === 2 ? (sourceId) => sourceId === 1 : undefined,
    onDrop: (drop) => dropped.push(drop),
    onKeyboardMove: () => {}
  });
  const destroySource = reorderItem(source, options(1));
  const destroyTarget = reorderItem(target, options(2));
  let navigationCount = 0;
  target.addEventListener('click', () => navigationCount += 1);
  source.dispatch('pointerdown', {
    button: 0,
    isPrimary: true,
    pointerId: 7,
    clientX: 20,
    clientY: 12
  });
  source.dispatch('pointermove', {
    pointerId: 7,
    clientX: 20,
    clientY: 15,
    preventDefault() {}
  });
  assert.equal(source.dataset.reorderDragging, undefined, 'small movement remains an ordinary click');

  source.dispatch('pointermove', {
    pointerId: 7,
    clientX: 20,
    clientY: 36,
    preventDefault() {}
  });
  source.dispatch('pointerup', {
    pointerId: 7,
    clientX: 20,
    clientY: 36,
    preventDefault() {}
  });

  const postDragClick = {
    defaultPrevented: false,
    immediatePropagationStopped: false,
    preventDefault() {
      this.defaultPrevented = true;
    },
    stopImmediatePropagation() {
      this.immediatePropagationStopped = true;
    }
  };
  target.dispatch('click', postDragClick);

  assert.deepEqual(dropped, [{ sourceId: 1, targetId: 2, placement: 'after', inside: true }]);
  assert.equal(source.draggable, false, 'the action never creates an HTML/file drag payload');
  assert.equal(source.capturedPointer, null);
  assert.equal(postDragClick.defaultPrevented, true);
  assert.equal(navigationCount, 0, 'the click directly following a drag must not navigate');

  target.dispatch('pointerdown', {
    button: 0,
    isPrimary: true,
    pointerId: 8,
    clientX: 20,
    clientY: 36
  });
  target.dispatch('pointerup', {
    pointerId: 8,
    clientX: 20,
    clientY: 36,
    preventDefault() {}
  });
  target.dispatch('click', {
    preventDefault() {},
    stopImmediatePropagation() {}
  });
  assert.equal(navigationCount, 1, 'a later intentional click still activates the row');

  destroySource.destroy();
  destroyTarget.destroy();
});

test('shared row action leaves modifier-click gestures entirely to list selection', () => {
  class FakeRow {
    dataset = {};
    listeners = new Map();
    draggable = false;
    title = '';

    addEventListener(type, listener) {
      const listeners = this.listeners.get(type) ?? [];
      listeners.push(listener);
      this.listeners.set(type, listeners);
    }
    removeEventListener() {}
    setAttribute() {}
    removeAttribute() {}
    dispatch(type, event) {
      for (const listener of this.listeners.get(type) ?? []) listener(event);
    }
  }

  const row = new FakeRow();
  let captured = false;
  row.setPointerCapture = () => { captured = true; };
  const action = reorderItem(row, {
    id: 1,
    group: 'todo:1',
    label: 'Todo 1',
    onDrop() {},
    onKeyboardMove() {}
  });

  for (const modifier of ['metaKey', 'ctrlKey', 'shiftKey']) {
    row.dispatch('pointerdown', {
      button: 0,
      isPrimary: true,
      pointerId: 1,
      clientX: 10,
      clientY: 10,
      metaKey: modifier === 'metaKey',
      ctrlKey: modifier === 'ctrlKey',
      shiftKey: modifier === 'shiftKey'
    });
  }
  assert.equal(captured, false);
  action.destroy();
});
