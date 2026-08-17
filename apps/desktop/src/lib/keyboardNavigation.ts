export type AppPanel = 'projects' | 'tree' | 'main';

export const appPanelOrder: AppPanel[] = ['projects', 'tree', 'main'];

export function isTerminalInputTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && Boolean(target.closest('.xterm'));
}

export function isTextEditingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target instanceof HTMLInputElement
    || target instanceof HTMLTextAreaElement
    || target.isContentEditable
    || Boolean(target.closest('[contenteditable="true"], [contenteditable="plaintext-only"]'));
}

export function panelForTarget(target: EventTarget | null): AppPanel | null {
  if (!(target instanceof HTMLElement)) return null;
  const panel = target.closest<HTMLElement>('[data-app-panel]')?.dataset.appPanel;
  return panel === 'projects' || panel === 'tree' || panel === 'main' ? panel : null;
}

export function focusAdjacentPanel(
  current: AppPanel | null,
  direction: -1 | 1,
  root: ParentNode = document
): AppPanel | null {
  const available = appPanelOrder.filter((panel) =>
    root.querySelector(`[data-app-panel="${panel}"]`)
  );
  if (available.length === 0) return null;
  const currentIndex = current ? available.indexOf(current) : -1;
  const start = currentIndex < 0 ? (direction > 0 ? -1 : 0) : currentIndex;
  const nextIndex = (start + direction + available.length) % available.length;
  const next = available[nextIndex];
  focusPanel(next, root);
  return next;
}

export function focusPanel(panel: AppPanel, root: ParentNode = document): boolean {
  const container = root.querySelector<HTMLElement>(`[data-app-panel="${panel}"]`);
  if (!container) return false;

  if (panel === 'main') {
    const terminal = container.querySelector<HTMLElement>('.xterm-helper-textarea');
    if (terminal) {
      terminal.focus();
      return true;
    }
  }

  const preferredSelectors = panel === 'projects'
    ? ['.project-select[aria-current="page"]', '.project-select:not(:disabled)', 'button:not(:disabled)']
    : panel === 'tree'
      ? ['.draft-row.selected, .tree-row.selected', '[data-tree-row]:not(:disabled)', 'input:not(:disabled)', 'button:not(:disabled)']
      : ['button:not(:disabled)', 'input:not(:disabled)', 'textarea:not(:disabled)', '[tabindex="0"]'];

  for (const selector of preferredSelectors) {
    const focusable = container.querySelector<HTMLElement>(selector);
    if (focusable) {
      focusable.focus();
      return true;
    }
  }

  container.focus();
  return document.activeElement === container;
}

export function moveListFocus(
  container: HTMLElement,
  currentTarget: EventTarget | null,
  selector: string,
  direction: -1 | 1
): boolean {
  const rows = Array.from(container.querySelectorAll<HTMLElement>(selector));
  if (rows.length === 0) return false;
  const target = currentTarget instanceof HTMLElement ? currentTarget : null;
  const row = target?.closest<HTMLElement>(selector) ?? null;
  const current = row ? rows.indexOf(row) : -1;
  const next = current < 0
    ? direction > 0 ? 0 : rows.length - 1
    : Math.min(rows.length - 1, Math.max(0, current + direction));
  rows[next]?.focus();
  return true;
}
