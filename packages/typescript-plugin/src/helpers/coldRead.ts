import type { CarrierStoreReader, ReadyFile } from "./carrierStore";

/**
 * The hard cap on a cold-read bounded-block, in milliseconds. A path the plugin
 * KNOWS the project owns (it is in `owned_sources`) but whose companion content
 * is not yet published (`ready_files` miss) bounded-blocks up to this long,
 * re-reading the manifest, before returning a negative. This is the C10
 * sticky-`TS2307` defense: a tsserver host API is synchronous and tsserver
 * caches a NEGATIVE result, so returning "absent" for a known companion before
 * the store has warmed pins a sticky failed resolution. The block is bounded —
 * never an unbounded wait — because the Rust side guarantees a later eviction
 * (a "file changed" notification) once the carrier warms.
 */
export const COLD_READ_BLOCK_CAP_MS = 150;

/**
 * One bounded-block poll iteration's synchronous sleep, in milliseconds. The
 * tsserver event loop is BLOCKED inside the host hook, so the wait MUST be a
 * real synchronous sleep, not `setTimeout`/`fs.watch` (which need the event
 * loop). `Atomics.wait` on a throwaway `SharedArrayBuffer` is the clean
 * synchronous-sleep idiom — it parks the thread without busy-spinning.
 */
const COLD_READ_POLL_INTERVAL_MS = 10;

/**
 * Synchronously sleep for `ms` milliseconds by parking the current thread on an
 * `Atomics.wait` against a throwaway `SharedArrayBuffer`. No busy-spin: the
 * thread is genuinely descheduled for the interval. Used only inside a host
 * hook where the event loop is already blocked, so async sleeping is impossible.
 */
function syncSleep(ms: number): void {
  if (ms <= 0) {
    return;
  }
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

/**
 * A cold-read outcome for a known-but-maybe-not-ready companion.
 *  - `ready`: the companion's `ReadyFile` (its content blob exists).
 *  - `lastGood`: no ready entry, but a previously-served blob is reused.
 *  - `negative`: the bounded-block elapsed with no ready entry and no last-good.
 */
export type ColdReadResult =
  | { kind: "ready"; readyFile: ReadyFile }
  | { kind: "lastGood"; content: string }
  | { kind: "negative" };

/**
 * Resolve a companion `providerPath` the project OWNS but that may not yet be in
 * `ready_files`:
 *  1. If it is already ready, return it immediately (no block).
 *  2. Else, if a last-good blob exists for it, return that (last-good beats
 *     blocking — a previously-good answer is better than a stall).
 *  3. Else bounded-block (re-reading the manifest each `COLD_READ_POLL_INTERVAL_MS`
 *     via a real synchronous sleep) up to `COLD_READ_BLOCK_CAP_MS`; return the
 *     companion the moment it becomes ready, or `negative` on timeout.
 *
 * `cap` / `interval` are injectable for deterministic tests.
 */
export function coldResolveCompanion(
  reader: CarrierStoreReader,
  providerPath: string,
  cap: number = COLD_READ_BLOCK_CAP_MS,
  interval: number = COLD_READ_POLL_INTERVAL_MS,
): ColdReadResult {
  const immediate = reader.readyFile(providerPath);
  if (immediate) {
    return { kind: "ready", readyFile: immediate };
  }

  const lastGood = reader.lastGoodBlobFor(providerPath);
  if (lastGood !== undefined) {
    return { kind: "lastGood", content: lastGood };
  }

  // First-ever cold read, no last-good: bounded-block on the manifest.
  const deadline = Date.now() + cap;
  for (;;) {
    const remaining = deadline - Date.now();
    if (remaining <= 0) {
      break;
    }
    syncSleep(Math.min(interval, remaining));
    const ready = reader.readyFile(providerPath);
    if (ready) {
      return { kind: "ready", readyFile: ready };
    }
  }
  return { kind: "negative" };
}
