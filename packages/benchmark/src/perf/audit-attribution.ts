/**
 * Overhead-attribution reader over Verter's existing audit substrate.
 *
 * This module only PARSES the existing `RequestAuditRecord` JSON emitted by
 * `compileWithAudit`; it adds no new Rust instrumentation. The audit record's
 * `kind_payload` (a `CompilePayload`) carries the per-phase timing + byte
 * split, `timings` carries the transport buckets, and `memory` carries RSS.
 *
 * The non-checker (axis-A / codegen) split, honestly named for the audit fields
 * that actually exist — there is NO dedicated "hashing/cache/sync" timing bucket
 * in the substrate today, so this does not pretend one exists:
 *   (i)   Verter codegen          → kind_payload.codegen_ms
 *   (ii)  source-map build        → kind_payload.sourcemap_ms
 *   (iii) parse/transform +       → kind_payload.parse_ms + transform_ms
 *         store/transport           + timings.{capture_inputs,store_read,
 *                                     store_merge,serialize}_ms
 *   (iv)  checker time (tsgo)     → measured separately by the carrier-typecheck
 *                                   workloads (axis B); the compile audit is the
 *                                   NON-checker (axis-A / codegen) attribution.
 *
 * Bytes generated, source-map bytes, and a CodeTransform-op count (a coarse
 * source-map-segment proxy; a real segment count is a deferred Rust follow-up)
 * come from the same payload. A dedicated hashing/cache/sync time bucket and a
 * real source-map segment count are tracked Rust follow-ups (see the manifest
 * `deferred` section); the gate does NOT gate a fabricated bucket.
 */

/** The compile audit payload (`kind_payload` when `kind === "Compile"`). */
export interface CompileAuditPayload {
  kind: "Compile";
  target: "Vdom" | "Vapor" | "Ide";
  parse_ms: number | null;
  transform_ms: number | null;
  codegen_ms: number | null;
  css_analysis_ms: number | null;
  sourcemap_ms: number | null;
  output_bytes: string;
  sourcemap_bytes: string;
  num_directives: number;
  num_components: number;
  num_style_blocks: number;
  num_script_blocks: number;
  code_transform_ops: number;
}

interface AuditTimings {
  total_ms: number;
  capture_inputs_ms: number;
  store_read_ms: number;
  store_merge_ms: number;
  serialize_ms: number;
  [k: string]: number;
}

interface AuditMemory {
  process_rss_peak_bytes: string;
  process_rss_after_bytes: string;
  process_rss_delta_bytes: string;
  bytes_parsed: string;
  [k: string]: string;
}

interface RequestAuditRecord {
  kind: string;
  kind_payload?: CompileAuditPayload | { kind: string };
  timings?: AuditTimings;
  memory?: AuditMemory;
}

/**
 * The non-checker (codegen-side) attribution for ONE compile.
 *
 * EVERY field is `number | null`: a missing / malformed / absent audit source is
 * represented as `null` (UNAVAILABLE), never coerced to `0`. A `0` would slip past
 * the gate's presence rail as a "present" datum and undercount a lower-is-better
 * ratio into a false pass; `null` is counted as missing instead.
 */
export interface OverheadAttribution {
  /** (i) carrier codegen (ms), or null when unmeasured. */
  readonly codegenMs: number | null;
  /** (ii) source-map build (ms), or null when unmeasured. */
  readonly sourcemapMs: number | null;
  /**
   * The PRESENT codegen-side emit aggregate: carrier codegen + source-map build
   * (ms) = `codegenMs + sourcemapMs`, or null when either phase is unmeasured.
   * This is the MEASURABLE portion of the non-checker budget — it deliberately
   * excludes the parse/transform/transport phases, whose timing the compile audit
   * record does not emit today (so the full `nonCheckerMs` aggregate is null on a
   * real compile). The axis-A gate's `codegen_time_ratio` reads this sub-signal.
   */
  readonly codegenSourcemapMs: number | null;
  /**
   * (iii) producer parse/transform + host store-read/merge/capture/serialize
   * transport (ms), or null when any component is unmeasured. Honest sum of the
   * audit fields that exist — NOT a dedicated "hashing/cache/sync" bucket (no
   * such bucket exists in the substrate today).
   */
  readonly parseTransformTransportMs: number | null;
  /** Total non-checker (codegen-side) wall (ms), or null when any phase is missing. */
  readonly nonCheckerMs: number | null;
  /** Bytes of generated carrier output, or null when unmeasured. */
  readonly outputBytes: number | null;
  /** Bytes of generated source-map, or null when unmeasured. */
  readonly sourceMapBytes: number | null;
  /** CodeTransform op count — the source-map-segment proxy — or null when unmeasured. */
  readonly codeTransformOps: number | null;
  /** Peak process RSS (bytes), or null when the audit RSS sampler is off. */
  readonly peakRssBytes: number | null;
}

