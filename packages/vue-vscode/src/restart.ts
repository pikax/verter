/**
 * Extracted language server restart logic for testability.
 *
 * The core restart flow is isolated from VS Code dependencies so it can
 * be unit-tested with mock implementations.
 */

export interface RestartDeps {
  /** Stop the current language server. May throw (e.g., timeout). */
  stop: () => Promise<void>;
  /** Create a new language server client and start it. */
  createAndStart: () => Promise<void>;
  /** Kill the tracked type provider child process (orphan cleanup on stop failure). */
  killTrackedTypeProvider: () => void;
  /** Reset dependent services (CSS service, diagnostics). */
  resetServices: () => void;
  log: {
    info(msg: string): void;
    warn(msg: string, ...args: unknown[]): void;
    error(msg: string, ...args: unknown[]): void;
  };
}

/**
 * Restart the language server with graceful stop-failure recovery.
 *
 * If `stop()` fails (e.g., shutdown timeout), the TSGO child process is
 * killed explicitly and a new server is still created. This prevents the
 * client from getting stuck pointing to a dead server instance.
 *
 * @returns `true` if the restart succeeded, `false` if it failed.
 */
export async function restartLanguageServer(deps: RestartDeps): Promise<boolean> {
  try {
    deps.log.info("Restarting language server...");
    try {
      await deps.stop();
    } catch (e) {
      deps.log.warn("Failed to stop language server cleanly, forcing restart", e);
      deps.killTrackedTypeProvider();
    }
    await deps.createAndStart();
    deps.resetServices();
    return true;
  } catch (e) {
    deps.log.error("Failed to restart language server", e);
    return false;
  }
}
