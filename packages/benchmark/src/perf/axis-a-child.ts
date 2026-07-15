/**
 * AXIS-A child runner — runs the native-compiler codegen workload in a CHILD
 * process that loads ONE SIDE's `@verter/native` build. Throughput is measured as
 * a SERIAL per-file `compileWithAudit` loop (each SFC compiled one at a time); the
 * candidate-vs-baseline ratio is valid because both sides run the identical serial
 * orchestration. Do NOT change this to a concurrent / whole-corpus compile API:
 * that would invalidate the ratio's apples-to-apples basis unless both sides change
 * together.
 *
 * Two native builds cannot coexist in a single Node process, so the
 * self-referential gate's axis-A comparison MUST run each side in its own child
 * loading THAT side's native package. The parent (workloads.ts `axisACodegen`)
 * spawns this script once per side with `--native <pkgRoot> --corpus <dir>
 * --threads <n>`, and reads the single JSON sample line from stdout.
 *
 * The reported metrics are all derived SOLELY from the audited compile path's
 * `RequestAuditRecord` (codegen/source-map ms, output/source-map bytes, the
 * CodeTransform-op count, and the CHILD's own audit peak RSS) — never the parent
 * harness's memory, never the Node child's own OS peak resident set, never a
 * static file count. An unavailable audit field is `null` (UNAVAILABLE), never `0`:
 *   - `carrierCount`   = count of audited compiles whose output_bytes > 0
 *                        (a real codegen-output signal, NOT sfcPaths.length).
 *   - `peakRssBytes`   = aggregate max of the audit `process_rss_peak_bytes`
 *                        for THIS child (null ⇒ the sampler did not arm ⇒ the
 *                        parent surfaces the metric as unavailable; axis-A peak
 *                        RSS is DEFERRED/ungated, so a null gates nothing — the
 *                        full gate does NOT fail on a missing peak RSS; NO
 *                        Node-process-RSS substitute is used).
 *   - `nonCheckerMs`   = codegen + source-map + parse/transform + transport ms,
 *                        or null when any phase is unmeasured.
 *
 * Usage (invoked by the parent, not by hand):
 *   node --import tsx axis-a-child.ts --native <pkgRoot> --corpus <dir> --threads <n>
 *
 * Output (stdout, exactly one line): a JSON `AxisAChildSample`.
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { parseCompileAttribution, aggregateAttribution } from "./audit-attribution.js";

const require = createRequire(import.meta.url);

/**
 * The compile-target preset names the native `compileWithAudit` accepts
 * (`crates/verter_napi/src/audit.rs::parse_compile_target` — an unknown name is a
 * hard `InvalidArg` error). Typed as a union so an invalid target is a COMPILE
 * error here, not a runtime surprise.
 */
export type CompileTargetName = "BUNDLER" | "IDE" | "ANALYSIS" | "META" | "TSX" | "TSC";

/**
 * The component-carrier codegen target the external-TS-engine benchmark measures:
 * the on-disk `.vue.tsx` / `.svelte.tsx` component IDE carrier (`CarrierIde`) that
 * tsgo/tsserver type-check as real files. `CompileTarget::IDE` is the canonical
 * preset for this surface (the audit tags it `Ide`); it is bit-identical to the
 * raw `CompileTarget::TSX` flag (`const IDE = Self::TSX.bits()` in
 * `crates/verter_compiler/src/compile/types.rs`), so this names the carrier by its
 * documented preset rather than the underlying flag.
 */
const CARRIER_TARGET: CompileTargetName = "IDE";

export interface AxisAChildSample {
  readonly totalMs: number;
  readonly filesPerSec: number;
  /** Audit-derived; `null` (never `0`) when the audited compile did not measure it. */
  readonly outputBytes: number | null;
  readonly sourceMapBytes: number | null;
  readonly codeTransformOps: number | null;
  readonly nonCheckerMs: number | null;
  /** Non-checker split (for the standalone runner's attribution display). */
  readonly codegenMs: number | null;
  readonly sourcemapMs: number | null;
  readonly parseTransformTransportMs: number | null;
  /** Count of audited compiles that emitted output (output_bytes > 0). */
  readonly carrierCount: number;
  /** This child's aggregate audit peak RSS (null ⇒ unavailable — never an OS-RSS substitute). */
  readonly peakRssBytes: number | null;
  /** Number of SFCs the child found in the corpus (the carrier-coverage expected count). */
  readonly sfcCount: number;
  /**
   * A stable content hash over every generated IDE carrier + its source-map
   * (sorted by canonicalId). Compared candidate-vs-baseline for equality (the
   * gate's contentEqualityGated rail): a codegen change that PRESERVES output_bytes
   * + carrierCount but alters the emitted carrier/source-map CONTENT yields a
   * DIFFERENT hash — the correctness signal the byte/count invariants cannot catch.
   * Both sides compile the SAME shared corpus dir at identical canonical paths, so
   * the hash is directly comparable without per-side normalization.
   *
   * `null` (UNAVAILABLE) when ANY expected carrier is MISSING — the host returned
   * no IDE result, or an absent/empty `code` or `sourceMap` (source maps are
   * requested on this path, so both must be non-empty). A missing carrier is NEVER
   * hashed from `""`: a coerced empty-string hash would compare EQUAL on two
   * both-sides-missing runs and slip past the content-equality rail. With `null`
   * the gate's content-equality presence rail hard-fails a full run instead.
   */
  readonly carrierContentHash: string | null;
}

