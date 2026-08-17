const fallbackProjectTitle = 'Project';

/** Keep folder defaults literal: only remove separators and take the final path segment. */
export function defaultProjectTitleFromPath(path: string, fallback = fallbackProjectTitle): string {
  const segments = path.trim().split(/[\\/]+/).filter(Boolean);
  return segments.at(-1)?.trim() || fallback;
}

/** Worktree titles use the branch leaf so `feat/inline-drafts` becomes `inline-drafts`. */
export function defaultWorktreeTitle(branch: string, path = ''): string {
  const branchTitle = defaultProjectTitleFromPath(branch, '');
  return branchTitle || defaultProjectTitleFromPath(path, 'Worktree');
}

/** Empty edits intentionally fall back instead of persisting an empty display name. */
export function resolvedProjectTitle(title: string, fallback: string): string {
  return title.trim() || fallback.trim() || fallbackProjectTitle;
}
