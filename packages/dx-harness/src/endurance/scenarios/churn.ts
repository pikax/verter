/**
 * Document-lifecycle CHURN lane: open → edit → query → close, repeated, with
 * process-TREE resident memory compared across two quiesced checkpoints.
 *
 * The other endurance lanes keep documents OPEN and hammer them. That workload
 * cannot see a per-document-version leak at all: a store that retains every
 * version of every document ever synced looks identical to a bounded one when
 * the session only ever holds a handful of documents. The retention question is
 * a LIFECYCLE question — what does a session still hold after a document has
 * been opened, edited, answered for, and closed, a thousand times over?
 *
 * The instrument is deliberately not a single Rust arena figure. An in-process
 * byte account can only report what it is asked to charge; a component that
 * retains outside it is invisible to it by construction. This lane reads the
 * operating system's resident-set figure for the server process AND every
 * descendant (the type-provider engine included), so anything the session
 * retains anywhere in its process tree is inside the measurement.
 *
 * Measurement shape — two QUIESCED checkpoints, not a slope through the noise:
 *
 *  1. a warm-up prefix of full cycles, so the baseline already contains every
 *     one-time cost (project load, provider program, lazily-initialised caches)
 *     and the comparison is not measuring startup;
 *  2. quiesce (host work counters unchanged across consecutive polls, no
 *     scanner/drain/sync WARN churn), then read the tree — the BASELINE;
 *  3. the remaining cycles;
 *  4. quiesce again, then read the tree — the FINAL reading.
 *
 * Growth is then `final <= baseline * factor + floor`. Both terms matter: the
 * factor catches proportional growth on a large baseline, and the absolute
 * floor keeps a small-baseline run from failing on allocator noise. A retained
 * document version is ~hundreds of KiB (provider text + carrier source + a
 * UTF-16 line index over each, twice over); a thousand cycles of it is hundreds
 * of MiB, far outside either tolerance, while a bounded session returns to its
 * baseline.
 *
 * Semantic strength is not traded for the memory figure: every cycle answers a
 * real provider-backed request, and the lane ends with a strict convergence
 * probe proving the session still answers correctly after the churn.
 */
import { writeFileSync } from "node:fs";
import { createWarnLineDrainer, GET_STATISTICS_METHOD } from "../../core/startupGate.js";
import {
  extractQuiescenceCounters,
  pollUntilQuiesced,
  type QuiescenceCounters,
} from "../../core/quiescence.js";
import { sampleProcessTreeRss, type ProcessTreeRssSample } from "../processTreeRss.js";
import { DEFAULT_ENDURANCE_LANE, type EnduranceLane, type EnduranceReceipt } from "../types.js";
import { carrierPath, ENDURANCE_TSCONFIG, type WorkspaceFiles } from "../workspace.js";
import { buildReceipt, convergeProbe, FailureBag, type ScenarioContext } from "./common.js";

/** The churned document and the stable consumer that keeps importing it. */
export interface ChurnFixture {
  readonly lane: EnduranceLane;
  /** The document opened, edited, queried and closed on every cycle. */
  readonly churnPath: string;
  /** A consumer that stays open for the whole run and imports the churn doc. */
  readonly consumerPath: string;
  readonly churnContent: string;
  readonly files: WorkspaceFiles;
  /** A needle in the churn document's template that a hover can target. */
  readonly hoverNeedle: string;
}

/**
 * A carrier big enough that ONE retained version is measurable.
 *
 * Retention is per synced version, so the per-cycle cost of a leak scales with
 * the document; a 200-byte fixture would hide a real leak inside allocator
 * noise for any cycle count a test can afford. `blocks` controls the size
 * (~230 source bytes each, plus the generated TypeScript surface the provider
 * holds for it).
 */
export function churnCarrierContent(blocks: number, lane: EnduranceLane): string {
  if (lane.framework !== "vue") {
    throw new Error(`churn carrier content is vue-only, got ${lane.framework}`);
  }
  const fields: string[] = [];
  const derived: string[] = [];
  const markup: string[] = [];
  for (let index = 0; index < blocks; index += 1) {
    fields.push(`  churnField${index}?: string;`);
    derived.push(
      `const churnLocal${index} = \`c${index}:\${props.churnLabel}:\${props.churnField${index} ?? ""}\`;`,
      `const churnLength${index} = churnLocal${index}.length;`,
    );
    markup.push(
      `    <span :title="churnLocal${index}" :data-len="churnLength${index}">{{ churnLocal${index} }}</span>`,
    );
  }
  return [
    '<script setup lang="ts">',
    "interface ChurnProps {",
    "  churnLabel: string;",
    ...fields,
    "}",
    "const props = defineProps<ChurnProps>();",
    'const emit = defineEmits<{ (e: "churn", value: string): void }>();',
    ...derived,
    "const churnHeadline = props.churnLabel.toUpperCase();",
    "function fireChurn() {",
    '  emit("churn", churnHeadline);',
    "}",
    "</script>",
    "",
    "<template>",
    "  <section>",
    '    <h1 :title="churnHeadline">{{ churnHeadline }}</h1>',
    ...markup,
    '    <button @click="fireChurn">go</button>',
    "  </section>",
    "</template>",
    "",
  ].join("\n");
}

function churnConsumerContent(churnImport: string): string {
  return [
    '<script setup lang="ts">',
    `import Churn from "${churnImport}";`,
    'const consumerLabel = "consumer";',
    "const consumerLength = consumerLabel.length;",
    "</script>",
    "",
    "<template>",
    '  <main :data-len="consumerLength">',
    '    <Churn :churn-label="consumerLabel" />',
    "  </main>",
    "</template>",
    "",
  ].join("\n");
}

export function churnFixture(
  blocks: number,
  lane: EnduranceLane = DEFAULT_ENDURANCE_LANE,
): ChurnFixture {
  const churnPath = carrierPath(lane, "Churn");
  const consumerPath = carrierPath(lane, "ChurnConsumer");
  const churnContent = churnCarrierContent(blocks, lane);
  return {
    lane,
    churnPath,
    consumerPath,
    churnContent,
    hoverNeedle: "{{ churnHeadline }}",
    files: {
      "tsconfig.json": ENDURANCE_TSCONFIG,
      [churnPath]: churnContent,
      [consumerPath]: churnConsumerContent(`./Churn.${lane.framework}`),
    },
  };
}

/** The verdict of one baseline→final process-tree comparison. */
export interface ChurnGrowthCheck {
  /**
   * True only when BOTH checkpoints observed the COMPLETE process tree and the
   * two readings cover a comparable member set. False ⇒ the bound was not
   * evaluated: the metric is UNAVAILABLE, which is reported as such and is never
   * a measured pass.
   */
  readonly observable: boolean;
  readonly baselineBytes: number | null;
  readonly finalBytes: number | null;
  readonly growthBytes: number | null;
  /** `final / baseline`, or null when the comparison was not evaluated. */
  readonly ratio: number | null;
  readonly allowedFactor: number;
  readonly allowedFloorBytes: number;
  /** The largest `final` that passes: `baseline * factor + floor`. */
  readonly allowedBytes: number | null;
  /**
   * True only when the bound was EVALUATED and satisfied. An unavailable metric
   * is `false`, so a lane that requires this proof cannot go green without it.
   */
  readonly pass: boolean;
  readonly detail: string;
}

/**
 * Decide the growth verdict (pure — the arithmetic is unit-testable without a
 * server).
 *
 * Three ways this refuses to produce a number, all reported as `observable:
 * false, pass: false`:
 *
 *  1. either checkpoint is not a complete whole-tree observation (the platform
 *     could not enumerate the table, or a discovered member was unreadable) —
 *     a partial tree can be flat while the unmeasured member grows;
 *  2. a process present at the BASELINE is gone from the FINAL tree — its bytes
 *     left the comparison, which makes growth look smaller than it was. A
 *     respawned provider is exactly this shape: the child that held every
 *     retained version dies, and the session reads as bounded.
 *
 * "Unavailable" is distinct from "measured and within bound", and the lane that
 * consumes this must surface it as a missing proof rather than a pass — the
 * charter's acceptance criterion is the complete process-tree evidence, and a
 * bound that was never evaluated has not been satisfied.
 */
