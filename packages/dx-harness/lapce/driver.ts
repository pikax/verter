/**
 * WSP1L `LapceInteractionDriver`: open, type, complete, navigate, close and
 * teardown against a Lapce host, capturing UI event-loop / decode / apply /
 * paint timings correlated with the WSP1 server timeline. The driver refuses to
 * certify a protocol-smoke-only run (WSP1L-AC3) and never treats a fixture as a
 * real-client paint claim.
 */

import type { InteractionTrace } from "@verter/lsp-test-client";
import { buildUiTimeline, type ServerImmediacyBound, type StallThreshold } from "./timeline.js";
import { versionsMatchPinnedManifest } from "./manifest.js";
import {
  LAPCE_FIXTURE_HOST,
  LAPCE_REAL_HOST,
  isRejectedLapceUiHost,
  type AutomationPath,
  type LapceHostKind,
  type LapceUiRun,
  type LapceVersionManifest,
  type RealClientClaimVerdict,
  type Certification,
  type ScriptedStep,
  type ScriptedStepKind,
  type UiInteractionRecord,
  type UiStage,
  type UiStageStamp,
  type UiTimeline,
} from "./types.js";

/** WSP1L.1 record for the hermetic host: deterministic emission, no GUI, no sleeps. */
export const FIXTURE_AUTOMATION_PATH: AutomationPath = {
  kind: "instrumented-fixture-host",
  perturbation:
    "deterministic scripted UI event emission; no GUI, no sleeps, no real Lapce process",
  recordedAs: "WSP1L.1 fixture path",
};

/** WSP1L.1 record for the real-host path this driver binds (volt launch stamp). */
export const REAL_LAPCE_AUTOMATION_PATH: AutomationPath = {
  kind: "volt-launch-stamp + Lapce UI instrumentation",
  perturbation:
    "one-time initialize launch stamp behind uiTrace.enabled (default off); the per-message LSP path stays untouched",
  recordedAs: "WSP1L.1 real-host path",
};

/** WSP1L.3: missing GUI instrumentation is recorded as unavailable, not simulated. */
export const GUI_INSTRUMENTATION_UNAVAILABLE_REASON =
  "real-Lapce GUI instrumentation on the reference-client machine class is not captured yet " +
  "(reference-machine-manifests.v1 population is empty-at-ratification); recorded unavailable, not simulated";

export interface LapceHost {
  readonly hostKind: LapceHostKind;
  readonly automationPath: AutomationPath;
  readonly usedSleepForReadiness: boolean;
  /** Drive one scripted step and return its raw UI instrumentation. */
  runScriptedStep(step: ScriptedStep): UiInteractionRecord;
}

export interface FixtureLapceHostOptions {
  /** Deterministic monotonic clock; defaults to an internal sequence starting at 0. */
  readonly clockMs?: () => number;
  /** Per-stage deltas from the clock value when the step starts. */
  readonly stageDeltas?: Partial<Record<UiStage, number>>;
  /** Inject a deliberate UI event-loop stall after a stage (WSP1L-AC1). */
  readonly stall?: { readonly afterStage: UiStage; readonly ms: number };
  /** Omit stages to prove unknown-metric handling (never guessed zeros). */
  readonly omitStages?: readonly UiStage[];
}

/**
 * Hermetic deterministic Lapce host. It is NOT a real Lapce build and never
 * claims to be: its runs record `hostKind: instrumented-fixture` and stay
 * inadmissible as real-client paint evidence.
 */
export class FixtureLapceHost implements LapceHost {
  readonly hostKind = LAPCE_FIXTURE_HOST;
  readonly automationPath = FIXTURE_AUTOMATION_PATH;
  readonly usedSleepForReadiness = false;
  readonly #clockMs: () => number;
  readonly #stageDeltas: Partial<Record<UiStage, number>>;
  readonly #stall: FixtureLapceHostOptions["stall"];
  readonly #omitStages: readonly UiStage[];

