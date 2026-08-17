export interface WebglRecoveryState {
  terminalVisible: boolean;
  documentVisible: boolean;
  hasRenderer: boolean;
  recovering: boolean;
}

/** WebGL recovery waits for a visible canvas and never loops for an unsupported renderer. */
export function shouldAttemptWebglRecovery(state: WebglRecoveryState): boolean {
  return state.recovering
    && state.terminalVisible
    && state.documentVisible
    && !state.hasRenderer;
}

/** Short bounded retries cover WKWebView restoring a context after a hidden window returns. */
export function webglRecoveryDelay(attempt: number): number | null {
  return [0, 100, 500, 2_000][attempt] ?? null;
}
