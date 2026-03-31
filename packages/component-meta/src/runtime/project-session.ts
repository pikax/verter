/**
 * ProjectSession — a lightweight caller-scoped handle with isolated overlays.
 *
 * Each session wraps one native MetaSession and holds one engine lease.
 * Overlay mutations bump overlayGeneration and invalidate the session memo.
 */

import type { ProjectEngine, LeaseId, NativeMetaSession } from "./project-engine.js";
import { decodeComponentMetaPayload } from "../type-graph.js";

type OverlayEntry = { kind: "upsert"; source: string } | { kind: "delete" };

export class ProjectSession {
  readonly engine: ProjectEngine;
  readonly leaseId: LeaseId;

  private _nativeSession: NativeMetaSession;
  private _overlays = new Map<string, OverlayEntry>();
  private _overlayGeneration = 0;
  private _localMemo = new Map<string, { gen: number; value: unknown }>();
  private _closed = false;

  constructor(engine: ProjectEngine, leaseId: LeaseId, nativeSession: NativeMetaSession) {
    this.engine = engine;
    this.leaseId = leaseId;
    this._nativeSession = nativeSession;
  }

  get closed(): boolean {
    return this._closed;
  }

  get overlayGeneration(): number {
    return this._overlayGeneration;
  }

  private ensureOpen(): void {
    if (this._closed) {
      throw new Error("Session is closed");
    }
    if (this.engine.state === "closed" || this.engine.state === "evicting") {
      throw new Error("Engine has been shut down");
    }
  }

  upsert(canonicalId: string, source: string): void {
    this.ensureOpen();
    this._overlays.set(canonicalId, { kind: "upsert", source });
    this._overlayGeneration++;
    this._localMemo.clear();
    this._nativeSession.upsert(canonicalId, source);
    this.engine.markActivity();
  }

  delete(canonicalId: string): void {
    this.ensureOpen();
    this._overlays.set(canonicalId, { kind: "delete" });
    this._overlayGeneration++;
    this._localMemo.clear();
    this._nativeSession.delete(canonicalId);
    this.engine.markActivity();
  }

  restoreBaseFile(canonicalId: string): void {
    this.ensureOpen();
    const hadOverlay = this._overlays.delete(canonicalId);
    if (hadOverlay) {
      this._overlayGeneration++;
    }
    this._localMemo.clear();
    this._nativeSession.reset(canonicalId);
    if (this._nativeSession.getEffectiveSource(canonicalId) === null) {
      this.engine.nativeProject.ensureLoaded(canonicalId);
    }
    this.engine.markActivity();
  }

  getEffectiveSource(canonicalId: string): string | undefined {
    this.ensureOpen();
    // Check session overlay first (in-memory)
    const overlay = this._overlays.get(canonicalId);
    if (overlay) {
      return overlay.kind === "upsert" ? overlay.source : undefined;
    }
    // Fall back to native session (which checks base)
    return this._nativeSession.getEffectiveSource(canonicalId) ?? undefined;
  }

  hasFile(canonicalId: string): boolean {
    this.ensureOpen();
    const overlay = this._overlays.get(canonicalId);
    if (overlay) {
      return overlay.kind === "upsert";
    }
    return this._nativeSession.hasFile(canonicalId);
  }

  trackedFileIds(): string[] {
    this.ensureOpen();
    return this._nativeSession.trackedFileIds();
  }

  /**
   * Ensure a disk-backed file is loaded into the shared native base project.
   */
  ensureBaseFile(canonicalId: string): boolean {
    this.ensureOpen();
    this.engine.markActivity();
    return this.engine.nativeProject.ensureLoaded(canonicalId);
  }

  /**
   * Refresh a shared base file from the native workspace.
   */
  refreshBaseFile(canonicalId: string): boolean {
    this.ensureOpen();
    this.engine.markActivity();
    return this.engine.nativeProject.refreshBase(canonicalId);
  }

  /**
   * Single native component-meta query. Returns decoded protobuf metadata or null.
   * Uses the native `getComponentMeta` method which combines enriched analysis
   * + type evaluation in one call.
   */
  getComponentMeta(canonicalId: string): unknown | null {
    this.ensureOpen();
    this.engine.markActivity();
    const payload = this._nativeSession.getComponentMeta(canonicalId);
    if (payload === null || payload === undefined) return null;
    return decodeComponentMetaPayload(payload);
  }

  /**
   * Declared-surface native component-meta query for Volar-compatible callers.
   */
  getDeclaredComponentMeta(canonicalId: string): unknown | null {
    this.ensureOpen();
    this.engine.markActivity();
    const payload = this._nativeSession.getDeclaredComponentMeta(canonicalId);
    if (payload === null || payload === undefined) return null;
    return decodeComponentMetaPayload(payload);
  }

  /**
   * Full native component-meta query with resolution sidecars.
   *
   * Falls back to `getComponentMeta()` when running against an older native
   * session that does not expose the dedicated resolved query yet.
   */
  getResolvedComponentMeta(canonicalId: string): unknown | null {
    this.ensureOpen();
    this.engine.markActivity();
    const payload = this._nativeSession.getResolvedComponentMeta
      ? this._nativeSession.getResolvedComponentMeta(canonicalId)
      : this._nativeSession.getComponentMeta(canonicalId);
    if (payload === null || payload === undefined) return null;
    return decodeComponentMetaPayload(payload);
  }

  /**
   * Provenance counters for observability. Returns parsed JSON or null.
   */
  getProvenance(): Record<string, number> {
    this.ensureOpen();
    const json = this._nativeSession.getProvenance();
    return JSON.parse(json);
  }

  /**
   * Session-local memoization. Invalidated when overlayGeneration changes.
   */
  getMemo<T>(key: string): T | undefined {
    const entry = this._localMemo.get(key);
    if (entry && entry.gen === this._overlayGeneration) {
      return entry.value as T;
    }
    return undefined;
  }

  setMemo(key: string, value: unknown): void {
    this._localMemo.set(key, { gen: this._overlayGeneration, value });
  }

  /**
   * Close the session, releasing the lease. Idempotent.
   */
  close(): void {
    if (this._closed) return;
    this._closed = true;
    try {
      this._nativeSession.close();
    } catch {
      // Best-effort
    }
    this.engine.releaseLease(this.leaseId);
    this._overlays.clear();
    this._localMemo.clear();
  }
}
