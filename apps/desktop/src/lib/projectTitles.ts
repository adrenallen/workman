const fallbackProjectTitle = 'Project';

/** Keep folder defaults literal: only remove separators and take the final path segment. */
export function defaultProjectTitleFromPath(path: string, fallback = fallbackProjectTitle): string {
  const value = path.trim();
  const windowsStyle = /^[A-Za-z]:[\\/]/.test(value) || value.startsWith('\\\\');
  const segments = value.split(windowsStyle ? /[\\/]+/ : /\/+/).filter(Boolean);
  return segments.at(-1)?.trim() || fallback;
}

/** Preserve the full branch to distinguish `feat/x` from `fix/x`. */
export function defaultWorktreeTitle(branch: string, path = ''): string {
  const branchTitle = branch
    .trim()
    .replace(/^refs\/heads\//, '')
    .replace(/^\/+|\/+$/g, '');
  return branchTitle || defaultProjectTitleFromPath(path, '');
}

/** Empty edits intentionally fall back instead of persisting an empty display name. */
export function resolvedProjectTitle(title: string, fallback: string): string {
  return title.trim() || fallback.trim() || fallbackProjectTitle;
}

interface RegisteredProjectTitle {
  path: string;
  name: string;
  display_name: string | null;
}

/** Prefer the user's current title when the picker returns an already-known path. */
export function registrationTitleForPath(
  path: string,
  projects: RegisteredProjectTitle[]
): string {
  const existing = projects.find((project) => project.path === path);
  return existing?.display_name?.trim()
    || existing?.name.trim()
    || defaultProjectTitleFromPath(path);
}

/** A derived default remains live only until the user edits the title. */
export function syncProjectTitleDefault(
  currentTitle: string,
  nextDefault: string,
  touched: boolean
): string {
  return touched ? currentTitle : nextDefault;
}