export function decideChurnGrowth(
  baseline: ProcessTreeRssSample,
  final: ProcessTreeRssSample,
  allowedFactor: number,
  allowedFloorBytes: number,
): ChurnGrowthCheck {
  const unevaluated = (detail: string): ChurnGrowthCheck => ({
    observable: false,
    baselineBytes: baseline.observable ? baseline.totalBytes : null,
    finalBytes: final.observable ? final.totalBytes : null,
    growthBytes: null,
    ratio: null,
    allowedFactor,
    allowedFloorBytes,
    allowedBytes: null,
    pass: false,
    detail,
  });

  if (!baseline.observable || !final.observable) {
    const which = !baseline.observable ? "baseline" : "final";
    const sample = !baseline.observable ? baseline : final;
    return unevaluated(
      `the ${which} process-tree reading is UNAVAILABLE, so the churn growth bound was NOT ` +
        `evaluated: ${sample.unavailable?.detail ?? "no detail"}`,
    );
  }
  const baselineBytes = baseline.totalBytes as number;
  const finalBytes = final.totalBytes as number;

  const finalPids = new Set(final.members.map((member) => member.pid));
  const departed = baseline.members
    .map((member) => member.pid)
    .filter((pid) => !finalPids.has(pid));
  if (departed.length > 0) {
    return unevaluated(
      `process(es) ${departed.join(", ")} were in the baseline tree and are absent from the ` +
        "final tree, so their bytes left the comparison and the growth figure would understate " +
        "retention — the churn growth bound was NOT evaluated",
    );
  }

  const allowedBytes = baselineBytes * allowedFactor + allowedFloorBytes;
  const pass = finalBytes <= allowedBytes;
  return {
    observable: true,
    baselineBytes,
    finalBytes,
    growthBytes: finalBytes - baselineBytes,
    ratio: baselineBytes > 0 ? finalBytes / baselineBytes : null,
    allowedFactor,
    allowedFloorBytes,
    allowedBytes,
    pass,
    detail:
      `baseline=${bytesToMib(baselineBytes)} final=${bytesToMib(finalBytes)} ` +
      `growth=${bytesToMib(finalBytes - baselineBytes)} ` +
      `allowed=${bytesToMib(allowedBytes)} (baseline*${allowedFactor}+${bytesToMib(allowedFloorBytes)})`,
  };
}

/**
 * The cycle count WSP6-AC1 names: "1000 open/edit/close cycles show no unbounded
 * retained-byte slope after quiescence".
 *
 * A shorter run is a perfectly good smoke lane and a perfectly bad acceptance
 * proof: a leak of a few tens of KiB per cycle disappears inside the absolute
 * growth floor at 200 cycles and is unmissable at 1000. The acceptance verdict
 * therefore refuses to be satisfied by a shorter run instead of quietly
 * reporting a pass for a bound the run could not have exercised.
 */
export const CHURN_ACCEPTANCE_MIN_CYCLES = 1_000;

/**
 * The host's retained OBJECT set at one quiesced checkpoint, as the server
 * reports it under `$/verter/getStatistics` → `retention`.
 *
 * RSS says how much the tree holds; this says which retained set is doing the
 * holding, so a rising figure can be attributed (superseded artifact versions,
 * pinned parse snapshots, captured roots) instead of guessed at. Read live from
 * the owning structures on the server — nothing here needs enabling.
 */
export interface RetentionReading {
  /** Live artifact versions in the indexed store (one per current key). */
  readonly liveArtifacts: number;
  /** Superseded versions still retained (root-reachable or not yet swept). */
  readonly retainedRetiredVersions: number;
  /** Live captured store roots. */
  readonly liveRoots: number;
  /** Live decl-lowering parse-snapshot leases. */
  readonly snapshotLeases: number;
  /** Complete carrier parses persisted for same-content adoption (bounded per file). */
  readonly carrierCandidates: number;
  /** Publication lanes the carrier store retains, live and terminal. */
  readonly publicationLanes: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly semanticNodes: number;
  /** Node slots the semantic arena physically holds (chunked; bounded by the live set). */
  readonly semanticNodeSlots: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly semanticMemoEntries: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly unresolvedReach: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly relationProofs: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly relateKeys: number;
  /** Resident union member views, released with their union's document. */
  readonly unionViews: number;
  /** Close-time semantic releases queued behind in-flight computations, not yet applied. */
  readonly deferredReleases: number;
  /** Resolved-import fact entries (one key per document content hash). */
  readonly resolvedImportFacts: number;
  /** Resolver component-meta states (one key per document, mode and view fingerprint). */
  readonly componentMetaStates: number;
  /** Registered source snapshots (base plus current overlay per document). */
  readonly registeredSources: number;
  /** Records interned in the signature kernel's current epoch: bounded by `signatureRecordCap`, not flat. */
  readonly signatureRecords: number;
  /** The record cap the kernel replaces its epoch at. */
  readonly signatureRecordCap: number;
  /** Queued releases applied so far (monotonic by design, so not a retention counter). */
  readonly releasesApplied: number;
  /** The longest a queued release waited for a zero-reader instant, in microseconds. */
  readonly releaseWaitMaxMicros: number;
  /** The slowest single close-time release, in microseconds. */
  readonly releaseElapsedMaxMicros: number;
  /** The last close-time release applied, or null before the first. */
  readonly lastRelease: LastReleaseReading | null;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly shapeCacheEntries: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly flowGraphs: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly flowHashEntries: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly flowLoweredEntries: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly mapperFingerprints: number;
  /** Cached framework-surface DTO entries (props/emits/slots per owner content version). */
  readonly frameworkSurfaceEntries: number;
  /** Bytes charged as pinned against the aggregate semantic retention account. */
  readonly pinnedBytes: number;
  /** Bytes charged as retained (reusable) against the aggregate account. */
  readonly retainedBytes: number;
  /** Reservations the aggregate account refused for pressure, lifetime. */
  readonly refusalsPressure: number;
  /**
   * Bytes the server's platform allocator reports as currently allocated
   * (`heapInUseBytes`), or null where the platform cannot say. Exact where a
   * resident-set figure is not: it excludes allocator settling.
   */
  readonly heapInUseBytes: number | null;
  /** Populated semantic memo slots per family label (reported, not bounded). */
  readonly semanticMemoFamilies?: Readonly<Record<string, number>>;
}

/** One applied close-time release (`retention.lastRelease`). */
export interface LastReleaseReading {
  readonly waitMicros: number;
  readonly elapsedMicros: number;
  readonly nodesScanned: number;
  readonly nodesReleased: number;
  readonly storageSlotsBefore: number;
  readonly storageSlotsAfter: number;
  readonly memoEntriesEvicted: number;
}

const LAST_RELEASE_KEYS: readonly (keyof LastReleaseReading)[] = [
  "waitMicros",
  "elapsedMicros",
  "nodesScanned",
  "nodesReleased",
  "storageSlotsBefore",
  "storageSlotsAfter",
  "memoEntriesEvicted",
];

function readLastRelease(value: unknown): LastReleaseReading | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const reading: Partial<Record<keyof LastReleaseReading, number>> = {};
  for (const key of LAST_RELEASE_KEYS) {
    const field = record[key];
    if (typeof field !== "number" || !Number.isFinite(field)) return null;
    reading[key] = field;
  }
  return reading as LastReleaseReading;
}

/** The `retention` object keys the host emits, exactly as serialized. */
const RETENTION_KEYS: readonly (keyof RetentionReading)[] = [
  "liveArtifacts",
  "retainedRetiredVersions",
  "liveRoots",
  "snapshotLeases",
  "carrierCandidates",
  "publicationLanes",
  "semanticNodes",
  "semanticNodeSlots",
  "semanticMemoEntries",
  "unresolvedReach",
  "relationProofs",
  "relateKeys",
  "unionViews",
  "shapeCacheEntries",
  "flowGraphs",
  "flowHashEntries",
  "flowLoweredEntries",
  "mapperFingerprints",
  "frameworkSurfaceEntries",
  "pinnedBytes",
  "retainedBytes",
  "refusalsPressure",
  "deferredReleases",
  "resolvedImportFacts",
  "componentMetaStates",
  "registeredSources",
  "signatureRecords",
  "signatureRecordCap",
  "releasesApplied",
  "releaseWaitMaxMicros",
  "releaseElapsedMaxMicros",
];

