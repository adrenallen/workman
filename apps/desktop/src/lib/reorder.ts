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
}

let activeDrag: ActiveDrag | null = null;
const markedTargets = new Set<HTMLElement>();

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
    node.draggable = enabled;
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

  function dragStart(event: DragEvent): void {
    if (options.disabled) {
      event.preventDefault();
      return;
    }
    activeDrag = { id: options.id, group: options.group, node };
    node.dataset.reorderDragging = 'true';
    event.dataTransfer?.setData('text/plain', String(options.id));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
  }

  function dragOver(event: DragEvent): void {
    if (!canAccept(options)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    mark(node, placementFor(node, event.clientY));
  }

  function dragLeave(event: DragEvent): void {
    if (!node.contains(event.relatedTarget as Node | null)) clearMark(node);
  }

  function drop(event: DragEvent): void {
    if (!activeDrag || !canAccept(options)) return;
    event.preventDefault();
    const placement = (node.dataset.reorderDrop as DropPlacement | undefined) ??
      placementFor(node, event.clientY);
    const sourceId = activeDrag.id;
    clearAllMarks();
    options.onDrop({ sourceId, targetId: options.id, placement });
  }

  function dragEnd(): void {
    node.removeAttribute('data-reorder-dragging');
    clearAllMarks();
    activeDrag = null;
  }

  function keyDown(event: KeyboardEvent): void {
    if (options.disabled || !event.altKey || event.metaKey || event.ctrlKey || event.shiftKey) return;
    const direction = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : null;
    if (direction === null) return;
    event.preventDefault();
    event.stopPropagation();
    options.onKeyboardMove(options.id, direction);
  }

  node.addEventListener('dragstart', dragStart);
  node.addEventListener('dragover', dragOver);
  node.addEventListener('dragleave', dragLeave);
  node.addEventListener('drop', drop);
  node.addEventListener('dragend', dragEnd);
  node.addEventListener('keydown', keyDown);
  configure();

  return {
    update(next: ReorderItemOptions) {
      options = next;
      configure();
    },
    destroy() {
      node.removeEventListener('dragstart', dragStart);
      node.removeEventListener('dragover', dragOver);
      node.removeEventListener('dragleave', dragLeave);
      node.removeEventListener('drop', drop);
      node.removeEventListener('dragend', dragEnd);
      node.removeEventListener('keydown', keyDown);
      if (activeDrag?.node === node) activeDrag = null;
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

function canAccept(options: ReorderItemOptions): boolean {
  return Boolean(
    !options.disabled &&
    activeDrag &&
    activeDrag.group === options.group &&
    activeDrag.id !== options.id
  );
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
  for (const target of [...markedTargets]) clearMark(target);
  activeDrag?.node.removeAttribute('data-reorder-dragging');
}
