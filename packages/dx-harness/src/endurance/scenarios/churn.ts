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
import {
  createWarnLineDrainer,
  GET_STATISTICS_METHOD,
} from "../../core/startupGate.js";
import {
  extractQuiescenceCounters,
  pollUntilQuiesced,
  type QuiescenceCounters,
} from "../../core/quiescence.js";
import { sampleProcessTreeRss, type ProcessTreeRssSample } from "../processTreeRss.js";
import {
  DEFAULT_ENDURANCE_LANE,
  type EnduranceLane,
  type EnduranceReceipt,
} from "../types.js";
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
    "  emit(\"churn\", churnHeadline);",
    "}",
    "</script>",
    "",
    "<template>",
    "  <section>",
    "    <h1 :title=\"churnHeadline\">{{ churnHeadline }}</h1>",
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
    "  <main :data-len=\"consumerLength\">",
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
  /** False ⇒ the metric is UNAVAILABLE on this host; never a silent pass. */
  readonly observable: boolean;
  readonly baselineBytes: number | null;
  readonly finalBytes: number | null;
  readonly growthBytes: number | null;
  /** `final / baseline`, or null when either reading is unavailable. */
  readonly ratio: number | null;
  readonly allowedFactor: number;
  readonly allowedFloorBytes: number;
  /** The largest `final` that passes: `baseline * factor + floor`. */
  readonly allowedBytes: number | null;
  readonly pass: boolean;
  readonly detail: string;
}

/**
 * Decide the growth verdict (pure — the arithmetic is unit-testable without a
 * server).
 *
 * An unobservable reading yields `pass: true` with `observable: false`: the
 * caller is required to report the metric as unavailable, which is the honest
 * outcome on a platform that cannot read process memory, and is distinct from
 * a measured pass.
 */
export function decideChurnGrowth(
  baseline: ProcessTreeRssSample,
  final: ProcessTreeRssSample,
  allowedFactor: number,
  allowedFloorBytes: number,
): ChurnGrowthCheck {
  const baselineBytes = baseline.observable ? baseline.totalBytes : null;
  const finalBytes = final.observable ? final.totalBytes : null;
  if (baselineBytes === null || finalBytes === null) {
    return {
      observable: false,
      baselineBytes,
      finalBytes,
      growthBytes: null,
      ratio: null,
      allowedFactor,
      allowedFloorBytes,
      allowedBytes: null,
      pass: true,
      detail:
        "process-tree resident memory is not readable on this host — the churn growth bound is UNAVAILABLE, not satisfied",
    };
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

function bytesToMib(bytes: number): string {
  return `${(bytes / 1024 ** 2).toFixed(1)}MiB`;
}

/** What the churn lane produced, beyond the shared endurance receipt. */
export interface ChurnScenarioResult {
  readonly receipt: EnduranceReceipt;
  readonly growth: ChurnGrowthCheck;
  readonly baseline: ProcessTreeRssSample;
  readonly final: ProcessTreeRssSample;
  readonly cyclesCompleted: number;
  /** True when BOTH checkpoints reached host quiescence before being read. */
  readonly quiescedAtBothCheckpoints: boolean;
}

export interface ChurnScenarioOptions {
  /** Server pid — the root of the measured process tree. */
  readonly serverPid: number;
  readonly fixture?: ChurnFixture;
  readonly cycles?: number;
  readonly warmupCycles?: number;
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
async function quiesceHost(
  context: ScenarioContext,
  timeoutMs: number,
): Promise<boolean> {
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
    const baselineQuiesced = await quiesceHost(context, context.config.churnQuiesceMs);
    const baseline = await sampleProcessTreeRss(options.serverPid);

    for (let cycle = warmupCycles; cycle < cycles; cycle += 1) {
      await runOneCycle(context, fixture, cycle, failures);
    }
    const finalQuiesced = await quiesceHost(context, context.config.churnQuiesceMs);
    const final = await sampleProcessTreeRss(options.serverPid);

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

    if (!baselineQuiesced) {
      failures.add(
        `the host did not quiesce within ${context.config.churnQuiesceMs}ms before the baseline reading`,
      );
    }
    if (!finalQuiesced) {
      failures.add(
        `the host did not quiesce within ${context.config.churnQuiesceMs}ms before the final reading`,
      );
    }

    const growth = decideChurnGrowth(
      baseline,
      final,
      context.config.churnGrowthFactor,
      context.config.churnGrowthFloorBytes,
    );
    return {
      receipt: buildReceipt(context, startedAtMs, { finalSanityPass, failures: failures.list }),
      growth,
      baseline,
      final,
      cyclesCompleted: cycles,
      quiescedAtBothCheckpoints: baselineQuiesced && finalQuiesced,
    };
  } finally {
    context.sampler?.stop();
  }
}