/**
 * Project a `$/verter/getStatistics` snapshot down to its retention reading,
 * or `null` when the server did not report one (an older server, or a
 * malformed field). A missing reading is an UNAVAILABLE metric: the consumer
 * labels it, and never reads it as zero.
 */
export function extractRetentionReading(snapshot: unknown): RetentionReading | null {
  const retention = (snapshot as { retention?: unknown } | null | undefined)?.retention;
  if (!retention || typeof retention !== "object") return null;
  const record = retention as Record<string, unknown>;
  const reading: Partial<Record<keyof RetentionReading, unknown>> = {};
  for (const key of RETENTION_KEYS) {
    const value = record[key];
    if (typeof value !== "number" || !Number.isFinite(value)) return null;
    reading[key] = value;
  }
  const heap = record.heapInUseBytes;
  (reading as { heapInUseBytes?: number | null }).heapInUseBytes =
    typeof heap === "number" && Number.isFinite(heap) ? heap : null;
  (reading as { lastRelease?: LastReleaseReading | null }).lastRelease = readLastRelease(
    record.lastRelease,
  );
  const families = record.semanticMemoFamilies;
  if (families && typeof families === "object") {
    const byFamily: Record<string, number> = {};
    for (const [family, count] of Object.entries(families as Record<string, unknown>)) {
      if (typeof count === "number" && Number.isFinite(count)) byFamily[family] = count;
    }
    (reading as { semanticMemoFamilies?: Record<string, number> }).semanticMemoFamilies = byFamily;
  }
  return reading as RetentionReading;
}

/** One quiesced process-tree reading, stamped with the cycles behind it. */
export interface ChurnCheckpoint {
  /** Cycles completed when this reading was taken. */
  readonly cyclesCompleted: number;
  /** Whether the host reached quiescence before the reading. */
  readonly quiesced: boolean;
  readonly sample: ProcessTreeRssSample;
  /** The host's retained object set, or null when the server reported none. */
  readonly retention: RetentionReading | null;
  /**
   * Cumulative JSON-RPC body bytes over the client's pipes at this reading:
   * `inbound` is what the server sent (its outbound bytes), `outbound` what the
   * harness sent. Absent when the client does not count. Reported, not bounded:
   * no shared limit exists for it, and a figure without a limit is evidence,
   * not a verdict.
   */
  readonly wireBytes?: { readonly inbound: number; readonly outbound: number };
}

/** Per-window retained-byte rate between two consecutive checkpoints. */
export interface ChurnSlopeSegment {
  readonly fromCycle: number;
  readonly toCycle: number;
  readonly cycles: number;
  readonly growthBytes: number;
  readonly bytesPerCycle: number;
}

/** The bands the plateau verdicts are read against (see {@link decideChurnSlope}). */
export interface ChurnPlateauBands {
  /**
   * How far the server's exact heap figure may rise over the late span, in
   * bytes. An absolute band, not a per-cycle allowance: a longer run has to
   * fit the same band.
   */
  readonly heapPlateauBytes: number;
  /**
   * How far the server's resident set may rise over the late span: the
   * allocator's settling allowance. The exact heap figure, not this, is the
   * server's retention instrument.
   */
  readonly rssSettlingBytes: number;
  /** How far a child's resident set may rise within one plateau segment. */
  readonly childPlateauBytes: number;
  /** The smallest single-window increment read as a level shift. */
  readonly shiftBytes: number;
  /** Readings a segment needs before its plateau counts as proven. */
  readonly minPlateauReadings: number;
}

/** The defaults of {@link ChurnPlateauBands}. */
export const CHURN_PLATEAU_BANDS: ChurnPlateauBands = {
  heapPlateauBytes: 2 * 1024 ** 2,
  rssSettlingBytes: 8 * 1024 ** 2,
  childPlateauBytes: 8 * 1024 ** 2,
  shiftBytes: 8 * 1024 ** 2,
  minPlateauReadings: 5,
};

/**
 * A least-squares fit over one span of quiesced readings, read as a plateau
 * test: the fitted rise across the span must fit an absolute band, and the
 * slope must not be significantly positive (its lower confidence bound —
 * 2.5 standard errors from the residuals — must not clear zero).
 */
export interface PlateauCheck {
  readonly fromCycle: number;
  readonly toCycle: number;
  readonly readings: number;
  /** Least-squares slope, in the reading's unit per cycle. */
  readonly perCycle: number;
  /** Standard error of that slope, from the fit's residuals. */
  readonly standardError: number;
  /** The fitted rise across the span: slope times the span's cycles. */
  readonly riseOverSpan: number;
  readonly band: number;
  readonly withinBand: boolean;
  /** `perCycle - 2.5 * standardError > 0`: a trend the noise cannot explain. */
  readonly significantlyRising: boolean;
  /**
   * Set when the series was judged as two flat levels (see
   * `twoLevelPlateau`): the levels and how many readings sat on each.
   */
  readonly levels?: {
    readonly low: number;
    readonly high: number;
    readonly lowReadings: number;
    readonly highReadings: number;
  };
  /** Within the band and not significantly rising. */
  readonly plateau: boolean;
}

/** A single-window increment set aside as a one-time level shift. */
export interface ChurnLevelShift {
  readonly fromCycle: number;
  readonly toCycle: number;
  readonly growthBytes: number;
}

/** One process-tree member's late-span verdict. */
export interface ChurnMemberSlope {
  readonly pid: number;
  readonly image: string | null;
  /** `root`: the spawned server. `child`: a descendant (the type-provider engine). */
  readonly role: "root" | "child";
  /**
   * The server's exact heap figure over the late span (the retention
   * instrument), or null for a child or when the server reports none.
   */
  readonly heap: PlateauCheck | null;
  /**
   * Resident-set plateau segments over the late span: one, or two around a
   * level shift.
   */
  readonly rss: readonly PlateauCheck[];
  readonly levelShift: ChurnLevelShift | null;
  /**
   * A level shift sits too close to the end for its post-shift segment to
   * prove a plateau: not a pass, and the scenario extends the run.
   */
  readonly inconclusive: boolean;
  readonly withinBound: boolean;
}

/** The retained-byte plateau verdict over the quiesced trajectory. */
export interface ChurnSlopeCheck {
  readonly observable: boolean;
  readonly cyclesCompleted: number;
  readonly minimumCycles: number;
  readonly bands: ChurnPlateauBands;
  /** Every consecutive window's whole-tree rate: the trajectory, reported as evidence. */
  readonly segments: readonly ChurnSlopeSegment[];
  /** First cycle of the late span the verdict is read over. */
  readonly lateFromCycle: number | null;
  /** Whole-tree least-squares rate over the late span (evidence). */
  readonly lateBytesPerCycle: number | null;
  /** The verdict: every member against its own plateau bands. */
  readonly members: readonly ChurnMemberSlope[];
  /** Some member's post-shift plateau is unproven: extend the run. */
  readonly inconclusive: boolean;
  readonly pass: boolean;
  readonly detail: string;
}

/** Fewest quiesced readings the late span must hold for a plateau to mean anything. */
export const CHURN_SLOPE_MIN_LATE_READINGS = 5;

/** Slope significance: the lower confidence bound is this many standard errors below the slope. */
const PLATEAU_CONFIDENCE_STANDARD_ERRORS = 2.5;

