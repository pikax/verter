/**
 * Env-driven configuration for the endurance harness.
 *
 * Every knob has a CI-runnable default; longer/stronger runs are opt-in via
 * the environment so the default lane stays bounded.
 *
 *  - VERTER_ENDURANCE_PROVIDER          tsserver | tsgo | shared-tsgo (default tsgo)
 *  - VERTER_ENDURANCE_REQUEST_TIMEOUT_MS  per-request timeout for storm/soak traffic (default 10000)
 *  - VERTER_ENDURANCE_PROBE_TIMEOUT_MS    per-request timeout for strict probes (default 30000)
 *  - VERTER_ENDURANCE_PROBE_LATENCY_BOUND_MS  hard per-probe latency bound (default 5000)
 *  - VERTER_ENDURANCE_P95_MAX_MS          soak/scale absolute p95 bound (default per-route: 2000
 *                                          tsgo/shared-tsgo, 5000 tsserver — grounded serial-engine ceiling)
 *  - VERTER_ENDURANCE_STORM_P95_MAX_MS    storm p95 bound (default per-route: 2000 tsgo/shared-tsgo,
 *                                          5000 tsserver — single-threaded engine capacity)
 *  - VERTER_ENDURANCE_DEGRADATION_FACTOR  late/early p95 trend factor (default 1.5)
 *  - VERTER_ENDURANCE_DEGRADATION_FLOOR_MS degradation noise floor (default 250 — a fail
 *                                            requires ratio > factor AND delta > floor)
 *  - VERTER_ENDURANCE_WINDOW_MS           latency trend window (default 30000)
 *  - VERTER_ENDURANCE_MAX_IN_FLIGHT       harness-side in-flight cap (default 8)
 *  - VERTER_ENDURANCE_RSS_MAX_BYTES       verter-lsp RSS ceiling (default 4 GiB)
 *  - VERTER_ENDURANCE_RSS_SAMPLE_MS       RSS poll interval (default 2000)
 *  - VERTER_ENDURANCE_HEAVY_UPDATE_CYCLES edit→query cycles (default 200)
 *  - VERTER_ENDURANCE_STORM_MS            storm duration (default 20000)
 *  - VERTER_ENDURANCE_STORM_WORKERS       storm worker count (default 8)
 *  - VERTER_ENDURANCE_SOAK_MS             soak duration (default 150000)
 *  - VERTER_ENDURANCE_TYPING_CPS          didChange/s typing cadence (default 12, human-realistic;
 *                                          ~80 probes the superhuman ceiling — informational only)
 *  - VERTER_ENDURANCE_CORPUS_DIR          external corpus for the scale lane (read-only)
 *  - VERTER_ENDURANCE_SYNTHETIC_SCALE     "1" → generate a synthetic corpus for the scale lane
 *  - VERTER_ENDURANCE_SCALE_OPEN_FILES    files to open in the scale lane (default 40)
 *  - VERTER_ENDURANCE_SCALE_CORPUS_FILES  synthetic corpus size (default 300)
 *  - VERTER_ENDURANCE_CHURN_CYCLES        open/edit/query/close cycles in the churn lane (default 1000)
 *  - VERTER_ENDURANCE_CHURN_WARMUP_CYCLES cycles before the baseline memory reading (default 100)
 *  - VERTER_ENDURANCE_CHURN_CARRIER_BLOCKS generated churn-carrier size in blocks (default 140)
 *  - VERTER_ENDURANCE_CHURN_GROWTH_FACTOR  final/baseline tree-RSS bound (default 1.25)
 *  - VERTER_ENDURANCE_CHURN_GROWTH_FLOOR_BYTES absolute churn growth slack (default 64 MiB)
 *  - VERTER_ENDURANCE_CHURN_QUIESCE_MS    quiescence budget before each churn reading (default 60000)
 *  - VERTER_ENDURANCE_CHURN_SLOPE_WINDOWS  quiesced readings after the baseline (default 18, min 2)
 *  - VERTER_ENDURANCE_CHURN_HEAP_PLATEAU_BYTES   server exact-heap rise band over the late span (default 2 MiB)
 *  - VERTER_ENDURANCE_CHURN_RSS_SETTLING_BYTES   server resident-set rise band over the late span (default 8 MiB)
 *  - VERTER_ENDURANCE_CHURN_CHILD_PLATEAU_BYTES  child resident-set rise band per plateau segment (default 8 MiB)
 *  - VERTER_ENDURANCE_CHURN_SHIFT_BYTES          smallest single-window increment read as a level shift (default 8 MiB)
 *  - VERTER_ENDURANCE_CHURN_MIN_PLATEAU_READINGS readings a plateau segment needs (default 5)
 *  - VERTER_ENDURANCE_CHURN_MAX_EXTENSION_CYCLES cycles the run may extend to prove a post-shift plateau (default 400)
 *  - VERTER_ENDURANCE_CHURN_RETENTION_PLATEAU_OBJECTS objects a counter's late rise may reach before a trend counts (default 4)
 *  - VERTER_ENDURANCE_RECEIPT             receipt destination (a `.json` file, or a directory)
 */