/** One compiled carrier's content (its IDE code + source-map) for hashing. */
export interface CarrierContentEntry {
  readonly canonicalId: string;
  readonly code: string;
  readonly sourceMap: string;
}

/** A stable sha256 hex of a UTF-8 string. */
function sha256Hex(s: string): string {
  return createHash("sha256").update(s, "utf-8").digest("hex");
}

/**
 * A stable content digest over every compiled carrier + its source-map, keyed by
 * canonicalId in sorted order. The hash mixes in the per-carrier code hash AND the
 * per-carrier source-map hash, so a content change in EITHER (even one that keeps
 * the byte count + carrier count unchanged) changes the digest. A NUL field
 * separator + a record separator keep the framing unambiguous (no concatenation
 * collision between adjacent fields/records).
 */
export function carrierContentDigest(entries: readonly CarrierContentEntry[]): string {
  const sorted = [...entries].sort((a, b) => (a.canonicalId < b.canonicalId ? -1 : 1));
  const h = createHash("sha256");
  for (const e of sorted) {
    h.update(e.canonicalId, "utf-8");
    h.update(NUL);
    h.update(sha256Hex(e.code), "utf-8");
    h.update(NUL);
    h.update(sha256Hex(e.sourceMap), "utf-8");
    h.update(RECORD_SEP);
  }
  return `sha256:${h.digest("hex")}`;
}

const NUL = Buffer.from([0]);
const RECORD_SEP = Buffer.from([0x1e]);

export interface ChildArgs {
  nativeRoot: string;
  corpusDir: string;
  threads: number;
}

function parseArgs(argv: string[]): ChildArgs {
  let nativeRoot = "";
  let corpusDir = "";
  let threads = 1;
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--native") nativeRoot = argv[++i];
    else if (a === "--corpus") corpusDir = argv[++i];
    else if (a === "--threads") threads = Number(argv[++i]);
  }
  if (!nativeRoot || !corpusDir) {
    throw new Error("axis-a-child requires --native <pkgRoot> and --corpus <dir>");
  }
  return { nativeRoot, corpusDir, threads };
}

/** The native package's loadable entry for a given package root. */
export function nativeEntry(nativeRoot: string): string {
  return join(nativeRoot, "dist", "index.js");
}

// Perf scope: Vue `.vue` carriers ONLY. Svelte (`.svelte`) discovery is a tracked
// follow-up (generalize to the registered carrier extensions) — see the baseline
// manifest `deferred` / design §2.7.1. The gate makes no current Svelte perf claim.
function collectVueFiles(dir: string): string[] {
  const out: string[] = [];
  const walk = (d: string): void => {
    let entries: import("node:fs").Dirent<string>[];
    try {
      entries = readdirSync(d, { withFileTypes: true, encoding: "utf-8" });
    } catch {
      return;
    }
    for (const e of entries) {
      if (e.name.startsWith(".")) continue;
      const p = join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name.endsWith(".vue")) out.push(p);
    }
  };
  walk(dir);
  out.sort();
  return out;
}

/**
 * Constructs the per-side native `VerterHost` for an axis-A run. Injectable (the
 * same discipline as the parent's `axisAChildRunner` / `spawnChild` seams) so a
 * spec can drive the carrier-content instrumentation path with a scripted host —
 * including the missing-carrier rail — without a real `@verter/native` build.
 */
export type VerterHostFactory = (args: ChildArgs) => VerterHostApi;

const defaultHostFactory: VerterHostFactory = (args) => {
  const entry = nativeEntry(args.nativeRoot);
  statSync(entry); // throws a clear error if the side's native build is absent
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const { VerterHost } = require(entry) as { VerterHost: new (cfg: unknown) => VerterHostApi };
  return new VerterHost({
    devMode: false,
    analysisLevel: "none",
    auditEnabled: true,
    footprintCapture: true,
    hostCpuThreads: args.threads,
  });
};

