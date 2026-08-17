export interface WebglRecoveryState {
  terminalVisible: boolean;
  documentVisible: boolean;
  hasRenderer: boolean;
  recovering: boolean;
}

/** A healthy context earns a fresh recovery budget after surviving normal GPU churn. */
export const WEBGL_STABLE_RESET_MS = 30_000;

/** WebGL recovery waits for a visible canvas and respects the view's unsupported-renderer latch. */
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
