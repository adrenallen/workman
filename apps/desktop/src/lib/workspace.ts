import type { ConnectionStatus, DaemonClient, ProcessView, Project } from './daemon';

export type WorkspaceSection =
  | 'terminal'
  | 'processes'
  | 'todos'
  | 'scratchpads'
  | 'agents'
  | 'settings';

export interface AgentsPanelProps {
  client: DaemonClient;
  project: Project;
  processes: ProcessView[];
  selectedProcessId: number | null;
  spawnSignal: number;
  connected: boolean;
  onSelectProcess: (processId: number) => void;
  onError: (message: string) => void;
}

export interface SettingsPanelProps {
  client: DaemonClient;
  project: Project;
  connection: ConnectionStatus;
  onError: (message: string) => void;
}

export interface WorkspaceSectionDefinition {
  id: WorkspaceSection;
  label: string;
  description: string;
  shortcut: number;
}

export const workspaceSections: WorkspaceSectionDefinition[] = [
  {
    id: 'terminal',
    label: 'Terminal',
    description: 'Work in the selected session',
    shortcut: 1
  },
  {
    id: 'processes',
    label: 'Processes',
    description: 'Run project commands',
    shortcut: 2
  },
  {
    id: 'todos',
    label: 'Todos',
    description: 'Plan and coordinate work',
    shortcut: 3
  },
  {
    id: 'scratchpads',
    label: 'Scratchpads',
    description: 'Read shared notes',
    shortcut: 4
  },
  {
    id: 'agents',
    label: 'Agents',
    description: 'Spawn and direct agents',
    shortcut: 5
  },
  {
    id: 'settings',
    label: 'Settings',
    description: 'Configure this workspace',
    shortcut: 6
  }
];
