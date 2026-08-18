export const POPOVER_HOVER_INTENT_MS = 200;
export const POPOVER_HOVER_GRACE_MS = 150;

export interface HoverIntentScheduler {
  set(callback: () => void, delayMs: number): unknown;
  clear(handle: unknown): void;
}

const browserScheduler: HoverIntentScheduler = {
  set: (callback, delayMs) => setTimeout(callback, delayMs),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>)
};

/**
 * Keeps a hover-open popover stable while the pointer crosses the trigger/content gap.
 * The controller owns timing only; callers keep ownership of the actual open state.
 */
export class PopoverHoverIntent {
  private openTimer: unknown = null;
  private closeTimer: unknown = null;
  private readonly scheduler: HoverIntentScheduler;

  constructor(scheduler: HoverIntentScheduler = browserScheduler) {
    this.scheduler = scheduler;
  }

  enterTrigger(open: () => void): void {
    this.cancelClose();
    this.cancelOpen();
    this.openTimer = this.scheduler.set(() => {
      this.openTimer = null;
      open();
    }, POPOVER_HOVER_INTENT_MS);
  }

  leaveTrigger(close: () => void): void {
    this.cancelOpen();
    this.scheduleClose(close);
  }

  enterContent(): void {
    this.cancelClose();
  }

  leaveContent(close: () => void): void {
    this.scheduleClose(close);
  }

  cancel(): void {
    this.cancelOpen();
    this.cancelClose();
  }

  private scheduleClose(close: () => void): void {
    this.cancelClose();
    this.closeTimer = this.scheduler.set(() => {
      this.closeTimer = null;
      close();
    }, POPOVER_HOVER_GRACE_MS);
  }

  private cancelOpen(): void {
    if (this.openTimer === null) return;
    this.scheduler.clear(this.openTimer);
    this.openTimer = null;
  }

  private cancelClose(): void {
    if (this.closeTimer === null) return;
    this.scheduler.clear(this.closeTimer);
    this.closeTimer = null;
  }
}
