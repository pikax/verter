/**
 * ProjectSession — a lightweight caller-scoped handle with isolated overlays.
 *
 * Each session wraps one native MetaSession and holds one engine lease.
 * Overlay mutations bump overlayGeneration and invalidate the session memo.
 */

import type { ProjectEngine, LeaseId, NativeMetaSession } from "./project-engine.js";
import { decodeComponentMetaPayload } from "../type-graph.js";

type OverlayEntry = { kind: "upsert"; source: string } | { kind: "delete" };

/**
 * Deep-freeze an object graph, handling cycles safely.
 */
function deepFreeze<T>(obj: T): T {
  if (obj === null || obj === undefined || typeof obj !== "object") return obj;
  const seen = new WeakSet<object>();
  const stack: object[] = [obj as object];
  while (stack.length > 0) {
    const current = stack.pop()!;
    if (seen.has(current)) continue;
    seen.add(current);
    Object.freeze(current);
    const values = Object.values(current);
    for (let i = 0; i < values.length; i++) {
      const v = values[i];
      if (v !== null && v !== undefined && typeof v === "object" && !Object.isFrozen(v)) {
        stack.push(v);
      }
    }
  }
  return obj;
}

interface MemoEntry {
  overlayGen: number;
  baseGen: number;
  value: unknown;
}

export class ProjectSession {
  readonly engine: ProjectEngine;
  readonly leaseId: LeaseId;

