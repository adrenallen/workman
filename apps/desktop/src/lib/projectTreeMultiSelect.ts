export type ProjectTreeMultiSelectGroup =
  | 'todos'
  | 'agents'
  | 'terminals'
  | 'scratchpads';

export type ProjectTreeBulkAction =
  | 'stop'
  | 'close'
  | 'complete'
  | 'delete'
  | 'archive';

export interface ProjectTreeMultiSelection {
  group: ProjectTreeMultiSelectGroup;
  ids: number[];
}

export interface ProjectTreeSelectionGesture {
  group: ProjectTreeMultiSelectGroup;
  id: number;
  orderedIds: number[];
  anchorId: number | null;
  toggle: boolean;
  range: boolean;
}

/** Apply a native-list modifier click while keeping selections inside one tree group. */
export function updateProjectTreeMultiSelection(
  current: ProjectTreeMultiSelection | null,
  gesture: ProjectTreeSelectionGesture
): ProjectTreeMultiSelection | null {
  const selected = new Set(
    current?.group === gesture.group ? current.ids : []
  );

  if (gesture.range) {
    const anchorIndex = gesture.anchorId === null
      ? -1
      : gesture.orderedIds.indexOf(gesture.anchorId);
    const clickedIndex = gesture.orderedIds.indexOf(gesture.id);
    if (anchorIndex >= 0 && clickedIndex >= 0) {
      const start = Math.min(anchorIndex, clickedIndex);
      const end = Math.max(anchorIndex, clickedIndex);
      for (const id of gesture.orderedIds.slice(start, end + 1)) selected.add(id);
    } else {
      selected.add(gesture.id);
    }
  } else if (gesture.toggle) {
    if (selected.has(gesture.id)) selected.delete(gesture.id);
    else selected.add(gesture.id);
  } else {
    selected.clear();
  }

  if (selected.size === 0) return null;
  const visible = gesture.orderedIds.filter((id) => selected.has(id));
  const hidden = [...selected].filter((id) => !gesture.orderedIds.includes(id));
  return { group: gesture.group, ids: [...visible, ...hidden] };
}

export function selectedInTreeGroup(
  selection: ProjectTreeMultiSelection | null,
  group: ProjectTreeMultiSelectGroup,
  id: number
): boolean {
  return selection?.group === group && selection.ids.includes(id);
}

export function bulkFailureMessage(
  actionPast: string,
  actionInfinitive: string,
  total: number,
  failures: Array<{ label: string; message: string }>
): string | null {
  if (failures.length === 0) return null;
  const succeeded = total - failures.length;
  const outcome = succeeded > 0
    ? `${succeeded} of ${total} selected items ${actionPast}; ${failures.length} failed.`
    : `Could not ${actionInfinitive} any of ${total} selected items.`;
  return `${outcome} ${failures.map((failure) => `${failure.label}: ${failure.message}`).join(' · ')}`;
}
