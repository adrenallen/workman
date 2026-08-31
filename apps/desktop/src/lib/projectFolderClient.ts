import type { DaemonClient } from './daemon';
import type {
  FolderedProject,
  ProjectFolder,
  ProjectFolderSettingsInput,
  ProjectRailLayoutEntry
} from './projectFolders';

export interface ProjectRailSnapshot {
  projects: FolderedProject[];
  folders: ProjectFolder[];
  layout: ProjectRailLayoutEntry[];
}

export function loadProjectRail(client: DaemonClient): Promise<ProjectRailSnapshot> {
  return client.control('project.rail', {});
}

export function createProjectFolder(
  client: DaemonClient,
  name: string
): Promise<ProjectRailSnapshot> {
  return client.control('project_folders.create', { name });
}

export function renameProjectFolder(
  client: DaemonClient,
  folderId: number,
  name: string
): Promise<ProjectRailSnapshot> {
  return client.control('project_folders.rename', { folder_id: folderId, name });
}

export function updateProjectFolderSettings(
  client: DaemonClient,
  folderId: number,
  settings: ProjectFolderSettingsInput
): Promise<ProjectRailSnapshot> {
  return client.control('project_folders.update_settings', {
    folder_id: folderId,
    name: settings.name,
    icon: settings.icon,
    name_color: settings.nameColor
  });
}

export function deleteProjectFolder(
  client: DaemonClient,
  folderId: number
): Promise<ProjectRailSnapshot> {
  return client.control('project_folders.delete', {
    folder_id: folderId,
    confirm_delete: true
  });
}

export function setProjectFolderCollapsed(
  client: DaemonClient,
  folderId: number,
  collapsed: boolean
): Promise<ProjectRailSnapshot> {
  return client.control('project_folders.set_collapsed', { folder_id: folderId, collapsed });
}

export function updateProjectLayout(
  client: DaemonClient,
  entries: ProjectRailLayoutEntry[]
): Promise<ProjectRailSnapshot> {
  return client.control('project.layout', { entries });
}
