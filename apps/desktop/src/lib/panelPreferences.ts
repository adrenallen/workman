export interface PanelPreference {
  collapsed: boolean;
  width: number;
}

interface ResizeOptions {
  current: number;
  direction?: 1 | -1;
  max: number;
  min: number;
  onResize: (width: number) => void;
  onEnd: (width: number) => void;
}

const storagePrefix = 'awm.panel.v1.';

export function clampPanelWidth(width: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Math.round(width)));
}

export function loadPanelPreference(
  key: string,
  fallback: PanelPreference,
  min: number,
  max: number
): PanelPreference {
  try {
    const raw = localStorage.getItem(`${storagePrefix}${key}`);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Partial<PanelPreference>;
    return {
      collapsed: typeof parsed.collapsed === 'boolean' ? parsed.collapsed : fallback.collapsed,
      width:
        typeof parsed.width === 'number'
          ? clampPanelWidth(parsed.width, min, max)
          : fallback.width
    };
  } catch {
    return fallback;
  }
}

export function savePanelPreference(key: string, preference: PanelPreference): void {
  try {
    localStorage.setItem(`${storagePrefix}${key}`, JSON.stringify(preference));
  } catch {
    // A disabled or full local store should not make panel controls unusable.
  }
}

export function startPanelResize(event: PointerEvent, options: ResizeOptions): void {
  if (event.button !== 0) return;
  event.preventDefault();
  const handle = event.currentTarget as HTMLElement;
  const pointerId = event.pointerId;
  const startX = event.clientX;
  const direction = options.direction ?? 1;
  let latest = options.current;

  handle.setPointerCapture(pointerId);
  document.body.classList.add('resizing-panel');

  const move = (next: PointerEvent): void => {
    latest = clampPanelWidth(
      options.current + (next.clientX - startX) * direction,
      options.min,
      options.max
    );
    options.onResize(latest);
  };

  const finish = (): void => {
    handle.removeEventListener('pointermove', move);
    handle.removeEventListener('pointerup', finish);
    handle.removeEventListener('pointercancel', finish);
    document.body.classList.remove('resizing-panel');
    if (handle.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId);
    options.onEnd(latest);
  };

  handle.addEventListener('pointermove', move);
  handle.addEventListener('pointerup', finish);
  handle.addEventListener('pointercancel', finish);
}
