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
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly semanticMemoEntries: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly unresolvedReach: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly relationProofs: number;
  /** Semantic-substrate retained objects (see the server's RetentionStatistics). */
  readonly relateKeys: number;
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
  /** Populated semantic memo slots per family label (reported, not bounded). */
  readonly semanticMemoFamilies?: Readonly<Record<string, number>>;
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
  "semanticMemoEntries",
  "unresolvedReach",
  "relationProofs",
  "relateKeys",
  "shapeCacheEntries",
  "flowGraphs",
  "flowHashEntries",
  "flowLoweredEntries",
  "mapperFingerprints",
  "frameworkSurfaceEntries",
  "pinnedBytes",
  "retainedBytes",
  "refusalsPressure",
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
  const reading: Partial<Record<keyof RetentionReading, number>> = {};
  for (const key of RETENTION_KEYS) {
    const value = record[key];
    if (typeof value !== "number" || !Number.isFinite(value)) return null;
    reading[key] = value;
  }
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
  /**
   * Whether this one window's rate is within the bound. Evidence, not the
   * verdict: one short window is dominated by allocator and GC noise (see
   * {@link decideChurnSlope}).
   */
  readonly withinBound: boolean;
}

/**
 * The single largest increment a late-span rate set aside as a one-time level
 * shift. On the tree-level figure it is attributed to the member that grew
 * most across it.
 */
export interface ChurnLevelShift {
  readonly fromCycle: number;
  readonly toCycle: number;
  readonly growthBytes: number;
  /** The member whose resident set grew most across the shift, if any grew. */
  readonly member: {
    readonly pid: number;
    readonly image: string | null;
    readonly growthBytes: number;
  } | null;
}

/** One process-tree member's late-span verdict. */
export interface ChurnMemberSlope {
  readonly pid: number;
  readonly image: string | null;
  /** `root`: the spawned server. `child`: a descendant (the type-provider engine). */
  readonly role: "root" | "child";
  readonly allowedBytesPerCycle: number;
  /** Least-squares rate over the late span, as measured. */
  readonly lateBytesPerCycle: number;
  /** This member's largest late increment, set aside as a one-time level shift. */
  readonly levelShift: ChurnLevelShift | null;
  /** The late-span rate with that level shift set aside: this member's verdict figure. */
  readonly verdictBytesPerCycle: number;
  readonly withinBound: boolean;
}

/** The retained-byte slope verdict over the quiesced trajectory. */
export interface ChurnSlopeCheck {
  readonly observable: boolean;
  readonly cyclesCompleted: number;
  readonly minimumCycles: number;
  /** The server's (tree root's) late-span bound. */
  readonly allowedBytesPerCycle: number;
  /** The bound for every descendant (the type-provider engine). */
  readonly allowedChildBytesPerCycle: number;
  /** Every consecutive window's whole-tree rate: the trajectory, reported as evidence. */
  readonly segments: readonly ChurnSlopeSegment[];
  /** The whole-tree rate over the final window. */
  readonly finalBytesPerCycle: number | null;
  /** First cycle of the late span the verdict is read over (the run's second half). */
  readonly lateFromCycle: number | null;
  /** Whole-tree least-squares rate over the late span, as measured (evidence). */
  readonly lateBytesPerCycle: number | null;
  /** The whole tree's largest late increment, set aside (evidence). */
  readonly levelShift: ChurnLevelShift | null;
  /** The whole-tree late-span rate with that level shift set aside (evidence). */
  readonly verdictBytesPerCycle: number | null;
  /** The verdict: every member's late-span rate against its own bound. */
  readonly members: readonly ChurnMemberSlope[];
  readonly pass: boolean;
  readonly detail: string;
}

/** Fewest quiesced readings the late span must hold for its slope to mean anything. */
export const CHURN_SLOPE_MIN_LATE_READINGS = 5;

