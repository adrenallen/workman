export type DropPlacement = 'before' | 'after';
export type ReorderDirection = -1 | 1;

export interface ReorderDrop {
  sourceId: number;
  targetId: number;
  placement: DropPlacement;
}

export interface ReorderItemOptions {
  id: number;
  group: string;
  disabled?: boolean;
  label?: string;
  onDrop: (drop: ReorderDrop) => void;
  onKeyboardMove: (id: number, direction: ReorderDirection) => void;
}

export interface TreeOrderItem {
  id: number;
  parentId: number | null;
}

interface ActiveDrag {
  id: number;
  group: string;
  node: HTMLElement;
  pointerId: number;
  startX: number;
  startY: number;
  dragging: boolean;
}

let activeDrag: ActiveDrag | null = null;
let suppressClickAfterDrag = false;
const markedTargets = new Set<HTMLElement>();
const reorderItems = new Map<HTMLElement, () => ReorderItemOptions>();
const dragThreshold = 5;

/** Move one ID before or after another while preserving every other relative position. */
export function moveOrderedId(
  orderedIds: number[],
  sourceId: number,
  targetId: number,
  placement: DropPlacement
): number[] {
  if (sourceId === targetId || !orderedIds.includes(sourceId) || !orderedIds.includes(targetId)) {
    return orderedIds;
  }
  const next = orderedIds.filter((id) => id !== sourceId);
  const targetIndex = next.indexOf(targetId);
  next.splice(targetIndex + (placement === 'after' ? 1 : 0), 0, sourceId);
  return next;
}

/**
 * Move one agent and its descendants among siblings.
 *
 * The returned list is a complete depth-first order suitable for the process.reorder RPC. Cross-
 * parent drops are rejected so lineage never changes as a side effect of visual reordering.
 */
export function moveTreeOrderBlock(
  orderedItems: TreeOrderItem[],
  sourceId: number,
  targetId: number,
  placement: DropPlacement
): number[] {
  const original = orderedItems.map((item) => item.id);
  const parentById = new Map(orderedItems.map((item) => [item.id, item.parentId]));
  if (
    sourceId === targetId ||
    !parentById.has(sourceId) ||
    !parentById.has(targetId) ||
    parentById.get(sourceId) !== parentById.get(targetId)
  ) {
    return original;
  }

  const sourceBlock = descendantBlock(orderedItems, sourceId);
  const targetBlock = descendantBlock(orderedItems, targetId);
  if (sourceBlock.includes(targetId) || targetBlock.includes(sourceId)) return original;

  const sourceSet = new Set(sourceBlock);
  const remaining = original.filter((id) => !sourceSet.has(id));
  const anchor = placement === 'before' ? targetBlock[0] : targetBlock.at(-1);
  if (anchor === undefined) return original;
  const anchorIndex = remaining.indexOf(anchor);
  if (anchorIndex < 0) return original;
  remaining.splice(anchorIndex + (placement === 'after' ? 1 : 0), 0, ...sourceBlock);
  return remaining;
}

/** Find the adjacent sibling used by the Alt+Arrow keyboard fallback. */
export function siblingTarget(
  orderedItems: TreeOrderItem[],
  sourceId: number,
  direction: ReorderDirection
): number | null {
  const source = orderedItems.find((item) => item.id === sourceId);
  if (!source) return null;
  const siblings = orderedItems.filter((item) => item.parentId === source.parentId);
  const index = siblings.findIndex((item) => item.id === sourceId);
  return siblings[index + direction]?.id ?? null;
}

