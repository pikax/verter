import type { BinaryCandidate, Launcher } from "./index.js";

/** The flag every launcher answers with its resolved native binary path. */
export declare const PRINT_PATH_FLAG: string;

/**
 * Env var recording which launcher tool names are already active in this
 * process tree — see {@link isLauncherActive}.
 */
export declare const ACTIVE_ENV_VAR: string;

/** Restore the exec bit on a resolved binary. No-op on Windows; best-effort. */
export declare function ensureExecutable(binary: string): void;

/** Whether a launcher for `toolName` is already active in this process tree. */
export declare function isLauncherActive(toolName: string): boolean;

/** `process.env` for a child, with `toolName` added to the active list. */
export declare function envWithToolMarked(toolName: string): NodeJS.ProcessEnv;

export interface AssertNotSelfSpawnOptions {
  readonly resolved: BinaryCandidate;
  readonly launcher: Launcher;
  /** The calling `bin/run.js`'s own path (pass `__filename`), when known. */
  readonly selfPath?: string;
}

/**
 * Throws when `resolved` is this launcher's own script (or a `PATH` hit that
 * is a node script rather than a native binary) instead of the real native
 * binary. Any caller that resolves a binary directly — not through
 * {@link runLauncherCli} — must call this before spawning it.
 */
export declare function assertNotSelfSpawn(options: AssertNotSelfSpawnOptions): void;

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