/**
 * Decide WSP6-AC1's actual claim: after quiescence, 1000 open/edit/close cycles
 * show no UNBOUNDED retained-byte slope.
 *
 * Two quiesced endpoints and a `final <= baseline * factor + floor` envelope
 * cannot express that claim. At a 75 MiB baseline a 1.25 factor plus a 64 MiB
 * floor admits ~83 MiB of growth, so ~85 KiB retained on every post-warm-up
 * cycle — a strictly LINEAR leak, the exact shape the criterion forbids — lands
 * inside the envelope and reads as a pass. An envelope describes a destination;
 * the criterion is about the trajectory.
 *
 * What separates bounded from unbounded is what the trajectory does LATE, and
 * each process in the tree has its own bounded dynamics:
 *
 *  - The server settles. With every retained-object counter flat and its live
 *    heap flat (a heap profile over the whole run: ~40 MiB, first and last
 *    quarters within 1 MiB), its committed memory still climbs for several
 *    hundred cycles while the allocator reaches steady-state fragmentation,
 *    then levels off: ~15 MiB before the run's midpoint, a few MiB after it.
 *    An early window of that curve reads 60-90 KiB/cycle.
 *  - The type-provider child is a Go runtime whose resident set follows its
 *    garbage collector's heap goal, not its live objects. It sits in one of two
 *    plateaus (~95-100 MiB, ~124-129 MiB) and moves up once, at a random cycle,
 *    by 20-29 MiB, sometimes with a few MiB of drift around the move (4 of 11
 *    runs, anywhere from cycle ~150 to ~900), then holds for hundreds of
 *    cycles.
 *
 * A retainer, by contrast, keeps its rate to the end of the run. So each member
 * is judged on its own least-squares rate over the LATE span (every reading
 * from the run's midpoint on), with that member's single largest late
 * increment set aside as a possible one-time level shift, against its own
 * bound: the server against `allowedBytesPerCycle`, a child against
 * `allowedChildBytesPerCycle`, which leaves room for garbage-collector drift
 * and still sits below half of the ~150 KiB/cycle the provider child leaked
 * before this node closed its per-version documents. A linear retainer in
 * either process — one retained document version is hundreds of KiB — still
 * breaches with one increment removed; two level shifts in one member, or a
 * slope that survives the removal, breach too. The whole-tree late rate and
 * every window's whole-tree rate stay in the detail as evidence, and the
 * envelope still bounds the total growth any shift can add.
 *
 * Refused outright — `observable: false, pass: false`, never a pass — when:
 *
 *  - the run was shorter than `minimumCycles` (the criterion's own run length);
 *  - fewer than two post-baseline windows exist, so there is no trajectory to read;
 *  - the late span holds fewer than {@link CHURN_SLOPE_MIN_LATE_READINGS}
 *    readings, or a member appears in fewer of them than that;
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
    readonly allowedBytesPerCycle: number;
    readonly minimumCycles: number;
    readonly allowedChildBytesPerCycle?: number;
  },
): ChurnSlopeCheck {
  const { allowedBytesPerCycle, minimumCycles } = options;
  const allowedChildBytesPerCycle =
    options.allowedChildBytesPerCycle ?? CHURN_CHILD_SLOPE_BYTES_PER_CYCLE;
  const cyclesCompleted =
    checkpoints.length > 0 ? checkpoints[checkpoints.length - 1].cyclesCompleted : 0;
  const unevaluated = (detail: string): ChurnSlopeCheck => ({
    observable: false,
    cyclesCompleted,
    minimumCycles,
    allowedBytesPerCycle,
    allowedChildBytesPerCycle,
    segments: [],
    finalBytesPerCycle: null,
    lateFromCycle: null,
    lateBytesPerCycle: null,
    levelShift: null,
    verdictBytesPerCycle: null,
    members: [],
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
    const bytesPerCycle = growthBytes / cycles;
    segments.push({
      fromCycle: from.cyclesCompleted,
      toCycle: to.cyclesCompleted,
      cycles,
      growthBytes,
      bytesPerCycle,
      withinBound: bytesPerCycle <= allowedBytesPerCycle,
    });
  }

  // The late span: every reading from the run's midpoint on.
  const midpoint =
    checkpoints[0].cyclesCompleted + (cyclesCompleted - checkpoints[0].cyclesCompleted) / 2;
  const late = checkpoints.filter((checkpoint) => checkpoint.cyclesCompleted >= midpoint);
  if (late.length < CHURN_SLOPE_MIN_LATE_READINGS) {
    return unevaluated(
      `the late span (cycle ${Math.ceil(midpoint)} on) holds ${late.length} quiesced ` +
        `reading(s), below the ${CHURN_SLOPE_MIN_LATE_READINGS} its slope needs — raise ` +
        "VERTER_ENDURANCE_CHURN_SLOPE_WINDOWS; the slope was NOT evaluated",
    );
  }
  const xs = late.map((checkpoint) => checkpoint.cyclesCompleted);

  // Whole-tree evidence.
  const tree = slopeWithOneShiftSetAside(
    xs,
    late.map((checkpoint) => checkpoint.sample.totalBytes as number),
  );
  const treeShift: ChurnLevelShift | null =
    tree.shiftIndex > 0
      ? {
          fromCycle: xs[tree.shiftIndex - 1],
          toCycle: xs[tree.shiftIndex],
          growthBytes: tree.shiftBytes,
          member: largestMemberGrowth(
            late[tree.shiftIndex - 1].sample,
            late[tree.shiftIndex].sample,
          ),
        }
      : null;

  // The verdict: every member against its own bound.
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
          `${CHURN_SLOPE_MIN_LATE_READINGS} a slope needs — the slope was NOT evaluated`,
      );
    }
    const memberXs = readings.map((reading) => reading.cycle);
    const fit = slopeWithOneShiftSetAside(
      memberXs,
      readings.map((reading) => reading.bytes),
    );
    const role = pid === rootPid ? "root" : "child";
    const allowed = role === "root" ? allowedBytesPerCycle : allowedChildBytesPerCycle;
    members.push({
      pid,
      image: readings[0].image,
      role,
      allowedBytesPerCycle: allowed,
      lateBytesPerCycle: fit.measured,
      levelShift:
        fit.shiftIndex > 0
          ? {
              fromCycle: memberXs[fit.shiftIndex - 1],
              toCycle: memberXs[fit.shiftIndex],
              growthBytes: fit.shiftBytes,
              member: null,
            }
          : null,
      verdictBytesPerCycle: fit.verdict,
      withinBound: fit.verdict <= allowed,
    });
  }

  const pass = members.every((member) => member.withinBound);
  const finalBytesPerCycle = segments[segments.length - 1].bytesPerCycle;
  const mib = (bytes: number) => `${(bytes / 1024 ** 2).toFixed(1)}MiB`;
  const memberDetail = members
    .map(
      (member) =>
        `${member.image ?? "process"}#${member.pid} (${member.role === "root" ? "server" : "child"}) ` +
        `${bytesPerCycleToKib(member.verdictBytesPerCycle)}/cycle, allowed <=` +
        `${bytesPerCycleToKib(member.allowedBytesPerCycle)}/cycle` +
        (member.withinBound ? "" : " BREACH") +
        (member.levelShift
          ? ` (measured ${bytesPerCycleToKib(member.lateBytesPerCycle)}/cycle, level shift ` +
            `[${member.levelShift.fromCycle}..${member.levelShift.toCycle}] ` +
            `+${mib(member.levelShift.growthBytes)} set aside)`
          : ""),
    )
    .join("; ");
  return {
    observable: true,
    cyclesCompleted,
    minimumCycles,
    allowedBytesPerCycle,
    allowedChildBytesPerCycle,
    segments,
    finalBytesPerCycle,
    lateFromCycle: xs[0],
    lateBytesPerCycle: tree.measured,
    levelShift: treeShift,
    verdictBytesPerCycle: tree.verdict,
    members,
    pass,
    detail:
      `late span [${xs[0]}..${cyclesCompleted}] over ${late.length} quiesced readings: ` +
      `${memberDetail}; whole tree ${bytesPerCycleToKib(tree.measured)}/cycle` +
      (treeShift
        ? ` (${bytesPerCycleToKib(tree.verdict)}/cycle with ${describeShift(treeShift)} set aside)`
        : "") +
      "; windows: " +
      segments
        .map(
          (segment) =>
            `[${segment.fromCycle}..${segment.toCycle}] ` +
            `${bytesPerCycleToKib(segment.bytesPerCycle)}/cycle`,
        )
        .join(" "),
  };
}

/**
 * Default late-span bound for a descendant process (the type-provider engine):
 * room for its garbage collector's drift, below half the ~150 KiB/cycle the
 * provider leaked before per-version documents were closed.
 */