/** Svelte action implementing native drag/drop and the Alt+Up/Down keyboard equivalent. */
export function reorderItem(node: HTMLElement, initial: ReorderItemOptions) {
  let options = initial;

  function configure(): void {
    const enabled = !options.disabled;
    // Native HTML drag does not reliably start on buttons in WKWebView. Pointer capture below owns
    // only gestures that cross the movement threshold, leaving ordinary button clicks unchanged.
    node.draggable = false;
    node.dataset.reorderable = enabled ? 'true' : 'false';
    if (enabled) {
      node.setAttribute('aria-keyshortcuts', 'Alt+ArrowUp Alt+ArrowDown');
      node.title = options.label
        ? `${options.label} · Drag to reorder · Alt+↑/↓`
        : 'Drag to reorder · Alt+↑/↓';
    } else {
      node.removeAttribute('aria-keyshortcuts');
      clearMark(node);
    }
  }

  function pointerDown(event: PointerEvent): void {
    if (options.disabled || event.button !== 0 || !event.isPrimary) return;
    suppressClickAfterDrag = false;
    activeDrag = {
      id: options.id,
      group: options.group,
      node,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      dragging: false
    };
    node.setPointerCapture(event.pointerId);
  }

  function pointerMove(event: PointerEvent): void {
    const drag = activeDrag;
    if (!drag || drag.node !== node || drag.pointerId !== event.pointerId) return;
    if (!drag.dragging) {
      const distance = Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY);
      if (distance < dragThreshold) return;
      drag.dragging = true;
      suppressClickAfterDrag = true;
      node.dataset.reorderDragging = 'true';
    }
    event.preventDefault();
    const target = pointerTarget(drag, event.clientX, event.clientY);
    if (target) mark(target, placementFor(target, event.clientY));
    else clearDropMarks();
  }

  function pointerUp(event: PointerEvent): void {
    const drag = activeDrag;
    if (!drag || drag.node !== node || drag.pointerId !== event.pointerId) return;
    const target = drag.dragging ? pointerTarget(drag, event.clientX, event.clientY) : null;
    const targetOptions = target ? reorderItems.get(target)?.() : null;
    const placement = target
      ? ((target.dataset.reorderDrop as DropPlacement | undefined) ??
        placementFor(target, event.clientY))
      : null;
    if (drag.dragging) event.preventDefault();
    finishPointerDrag(drag);
    if (targetOptions && placement) {
      options.onDrop({ sourceId: drag.id, targetId: targetOptions.id, placement });
    }
  }

  function pointerCancel(event: PointerEvent): void {
    const drag = activeDrag;
    if (!drag || drag.node !== node || drag.pointerId !== event.pointerId) return;
    finishPointerDrag(drag);
  }

  function click(event: MouseEvent): void {
    if (!suppressClickAfterDrag) return;
    suppressClickAfterDrag = false;
    event.preventDefault();
    event.stopImmediatePropagation();
  }

  function keyDown(event: KeyboardEvent): void {
    if (!activeDrag) suppressClickAfterDrag = false;
    if (options.disabled || !event.altKey || event.metaKey || event.ctrlKey || event.shiftKey) return;
    const direction = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : null;
    if (direction === null) return;
    event.preventDefault();
    event.stopPropagation();
    options.onKeyboardMove(options.id, direction);
  }

  reorderItems.set(node, () => options);
  node.addEventListener('pointerdown', pointerDown);
  node.addEventListener('pointermove', pointerMove);
  node.addEventListener('pointerup', pointerUp);
  node.addEventListener('pointercancel', pointerCancel);
  node.addEventListener('click', click, true);
  node.addEventListener('keydown', keyDown);
  configure();

  return {
    update(next: ReorderItemOptions) {
      options = next;
      configure();
    },
    destroy() {
      reorderItems.delete(node);
      node.removeEventListener('pointerdown', pointerDown);
      node.removeEventListener('pointermove', pointerMove);
      node.removeEventListener('pointerup', pointerUp);
      node.removeEventListener('pointercancel', pointerCancel);
      node.removeEventListener('click', click, true);
      node.removeEventListener('keydown', keyDown);
      if (activeDrag?.node === node) finishPointerDrag(activeDrag);
      clearMark(node);
    }
  };
}

function descendantBlock(items: TreeOrderItem[], rootId: number): number[] {
  const descendants = new Set([rootId]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const item of items) {
      if (item.parentId !== null && descendants.has(item.parentId) && !descendants.has(item.id)) {
        descendants.add(item.id);
        changed = true;
      }
    }
  }
  return items.filter((item) => descendants.has(item.id)).map((item) => item.id);
}

function pointerTarget(drag: ActiveDrag, clientX: number, clientY: number): HTMLElement | null {
  for (const [candidate, readOptions] of reorderItems) {
    const candidateOptions = readOptions();
    if (
      candidate === drag.node ||
      candidateOptions.disabled ||
      candidateOptions.group !== drag.group
    ) continue;
    const bounds = candidate.getBoundingClientRect();
    if (
      clientX >= bounds.left &&
      clientX <= bounds.right &&
      clientY >= bounds.top &&
      clientY <= bounds.bottom
    ) return candidate;
  }
  return null;
}

function finishPointerDrag(drag: ActiveDrag): void {
  clearAllMarks();
  drag.node.removeAttribute('data-reorder-dragging');
  if (drag.node.hasPointerCapture(drag.pointerId)) drag.node.releasePointerCapture(drag.pointerId);
  if (activeDrag === drag) activeDrag = null;
}

function placementFor(node: HTMLElement, clientY: number): DropPlacement {
  const bounds = node.getBoundingClientRect();
  return clientY < bounds.top + bounds.height / 2 ? 'before' : 'after';
}

function mark(node: HTMLElement, placement: DropPlacement): void {
  for (const target of markedTargets) {
    if (target !== node) clearMark(target);
  }
  node.dataset.reorderDrop = placement;
  markedTargets.add(node);
}

function clearMark(node: HTMLElement): void {
  node.removeAttribute('data-reorder-drop');
  markedTargets.delete(node);
}

function clearAllMarks(): void {
  clearDropMarks();
  activeDrag?.node.removeAttribute('data-reorder-dragging');
}

function clearDropMarks(): void {
  for (const target of [...markedTargets]) clearMark(target);
}
