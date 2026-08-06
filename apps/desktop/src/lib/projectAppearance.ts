export const PROJECT_ICON_CHOICES = [
  { id: 'code-2', label: 'Code' },
  { id: 'boxes', label: 'Modules' },
  { id: 'bot', label: 'Automation' },
  { id: 'database', label: 'Data' },
  { id: 'globe-2', label: 'Web' },
  { id: 'terminal', label: 'CLI' },
  { id: 'workflow', label: 'Workflow' },
  { id: 'rocket', label: 'Launch' }
] as const;

export const PROJECT_ICON_COLOR_CHOICES = [
  { id: 'slate', label: 'Slate' },
  { id: 'blue', label: 'Blue' },
  { id: 'teal', label: 'Teal' },
  { id: 'amber', label: 'Amber' },
  { id: 'violet', label: 'Violet' },
  { id: 'rose', label: 'Rose' }
] as const;

export type ProjectIconName = (typeof PROJECT_ICON_CHOICES)[number]['id'];
export type ProjectIconColor = (typeof PROJECT_ICON_COLOR_CHOICES)[number]['id'];

export interface ProjectSettingsInput {
  displayName: string;
  icon: ProjectIconName | null;
  iconColor: ProjectIconColor | null;
}

const projectIconNames = new Set<string>(PROJECT_ICON_CHOICES.map((choice) => choice.id));
const projectIconColors = new Set<string>(PROJECT_ICON_COLOR_CHOICES.map((choice) => choice.id));

export function normalizeProjectIcon(value: string | null | undefined): ProjectIconName | null {
  return value && projectIconNames.has(value) ? value as ProjectIconName : null;
}

export function normalizeProjectIconColor(value: string | null | undefined): ProjectIconColor {
  return value && projectIconColors.has(value) ? value as ProjectIconColor : 'slate';
}

export function projectIconColorValue(value: string | null | undefined): string {
  return `var(--project-icon-${normalizeProjectIconColor(value)})`;
}