export const CHURN_CHILD_SLOPE_BYTES_PER_CYCLE = 64 * 1024;

/**
 * Least-squares rate of `ys` over `xs`, as measured and with the single
 * largest positive increment set aside (every reading after it lowered by it).
 */
function slopeWithOneShiftSetAside(
  xs: readonly number[],
  ys: readonly number[],
): { measured: number; verdict: number; shiftIndex: number; shiftBytes: number } {
  const measured = leastSquaresSlope(xs, ys);
  let shiftIndex = -1;
  for (let index = 1; index < ys.length; index += 1) {
    const growth = ys[index] - ys[index - 1];
    if (growth > 0 && (shiftIndex < 0 || growth > ys[shiftIndex] - ys[shiftIndex - 1])) {
      shiftIndex = index;
    }
  }
  if (shiftIndex < 0) return { measured, verdict: measured, shiftIndex, shiftBytes: 0 };
  const shiftBytes = ys[shiftIndex] - ys[shiftIndex - 1];
  const adjusted = ys.map((y, index) => (index >= shiftIndex ? y - shiftBytes : y));
  return { measured, verdict: leastSquaresSlope(xs, adjusted), shiftIndex, shiftBytes };
}

/** Ordinary least-squares slope of `ys` over `xs` (bytes per cycle). */
function leastSquaresSlope(xs: readonly number[], ys: readonly number[]): number {
  const n = xs.length;
  const meanX = xs.reduce((sum, x) => sum + x, 0) / n;
  const meanY = ys.reduce((sum, y) => sum + y, 0) / n;
  let covariance = 0;
  let variance = 0;
  for (let index = 0; index < n; index += 1) {
    covariance += (xs[index] - meanX) * (ys[index] - meanY);
    variance += (xs[index] - meanX) ** 2;
  }
  return variance === 0 ? 0 : covariance / variance;
}