/**
 * Decide WSP6-AC1's actual claim: after quiescence, 1000 open/edit/close cycles
 * show no UNBOUNDED retained-byte slope.
 *
 * Two quiesced endpoints and a `final <= baseline * factor + floor` envelope
 * cannot express that claim. At a 75 MiB baseline a 1.25 factor plus a 64 MiB
 * floor admits ~83 MiB of growth, so ~85 KiB retained on every post-warm-up
 * cycle — a strictly LINEAR leak, the exact shape the criterion forbids — lands
 * inside the envelope and reads as a pass. An envelope describes a destination;
 * the criterion is about the trajectory. A per-cycle allowance cannot express
 * it either: any rate under the allowance is a leak with a lower gradient, and
 * the growth it admits scales with the session.
 *
 * So the verdict is a PLATEAU over the late span (every reading from the
 * run's midpoint on), read per process against absolute bands:
 *
 *  - The server's exact heap figure (`heapInUseBytes`, the allocator's own
 *    in-use count) is the retention instrument. Its fitted rise over the
 *    late span must fit `heapPlateauBytes`, and its slope must not be
 *    significantly positive. Unlike a resident-set figure it holds no
 *    allocator settling, so a slow linear retainer shows as a trend the
 *    noise cannot explain, however long it takes. A server that reports no
 *    such figure has not produced the evidence: UNAVAILABLE, never a pass.
 *  - The server's resident set must fit `rssSettlingBytes` over the late
 *    span. Measured with every retained-object counter and the exact heap
 *    figure flat, the server's committed memory still climbs for several
 *    hundred cycles while the allocator reaches steady-state fragmentation,
 *    a few MiB of it after the midpoint. That band is allocator settling,
 *    read against an exact instrument that would show a retainer instead.
 *  - A child (the type-provider engine, a Go runtime whose resident set
 *    follows its collector's heap goal) has no exact figure. Its resident set
 *    must fit `childPlateauBytes` within each plateau segment. It moves
 *    between plateaus once in some runs, by 20-35 MiB at a random cycle: one
 *    single-window increment of at least `shiftBytes` is read as that level
 *    shift, and the segments before and after it must each be a plateau. A
 *    post-shift segment shorter than `minPlateauReadings` proves nothing —
 *    the verdict is INCONCLUSIVE, not a pass, and the scenario extends the
 *    run until it is proven or the extension budget is spent. A second shift
 *    is a breach.
 *
 * The bands are absolute, so extending the run tightens the test rather than
 * loosening it. Every window's whole-tree rate stays in the detail as
 * evidence, and the envelope still bounds the total growth any shift can add.
 *
 * Refused outright — `observable: false, pass: false`, never a pass — when:
 *
 *  - the run was shorter than `minimumCycles` (the criterion's own run length);
 *  - fewer than two post-baseline windows exist, so there is no trajectory to read;
 *  - the late span holds fewer than {@link CHURN_SLOPE_MIN_LATE_READINGS}
 *    readings, or a member appears in fewer of them than that;
 *  - the server reported no exact heap figure at some late reading;
 *  - any checkpoint was read without the host reaching quiescence, so the figure
 *    includes in-flight work rather than what the session retains;
 *  - any checkpoint is not a complete whole-tree observation;
 *  - a process present at the first checkpoint is missing from a later one, which
 *    takes its retained bytes out of the comparison (a respawned provider is
 *    exactly this shape).
 */
