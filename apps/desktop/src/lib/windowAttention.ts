import { invoke } from '@tauri-apps/api/core';

/** macOS can retain a key window in an inactive application. Also honor WebView blur. */
export async function isWorkmanWindowFocused(): Promise<boolean> {
  if (typeof document !== 'undefined' && (document.hidden || !document.hasFocus())) return false;
  const focused = await invoke<boolean>('native_notification_window_focused');
  return focused && (typeof document === 'undefined' || (!document.hidden && document.hasFocus()));
}

export const AGENT_READ_DWELL_MS = 3_000;
export interface AgentReadTarget {
  processId: number;
  projectId: number;
}

/** Repeated status snapshots do not restart the dwell; blur/navigation invalidate pending reads. */
export function createAgentReadDwell(
  onReady: (target: AgentReadTarget, stillCurrent: () => boolean) => void
): { update: (target: AgentReadTarget | null) => void; reset: () => void } {
  let current: AgentReadTarget | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  const reset = (): void => {
    generation += 1;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    current = null;
  };
  return {
    reset,
    update(target) {
      if (current?.processId === target?.processId && current?.projectId === target?.projectId) return;
      reset();
      current = target;
      if (!target) return;
      const request = generation;
      timer = setTimeout(() => {
        timer = undefined;
        onReady(target, () => generation === request);
      }, AGENT_READ_DWELL_MS);
    }
  };
}