/** A required numeric audit field: missing/non-finite ⇒ `null`, never `0`. */
function reqNum(v: number | null | undefined): number | null {
  return typeof v === "number" && Number.isFinite(v) ? v : null;
}

/** A required bigint-as-string byte field: missing/malformed ⇒ `null`, never `0`. */
function reqBytes(v: string | null | undefined): number | null {
  if (v == null) return null;
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

/** Sum a list of nullable terms; ANY null ⇒ the sum is unknown (`null`). */
function sumOrNull(terms: readonly (number | null)[]): number | null {
  let acc = 0;
  for (const t of terms) {
    if (t == null) return null;
    acc += t;
  }
  return acc;
}

/**
 * Parse a `RequestAuditRecord` JSON Buffer (from `compileWithAudit`) into the
 * four-bucket attribution. Returns `null` if the record is not a Compile
 * payload (e.g. audit disabled ⇒ `compileWithAudit` returned null upstream).
 * A present-but-incomplete record yields `null` for the affected gated fields —
 * never `0` (so the gate reads partial instrumentation as missing).
 */
export function parseCompileAttribution(buf: Buffer | null): OverheadAttribution | null {
  if (buf === null) return null;
  const rec = JSON.parse(buf.toString("utf-8")) as RequestAuditRecord;
  const p = rec.kind_payload;
  if (!p || p.kind !== "Compile") return null;
  const cp = p as CompileAuditPayload;
  const t = rec.timings;

  const codegenMs = reqNum(cp.codegen_ms);
  const sourcemapMs = reqNum(cp.sourcemap_ms);
  // (iii) = the producer-side parse/transform PLUS the host store-read /
  // store-merge / capture / serialize transport buckets from the request
  // timings. Named for what it actually sums — no fabricated hashing bucket. A
  // missing parse/transform ms OR an absent timings record ⇒ the bucket (and the
  // total) is null, not an undercounted partial sum.
  const transportMs = t
    ? sumOrNull(
        [t.capture_inputs_ms, t.store_read_ms, t.store_merge_ms, t.serialize_ms].map(reqNum),
      )
    : null;
  const parseTransformTransportMs = sumOrNull([
    reqNum(cp.parse_ms),
    reqNum(cp.transform_ms),
    transportMs,
  ]);

  const peakRss = reqBytes(rec.memory?.process_rss_peak_bytes);

  return {
    codegenMs,
    sourcemapMs,
    codegenSourcemapMs: sumOrNull([codegenMs, sourcemapMs]),
    parseTransformTransportMs,
    nonCheckerMs: sumOrNull([codegenMs, sourcemapMs, parseTransformTransportMs]),
    outputBytes: reqBytes(cp.output_bytes),
    sourceMapBytes: reqBytes(cp.sourcemap_bytes),
    codeTransformOps: reqNum(cp.code_transform_ops),
    // RSS reads null when the audit process-RSS sampler is not armed for the
    // compile path; the gate then treats the metric as UNAVAILABLE (failing a
    // full run) rather than substituting any other process's memory. A `0` or
    // non-positive reading is likewise unavailable.
    peakRssBytes: peakRss != null && peakRss > 0 ? peakRss : null,
  };
}

/**
 * Sum a set of per-file attributions into an aggregate. The SUMMED fields
 * (codegen / source-map / non-checker ms, output + source-map bytes, transform
 * ops) propagate nullness: if ANY contributing compile is missing the field, the
 * aggregate field is `null` (the gate then reads it as missing instrumentation —
 * a partial sum would silently undercount). `peakRssBytes` is the deliberate
 * EXCEPTION: it is a PEAK, aggregated as the MAX of the PRESENT contributors (a
 * single missing per-compile RSS does not null out the batch peak), and is `null`
 * only when NO contributor reported RSS. `peakRssBytes` is NOT in the axis-A
 * gated set (deferred — see the manifest `deferred` section), so this
 * max-of-present peak never feeds a gate decision.
 */
export function aggregateAttribution(items: readonly OverheadAttribution[]): OverheadAttribution {
  const col = (pick: (a: OverheadAttribution) => number | null): number | null =>
    sumOrNull(items.map(pick));
  let peakRssBytes: number | null = null;
  for (const it of items) {
    if (it.peakRssBytes != null) peakRssBytes = Math.max(peakRssBytes ?? 0, it.peakRssBytes);
  }
  return {
    codegenMs: col((a) => a.codegenMs),
    sourcemapMs: col((a) => a.sourcemapMs),
    codegenSourcemapMs: col((a) => a.codegenSourcemapMs),
    parseTransformTransportMs: col((a) => a.parseTransformTransportMs),
    nonCheckerMs: col((a) => a.nonCheckerMs),
    outputBytes: col((a) => a.outputBytes),
    sourceMapBytes: col((a) => a.sourceMapBytes),
    codeTransformOps: col((a) => a.codeTransformOps),
    peakRssBytes,
  };
}
