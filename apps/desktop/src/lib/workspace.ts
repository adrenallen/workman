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
  project: Project | null;
  connection: ConnectionStatus;
  onError: (message: string) => void;
  onProfileSwitched: () => void;
}

export interface WorkspaceSectionDefinition {
  id: WorkspaceSection;
  icon: string;
  label: string;
  description: string;
  shortcut: number;
}

export const workspaceSections: WorkspaceSectionDefinition[] = [
  {
    id: 'terminal',
    icon: '>_',
    label: 'Terminal',
    description: 'Work in the selected session',
    shortcut: 1
  },
  {
    id: 'processes',
    icon: '▤',
    label: 'Processes',
    description: 'Run project commands',
    shortcut: 2
  },
  {
    id: 'todos',
    icon: '◇',
    label: 'Todos',
    description: 'Plan and coordinate work',
    shortcut: 3
  },
  {
    id: 'scratchpads',
    icon: '≡',
    label: 'Scratchpads',
    description: 'Read shared notes',
    shortcut: 4
  },
  {
    id: 'agents',
    icon: '◎',
    label: 'Agents',
    description: 'Spawn and direct agents',
    shortcut: 5
  },
  {
    id: 'settings',
    icon: '⚙',
    label: 'Settings',
    description: 'Configure this workspace',
    shortcut: 6
  }
];
