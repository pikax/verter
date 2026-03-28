/**
 * MetaRuntime — process-global singleton managing the pooled engine registry.
 *
 * Engines are keyed by normalized config fingerprint. Concurrent opens for
 * the same key share one pending bootstrap promise. Idle engines are evicted
 * by a periodic sweep. Forgotten sessions are recovered via WeakRef sweep
 * and FinalizationRegistry (if available).
 */

import { IDLE_TTL_MS, SWEEP_INTERVAL_MS, POOL_CAP } from "./constants.js";
import { type EngineKeyInput, computeEngineKey } from "./engine-key.js";
import { ProjectEngine, type NativeMetaProject } from "./project-engine.js";
import { ProjectSession } from "./project-session.js";

export interface EngineBootstrapResult {
  nativeProject: NativeMetaProject;
  baseFileIds: string[];
}

export type BootstrapFn = (key: string, input: EngineKeyInput) => Promise<EngineBootstrapResult>;

interface LeaseReleaseToken {
  engineKey: string;
  leaseId: string;
  incarnation: number;
}

export class MetaRuntimeImpl {
  private engines = new Map<string, ProjectEngine>();
  private pendingEngines = new Map<string, Promise<ProjectEngine>>();
  private weakSessions = new Map<string, WeakRef<ProjectSession>>();
  private registry: FinalizationRegistry<LeaseReleaseToken> | null = null;
  private evictionTimer: ReturnType<typeof setInterval> | null = null;
  private hooksRegistered = false;
  private beforeExitHook: (() => void) | null = null;
  private exitHook: (() => void) | null = null;
  private _shuttingDown = false;
  private _incarnationCounter = 0;

  // Diagnostics counters
  readonly diagnostics = {
    enginesCreated: 0,
    enginesReused: 0,
    enginesEvictedIdle: 0,
    enginesForceEvicted: 0,
    forgottenSessionsRecovered: 0,
    finalizerReleases: 0,
    closeCalls: 0,
  };

  constructor() {
    if (typeof FinalizationRegistry !== "undefined") {
      this.registry = new FinalizationRegistry((token: LeaseReleaseToken) => {
        this.diagnostics.finalizerReleases++;
        const engine = this.engines.get(token.engineKey);
        if (engine && engine.incarnation === token.incarnation) {
          engine.releaseLease(token.leaseId);
        }
        this.weakSessions.delete(token.leaseId);
      });
    }
  }

  get shuttingDown(): boolean {
    return this._shuttingDown;
  }

  get engineCount(): number {
    return this.engines.size;
  }

  get sessionCount(): number {
    return this.weakSessions.size;
  }

  private ensureEvictionTimer(): void {
    if (this.evictionTimer) return;
    this.evictionTimer = setInterval(() => this.sweepAndEvict(), SWEEP_INTERVAL_MS);
    // Unref so the timer doesn't keep the process alive
    if (typeof this.evictionTimer === "object" && "unref" in this.evictionTimer) {
      this.evictionTimer.unref();
    }
  }

  private ensureProcessHooks(): void {
    if (this.hooksRegistered) return;
    if (typeof process === "undefined") return;
    this.hooksRegistered = true;
    this.beforeExitHook = () => this.shutdownNow();
    this.exitHook = () => this.shutdownNow();
    process.once("beforeExit", this.beforeExitHook);
    process.once("exit", this.exitHook);
  }

  private removeProcessHooks(): void {
    if (!this.hooksRegistered) return;
    if (typeof process !== "undefined") {
      if (this.beforeExitHook) {
        process.removeListener("beforeExit", this.beforeExitHook);
      }
      if (this.exitHook) {
        process.removeListener("exit", this.exitHook);
      }
    }
    this.beforeExitHook = null;
    this.exitHook = null;
    this.hooksRegistered = false;
  }

  /**
   * Get or create an engine for the given key.
   * Concurrent calls for the same key share one pending promise.
   */
  async getOrCreateEngine(input: EngineKeyInput, bootstrap: BootstrapFn): Promise<ProjectEngine> {
    if (this._shuttingDown) {
      throw new Error("MetaRuntime is shutting down");
    }

    const key = computeEngineKey(input);

    // Check existing
    const existing = this.engines.get(key);
    if (existing && existing.state === "active") {
      this.diagnostics.enginesReused++;
      existing.markActivity();
      return existing;
    }

    // Check pending
    const pending = this.pendingEngines.get(key);
    if (pending) {
      this.diagnostics.enginesReused++;
      return pending;
    }

    // Enforce soft cap — try evicting idle engines first
    if (this.engines.size >= POOL_CAP) {
      this.evictIdleEngines();
      if (this.engines.size >= POOL_CAP) {
        // All engines are active — allow but warn
        if (typeof process !== "undefined" && process.env?.DEBUG) {
          console.warn(
            `[verter:meta-runtime] Engine count (${this.engines.size}) exceeds soft cap (${POOL_CAP})`,
          );
        }
      }
    }

    // Bootstrap new engine
    const promise = this.bootstrapEngine(key, input, bootstrap);
    this.pendingEngines.set(key, promise);

    try {
      const engine = await promise;
      if (this._shuttingDown) {
        // Shutdown started during bootstrap — dispose immediately
        engine.shutdownNow(true);
        throw new Error("MetaRuntime shut down during engine bootstrap");
      }
      this.engines.set(key, engine);
      this.diagnostics.enginesCreated++;
      this.ensureEvictionTimer();
      this.ensureProcessHooks();
      return engine;
    } finally {
      this.pendingEngines.delete(key);
    }
  }

