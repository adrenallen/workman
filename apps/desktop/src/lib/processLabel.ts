import type { ProcessView } from './daemon';

export function workingDirLabel(path: string): string {
  const parts = path.split('/').filter(Boolean);
  return parts[0] === 'Users' && parts.length > 2 ? `~/${parts.slice(2).join('/')}` : path;
}

/** Keep generated terminal names quiet; an explicit name describes the terminal's purpose. */
export function processLabel(process: Pick<ProcessView, 'id' | 'kind' | 'name' | 'working_dir'>): string {
  if (process.kind !== 'terminal') return process.name;
  const name = process.name.trim();
  const generated = /^Terminal(?: [1-9]\d*)?$/.test(name)
    || new RegExp(`^terminal--${process.id}(?:-(?:[2-9]|[1-9]\\d+))?$`).test(name);
  return name && !generated ? process.name : workingDirLabel(process.working_dir);
}