import {
  ENDURANCE_PROVIDER_ROUTES,
  type EnduranceConfig,
  type EnduranceProviderRoute,
} from "./types.js";

function readInt(
  env: NodeJS.ProcessEnv,
  name: string,
  fallback: number,
  { min }: { min: number },
): number {
  const raw = env[name];
  if (raw === undefined || raw === "") return fallback;
  const value = Number(raw);
  if (!Number.isFinite(value) || value < min) {
    throw new Error(`${name} must be a number >= ${min}, got ${JSON.stringify(raw)}`);
  }
  return Math.floor(value);
}

function readNumber(
  env: NodeJS.ProcessEnv,
  name: string,
  fallback: number,
  { min }: { min: number },
): number {
  const raw = env[name];
  if (raw === undefined || raw === "") return fallback;
  const value = Number(raw);
  if (!Number.isFinite(value) || value < min) {
    throw new Error(`${name} must be a number >= ${min}, got ${JSON.stringify(raw)}`);
  }
  return value;
}

function readRoute(env: NodeJS.ProcessEnv): EnduranceProviderRoute {
  const raw = env.VERTER_ENDURANCE_PROVIDER ?? "tsgo";
  if (!(ENDURANCE_PROVIDER_ROUTES as readonly string[]).includes(raw)) {
    throw new Error(
      `VERTER_ENDURANCE_PROVIDER must be one of ${ENDURANCE_PROVIDER_ROUTES.join(", ")}, got ${JSON.stringify(raw)}`,
    );
  }
  return raw as EnduranceProviderRoute;
}

/** Resolve the endurance configuration from the environment (defaults are CI-sized). */
/** `<cycle>:<path>[:<settleMs>]`; a Windows drive letter in `path` is kept whole. */
function readCheckpointHook(raw: string | undefined): EnduranceConfig["churnCheckpointHook"] {
  if (!raw || raw.length === 0) return null;
  const first = raw.indexOf(":");
  if (first <= 0)
    throw new Error(`VERTER_ENDURANCE_CHURN_CHECKPOINT_HOOK wants <cycle>:<path>, got ${raw}`);
  const cycle = Number(raw.slice(0, first));
  let rest = raw.slice(first + 1);
  let settleMs = 8000;
  const tail = /:(\d+)$/.exec(rest);
  if (tail && rest.length - tail[0].length > 1) {
    settleMs = Number(tail[1]);
    rest = rest.slice(0, rest.length - tail[0].length);
  }
  if (!Number.isInteger(cycle) || cycle < 0 || rest.length === 0) {
    throw new Error(`VERTER_ENDURANCE_CHURN_CHECKPOINT_HOOK wants <cycle>:<path>, got ${raw}`);
  }
  return { cycle, path: rest, settleMs };
}