  constructor(options: FixtureLapceHostOptions = {}) {
    this.#clockMs =
      options.clockMs ??
      (() => {
        throw new Error("FixtureLapceHost requires an explicit deterministic clockMs");
      });
    this.#stageDeltas = options.stageDeltas ?? {
      input_dispatched: 0,
      decoded: 4,
      applied: 8,
      painted: 12,
    };
    this.#stall = options.stall;
    this.#omitStages = options.omitStages ?? [];
  }

  runScriptedStep(step: ScriptedStep): UiInteractionRecord {
    const start = this.#clockMs();
    const stamps: UiStageStamp[] = [];
    let extra = 0;
    const stages: UiStage[] = ["input_dispatched", "decoded", "applied", "painted"];
    for (const stage of stages) {
      if (this.#omitStages.includes(stage)) continue;
      const delta = this.#stageDeltas[stage];
      if (delta === undefined) continue;
      stamps.push({ stage, atMs: start + delta + extra });
      // The stall delays every stage AFTER the named one, never the stage itself.
      if (this.#stall?.afterStage === stage) extra += this.#stall.ms;
    }
    return { step, stamps };
  }
}

export interface LapceInteractionDriverOptions {
  readonly host: LapceHost;
  /** WSP1 server traces keyed by request epoch for correlation (WSP1L.2). */
  readonly serverTraces: readonly InteractionTrace[];
  readonly versions: LapceVersionManifest;
  readonly stallThreshold: StallThreshold;
  readonly immediacyBound: ServerImmediacyBound;
  readonly protocolSmokePassed: boolean;
  readonly completenessState?: LapceUiRun["completenessState"];
  readonly receiptBasis: LapceUiRun["receiptBasis"];
}

/**
 * Drives a scripted interaction (open / type / complete / navigate / close)
 * against a Lapce host and captures the correlated UI timeline per step.
 */
export class LapceInteractionDriver {
  readonly #options: LapceInteractionDriverOptions;
  readonly #timelines: UiTimeline[] = [];
  #tornDown = false;

  constructor(options: LapceInteractionDriverOptions) {
    this.#options = options;
  }

  open(label?: string): UiTimeline {
    return this.#step("open", label);
  }

  type(label?: string): UiTimeline {
    return this.#step("type", label);
  }

  complete(label?: string): UiTimeline {
    return this.#step("complete", label);
  }

  navigate(label?: string): UiTimeline {
    return this.#step("navigate", label);
  }

  close(label?: string): UiTimeline {
    return this.#step("close", label);
  }

  /** Run a full script and return the certified-able run record. */
  runScriptedInteraction(script: readonly ScriptedStep[]): LapceUiRun {
    for (const step of script) this.#step(step.kind, step.label);
    return this.toRun();
  }

  /** The run record for everything driven so far (certification is separate). */
  toRun(): LapceUiRun {
    return {
      schema: "lapce-ui-run.v1",
      hostKind: this.#options.host.hostKind,
      automationPath: this.#options.host.automationPath,
      usedSleepForReadiness: this.#options.host.usedSleepForReadiness,
      versions: this.#options.versions,
      protocolSmokePassed: this.#options.protocolSmokePassed,
      timelines: [...this.#timelines],
      receiptBasis: this.#options.receiptBasis,
      completenessState: this.#options.completenessState ?? "partial",
    };
  }

  /** Release the host; idempotent. Any further step fails loud. */
  teardown(): void {
    this.#tornDown = true;
  }

  #step(kind: ScriptedStepKind, label?: string): UiTimeline {
    if (this.#tornDown) {
      throw new Error("LapceInteractionDriver is torn down; no further steps may run");
    }
    const epoch = this.#nextEpoch();
    const step: ScriptedStep = { kind, label, requestEpoch: epoch, sourceEpoch: epoch };
    const record = this.#options.host.runScriptedStep(step);
    const trace = this.#options.serverTraces.find((candidate) => candidate.requestEpoch === epoch);
    if (trace === undefined) {
      throw new Error(
        `no WSP1 server InteractionTrace for request epoch ${epoch}; ` +
          `UI timestamps cannot be correlated without the server timeline (WSP1L.2)`,
      );
    }
    const timeline = buildUiTimeline(record, trace, {
      stallThreshold: this.#options.stallThreshold,
      immediacyBound: this.#options.immediacyBound,
    });
    this.#timelines.push(timeline);
    return timeline;
  }

  #nextEpoch(): number {
    return this.#timelines.length + 1;
  }
}

