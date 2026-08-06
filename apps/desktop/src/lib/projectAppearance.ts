import type { Component } from 'svelte';

const lucideIconModules = import.meta.glob(
  [
    '../../node_modules/@lucide/svelte/dist/icons/*.js',
    '!../../node_modules/@lucide/svelte/dist/icons/index.js'
  ],
  { eager: true, import: 'default' }
) as Record<string, Component>;

function iconLabel(name: string): string {
  return name
    .split('-')
    .map((part) => part.length <= 2 && /^\d+$/.test(part) ? part : `${part.slice(0, 1).toUpperCase()}${part.slice(1)}`)
    .join(' ');
}

export const PROJECT_ICON_CHOICES = Object.entries(lucideIconModules)
  .map(([path, component]) => {
    const id = path.split('/').at(-1)?.replace(/\.js$/, '') ?? '';
    return { id, label: iconLabel(id), component };
  })
  .filter((choice) => choice.id && choice.id !== 'index')
  .sort((left, right) => left.label.localeCompare(right.label));

export const PROJECT_ICON_COLOR_CHOICES = [
  { id: 'slate', label: 'Slate' },
  { id: 'blue', label: 'Blue' },
  { id: 'teal', label: 'Teal' },
  { id: 'amber', label: 'Amber' },
  { id: 'violet', label: 'Violet' },
  { id: 'rose', label: 'Rose' }
] as const;

export type ProjectIconName = string;
export type ProjectIconColor = (typeof PROJECT_ICON_COLOR_CHOICES)[number]['id'];

export interface ProjectSettingsInput {
  displayName: string;
  icon: ProjectIconName | null;
  iconColor: ProjectIconColor | null;
}

const projectIcons = new Map(PROJECT_ICON_CHOICES.map((choice) => [choice.id, choice.component]));
const projectIconColors = new Set<string>(PROJECT_ICON_COLOR_CHOICES.map((choice) => choice.id));

export function normalizeProjectIcon(value: string | null | undefined): ProjectIconName | null {
  return value && projectIcons.has(value) ? value : null;
}

export function projectIconComponent(value: string | null | undefined) {
  return value ? projectIcons.get(value) ?? null : null;
}

export function isProjectImageReference(value: string | null | undefined): boolean {
  return value?.startsWith('image:.workman/icon.') ?? false;
}

export function normalizeProjectIconColor(value: string | null | undefined): ProjectIconColor {
  return value && projectIconColors.has(value) ? value as ProjectIconColor : 'slate';
}

export function projectIconColorValue(value: string | null | undefined): string {
  return `var(--project-icon-${normalizeProjectIconColor(value)})`;
}