export function decideChurnSlope(
  checkpoints: readonly ChurnCheckpoint[],
  options: {
    readonly minimumCycles: number;
    readonly bands?: Partial<ChurnPlateauBands>;
    /**
     * First cycle of the late span. Defaults to the midpoint between the
     * baseline and the final reading; the scenario pins it to the planned
     * run's midpoint so an extension lengthens the span instead of moving it.
     */
    readonly lateFromCycle?: number;
  },
): ChurnSlopeCheck {
  const { minimumCycles } = options;
  const bands: ChurnPlateauBands = { ...CHURN_PLATEAU_BANDS, ...options.bands };
  const cyclesCompleted =
    checkpoints.length > 0 ? checkpoints[checkpoints.length - 1].cyclesCompleted : 0;
  const unevaluated = (detail: string): ChurnSlopeCheck => ({
    observable: false,
    cyclesCompleted,
    minimumCycles,
    bands,
    segments: [],
    lateFromCycle: null,
    lateBytesPerCycle: null,
    members: [],
    inconclusive: false,
    pass: false,
    detail,
  });

  if (checkpoints.length < 3) {
    return unevaluated(
      "the retained-byte slope needs a baseline plus at least two later quiesced readings, " +
        `got ${checkpoints.length} — NOT evaluated`,
    );
  }
  if (cyclesCompleted < minimumCycles) {
    return unevaluated(
      `the run completed ${cyclesCompleted} cycles, below the ${minimumCycles} the acceptance ` +
        "criterion names, so the retained-byte slope was NOT evaluated",
    );
  }
  const unquiesced = checkpoints.filter((checkpoint) => !checkpoint.quiesced);
  if (unquiesced.length > 0) {
    return unevaluated(
      `checkpoint(s) at cycle ${unquiesced.map((c) => c.cyclesCompleted).join(", ")} were read ` +
        "without the host reaching quiescence, so the slope was NOT evaluated",
    );
  }
  const unobservable = checkpoints.filter((checkpoint) => !checkpoint.sample.observable);
  if (unobservable.length > 0) {
    return unevaluated(
      `checkpoint(s) at cycle ${unobservable.map((c) => c.cyclesCompleted).join(", ")} are not ` +
        "complete whole-tree observations: " +
        `${unobservable[0].sample.unavailable?.detail ?? "no detail"} — NOT evaluated`,
    );
  }
  const firstPids = checkpoints[0].sample.members.map((member) => member.pid);
  for (const checkpoint of checkpoints.slice(1)) {
    const present = new Set(checkpoint.sample.members.map((member) => member.pid));
    const departed = firstPids.filter((pid) => !present.has(pid));
    if (departed.length > 0) {
      return unevaluated(
        `process(es) ${departed.join(", ")} left the tree before the reading at cycle ` +
          `${checkpoint.cyclesCompleted}, so their retained bytes left the comparison — ` +
          "the slope was NOT evaluated",
      );
    }
  }

  const segments: ChurnSlopeSegment[] = [];
  for (let index = 1; index < checkpoints.length; index += 1) {
    const from = checkpoints[index - 1];
    const to = checkpoints[index];
    const cycles = to.cyclesCompleted - from.cyclesCompleted;
    if (cycles <= 0) {
      return unevaluated(
        `the reading at cycle ${to.cyclesCompleted} did not advance past the previous one ` +
          `(${from.cyclesCompleted}), so no per-cycle rate exists — NOT evaluated`,
      );
    }
    const growthBytes = (to.sample.totalBytes as number) - (from.sample.totalBytes as number);
    segments.push({
      fromCycle: from.cyclesCompleted,
      toCycle: to.cyclesCompleted,
      cycles,
      growthBytes,
      bytesPerCycle: growthBytes / cycles,
    });
  }

  // The late span: every reading from the midpoint on.
  const midpoint =
    options.lateFromCycle ??
    checkpoints[0].cyclesCompleted + (cyclesCompleted - checkpoints[0].cyclesCompleted) / 2;
  const late = checkpoints.filter((checkpoint) => checkpoint.cyclesCompleted >= midpoint);
  if (late.length < CHURN_SLOPE_MIN_LATE_READINGS) {
    return unevaluated(
      `the late span (cycle ${Math.ceil(midpoint)} on) holds ${late.length} quiesced ` +
        `reading(s), below the ${CHURN_SLOPE_MIN_LATE_READINGS} a plateau needs — raise ` +
        "VERTER_ENDURANCE_CHURN_SLOPE_WINDOWS; the slope was NOT evaluated",
    );
  }
  const xs = late.map((checkpoint) => checkpoint.cyclesCompleted);
  const lateBytesPerCycle = leastSquares(
    xs,
    late.map((checkpoint) => checkpoint.sample.totalBytes as number),
  ).slope;

  const rootPid = checkpoints[0].sample.members[0]?.pid;
  const memberPids = [
    ...new Set(late.flatMap((checkpoint) => checkpoint.sample.members.map((m) => m.pid))),
  ];
  const members: ChurnMemberSlope[] = [];
  for (const pid of memberPids) {
    const readings = late.flatMap((checkpoint) => {
      const member = checkpoint.sample.members.find((m) => m.pid === pid);
      return member && member.rssBytes !== null
        ? [{ cycle: checkpoint.cyclesCompleted, bytes: member.rssBytes, image: member.image }]
        : [];
    });
    if (readings.length < CHURN_SLOPE_MIN_LATE_READINGS) {
      return unevaluated(
        `process ${pid} appears in ${readings.length} late reading(s), below the ` +
          `${CHURN_SLOPE_MIN_LATE_READINGS} a plateau needs — the slope was NOT evaluated`,
      );
    }
    const role = pid === rootPid ? "root" : "child";
    const cycles = readings.map((reading) => reading.cycle);
    const rss = readings.map((reading) => reading.bytes);

    // The server: its exact heap figure is the retention instrument.
    let heap: PlateauCheck | null = null;
    if (role === "root") {
      const heapReadings = late.map((checkpoint) => checkpoint.retention?.heapInUseBytes ?? null);
      const missing = heapReadings.some((bytes) => bytes === null);
      if (missing) {
        return unevaluated(
          "the server reported no exact heap figure (retention.heapInUseBytes) at a late " +
            "reading, so its retained bytes are UNAVAILABLE — NOT evaluated",
        );
      }
      heap = plateau(xs, heapReadings as number[], bands.heapPlateauBytes, true);
    }

    // The resident set: one plateau, or two around a single level shift.
    const shiftIndex = largestIncrement(rss);
    const shiftBytes = shiftIndex > 0 ? rss[shiftIndex] - rss[shiftIndex - 1] : 0;
    const band = role === "root" ? bands.rssSettlingBytes : bands.childPlateauBytes;
    let levelShift: ChurnLevelShift | null = null;
    let inconclusive = false;
    let rssChecks: PlateauCheck[];
    let secondShift = false;
    if (shiftIndex > 0 && shiftBytes >= bands.shiftBytes) {
      levelShift = {
        fromCycle: cycles[shiftIndex - 1],
        toCycle: cycles[shiftIndex],
        growthBytes: shiftBytes,
      };
      const before = { xs: cycles.slice(0, shiftIndex), ys: rss.slice(0, shiftIndex) };
      const after = { xs: cycles.slice(shiftIndex), ys: rss.slice(shiftIndex) };
      secondShift = [before, after].some((segment) => {
        const index = largestIncrement(segment.ys);
        return index > 0 && segment.ys[index] - segment.ys[index - 1] >= bands.shiftBytes;
      });
      inconclusive = after.ys.length < bands.minPlateauReadings;
      rssChecks = [before, after]
        .filter((segment) => segment.ys.length >= 2)
        .map((segment) => plateau(segment.xs, segment.ys, band, false));
    } else {
      rssChecks = [plateau(cycles, rss, band, false)];
    }
    const withinBound =
      !inconclusive &&
      !secondShift &&
      rssChecks.every((check) => check.plateau) &&
      (heap === null || heap.plateau);
    members.push({
      pid,
      image: readings[0].image,
      role,
      heap,
      rss: rssChecks,
      levelShift,
      inconclusive,
      withinBound,
    });
  }

  const inconclusive = members.some((member) => member.inconclusive);
  const pass = members.every((member) => member.withinBound);
  const mib = (bytes: number) => `${(bytes / 1024 ** 2).toFixed(1)}MiB`;
  const describeCheck = (label: string, check: PlateauCheck) =>
    `${label} [${check.fromCycle}..${check.toCycle}] rise ${mib(check.riseOverSpan)} of ` +
    `${mib(check.band)} (${bytesPerCycleToKib(check.perCycle)}/cycle ± ` +
    `${bytesPerCycleToKib(check.standardError)})` +
    (check.plateau ? "" : check.withinBand ? " RISING" : " BREACH");
  const memberDetail = members
    .map((member) => {
      const parts = [
        ...(member.heap ? [describeCheck("heap", member.heap)] : []),
        ...member.rss.map((check) => describeCheck("rss", check)),
      ];
      const shift = member.levelShift
        ? ` level shift [${member.levelShift.fromCycle}..${member.levelShift.toCycle}] ` +
          `+${mib(member.levelShift.growthBytes)}${member.inconclusive ? " (post-shift plateau unproven: INCONCLUSIVE)" : ""}`
        : "";
      return (
        `${member.image ?? "process"}#${member.pid} (${member.role === "root" ? "server" : "child"})` +
        `${member.withinBound ? "" : " BREACH"}: ${parts.join(", ")}${shift}`
      );
    })
    .join("; ");
  return {
    observable: true,
    cyclesCompleted,
    minimumCycles,
    bands,
    segments,
    lateFromCycle: xs[0],
    lateBytesPerCycle,
    members,
    inconclusive,
    pass,
    detail:
      `late span [${xs[0]}..${cyclesCompleted}] over ${late.length} quiesced readings: ` +
      `${memberDetail}; whole tree ${bytesPerCycleToKib(lateBytesPerCycle)}/cycle; windows: ` +
      segments
        .map(
          (segment) =>
            `[${segment.fromCycle}..${segment.toCycle}] ` +
            `${bytesPerCycleToKib(segment.bytesPerCycle)}/cycle`,
        )
        .join(" "),
  };
}

/** Least-squares slope of `ys` over `xs` with its standard error from the residuals. */
export function leastSquares(
  xs: readonly number[],
  ys: readonly number[],
): { slope: number; standardError: number } {
  const n = xs.length;
  const meanX = xs.reduce((sum, x) => sum + x, 0) / n;
  const meanY = ys.reduce((sum, y) => sum + y, 0) / n;
  let covariance = 0;
  let variance = 0;
  for (let index = 0; index < n; index += 1) {
    covariance += (xs[index] - meanX) * (ys[index] - meanY);
    variance += (xs[index] - meanX) ** 2;
  }
  if (variance === 0) return { slope: 0, standardError: 0 };
  const slope = covariance / variance;
  const intercept = meanY - slope * meanX;
  let residuals = 0;
  for (let index = 0; index < n; index += 1) {
    residuals += (ys[index] - (intercept + slope * xs[index])) ** 2;
  }
  const standardError = n > 2 ? Math.sqrt(residuals / (n - 2) / variance) : 0;
  return { slope, standardError };
}

/**
 * Read `ys` over `xs` as a plateau against `band`. With `testTrend`, a slope
 * whose lower confidence bound clears zero is a breach even inside the band.
 */
export function plateau(
  xs: readonly number[],
  ys: readonly number[],
  band: number,
  testTrend: boolean,
): PlateauCheck {
  const { slope, standardError } = leastSquares(xs, ys);
  const span = xs[xs.length - 1] - xs[0];
  const riseOverSpan = slope * span;
  const withinBand = riseOverSpan <= band;
  const significantlyRising =
    testTrend && slope - PLATEAU_CONFIDENCE_STANDARD_ERRORS * standardError > 0;
  return {
    fromCycle: xs[0],
    toCycle: xs[xs.length - 1],
    readings: xs.length,
    perCycle: slope,
    standardError,
    riseOverSpan,
    band,
    withinBand,
    significantlyRising,
    plateau: withinBand && !significantlyRising,
  };
}

/**
 * A retained-object counter can alternate between two flat levels from one
 * checkpoint to the next without retaining anything: the semantic memo holds
 * a cycle's classification entries only when the semantic path answered the
 * hover before the provider did, and the close releases them either way. A
 * linear fit over such a series reads the ORDER of high and low readings as
 * a trend. So a series that splits, at its largest gap wider than the band,
 * into two groups of at least three readings is judged as the two plateaus
 * it is: each group must be flat on its own, and a drift riding on either
 * level still breaches. A drift alone never splits that way (its readings
 * climb in steps no wider than the band, or its largest gap isolates one
 * reading), so it is judged whole. Returns null when no such split exists.
 */