/**
 * WSP1L-AC3 and friends: certify a run record. Refusals are explicit — a
 * protocol smoke that passed with no UI timeline is never certified.
 */
export function certifyRun(run: LapceUiRun): Certification {
  if (run.usedSleepForReadiness) {
    return {
      certified: false,
      rule: "sleep-as-readiness",
      reason: "sleep-as-readiness is forbidden; the run is not real-editor responsiveness evidence",
    };
  }
  if (isRejectedLapceUiHost(run.hostKind)) {
    return {
      certified: false,
      rule: "WSP1L-AC3",
      reason:
        `hostKind '${run.hostKind}' cannot certify a Lapce UI timeline ` +
        `(protocol smoke / mock LSP / screenshot / raw LSP are not the real editor event loop)`,
    };
  }
  if (run.protocolSmokePassed && run.timelines.length === 0) {
    return {
      certified: false,
      rule: "WSP1L-AC3",
      reason:
        "protocol smoke passed but no UI timeline was captured; refusing to certify " +
        "(a protocol exchange is not real-editor responsiveness proof)",
    };
  }
  if (!run.protocolSmokePassed && run.timelines.length === 0) {
    return {
      certified: false,
      rule: "empty-run",
      reason: "neither a protocol exchange nor a UI timeline was captured; nothing to certify",
    };
  }
  const manifest = versionsMatchPinnedManifest(run.versions);
  if (!manifest.ok) {
    return {
      certified: false,
      rule: "version-manifest-drift",
      reason: `run versions drifted from the pinned manifest: ${manifest.drift.join("; ")}`,
    };
  }
  if (run.completenessState === "stale") {
    return {
      certified: false,
      rule: "stale-basis",
      reason: "stale runs are never published as current (canonical observation basis)",
    };
  }
  return { certified: true, run };
}

/** Throwing variant for ingestion pipelines. */
export function assertCertified(run: LapceUiRun): LapceUiRun {
  const verdict = certifyRun(run);
  if (!verdict.certified) {
    throw new Error(`${verdict.rule}: ${verdict.reason}`);
  }
  return run;
}

/**
 * A real-client paint/product claim additionally requires the real Lapce host
 * with a measured input-to-paint; the fixture host stays explicitly
 * inadmissible and carries the unavailability reason (WSP1L.3, AC-RESOURCE).
 */
export function assertRealLapceProductClaim(run: LapceUiRun): RealClientClaimVerdict {
  if (run.hostKind === LAPCE_REAL_HOST) {
    const headline = run.timelines.find(
      (timeline) => timeline.inputToPaintMs.status === "measured",
    );
    if (headline !== undefined) {
      return {
        admissible: true,
        hostKind: run.hostKind,
        reason: "real-Lapce host with measured input-to-paint evidence",
      };
    }
    return {
      admissible: false,
      hostKind: run.hostKind,
      reason:
        `real-client claim requires at least one measured input-to-paint timeline; ` +
        `${run.timelines.length === 0 ? "none were captured" : `all ${run.timelines.length} timelines carry unknowns`}`,
    };
  }
  const host = run.hostKind === LAPCE_FIXTURE_HOST ? "the instrumented fixture host" : run.hostKind;
  return {
    admissible: false,
    hostKind: run.hostKind,
    reason: `${host} cannot carry a real-client paint claim; ${GUI_INSTRUMENTATION_UNAVAILABLE_REASON}`,
  };
}