/** The tree member whose resident set grew most between two readings. */
function largestMemberGrowth(
  from: ProcessTreeRssSample,
  to: ProcessTreeRssSample,
): ChurnLevelShift["member"] {
  const before = new Map(from.members.map((member) => [member.pid, member.rssBytes]));
  let best: ChurnLevelShift["member"] = null;
  for (const member of to.members) {
    const previous = before.get(member.pid);
    if (previous === undefined || previous === null || member.rssBytes === null) continue;
    const growthBytes = member.rssBytes - previous;
    if (growthBytes > 0 && (best === null || growthBytes > best.growthBytes)) {
      best = { pid: member.pid, image: member.image, growthBytes };
    }
  }
  return best;
}

function describeShift(shift: ChurnLevelShift): string {
  const mib = (bytes: number) => `${(bytes / 1024 ** 2).toFixed(1)}MiB`;
  const who = shift.member
    ? ` (${shift.member.image ?? "process"}#${shift.member.pid} ${mib(shift.member.growthBytes)})`
    : "";
  return `[${shift.fromCycle}..${shift.toCycle}] +${mib(shift.growthBytes)}${who}`;
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
  "semanticMemoEntries",
  "unresolvedReach",
  "relationProofs",
  "relateKeys",
  "shapeCacheEntries",
  "flowGraphs",
  "flowHashEntries",
  "flowLoweredEntries",
  "mapperFingerprints",
  "frameworkSurfaceEntries",
] as const satisfies readonly (keyof RetentionReading)[];

export type ChurnRetentionCounter = (typeof CHURN_RETENTION_COUNTERS)[number];

/** One counter's growth between the baseline and the final quiesced reading. */
export interface ChurnRetentionTrend {
  readonly counter: ChurnRetentionCounter;
  readonly baseline: number;
  readonly final: number;
  readonly peak: number;
  readonly perCycle: number;
  readonly withinBound: boolean;
}

/** The object-lifetime verdict: retained objects do not accumulate per cycle. */
export interface ChurnRetentionCheck {
  /** False when any checkpoint lacked a reading, or the run was too short. */
  readonly observable: boolean;
  readonly cyclesCompleted: number;
  readonly allowedObjectsPerCycle: number;
  readonly trends: readonly ChurnRetentionTrend[];
  /** Pressure refusals at the final reading; the standard corpus must show 0. */
  readonly pressureRefusals: number | null;
  readonly pass: boolean;
  readonly detail: string;
}

/**
 * Decide the object-lifetime half of WSP6.1 ("measure process-tree memory AND
 * object lifetimes"): across the quiesced checkpoints, no retained-object
 * counter may grow in proportion to the cycles run, and the aggregate account
 * must have refused nothing for pressure (WSP6.3: pressure outcomes stay
 * explicit and must not occur on the admitted standard corpus).
 *
 * Why objects and not only bytes: an RSS plateau can hide a slow object leak
 * behind allocator reuse for hundreds of cycles, and an RSS rise cannot say
 * which retained set is responsible. Each counter is read live from its owning
 * structure, so its trend names the retainer. The bound is per cycle so a
 * counter that is legitimately amortised (an occasional sweep leaves a few
 * superseded versions behind until the next one) still passes, while a
 * counter that keeps one object per superseded document version — one or more
 * per cycle — cannot.
 *
 * Refused (`observable: false, pass: false`, never a pass) when any
 * checkpoint carries no reading, the readings span no cycles, or a checkpoint
 * was not quiesced. An unreported retention is an UNAVAILABLE metric.
 */