  private _nativeSession: NativeMetaSession;
  private _overlays = new Map<string, OverlayEntry>();
  private _overlayGeneration = 0;
  private _localMemo = new Map<string, { gen: number; value: unknown }>();
  /** Decoded native-meta memo: keyed by `"kind:canonicalId"`. */
  private _decodedMemo = new Map<string, MemoEntry>();
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
    this._decodedMemo.clear();
    this._nativeSession.upsert(canonicalId, source);
    this.engine.markActivity();
  }

  delete(canonicalId: string): void {
    this.ensureOpen();
    this._overlays.set(canonicalId, { kind: "delete" });
    this._overlayGeneration++;
    this._localMemo.clear();
    this._decodedMemo.clear();
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
    this._decodedMemo.clear();
    this._nativeSession.reset(canonicalId);
    if (this._nativeSession.getEffectiveSource(canonicalId) === null) {
      const loaded = this.engine.nativeProject.ensureLoaded(canonicalId);
      if (loaded) {
        this.engine.baseGeneration++;
      }
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
    const loaded = this.engine.nativeProject.ensureLoaded(canonicalId);
    if (loaded) {
      this.engine.baseGeneration++;
    }
    return loaded;
  }

  /**
   * Refresh a shared base file from the native workspace.
   */
  refreshBaseFile(canonicalId: string): boolean {
    this.ensureOpen();
    this.engine.markActivity();
    const refreshed = this.engine.nativeProject.refreshBase(canonicalId);
    this.engine.baseGeneration++;
    return refreshed;
  }

  // ─────────────────────────────────────────────────────────────────────
  // Component-meta queries with decoded-result memo
  // ─────────────────────────────────────────────────────────────────────

  /**
   * Single native component-meta query. Returns decoded protobuf metadata or null.
   * Memoized: repeated calls with unchanged overlay + base state skip decode.
   */
  getComponentMeta(canonicalId: string): unknown | null {
    return this._memoizedDecode("full", canonicalId, () => {
      const payload = this._nativeSession.getComponentMeta(canonicalId);
      if (payload === null || payload === undefined) return null;
      return decodeComponentMetaPayload(payload);
    });
  }

  /**
   * Batch native component-meta query. All `canonicalIds` resolve under
   * a single shared overlay view and a single scheduler dispatch on
   * the native side; host-owned admission caches
   * (`MaterializeStructureDb`, `ComponentMetaResultDb`,
   * `SemanticGraphStore`) are shared across the batch.
   *
   * Returns one slot per input in input order — decoded metadata for
   * successful slots, `null` for missing canonicals or per-id failures.
   *
   * Per-id decode results are NOT memoized through `_memoizedDecode`
   * because the batch is the canonical entry point for callers that
   * want one scheduler context; subsequent per-id `getComponentMeta`
   * calls will hit the per-id memo on warm state.
   */
  getComponentMetaBatch(canonicalIds: string[]): Array<unknown | null> {
    this.ensureOpen();
    const payloads = this._nativeSession.getComponentMetaBatch(canonicalIds);
    return payloads.map((payload) => {
      // Sentinel: empty buffer means "no result" (missing canonical or
      // per-id failure).
      if (payload === null || payload === undefined || payload.length === 0) {
        return null;
      }
      return decodeComponentMetaPayload(payload);
    });
  }

  /**
   * Full native component-meta query with resolution sidecars.
   */
  getResolvedComponentMeta(canonicalId: string): unknown | null {
    return this._memoizedDecode("resolved", canonicalId, () => {
      const nativeSession = this._nativeSession as {
        getResolvedComponentMeta?: (canonicalId: string) => unknown | null;
      };
      const getResolvedComponentMeta = nativeSession.getResolvedComponentMeta;
      if (typeof getResolvedComponentMeta !== "function") {
        throw new Error(
          "Resolved component-meta query is unavailable on the active native session",
        );
      }
      const payload = getResolvedComponentMeta.call(this._nativeSession, canonicalId);
      if (payload === null || payload === undefined) return null;
      return decodeComponentMetaPayload(payload as ArrayBuffer | ArrayBufferView);
    });
  }

  /**
   * Selective surface. Returns the
   * `verter.v1.ComponentMetaSurface` proto bytes (eager scalars +
   * `NamedTypeHandle` for every type-bearing field). Returns `null`
   * when the canonical does not resolve. Returns `null` AND logs a
   * diagnostic when the bridge surfaced an error envelope (D114
   * magic-byte prefix `0xFF`); callers that want the typed envelope
   * use the lower-level NAPI surface directly.
   */
  getComponentMetaSurface(canonicalId: string): Buffer | null {
    this.ensureOpen();
    const nativeSession = this._nativeSession as {
      getComponentMetaSurface?: (canonicalId: string) => Buffer | null;
    };
    const fn = nativeSession.getComponentMetaSurface;
    if (typeof fn !== "function") {
      throw new Error(
        "Selective component-meta surface API is unavailable on the active native session",
      );
    }
    const buf = fn.call(this._nativeSession, canonicalId);
    if (buf === null || buf === undefined) return null;
    if (buf.length > 0 && buf[0] === 0xff) return null;
    return buf;
  }

  /**
   * Selective surface. Resolves a
   * `verter.v1.TypeHandle` (caller pre-encodes via the proto module)
   * to a one-layer `verter.v1.TypeExpansion`. Returns the raw bytes;
   * D114 magic-byte error envelopes (`buf[0] === 0xFF`) are surfaced
   * unchanged for the caller to decode as `verter.v1.TypeHandleError`.
   */
  getComponentMetaTypeExpansion(handleBuf: Buffer, depth?: number): Buffer {
    this.ensureOpen();
    const nativeSession = this._nativeSession as {
      getComponentMetaTypeExpansion?: (handleBuf: Buffer, depth?: number) => Buffer;
    };
    const fn = nativeSession.getComponentMetaTypeExpansion;
    if (typeof fn !== "function") {
      throw new Error(
        "Selective component-meta type expansion is unavailable on the active native session",
      );
    }
    return fn.call(this._nativeSession, handleBuf, depth);
  }

  /**
   * Internal memo helper. Returns frozen decoded result on hit,
   * or decodes, freezes, and caches on miss.
   */
  private _memoizedDecode(
    kind: string,
    canonicalId: string,
    decode: () => unknown | null,
  ): unknown | null {
    this.ensureOpen();
    this.engine.markActivity();

    const key = `${kind}:${canonicalId}`;
    const entry = this._decodedMemo.get(key);
    if (
      entry &&
      entry.overlayGen === this._overlayGeneration &&
      entry.baseGen === this.engine.baseGeneration
    ) {
      return entry.value;
    }

    const result = decode();
    // Deep-freeze so callers cannot mutate the shared memoized object
    if (result !== null) {
      deepFreeze(result);
    }
    this._decodedMemo.set(key, {
      overlayGen: this._overlayGeneration,
      baseGen: this.engine.baseGeneration,
      value: result,
    });
    return result;
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
    this._decodedMemo.clear();
  }
}
