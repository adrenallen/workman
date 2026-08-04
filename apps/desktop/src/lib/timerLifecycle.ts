import { readable, writable, type Readable } from 'svelte/store';

export type TimerKind = 'delay' | 'idle_any' | 'idle_all';
export type TimerLifecycleKind =
  | 'created'
  | 'fired'
  | 'delivered'
  | 'cancelled'
  | 'paused'
  | 'resumed';

export interface TimerView {
  id: number;
  owner_actor: string;
  delivery_process_id: number;
  body: string;
  kind: TimerKind;
  watch_process_ids: number[];
  interval_ms: number | null;
  repeating: boolean;
  max_wait_deadline: number | null;
  paused: boolean;
  fired: boolean;
  fired_at: number | null;
  created_at: number;
  due_at: number;
  paused_at: number | null;
}

export interface TimerLifecycleEvent {
  sequence: number;
  kind: TimerLifecycleKind;
  timer_id: number | null;
  project_id: number;
  delivery_process_id: number;
  at: number;
  reason?: 'delay' | 'idle_transition' | 'max_wait' | 'already_satisfied';
  timer?: TimerView;
}

const mutableLiveTimers = writable<Record<number, TimerView>>({});

/** Active and paused timers reconciled from the existing process status stream. */
export const liveTimers: Readable<Record<number, TimerView>> = readable({}, (set) =>
  mutableLiveTimers.subscribe(set)
);

/** Called only by the daemon event adapter. */
export function updateTimerLifecycle(
  snapshot: TimerView[] | undefined,
  events: TimerLifecycleEvent[]
): void {
  mutableLiveTimers.update((current) => {
    const next: Record<number, TimerView> = snapshot
      ? Object.fromEntries(snapshot.filter((timer) => !timer.fired).map((timer) => [timer.id, timer]))
      : { ...current };

    for (const event of events) {
      if (event.timer_id === null) continue;
      if (event.kind === 'cancelled') {
        delete next[event.timer_id];
        continue;
      }
      if (event.kind === 'fired' || event.kind === 'delivered') {
        if (event.timer?.repeating && !event.timer.fired) next[event.timer_id] = event.timer;
        else delete next[event.timer_id];
        continue;
      }
      if (event.timer && !event.timer.fired) next[event.timer_id] = event.timer;
    }

    return next;
  });
}

export function resetTimerLifecycle(): void {
  mutableLiveTimers.set({});
}
