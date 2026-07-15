/**
 * Shared init-generation matching core for the Verter startup-readiness gates.
 *
 * The Verter LSP server announces background-init completion through TWO
 * server→client notifications, each carrying the init generation it belongs to:
 *
 *  - `$/verter/ready` — `VerterReadyParams { gen }`
 *    (crates/verter_lsp/src/protocol_types.rs:54-64; sent on the main
 *    background-init path at crates/verter_lsp/src/background_init.rs:551-553).
 *  - `$/verter/typeProviderSyncComplete` — `TypeProviderSyncCompleteParams { gen }`
 *    (protocol_types.rs:69-79; sent from a task spawned off the workspace-scanner
 *    oneshot — background_init.rs:343/381/466-470).
 *
 * `ready` alone is NON-semantic. It is published on the main init path, while
 * `typeProviderSyncComplete` is published from a *separately spawned* task gated
 * on the scanner's `done` channel — so the two RACE and either may arrive first.
 * Cross-file type resolution (barrel re-exports, imported types) is only reliable
 * once BOTH have arrived for the SAME generation; a probe fired on `ready` alone
 * can read a half-synced project.
 *
 * Re-initialization (config change, restart) bumps the generation, and the server
 * discards superseded generations (background_init.rs:323-326/386-391/477-478).
 * The gate mirrors that: it tracks the highest generation seen on each channel and
 * is satisfied only when BOTH channels have reached the SAME — necessarily
 * newest — generation. A later `ready(N+1)` after a matched pair at `N` re-arms
 * the gate until `sync(N+1)` arrives.
 *
 * This module is pure: importing it does no I/O and mutates no globals.
 */

/** Which readiness notification a {@link GenerationEvent} came from. */
export type GenerationChannel = "ready" | "sync";

/** A single observed readiness signal carrying its init generation. */
export interface GenerationEvent {
  readonly channel: GenerationChannel;
  readonly generation: number;
}

/** The gate's verdict given everything observed so far. */
export interface GenerationGateDecision {
  /** Both channels have reached the same (newest) generation. */
  readonly satisfied: boolean;
  /** The matched generation when {@link satisfied}, else `null`. */
  readonly matchedGeneration: number | null;
  /** Highest generation seen on either channel (`null` if nothing seen). */
  readonly newestGeneration: number | null;
  /** Highest generation seen on the `ready` channel. */
  readonly maxReadyGeneration: number | null;
  /** Highest generation seen on the `sync` channel. */
  readonly maxSyncGeneration: number | null;
}

/** A real init generation is a non-negative integer. */
function isValidGeneration(generation: number): boolean {
  return Number.isInteger(generation) && generation >= 0;
}

function higher(current: number | null, candidate: number): number {
  return current === null ? candidate : Math.max(current, candidate);
}

/**
 * Incrementally tracks readiness signals and reports whether the startup gate is
 * satisfied. Tracking the *maximum* generation per channel (not the last) makes
 * the gate robust to out-of-order, superseded, late-arriving notifications.
 */
export class GenerationGate {
  private maxReady: number | null = null;
  private maxSync: number | null = null;

  observeReady(generation: number): void {
    if (!isValidGeneration(generation)) return;
    this.maxReady = higher(this.maxReady, generation);
  }

  observeSync(generation: number): void {
    if (!isValidGeneration(generation)) return;
    this.maxSync = higher(this.maxSync, generation);
  }

  observe(event: GenerationEvent): void {
    if (event.channel === "ready") this.observeReady(event.generation);
    else this.observeSync(event.generation);
  }

  get decision(): GenerationGateDecision {
    const { maxReady, maxSync } = this;
    const newestGeneration =
      maxReady === null ? maxSync : maxSync === null ? maxReady : Math.max(maxReady, maxSync);
    // Satisfied iff the newest generation has been seen on BOTH channels. Because
    // each channel only ever advances, that reduces to the two maxima being equal.
    const satisfied = maxReady !== null && maxSync !== null && maxReady === maxSync;
    return {
      satisfied,
      matchedGeneration: satisfied ? maxReady : null,
      newestGeneration,
      maxReadyGeneration: maxReady,
      maxSyncGeneration: maxSync,
    };
  }

  get satisfied(): boolean {
    return this.decision.satisfied;
  }

  get matchedGeneration(): number | null {
    return this.decision.matchedGeneration;
  }
}

/** Fold a finite sequence of events through a fresh {@link GenerationGate}. */
export function evaluateGenerationGate(events: Iterable<GenerationEvent>): GenerationGateDecision {
  const gate = new GenerationGate();
  for (const event of events) gate.observe(event);
  return gate.decision;
}
