import type { Project } from './daemon';

export function selectProjectOptimistically(
  projects: Project[],
  projectId: number
): Project[] {
  return projects.map((project) => {
    const selected = project.id === projectId;
    return project.selected === selected ? project : { ...project, selected };
  });
}

export function beginOptimisticNavigation(
  apply: () => void,
  hydrate: () => Promise<void>,
  onError: (cause: unknown) => void
): void {
  apply();
  queueMicrotask(() => {
    void hydrate().catch(onError);
  });
}