export function twoLevelPlateau(
  xs: readonly number[],
  ys: readonly number[],
  band: number,
): PlateauCheck | null {
  if (xs.length < 6) return null;
  const order = ys.map((_, index) => index).sort((a, b) => ys[a] - ys[b]);
  let cut = -1;
  let widest = band;
  for (let position = 1; position < order.length; position += 1) {
    const gap = ys[order[position]] - ys[order[position - 1]];
    if (gap > widest) {
      widest = gap;
      cut = position;
    }
  }
  if (cut < 3 || order.length - cut < 3) return null;
  const lowIndexes = order.slice(0, cut).sort((a, b) => a - b);
  const highIndexes = order.slice(cut).sort((a, b) => a - b);
  const fitOf = (indexes: readonly number[]) =>
    plateau(
      indexes.map((index) => xs[index]),
      indexes.map((index) => ys[index]),
      band,
      true,
    );
  const low = fitOf(lowIndexes);
  const high = fitOf(highIndexes);
  if (!low.plateau || !high.plateau) return null;
  const steeper = low.perCycle >= high.perCycle ? low : high;
  return {
    fromCycle: xs[0],
    toCycle: xs[xs.length - 1],
    readings: xs.length,
    perCycle: steeper.perCycle,
    standardError: steeper.standardError,
    riseOverSpan: steeper.riseOverSpan,
    band,
    withinBand: true,
    significantlyRising: false,
    plateau: true,
    levels: {
      low: ys[order[cut - 1]],
      high: ys[order[cut]],
      lowReadings: lowIndexes.length,
      highReadings: highIndexes.length,
    },
  };
}

/** Index of the largest positive increment in `ys` (0 when none is positive). */
function largestIncrement(ys: readonly number[]): number {
  let index = 0;
  for (let candidate = 1; candidate < ys.length; candidate += 1) {
    const growth = ys[candidate] - ys[candidate - 1];
    if (growth > 0 && (index === 0 || growth > ys[index] - ys[index - 1])) {
      index = candidate;
    }
  }
  return index;
}

function bytesPerCycleToKib(bytes: number): string {
  return `${(bytes / 1024).toFixed(1)}KiB`;
}

/** The retained-object counters whose per-cycle growth the lifetime verdict bounds. */
export const CHURN_RETENTION_COUNTERS = [
  "liveArtifacts",
  "retainedRetiredVersions",
  "liveRoots",
  "snapshotLeases",
  "carrierCandidates",
  "publicationLanes",
  "semanticNodes",
  "semanticNodeSlots",
  "semanticMemoEntries",
  "unresolvedReach",
  "relationProofs",
  "relateKeys",
  "unionViews",
  "shapeCacheEntries",
  "flowGraphs",
  "flowHashEntries",
  "flowLoweredEntries",
  "mapperFingerprints",
  "frameworkSurfaceEntries",
  "deferredReleases",
  "resolvedImportFacts",
  "componentMetaStates",
  "registeredSources",
] as const satisfies readonly (keyof RetentionReading)[];

export type ChurnRetentionCounter = (typeof CHURN_RETENTION_COUNTERS)[number];

/**
 * Counters bounded by a cap the server reports rather than flat: the
 * signature kernel's tables are append-only within an epoch and the epoch
 * is replaced once they pass the cap, so the count is a sawtooth whose
 * period is far longer than a lane. Such a counter breaches when any
 * reading exceeds its cap.
 */
export const CHURN_CAPPED_COUNTERS = [
  { counter: "signatureRecords", cap: "signatureRecordCap" },
] as const satisfies readonly { counter: keyof RetentionReading; cap: keyof RetentionReading }[];

/** One counter's late-span plateau. */
export interface ChurnRetentionTrend {
  readonly counter: ChurnRetentionCounter;
  readonly baseline: number;
  readonly final: number;
  readonly peak: number;
  /** The late-span fit, in objects per cycle. */
  readonly late: PlateauCheck;
  readonly withinBound: boolean;
}

/** The object-lifetime verdict: no retained-object counter keeps rising. */
export interface ChurnRetentionCheck {
  /** False when any checkpoint lacked a reading, or the run was too short. */
  readonly observable: boolean;
  readonly cyclesCompleted: number;
  /** Objects a counter's fitted late-span rise may reach before a trend counts. */
  readonly plateauObjects: number;
  readonly trends: readonly ChurnRetentionTrend[];
  /** Pressure refusals at the final reading; the standard corpus must show 0. */
  readonly pressureRefusals: number | null;
  readonly pass: boolean;
  readonly detail: string;
}

/** Default objects a counter's fitted late-span rise may reach before a trend counts. */
export const CHURN_RETENTION_PLATEAU_OBJECTS = 4;

/**
 * Decide the object-lifetime half of WSP6.1 ("measure process-tree memory AND
 * object lifetimes"): over the late span, no retained-object counter may show
 * a rising trend, and the aggregate account must have refused nothing for
 * pressure (WSP6.3: pressure outcomes stay explicit and must not occur on the
 * admitted standard corpus).
 *
 * Why objects and not only bytes: an RSS plateau can hide a slow object leak
 * behind allocator reuse for hundreds of cycles, and an RSS rise cannot say
 * which retained set is responsible. Each counter is read live from its owning
 * structure, so its trend names the retainer. A counter breaches when its
 * least-squares slope over the late span is significantly positive (its lower
 * confidence bound clears zero) AND its fitted rise exceeds `plateauObjects`:
 * an amortised sweep that leaves a few superseded versions behind between
 * sweeps is noise around a level, a retainer that keeps one object per
 * document version — or one per ten — is a trend the noise cannot explain,
 * at any run length.
 *
 * Refused (`observable: false, pass: false`, never a pass) when any
 * checkpoint carries no reading, the late span is too short, or a checkpoint
 * was not quiesced. An unreported retention is an UNAVAILABLE metric.
 */
