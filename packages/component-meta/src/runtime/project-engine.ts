/**
 * ProjectEngine — wraps one native MetaProject and tracks leases/activity.
 *
 * The engine is the shared, heavy, long-lived unit in the pool.
 * It is never exposed to public callers directly.
 */

import type { CheckerWorkspace } from "../compat/checker.js";

export type LeaseId = string;
export type EngineState = "bootstrapping" | "active" | "evicting" | "closed";

let nextLeaseCounter = 1;

export function generateLeaseId(): LeaseId {
  return `lease-${nextLeaseCounter++}`;
}

/**
 * The native project interface (matches MetaProject from @verter/native).
 */
export interface NativeMetaProject {
  upsertBase(canonicalId: string, source: string | Buffer): void;
  configureProjects(projects: unknown[]): void;
  openSession(): NativeMetaSession;
  clearCaches(): void;
  shutdown(): void;
  readonly isShutdown: boolean;
  readonly sessionCount: number;
  baseFileIds(): string[];
}

export interface NativeMetaSession {
  upsert(canonicalId: string, source: string | Buffer): void;
  delete(canonicalId: string): void;
  getAnalysis(canonicalOrAlias: string): string | null;
  resolveImportedTypes(canonicalOrAlias: string): string | null;
  getEffectiveSource(canonicalId: string): string | null;
  hasFile(canonicalId: string): boolean;
  trackedFileIds(): string[];
  close(): void;
  readonly isClosed: boolean;
  readonly overlayGeneration: number;
  /** Evaluate type annotations using the native lightweight evaluator. Optional. */
  evaluateTypes?(canonicalOrAlias: string): string | null;
}

export class ProjectEngine {
  readonly key: string;
  readonly root: string;
  readonly workspace: CheckerWorkspace | undefined;
  readonly incarnation: number;

  private _nativeProject: NativeMetaProject;
  private _liveLeases = new Set<LeaseId>();
  private _lastActivityMs = Date.now();
  private _state: EngineState;

  constructor(
    key: string,
    root: string,
    nativeProject: NativeMetaProject,
    workspace?: CheckerWorkspace,
    incarnation = 1,
  ) {
    this.key = key;
    this.root = root;
    this._nativeProject = nativeProject;
    this.workspace = workspace;
    this.incarnation = incarnation;
    this._state = "active";
  }

  get state(): EngineState {
    return this._state;
  }

  get nativeProject(): NativeMetaProject {
    return this._nativeProject;
  }

  get leaseCount(): number {
    return this._liveLeases.size;
  }

  get lastActivityMs(): number {
    return this._lastActivityMs;
  }

  markActivity(): void {
    this._lastActivityMs = Date.now();
  }

  acquireLease(): LeaseId {
    if (this._state !== "active") {
      throw new Error(`Cannot acquire lease: engine is ${this._state}`);
    }
    const id = generateLeaseId();
    this._liveLeases.add(id);
    this.markActivity();
    return id;
  }

  releaseLease(leaseId: LeaseId): void {
    this._liveLeases.delete(leaseId);
    this.markActivity();
  }

  hasLease(leaseId: LeaseId): boolean {
    return this._liveLeases.has(leaseId);
  }

  isEvictable(idleTtlMs: number): boolean {
    if (this._state !== "active") return false;
    if (this._liveLeases.size > 0) return false;
    return Date.now() - this._lastActivityMs >= idleTtlMs;
  }

  clearCaches(): void {
    if (this._state === "closed") return;
    this._nativeProject.clearCaches();
  }

  /**
   * Begin eviction. Sets state to "evicting", then "closed".
   * Returns false if the engine has live leases (unless forced).
   */
  shutdownNow(force = false): boolean {
    if (this._state === "closed") return true;
    if (!force && this._liveLeases.size > 0) return false;

    this._state = "evicting";
    try {
      this._nativeProject.shutdown();
    } catch {
      // Best-effort — mark closed anyway
    }
    this._state = "closed";
    this._liveLeases.clear();
    return true;
  }
}