export function runAxisA(
  args: ChildArgs,
  hostFactory: VerterHostFactory = defaultHostFactory,
): AxisAChildSample {
  const sfcPaths = collectVueFiles(args.corpusDir);
  if (sfcPaths.length === 0) throw new Error(`no .vue files under corpus dir ${args.corpusDir}`);

  const host = hostFactory(args);

  // SERIAL per-file throughput loop: upsert + audited compile each SFC in order
  // (one at a time, not a whole-corpus/concurrent call). filesPerSec is this serial
  // wall divided by file count; the candidate-vs-baseline ratio is valid because
  // both sides run the identical serial orchestration.
  const t0 = performance.now();
  const attributions = [];
  let carrierCount = 0;
  for (const p of sfcPaths) {
    const src = readFileSync(p);
    host.upsert({ inputId: p, source: src, compileProfile: { sourceMap: true } });
    const buf = host.compileWithAudit(p, CARRIER_TARGET);
    const attr = parseCompileAttribution(buf);
    if (attr) {
      attributions.push(attr);
      if ((attr.outputBytes ?? 0) > 0) carrierCount++;
    }
  }
  const totalMs = performance.now() - t0;
  const agg = attributions.length > 0 ? aggregateAttribution(attributions) : null;

  // Content-correctness pass (UNTIMED — never charged to filesPerSec/totalMs):
  // read each already-compiled IDE carrier's code + source-map and hash them, so a
  // codegen change that preserves output_bytes + carrierCount but alters the
  // emitted CONTENT is caught by the gate's carrier-content equality rail. The
  // carriers are warm from the timed loop above, so this is a cache read.
  //
  // Every expected carrier MUST yield a non-empty IDE `code` AND a non-empty
  // `sourceMap` (source maps are requested on this path). A MISSING carrier (the
  // host returned no IDE result, or an absent/empty code or source-map) is MISSING
  // instrumentation — NEVER hashed from "" (which would let a both-sides-missing
  // carrier compare as an equal empty-string hash and pass the content-equality
  // rail). When ANY expected carrier is missing the content hash is UNAVAILABLE
  // (null), and the gate's content-equality presence rail hard-fails a full run.
  const carrierEntries: CarrierContentEntry[] = [];
  let missingCarrier = false;
  for (const p of sfcPaths) {
    const ide = host.getIde(p, { sourceMap: true });
    const code = ide?.code;
    const sourceMap = ide?.sourceMap;
    if (!code || !sourceMap) {
      missingCarrier = true;
      continue;
    }
    carrierEntries.push({ canonicalId: p.replace(/\\/g, "/"), code, sourceMap });
  }
  const carrierContentHash: string | null = missingCarrier
    ? null
    : carrierContentDigest(carrierEntries);

  // Peak RSS for THIS child is the audit substrate's `process_rss_peak_bytes`
  // ONLY. When the compile-path RSS sampler is not armed the audit value is
  // `null`, recorded honestly as UNAVAILABLE: axis-A peak RSS is DEFERRED/ungated,
  // so a null gates nothing (the full gate does NOT fail on a missing peak RSS).
  // There is NO fallback to the Node child's own OS peak resident set: that is a
  // parallel measurement path under an audit metric (the parent surfaces null
  // honestly).
  const peakRssBytes = agg?.peakRssBytes ?? null;

  return {
    totalMs,
    filesPerSec: (sfcPaths.length / totalMs) * 1000,
    outputBytes: agg?.outputBytes ?? null,
    sourceMapBytes: agg?.sourceMapBytes ?? null,
    codeTransformOps: agg?.codeTransformOps ?? null,
    nonCheckerMs: agg?.nonCheckerMs ?? null,
    codegenMs: agg?.codegenMs ?? null,
    sourcemapMs: agg?.sourcemapMs ?? null,
    parseTransformTransportMs: agg?.parseTransformTransportMs ?? null,
    carrierCount,
    peakRssBytes,
    sfcCount: sfcPaths.length,
    carrierContentHash,
  };
}

export interface VerterHostApi {
  upsert(input: { inputId: string; source: Buffer; compileProfile: { sourceMap: boolean } }): void;
  compileWithAudit(id: string, target: CompileTargetName): Buffer | null;
  getIde(
    id: string,
    profile?: { sourceMap?: boolean },
  ): { code: string; sourceMap?: string } | null;
}

const invokedDirectly = process.argv[1]?.replace(/\\/g, "/").endsWith("perf/axis-a-child.ts");
if (invokedDirectly) {
  try {
    const sample = runAxisA(parseArgs(process.argv));
    process.stdout.write(JSON.stringify(sample) + "\n");
  } catch (e) {
    process.stderr.write(`axis-a-child failed: ${(e as Error).message}\n`);
    process.exit(1);
  }
}