export function decideChurnRetention(
  checkpoints: readonly ChurnCheckpoint[],
  options: { readonly plateauObjects?: number; readonly lateFromCycle?: number } = {},
): ChurnRetentionCheck {
  const plateauObjects = options.plateauObjects ?? CHURN_RETENTION_PLATEAU_OBJECTS;
  const cyclesCompleted =
    checkpoints.length > 0 ? checkpoints[checkpoints.length - 1].cyclesCompleted : 0;
  const unevaluated = (detail: string): ChurnRetentionCheck => ({
    observable: false,
    cyclesCompleted,
    plateauObjects,
    trends: [],
    pressureRefusals: null,
    pass: false,
    detail,
  });
  if (checkpoints.length < 2) {
    return unevaluated(
      `the retained-object trend needs a baseline and a later quiesced reading, got ${checkpoints.length} — NOT evaluated`,
    );
  }
  const missing = checkpoints.filter((checkpoint) => checkpoint.retention === null);
  if (missing.length > 0) {
    return unevaluated(
      `checkpoint(s) at cycle ${missing.map((c) => c.cyclesCompleted).join(", ")} carry no retention ` +
        "reading (the server reported none), so the retained-object trend is UNAVAILABLE — NOT evaluated",
    );
  }
  const unquiesced = checkpoints.filter((checkpoint) => !checkpoint.quiesced);
  if (unquiesced.length > 0) {
    return unevaluated(
      `checkpoint(s) at cycle ${unquiesced.map((c) => c.cyclesCompleted).join(", ")} were read ` +
        "without the host reaching quiescence, so the retained-object trend was NOT evaluated",
    );
  }
  const first = checkpoints[0];
  const last = checkpoints[checkpoints.length - 1];
  if (last.cyclesCompleted - first.cyclesCompleted <= 0) {
    return unevaluated(
      `the final reading at cycle ${last.cyclesCompleted} did not advance past the baseline ` +
        `(${first.cyclesCompleted}), so no per-cycle rate exists — NOT evaluated`,
    );
  }
  const midpoint =
    options.lateFromCycle ??
    first.cyclesCompleted + (last.cyclesCompleted - first.cyclesCompleted) / 2;
  const late = checkpoints.filter((checkpoint) => checkpoint.cyclesCompleted >= midpoint);
  if (late.length < CHURN_SLOPE_MIN_LATE_READINGS) {
    return unevaluated(
      `the late span (cycle ${Math.ceil(midpoint)} on) holds ${late.length} quiesced ` +
        `reading(s), below the ${CHURN_SLOPE_MIN_LATE_READINGS} a plateau needs — NOT evaluated`,
    );
  }
  const readings = checkpoints.map((checkpoint) => checkpoint.retention as RetentionReading);
  const lateXs = late.map((checkpoint) => checkpoint.cyclesCompleted);
  const trends: ChurnRetentionTrend[] = CHURN_RETENTION_COUNTERS.map((counter) => {
    const baseline = readings[0][counter];
    const final = readings[readings.length - 1][counter];
    const peak = Math.max(...readings.map((reading) => reading[counter]));
    const lateYs = late.map((checkpoint) => (checkpoint.retention as RetentionReading)[counter]);
    const whole = plateau(lateXs, lateYs, plateauObjects, true);
    const fit = whole.plateau ? whole : (twoLevelPlateau(lateXs, lateYs, plateauObjects) ?? whole);
    return {
      counter,
      baseline,
      final,
      peak,
      late: fit,
      withinBound: !(fit.significantlyRising && !fit.withinBand),
    };
  });
  const pressureRefusals = readings[readings.length - 1].refusalsPressure;
  const capped = CHURN_CAPPED_COUNTERS.map(({ counter, cap }) => {
    const peak = Math.max(...readings.map((reading) => reading[counter]));
    const bound = Math.min(...readings.map((reading) => reading[cap]));
    return { counter, peak, bound, withinBound: peak <= bound };
  });
  const breached = trends.filter((trend) => !trend.withinBound);
  const pass =
    breached.length === 0 && pressureRefusals === 0 && capped.every((c) => c.withinBound);
  return {
    observable: true,
    cyclesCompleted,
    plateauObjects,
    trends,
    pressureRefusals,
    pass,
    detail:
      `retained objects over the late span [${lateXs[0]}..${cyclesCompleted}], a trend counts past ` +
      `${plateauObjects} objects: ` +
      trends
        .map(
          (trend) =>
            `${trend.counter} ${trend.baseline}→${trend.final} (peak ${trend.peak}, late ` +
            `${trend.late.perCycle.toFixed(3)}±${trend.late.standardError.toFixed(3)}/cycle, ` +
            `rise ${trend.late.riseOverSpan.toFixed(1)}` +
            (trend.late.levels
              ? `, two levels ${trend.late.levels.low}×${trend.late.levels.lowReadings}/${trend.late.levels.high}×${trend.late.levels.highReadings}`
              : "") +
            `)${trend.withinBound ? "" : " BREACH"}`,
        )
        .join("; ") +
      capped
        .map(
          (c) => `; ${c.counter} peak ${c.peak} of cap ${c.bound}${c.withinBound ? "" : " BREACH"}`,
        )
        .join("") +
      `; pressure refusals=${pressureRefusals}${pressureRefusals === 0 ? "" : " BREACH"}`,
  };
}

/** Render a checkpoint's wire byte counts for a receipt line. */
export function describeWireBytes(
  wireBytes: { readonly inbound: number; readonly outbound: number } | undefined,
): string {
  if (wireBytes === undefined) return "wire: UNAVAILABLE (client does not count)";
  return `wire server→client=${bytesToMib(wireBytes.inbound)} client→server=${bytesToMib(wireBytes.outbound)}`;
}

/** Render one retention reading for a receipt line. */
function formatMicros(micros: number): string {
  return micros >= 1000 ? `${(micros / 1000).toFixed(1)}ms` : `${micros}us`;
}

export function describeRetentionReading(reading: RetentionReading | null): string {
  if (reading === null) return "retention: UNAVAILABLE (server reported none)";
  return (
    `artifacts=${reading.liveArtifacts} retired=${reading.retainedRetiredVersions} ` +
    `roots=${reading.liveRoots} leases=${reading.snapshotLeases} ` +
    `candidates=${reading.carrierCandidates} lanes=${reading.publicationLanes} ` +
    `nodes=${reading.semanticNodes}/${reading.semanticNodeSlots} memo=${reading.semanticMemoEntries} reach=${reading.unresolvedReach} ` +
    `proofs=${reading.relationProofs} relateKeys=${reading.relateKeys} unionViews=${reading.unionViews} shapes=${reading.shapeCacheEntries} ` +
    `flow=${reading.flowGraphs}/${reading.flowHashEntries}/${reading.flowLoweredEntries} mappers=${reading.mapperFingerprints} surfaces=${reading.frameworkSurfaceEntries} ` +
    `pinned=${bytesToMib(reading.pinnedBytes)} retainedBytes=${bytesToMib(reading.retainedBytes)} ` +
    `pressureRefusals=${reading.refusalsPressure} ` +
    `importFacts=${reading.resolvedImportFacts} metaStates=${reading.componentMetaStates} ` +
    `registeredSources=${reading.registeredSources} signatureRecords=${reading.signatureRecords} ` +
    `deferred=${reading.deferredReleases} releases=${reading.releasesApplied} ` +
    `releaseWaitMax=${formatMicros(reading.releaseWaitMaxMicros)} releaseMax=${formatMicros(reading.releaseElapsedMaxMicros)} ` +
    (reading.lastRelease
      ? `lastRelease={wait=${formatMicros(reading.lastRelease.waitMicros)} took=${formatMicros(reading.lastRelease.elapsedMicros)} ` +
        `scanned=${reading.lastRelease.nodesScanned} released=${reading.lastRelease.nodesReleased} ` +
        `slots=${reading.lastRelease.storageSlotsBefore}>${reading.lastRelease.storageSlotsAfter} memo=${reading.lastRelease.memoEntriesEvicted}} `
      : "lastRelease=none ") +
    `heapInUse=${reading.heapInUseBytes === null ? "n/a" : bytesToMib(reading.heapInUseBytes)}` +
    (reading.semanticMemoFamilies
      ? ` memoFamilies={${Object.entries(reading.semanticMemoFamilies)
          .filter(([, count]) => count > 0)
          .map(([family, count]) => `${family}:${count}`)
          .join(",")}}`
      : "")
  );
}

function bytesToMib(bytes: number): string {
  return `${(bytes / 1024 ** 2).toFixed(1)}MiB`;
}

/** What the churn lane produced, beyond the shared endurance receipt. */
export interface ChurnScenarioResult {
  readonly receipt: EnduranceReceipt;
  readonly growth: ChurnGrowthCheck;
  /** The WSP6-AC1 verdict: bounded retained-byte rate over every quiesced window. */
  readonly slope: ChurnSlopeCheck;
  /** The object-lifetime verdict: no retained-object counter grows per cycle. */
  readonly retention: ChurnRetentionCheck;
  /** Cycles run past the planned count to prove a post-shift plateau. */
  readonly extendedCycles: number;
  /** Every quiesced reading taken, baseline first. */
  readonly checkpoints: readonly ChurnCheckpoint[];
  readonly baseline: ProcessTreeRssSample;
  readonly final: ProcessTreeRssSample;
  readonly cyclesCompleted: number;
  /** True when EVERY checkpoint reached host quiescence before being read. */
  readonly quiescedAtBothCheckpoints: boolean;
}

export interface ChurnScenarioOptions {
  /** Server pid — the root of the measured process tree. */
  readonly serverPid: number;
  readonly fixture?: ChurnFixture;
  readonly cycles?: number;
  readonly warmupCycles?: number;
  /** Quiesced readings taken AFTER the baseline (default from config, min 2). */
  readonly windows?: number;
  /**
   * Cycles the slope verdict requires before it will evaluate at all. Defaults
   * to {@link CHURN_ACCEPTANCE_MIN_CYCLES}; a shorter smoke run may lower it and
   * gets an explicitly unevaluated verdict, never a pass.
   */
  readonly minimumCycles?: number;
}

/**
 * Wait for the host to go quiet, so a memory reading measures what the session
 * RETAINS rather than what it happens to be mid-flight on.
 *
 * Reuses the shared quiescence decision (host work counters unchanged over
 * consecutive polls, no scanner/drain/sync WARN churn in the window) — this
 * lane does not invent a second readiness rule, and never sleeps a fixed
 * duration and calls it quiet.
 */
