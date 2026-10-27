/**
 * The desktop language-server lifecycle: which server start is current, and
 * which state the user is shown for it.
 *
 * Every client the extension starts — the initial one and each restart — opens
 * a fresh activation epoch. Callbacks from a client carry the epoch it was
 * started under, so an exit, connection error or status report from a client
 * that has since been replaced is recognised as stale: it never drives recovery
 * and never reaches the status bar as the current state.
 *
 * The lifecycle records state only. Recovery itself (backoff, the restart
 * budget, the terminal give-up) belongs to the restart supervisor; this object
 * makes sure each lost server enters that supervisor exactly once.
 */
import type { Disposable } from "./startAttemptScope";

/**
 * - `starting` — a client for the current epoch is starting.
 * - `running` — the current epoch's client completed its start.
 * - `recovering` — the current server was lost and an automatic restart is due.
 * - `stopped` — recovery gave up, or a manual restart failed; only a manual
 *   restart starts another server.
 * - `disposed` — the owning start attempt is gone.
 */
export type LifecyclePhase = "starting" | "running" | "recovering" | "stopped" | "disposed";

export interface LifecycleSnapshot {
  readonly phase: LifecyclePhase;
  readonly epoch: number;
  /** The automatic restart in progress; 0 outside recovery. */
  readonly restartAttempt: number;
  readonly maxRestarts: number;
  /** Why the server was lost or stopped. Absent while starting cleanly or running. */
  readonly reason?: string;
}

export class DesktopLanguageLifecycle implements Disposable {
  private phase: LifecyclePhase = "starting";
  private epoch = 0;
  private lostEpoch = 0;
  private restartAttempt = 0;
  private reason: string | undefined;
  private readonly listeners = new Set<() => void>();

  constructor(private readonly maxRestarts: number) {}

  /** The epoch of the client started most recently. */
  get currentEpoch(): number {
    return this.epoch;
  }

  isCurrent(epoch: number): boolean {
    return this.phase !== "disposed" && epoch === this.epoch;
  }

  /** Open the epoch a new client starts under. Earlier epochs become stale. */
  beginEpoch(): number {
    if (this.phase === "disposed") {
      return this.epoch;
    }
    this.epoch += 1;
    this.phase = "starting";
    if (this.restartAttempt === 0) {
      this.reason = undefined;
    }
    this.emit();
    return this.epoch;
  }

  /**
   * The epoch's client completed its start. False for a stale epoch, and for an
   * epoch whose loss was already reported: that server is not running, and
   * recovery for it has been (or is being) handed to the supervisor.
   */
  markRunning(epoch: number): boolean {
    if (!this.isCurrent(epoch) || this.lostEpoch === epoch) {
      return false;
    }
    this.phase = "running";
    this.restartAttempt = 0;
    this.reason = undefined;
    this.emit();
    return true;
  }

  /**
   * The epoch's server is gone: its process exited, its connection kept
   * erroring, its start failed or it stopped answering.
   *
   * True exactly once per current epoch — the caller then owns starting
   * recovery. Every later report for the same epoch, and any report for a stale
   * epoch, is false, so one lost server never enters recovery twice.
   */
  reportLost(epoch: number, reason: string): boolean {
    if (!this.isCurrent(epoch) || this.lostEpoch === epoch || this.phase === "stopped") {
      return false;
    }
    this.lostEpoch = epoch;
    this.phase = "recovering";
    this.reason = reason;
    this.emit();
    return true;
  }

  /** The supervisor scheduled automatic restart `attempt` of `maxRestarts`. */
  markRecovering(attempt: number, reason: string): void {
    if (this.phase === "disposed") {
      return;
    }
    this.phase = "recovering";
    this.restartAttempt = attempt;
    this.reason = reason;
    this.emit();
  }

  /** No server is running and none will start without a manual restart. */
  markStopped(reason: string): void {
    if (this.phase === "disposed") {
      return;
    }
    this.phase = "stopped";
    this.restartAttempt = 0;
    this.reason = reason;
    this.emit();
  }

  snapshot(): LifecycleSnapshot {
    return {
      phase: this.phase,
      epoch: this.epoch,
      restartAttempt: this.restartAttempt,
      maxRestarts: this.maxRestarts,
      ...(this.reason === undefined ? {} : { reason: this.reason }),
    };
  }

  onDidChange(listener: () => void): Disposable {
    if (this.phase === "disposed") {
      return { dispose() {} };
    }
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  dispose(): void {
    this.phase = "disposed";
    this.listeners.clear();
  }

  private emit(): void {
    for (const listener of [...this.listeners]) {
      listener();
    }
  }
}

/**
 * What a live client does with a connection error: tolerate the first three,
 * then shut the connection down and treat the server as lost. The threshold
 * mirrors `DefaultErrorHandler.error` in vscode-languageclient (11.0.0,
 * lib/common/client.js: `count && count <= 3` continues, otherwise shuts down).
 */
export function connectionErrorDisposition(count: number | undefined): "continue" | "shutdown" {
  return count !== undefined && count <= 3 ? "continue" : "shutdown";
}
