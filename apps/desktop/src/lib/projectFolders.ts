import type { Project } from './daemon';
import type { SidebarIdentityColor } from './projectAppearance';

export interface ProjectFolder {
  id: number;
  name: string;
  icon: string | null;
  name_color: string | null;
  collapsed: boolean;
  sort_order: number;
}

export interface ProjectFolderSettingsInput {
  name: string;
  icon: string | null;
  nameColor: SidebarIdentityColor | null;
}

export interface ProjectFolderMenuRequest {
  folder: ProjectFolder;
  projectCount: number;
  x: number;
  y: number;
  restoreFocus: HTMLElement | null;
}

export interface FolderedProject extends Project {
  folder_id: number | null;
}

export type ProjectRailLayoutEntry =
  | { kind: 'project'; id: number }
  | { kind: 'folder'; id: number; project_ids: number[] };

export interface ProjectRailDrop {
  sourceId: number;
  targetId: number;
  placement: 'before' | 'after';
  inside?: boolean;
}

export interface ProjectRailLayoutState {
  projects: FolderedProject[];
  folders: ProjectFolder[];
}

// Folder membership is intentionally single-level. The project rail moves one project at a time;
// modifier multi-selection belongs to the selected project's inner tree and is never consumed here.

export function projectDragId(projectId: number): number {
  return projectId;
}

export function folderDragId(folderId: number): number {
  return -folderId;
}

export function buildProjectRailLayout(
  projects: FolderedProject[],
  folders: ProjectFolder[]
): ProjectRailLayoutEntry[] {
  const folderIds = new Set(folders.map((folder) => folder.id));
  const projectsByFolder = new Map<number, FolderedProject[]>();
  for (const project of projects) {
    if (project.folder_id === null || !folderIds.has(project.folder_id)) continue;
    const children = projectsByFolder.get(project.folder_id) ?? [];
    children.push(project);
    projectsByFolder.set(project.folder_id, children);
  }

  const topLevel = [
    ...folders.map((folder) => ({
      kind: 'folder' as const,
      id: folder.id,
      sortOrder: folder.sort_order,
      project_ids: sortProjects(projectsByFolder.get(folder.id) ?? []).map((project) => project.id)
    })),
    ...projects
      .filter((project) => project.folder_id === null || !folderIds.has(project.folder_id))
      .map((project) => ({
        kind: 'project' as const,
        id: project.id,
        sortOrder: project.sort_order
      }))
  ];

  return topLevel
    .sort((left, right) =>
      left.sortOrder - right.sortOrder
      || Number(left.kind === 'project') - Number(right.kind === 'project')
      || left.id - right.id
    )
    .map(({ sortOrder: _, ...entry }) => entry);
}

export function moveProjectRailEntry(
  layout: ProjectRailLayoutEntry[],
  drop: ProjectRailDrop
): ProjectRailLayoutEntry[] {
  if (drop.sourceId === drop.targetId || drop.sourceId === 0 || drop.targetId === 0) return layout;
  const next = cloneLayout(layout);
  const sourceIsFolder = drop.sourceId < 0;
  const sourceId = Math.abs(drop.sourceId);
  const targetIsFolder = drop.targetId < 0;
  const targetId = Math.abs(drop.targetId);

  const source = removeSource(next, sourceIsFolder, sourceId);
  if (!source) return layout;

  if (targetIsFolder) {
    const targetIndex = next.findIndex((entry) => entry.kind === 'folder' && entry.id === targetId);
    if (targetIndex < 0) return layout;
    const target = next[targetIndex];
    if (target.kind !== 'folder') return layout;
    if (drop.inside) {
      if (source.kind !== 'project') return layout;
      target.project_ids.push(source.id);
      return next;
    }
    next.splice(targetIndex + (drop.placement === 'after' ? 1 : 0), 0, source);
    return next;
  }

  const targetTopIndex = next.findIndex(
    (entry) => entry.kind === 'project' && entry.id === targetId
  );
  if (targetTopIndex >= 0) {
    next.splice(targetTopIndex + (drop.placement === 'after' ? 1 : 0), 0, source);
    return next;
  }

  if (source.kind !== 'project') return layout;
  for (const entry of next) {
    if (entry.kind !== 'folder') continue;
    const targetChildIndex = entry.project_ids.indexOf(targetId);
    if (targetChildIndex < 0) continue;
    entry.project_ids.splice(
      targetChildIndex + (drop.placement === 'after' ? 1 : 0),
      0,
      source.id
    );
    return next;
  }
  return layout;
}