async function quiesceHost(context: ScenarioContext, timeoutMs: number): Promise<boolean> {
  const client = context.session.client;
  const drainWarnLines = createWarnLineDrainer(client.stderr);
  drainWarnLines();
  const pollCounters = async (): Promise<QuiescenceCounters> =>
    extractQuiescenceCounters(
      await client.sendRequest(GET_STATISTICS_METHOD, {}, context.config.probeTimeoutMs),
    );
  const result = await pollUntilQuiesced(pollCounters, drainWarnLines, {
    intervalMs: 250,
    timeoutMs,
  });
  return result.quiesced;
}

/**
 * Read the host's retained object set, AFTER quiescence, so the figure is what
 * the session keeps rather than what it is mid-flight on. `null` when the
 * server reports no retention (labelled UNAVAILABLE downstream, never zero).
 */
async function readRetention(context: ScenarioContext): Promise<RetentionReading | null> {
  const snapshot: unknown = await context.session.client.sendRequest(
    GET_STATISTICS_METHOD,
    {},
    context.config.probeTimeoutMs,
  );
  return extractRetentionReading(snapshot);
}

/** One open → edit → query → close cycle over the churn document. */
async function runOneCycle(
  context: ScenarioContext,
  fixture: ChurnFixture,
  cycle: number,
  failures: FailureBag,
): Promise<void> {
  const { session } = context;
  session.openFile(fixture.churnPath, fixture.churnContent);
  session.changeFile(
    fixture.churnPath,
    fixture.churnContent.replace(
      "const churnHeadline = props.churnLabel.toUpperCase();",
      `const churnHeadline = props.churnLabel.toUpperCase() + "${cycle}";`,
    ),
  );
  // A real provider-backed request per cycle. It is also the backpressure: the
  // harness cannot outrun the server and read memory while a thousand unread
  // notifications are still queued, which would measure the QUEUE, not retention.
  const outcome = await context.session.runProbe(
    {
      kind: "hover",
      relativePath: fixture.churnPath,
      needle: fixture.hoverNeedle,
      cursorOffset: 4,
      expectIncludes: [],
      informational: true,
      label: `churn cycle ${cycle} hover`,
    },
    context.config.probeTimeoutMs,
  );
  if (outcome.classification !== "answered") {
    failures.add(`churn cycle ${cycle} hover settled as ${outcome.classification}`);
  }
  session.closeFile(fixture.churnPath);
}

export async function runChurnScenario(
  context: ScenarioContext,
  options: ChurnScenarioOptions,
): Promise<ChurnScenarioResult> {
  const fixture = options.fixture ?? churnFixture(context.config.churnCarrierBlocks, context.lane);
  const cycles = options.cycles ?? context.config.churnCycles;
  const warmupCycles = Math.min(options.warmupCycles ?? context.config.churnWarmupCycles, cycles);
  const failures = new FailureBag();
  const startedAtMs = Date.now();
  context.sampler?.start();
  try {
    // The consumer stays open for the whole run: a session that closed its last
    // document could reclaim by tearing the project down, which would prove
    // nothing about retention during editing.
    context.session.openFile(fixture.consumerPath);

    for (let cycle = 0; cycle < warmupCycles; cycle += 1) {
      await runOneCycle(context, fixture, cycle, failures);
    }
    const checkpoints: ChurnCheckpoint[] = [];
    const takeCheckpoint = async (cyclesCompleted: number): Promise<ChurnCheckpoint> => {
      const quiesced = await quiesceHost(context, context.config.churnQuiesceMs);
      if (!quiesced) {
        failures.add(
          `the host did not quiesce within ${context.config.churnQuiesceMs}ms before the ` +
            `reading at cycle ${cyclesCompleted}`,
        );
      }
      const checkpoint: ChurnCheckpoint = {
        cyclesCompleted,
        quiesced,
        sample: await sampleProcessTreeRss(options.serverPid),
        retention: await readRetention(context),
        wireBytes: context.session.client.wireBytes,
      };
      checkpoints.push(checkpoint);
      const hook = context.config.churnCheckpointHook;
      if (hook !== null && hook.cycle === cyclesCompleted) {
        // A profiler flush at a quiesced checkpoint: the profile then
        // describes the reading just taken, not a cycle in flight.
        writeFileSync(hook.path, `${cyclesCompleted}\n`);
        await new Promise((resolve) => setTimeout(resolve, hook.settleMs));
      }
      return checkpoint;
    };

    // The baseline, then one reading per measured window. Windows — not just two
    // endpoints — are what make a LINEAR leak visible: an envelope between two
    // points admits a constant per-cycle drip, a per-window rate does not.
    const baselineCheckpoint = await takeCheckpoint(warmupCycles);
    const windows = Math.max(2, options.windows ?? context.config.churnSlopeWindows);
    const measuredCycles = Math.max(0, cycles - warmupCycles);
    let completed = warmupCycles;
    for (let window = 1; window <= windows; window += 1) {
      const target = warmupCycles + Math.round((measuredCycles * window) / windows);
      for (; completed < target; completed += 1) {
        await runOneCycle(context, fixture, completed, failures);
      }
      await takeCheckpoint(completed);
    }
    // The verdicts read the late span from the PLANNED run's midpoint, so an
    // extension lengthens the span rather than moving it. A level shift too
    // close to the end leaves its post-shift plateau unproven: run on, one
    // window at a time, until it is proven or the extension budget is spent.
    const lateFromCycle = warmupCycles + measuredCycles / 2;
    const minimumCycles = options.minimumCycles ?? CHURN_ACCEPTANCE_MIN_CYCLES;
    const evaluate = () => ({
      slope: decideChurnSlope(checkpoints, {
        minimumCycles,
        bands: context.config.churnPlateauBands,
        lateFromCycle,
      }),
      retention: decideChurnRetention(checkpoints, {
        plateauObjects: context.config.churnRetentionPlateauObjects,
        lateFromCycle,
      }),
    });
    let verdicts = evaluate();
    const windowCycles = Math.max(1, Math.round(measuredCycles / windows));
    let extendedCycles = 0;
    while (
      verdicts.slope.observable &&
      verdicts.slope.inconclusive &&
      extendedCycles + windowCycles <= context.config.churnMaxExtensionCycles
    ) {
      const target = completed + windowCycles;
      for (; completed < target; completed += 1) {
        await runOneCycle(context, fixture, completed, failures);
      }
      await takeCheckpoint(completed);
      extendedCycles += windowCycles;
      verdicts = evaluate();
    }
    const finalCheckpoint = checkpoints[checkpoints.length - 1];
    const baselineQuiesced = baselineCheckpoint.quiesced;
    const finalQuiesced = finalCheckpoint.quiesced;
    const baseline = baselineCheckpoint.sample;
    const final = finalCheckpoint.sample;

    // WSP6.3: the memory figure is worthless if the session got there by
    // answering less. The session must still answer a strict, content-checked
    // request after the churn.
    context.session.openFile(fixture.churnPath, fixture.churnContent);
    const finalSanityPass = await convergeProbe(
      context,
      {
        kind: "hover",
        relativePath: fixture.churnPath,
        needle: fixture.hoverNeedle,
        cursorOffset: 4,
        expectIncludes: ["churnHeadline"],
        forbidIncludes: ["any"],
        label: "churn post-run sanity hover",
      },
      failures,
      { timeoutMs: context.config.probeTimeoutMs },
    );

    const growth = decideChurnGrowth(
      baseline,
      final,
      context.config.churnGrowthFactor,
      context.config.churnGrowthFloorBytes,
    );
    const { slope, retention } = verdicts;
    return {
      receipt: buildReceipt(context, startedAtMs, { finalSanityPass, failures: failures.list }),
      growth,
      slope,
      retention,
      checkpoints,
      baseline,
      final,
      cyclesCompleted: completed,
      extendedCycles,
      quiescedAtBothCheckpoints: checkpoints.every((checkpoint) => checkpoint.quiesced),
    };
  } finally {
    context.sampler?.stop();
  }
}
