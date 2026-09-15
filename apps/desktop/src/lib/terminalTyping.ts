/** xterm can emit IME/dead-key text in a later task than its keydown event. */
export class TerminalCompositionInput {
  private deferred = false;
  private generation = 0;
  private disposed = false;

  private activity: () => void;
  private schedule: (callback: () => void) => unknown;

  constructor(activity: () => void, schedule: (callback: () => void) => unknown = (callback) => setTimeout(callback, 0)) {
    this.activity = activity;
    this.schedule = schedule;
  }

  keyDown(event: Pick<KeyboardEvent, 'keyCode' | 'key' | 'isComposing'>): void {
    if (event.keyCode === 229 || event.key === 'Dead' || event.isComposing) this.textInput();
  }

  start(): void { this.textInput(); }
  update(): void { this.textInput(); }
  end(): void { this.textInput(); }

  textInput(): void {
    if (this.disposed) return;
    this.deferred = true;
    const generation = ++this.generation;
    this.activity();
    // xterm schedules its commit after our capture/keydown handler. Keep provenance through
    // that task, without leaving a token for an unrelated terminal-protocol reply later.
    this.schedule(() => this.schedule(() => {
      if (generation === this.generation) this.deferred = false;
    }));
  }

  isUserInput(): boolean { return !this.disposed && this.deferred; }
  dispose(): void { this.disposed = true; this.generation++; }
}

export interface TerminalInputPacket {
  processId: number;
  data: Uint8Array;
  userInitiated: boolean;
}

/** Preserve provenance across a short input batch and reset it between batches. */
export class TerminalInputBatch {
  processId: number | null = null;
  private bytes: number[] = [];
  private userInitiated = false;

  push(processId: number, bytes: Uint8Array, userInitiated: boolean): void {
    if (this.processId !== null && this.processId !== processId) throw new Error('Flush input before changing terminals');
    this.processId = processId;
    for (const byte of bytes) this.bytes.push(byte);
    this.userInitiated ||= userInitiated;
  }

  drain(): TerminalInputPacket | null {
    const packet = this.processId !== null && (this.bytes.length > 0 || this.userInitiated)
      ? { processId: this.processId, data: Uint8Array.from(this.bytes), userInitiated: this.userInitiated }
      : null;
    this.processId = null;
    this.bytes = [];
    this.userInitiated = false;
    return packet;
  }
}