export function moveProjectRailEntryFromKeyboard(
  layout: ProjectRailLayoutEntry[],
  sourceId: number,
  direction: -1 | 1
): ProjectRailLayoutEntry[] {
  const sourceIsFolder = sourceId < 0;
  const id = Math.abs(sourceId);
  if (sourceIsFolder) {
    const index = layout.findIndex((entry) => entry.kind === 'folder' && entry.id === id);
    const target = layout[index + direction];
    return target
      ? moveProjectRailEntry(layout, {
          sourceId,
          targetId: entryDragId(target),
          placement: direction < 0 ? 'before' : 'after'
        })
      : layout;
  }

  const topIndex = layout.findIndex((entry) => entry.kind === 'project' && entry.id === id);
  if (topIndex >= 0) {
    const target = layout[topIndex + direction];
    return target
      ? moveProjectRailEntry(layout, {
          sourceId,
          targetId: entryDragId(target),
          placement: direction < 0 ? 'before' : 'after'
        })
      : layout;
  }

  for (const entry of layout) {
    if (entry.kind !== 'folder') continue;
    const index = entry.project_ids.indexOf(id);
    const targetId = entry.project_ids[index + direction];
    if (index >= 0 && targetId !== undefined) {
      return moveProjectRailEntry(layout, {
        sourceId,
        targetId: projectDragId(targetId),
        placement: direction < 0 ? 'before' : 'after'
      });
    }
  }
  return layout;
}

export function applyProjectRailLayout(
  projects: FolderedProject[],
  folders: ProjectFolder[],
  layout: ProjectRailLayoutEntry[]
): ProjectRailLayoutState {
  const projectById = new Map(projects.map((project) => [project.id, project]));
  const folderById = new Map(folders.map((folder) => [folder.id, folder]));
  const nextProjects: FolderedProject[] = [];
  const nextFolders: ProjectFolder[] = [];

  layout.forEach((entry, topLevelOrder) => {
    if (entry.kind === 'project') {
      const project = projectById.get(entry.id);
      if (project) nextProjects.push({ ...project, folder_id: null, sort_order: topLevelOrder });
      return;
    }
    const folder = folderById.get(entry.id);
    if (folder) nextFolders.push({ ...folder, sort_order: topLevelOrder });
    entry.project_ids.forEach((projectId, childOrder) => {
      const project = projectById.get(projectId);
      if (project) {
        nextProjects.push({ ...project, folder_id: entry.id, sort_order: childOrder });
      }
    });
  });

  return { projects: nextProjects, folders: nextFolders };
}

export function projectRailLayoutSignature(layout: ProjectRailLayoutEntry[]): string {
  return JSON.stringify(layout);
}

function removeSource(
  layout: ProjectRailLayoutEntry[],
  sourceIsFolder: boolean,
  sourceId: number
): ProjectRailLayoutEntry | null {
  if (sourceIsFolder) {
    const index = layout.findIndex((entry) => entry.kind === 'folder' && entry.id === sourceId);
    return index < 0 ? null : layout.splice(index, 1)[0];
  }
  const topIndex = layout.findIndex(
    (entry) => entry.kind === 'project' && entry.id === sourceId
  );
  if (topIndex >= 0) return layout.splice(topIndex, 1)[0];
  for (const entry of layout) {
    if (entry.kind !== 'folder') continue;
    const childIndex = entry.project_ids.indexOf(sourceId);
    if (childIndex >= 0) {
      entry.project_ids.splice(childIndex, 1);
      return { kind: 'project', id: sourceId };
    }
  }
  return null;
}

function entryDragId(entry: ProjectRailLayoutEntry): number {
  return entry.kind === 'folder' ? folderDragId(entry.id) : projectDragId(entry.id);
}

function cloneLayout(layout: ProjectRailLayoutEntry[]): ProjectRailLayoutEntry[] {
  return layout.map((entry) => entry.kind === 'folder'
    ? { ...entry, project_ids: [...entry.project_ids] }
    : { ...entry });
}

function sortProjects(projects: FolderedProject[]): FolderedProject[] {
  return [...projects].sort(
    (left, right) => left.sort_order - right.sort_order || left.id - right.id
  );
}
