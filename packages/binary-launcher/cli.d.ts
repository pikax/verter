import type { Launcher } from "./index.js";

/** The flag every launcher answers with its resolved native binary path. */
export declare const PRINT_PATH_FLAG: string;

/** Restore the exec bit on a resolved binary. No-op on Windows; best-effort. */
export declare function ensureExecutable(binary: string): void;

export interface RunLauncherCliOptions {
  readonly launcher: Launcher;
  /** Arguments after the node executable and script, i.e. `argv.slice(2)`. */
  readonly argv: readonly string[];
  /**
   * The calling `bin/run.js`'s own path (pass `__filename`). Used to refuse
   * spawning a resolved candidate that turns out to be this launcher's own
   * script — see the self-spawn guard in `runLauncherCli`.
   */
  readonly selfPath?: string;
  readonly stderr?: NodeJS.WritableStream;
  readonly stdout?: NodeJS.WritableStream;
}

/**
 * Resolve the native binary and hand it the process stdio. Returns the exit
 * code the caller should exit with.
 */
export declare function runLauncherCli(options: RunLauncherCliOptions): number;