export function decideChurnRetention(
  checkpoints: readonly ChurnCheckpoint[],
  options: { readonly allowedObjectsPerCycle: number },
): ChurnRetentionCheck {
  const { allowedObjectsPerCycle } = options;
  const cyclesCompleted =
    checkpoints.length > 0 ? checkpoints[checkpoints.length - 1].cyclesCompleted : 0;
  const unevaluated = (detail: string): ChurnRetentionCheck => ({
    observable: false,
    cyclesCompleted,
    allowedObjectsPerCycle,
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
  const cycles = last.cyclesCompleted - first.cyclesCompleted;
  if (cycles <= 0) {
    return unevaluated(
      `the final reading at cycle ${last.cyclesCompleted} did not advance past the baseline ` +
        `(${first.cyclesCompleted}), so no per-cycle rate exists — NOT evaluated`,
    );
  }
  const readings = checkpoints.map((checkpoint) => checkpoint.retention as RetentionReading);
  const trends: ChurnRetentionTrend[] = CHURN_RETENTION_COUNTERS.map((counter) => {
    const baseline = readings[0][counter];
    const final = readings[readings.length - 1][counter];
    const peak = Math.max(...readings.map((reading) => reading[counter]));
    const perCycle = (final - baseline) / cycles;
    return {
      counter,
      baseline,
      final,
      peak,
      perCycle,
      withinBound: perCycle <= allowedObjectsPerCycle,
    };
  });
  const pressureRefusals = readings[readings.length - 1].refusalsPressure;
  const breached = trends.filter((trend) => !trend.withinBound);
  const pass = breached.length === 0 && pressureRefusals === 0;
  return {
    observable: true,
    cyclesCompleted,
    allowedObjectsPerCycle,
    trends,
    pressureRefusals,
    pass,
    detail:
      `retained objects over ${cycles} measured cycles, allowed <=${allowedObjectsPerCycle}/cycle: ` +
      trends
        .map(
          (trend) =>
            `${trend.counter} ${trend.baseline}→${trend.final} (peak ${trend.peak}, ` +
            `${trend.perCycle.toFixed(3)}/cycle)${trend.withinBound ? "" : " BREACH"}`,
        )
        .join("; ") +
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
export function describeRetentionReading(reading: RetentionReading | null): string {
  if (reading === null) return "retention: UNAVAILABLE (server reported none)";
  return (
    `artifacts=${reading.liveArtifacts} retired=${reading.retainedRetiredVersions} ` +
    `roots=${reading.liveRoots} leases=${reading.snapshotLeases} ` +
    `candidates=${reading.carrierCandidates} lanes=${reading.publicationLanes} ` +
    `nodes=${reading.semanticNodes} memo=${reading.semanticMemoEntries} reach=${reading.unresolvedReach} ` +
    `proofs=${reading.relationProofs} relateKeys=${reading.relateKeys} shapes=${reading.shapeCacheEntries} ` +
    `flow=${reading.flowGraphs}/${reading.flowHashEntries}/${reading.flowLoweredEntries} mappers=${reading.mapperFingerprints} surfaces=${reading.frameworkSurfaceEntries} ` +
    `pinned=${bytesToMib(reading.pinnedBytes)} retainedBytes=${bytesToMib(reading.retainedBytes)} ` +
    `pressureRefusals=${reading.refusalsPressure}` +
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
    const slope = decideChurnSlope(checkpoints, {
      allowedBytesPerCycle: context.config.churnSlopeBytesPerCycle,
      allowedChildBytesPerCycle: context.config.churnSlopeChildBytesPerCycle,
      minimumCycles: options.minimumCycles ?? CHURN_ACCEPTANCE_MIN_CYCLES,
    });
    const retention = decideChurnRetention(checkpoints, {
      allowedObjectsPerCycle: context.config.churnRetentionObjectsPerCycle,
    });
    return {
      receipt: buildReceipt(context, startedAtMs, { finalSanityPass, failures: failures.list }),
      growth,
      slope,
      retention,
      checkpoints,
      baseline,
      final,
      cyclesCompleted: completed,
      quiescedAtBothCheckpoints: checkpoints.every((checkpoint) => checkpoint.quiesced),
    };
  } finally {
    context.sampler?.stop();
  }
}