  private async bootstrapEngine(
    key: string,
    input: EngineKeyInput,
    bootstrap: BootstrapFn,
  ): Promise<ProjectEngine> {
    const incarnation = ++this._incarnationCounter;
    const result = await bootstrap(key, input);
    return new ProjectEngine(key, input.root, result.nativeProject, undefined, incarnation);
  }

  /**
   * Open a session on an engine.
   * Acquires a lease and wraps a native session.
   */
  openSession(engine: ProjectEngine): ProjectSession {
    if (this._shuttingDown) {
      throw new Error("MetaRuntime is shutting down");
    }
    if (engine.state !== "active") {
      throw new Error(`Cannot open session: engine is ${engine.state}`);
    }
    const leaseId = engine.acquireLease();
    const nativeSession = engine.nativeProject.openSession();
    const session = new ProjectSession(engine, leaseId, nativeSession);

    // Track for leak recovery
    this.weakSessions.set(leaseId, new WeakRef(session));

    // Register finalizer for accelerated cleanup
    if (this.registry) {
      const token: LeaseReleaseToken = {
        engineKey: engine.key,
        leaseId,
        incarnation: engine.incarnation,
      };
      this.registry.register(session, token, session);
    }

    return session;
  }

  /**
   * Close a session, unregistering its finalizer.
   */
  closeSession(session: ProjectSession): void {
    this.diagnostics.closeCalls++;
    if (this.registry) {
      this.registry.unregister(session);
    }
    this.weakSessions.delete(session.leaseId);
    session.close();
  }

  /**
   * Force-evict a specific engine by key.
   */
  evictEngine(key: string): void {
    const engine = this.engines.get(key);
    if (!engine) return;
    this.diagnostics.enginesForceEvicted++;
    engine.shutdownNow(true);
    this.engines.delete(key);
  }

  /**
   * Evict idle engines that have no live leases and exceeded TTL.
   */
  private evictIdleEngines(): void {
    for (const [key, engine] of this.engines) {
      if (engine.isEvictable(IDLE_TTL_MS)) {
        if (engine.shutdownNow()) {
          this.engines.delete(key);
          this.diagnostics.enginesEvictedIdle++;
        }
      }
    }
  }

  /**
   * Periodic sweep: evict idle engines and recover forgotten sessions.
   */
  private sweepAndEvict(): void {
    this.evictIdleEngines();

    // Sweep weak refs for GC'd sessions
    for (const [leaseId, ref] of this.weakSessions) {
      if (!ref.deref()) {
        // Session was GC'd without close() — release its lease
        this.diagnostics.forgottenSessionsRecovered++;
        this.weakSessions.delete(leaseId);
        // Find and release the lease from its engine
        for (const engine of this.engines.values()) {
          if (engine.hasLease(leaseId)) {
            engine.releaseLease(leaseId);
            break;
          }
        }
      }
    }

    // Stop timer if no engines left
    if (this.engines.size === 0 && this.pendingEngines.size === 0) {
      this.stopTimer();
      this.removeProcessHooks();
    }
  }

  private stopTimer(): void {
    if (this.evictionTimer) {
      clearInterval(this.evictionTimer);
      this.evictionTimer = null;
    }
  }

  /**
   * Synchronous, idempotent shutdown of all engines.
   * Safe to call from process exit hooks.
   */
  shutdownNow(): void {
    if (this._shuttingDown) return;
    this._shuttingDown = true;
    this.stopTimer();
    this.removeProcessHooks();

    // Shutdown all engines
    for (const [key, engine] of this.engines) {
      engine.shutdownNow(true);
      this.engines.delete(key);
    }
    this.weakSessions.clear();
  }

  /**
   * Reset the runtime for reuse after shutdown (e.g., in tests).
   */
  reset(): void {
    this.shutdownNow();
    this._shuttingDown = false;
    this.diagnostics.enginesCreated = 0;
    this.diagnostics.enginesReused = 0;
    this.diagnostics.enginesEvictedIdle = 0;
    this.diagnostics.enginesForceEvicted = 0;
    this.diagnostics.forgottenSessionsRecovered = 0;
    this.diagnostics.finalizerReleases = 0;
    this.diagnostics.closeCalls = 0;
  }
}

// Process-global singleton
let instance: MetaRuntimeImpl | null = null;

export function createMetaRuntime(): MetaRuntimeImpl {
  return new MetaRuntimeImpl();
}

export function getMetaRuntime(): MetaRuntimeImpl {
  if (!instance) {
    instance = createMetaRuntime();
  }
  return instance;
}

/**
 * Synchronous, idempotent shutdown of the global runtime.
 * Call from `process.on('exit')` or test teardown.
 */
export function shutdownMetaRuntime(): void {
  if (instance) {
    instance.shutdownNow();
    instance = null;
  }
}