export function loadEnduranceConfig(env: NodeJS.ProcessEnv = process.env): EnduranceConfig {
  const route = readRoute(env);
  const corpusDirRaw = env.VERTER_ENDURANCE_CORPUS_DIR;
  return {
    route,
    requestTimeoutMs: readInt(env, "VERTER_ENDURANCE_REQUEST_TIMEOUT_MS", 10_000, { min: 100 }),
    probeTimeoutMs: readInt(env, "VERTER_ENDURANCE_PROBE_TIMEOUT_MS", 30_000, { min: 100 }),
    probeLatencyBoundMs: readInt(env, "VERTER_ENDURANCE_PROBE_LATENCY_BOUND_MS", 5_000, {
      min: 1,
    }),
    p95MaxMs: readInt(env, "VERTER_ENDURANCE_P95_MAX_MS", route === "tsserver" ? 5_000 : 2_000, {
      min: 1,
    }),
    stormP95MaxMs: readInt(
      env,
      "VERTER_ENDURANCE_STORM_P95_MAX_MS",
      route === "tsserver" ? 5_000 : 2_000,
      { min: 1 },
    ),
    scaleStormP95MaxMs: readInt(
      env,
      "VERTER_ENDURANCE_SCALE_STORM_P95_MAX_MS",
      route === "tsserver" ? 8_000 : 2_000,
      { min: 1 },
    ),
    degradationFactor: readNumber(env, "VERTER_ENDURANCE_DEGRADATION_FACTOR", 1.5, { min: 1 }),
    degradationFloorMs: readInt(env, "VERTER_ENDURANCE_DEGRADATION_FLOOR_MS", 250, { min: 1 }),
    windowMs: readInt(env, "VERTER_ENDURANCE_WINDOW_MS", 30_000, { min: 1000 }),
    maxInFlight: readInt(env, "VERTER_ENDURANCE_MAX_IN_FLIGHT", 8, { min: 1 }),
    rssMaxBytes: readInt(env, "VERTER_ENDURANCE_RSS_MAX_BYTES", 4 * 1024 ** 3, { min: 1 }),
    rssSampleMs: readInt(env, "VERTER_ENDURANCE_RSS_SAMPLE_MS", 2_000, { min: 100 }),
    heavyUpdateCycles: readInt(env, "VERTER_ENDURANCE_HEAVY_UPDATE_CYCLES", 200, { min: 1 }),
    stormDurationMs: readInt(env, "VERTER_ENDURANCE_STORM_MS", 20_000, { min: 500 }),
    stormWorkers: readInt(env, "VERTER_ENDURANCE_STORM_WORKERS", 8, { min: 1 }),
    soakDurationMs: readInt(env, "VERTER_ENDURANCE_SOAK_MS", 150_000, { min: 1000 }),
    typingCps: readInt(env, "VERTER_ENDURANCE_TYPING_CPS", 12, { min: 1 }),
    corpusDir: corpusDirRaw && corpusDirRaw.length > 0 ? corpusDirRaw : null,
    syntheticScale: env.VERTER_ENDURANCE_SYNTHETIC_SCALE === "1",
    scaleOpenFiles: readInt(env, "VERTER_ENDURANCE_SCALE_OPEN_FILES", 40, { min: 1 }),
    scaleCorpusFiles: readInt(env, "VERTER_ENDURANCE_SCALE_CORPUS_FILES", 300, { min: 2 }),
    churnCycles: readInt(env, "VERTER_ENDURANCE_CHURN_CYCLES", 1_000, { min: 2 }),
    churnWarmupCycles: readInt(env, "VERTER_ENDURANCE_CHURN_WARMUP_CYCLES", 100, { min: 1 }),
    churnCarrierBlocks: readInt(env, "VERTER_ENDURANCE_CHURN_CARRIER_BLOCKS", 140, { min: 1 }),
    churnGrowthFactor: readNumber(env, "VERTER_ENDURANCE_CHURN_GROWTH_FACTOR", 1.25, { min: 1 }),
    churnGrowthFloorBytes: readInt(
      env,
      "VERTER_ENDURANCE_CHURN_GROWTH_FLOOR_BYTES",
      64 * 1024 ** 2,
      { min: 1 },
    ),
    churnQuiesceMs: readInt(env, "VERTER_ENDURANCE_CHURN_QUIESCE_MS", 60_000, { min: 1000 }),
    churnSlopeWindows: readInt(env, "VERTER_ENDURANCE_CHURN_SLOPE_WINDOWS", 18, { min: 2 }),
    churnPlateauBands: {
      heapPlateauBytes: readInt(env, "VERTER_ENDURANCE_CHURN_HEAP_PLATEAU_BYTES", 2 * 1024 ** 2, {
        min: 1,
      }),
      rssSettlingBytes: readInt(env, "VERTER_ENDURANCE_CHURN_RSS_SETTLING_BYTES", 8 * 1024 ** 2, {
        min: 1,
      }),
      childPlateauBytes: readInt(env, "VERTER_ENDURANCE_CHURN_CHILD_PLATEAU_BYTES", 8 * 1024 ** 2, {
        min: 1,
      }),
      shiftBytes: readInt(env, "VERTER_ENDURANCE_CHURN_SHIFT_BYTES", 8 * 1024 ** 2, { min: 1 }),
      minPlateauReadings: readInt(env, "VERTER_ENDURANCE_CHURN_MIN_PLATEAU_READINGS", 5, {
        min: 2,
      }),
    },
    churnMaxExtensionCycles: readInt(env, "VERTER_ENDURANCE_CHURN_MAX_EXTENSION_CYCLES", 400, {
      min: 0,
    }),
    churnRetentionPlateauObjects: readInt(
      env,
      "VERTER_ENDURANCE_CHURN_RETENTION_PLATEAU_OBJECTS",
      4,
      { min: 0 },
    ),
    churnCheckpointHook: readCheckpointHook(env.VERTER_ENDURANCE_CHURN_CHECKPOINT_HOOK),
    receiptPath:
      env.VERTER_ENDURANCE_RECEIPT && env.VERTER_ENDURANCE_RECEIPT.length > 0
        ? env.VERTER_ENDURANCE_RECEIPT
        : null,
  };
}
