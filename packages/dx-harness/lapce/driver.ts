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
  stampUnixMsToTimelineMs,
  type LaunchStampPayload,
  type StampClockAnchor,
  type UiStampPayload,
} from "./stamp.js";
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
    "one-time initialize launch-stamp line on the driven client's host stderr behind " +
    "uiTrace.enabled (default off); no UI notification, per-message LSP path untouched",
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
  /**
   * Cancellation/boundary seam (charter snapshot rules): stop the host, abort
   * in-flight work and release retained handles. Idempotent; a released host
   * refuses further steps loud.
   */
  release(): void;
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
  #released = false;

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
    if (this.#released) {
      throw new Error("FixtureLapceHost was released; no further steps may run");
    }
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

  release(): void {
    this.#released = true;
  }
}

/**
 * Provenance-checked capture of one real Lapce driving session (WSP1L.1 real
 * path): the parsed instrumentation lines a driven client emitted on host
 * stderr plus the anchor that joins the stamps' Unix-ms clock to the run
 * timeline's clock. Every field comes from the capture; nothing is synthesized.
 */
export interface RealLapceCaptureSession {
  /** Pinned identity of the driven Lapce build (version-manifest item `lapce-client`). */
  readonly lapceClientVersion: string;
  /** `verter launch-stamp` lines captured from the session; at least one issued launch. */
  readonly launchStamps: readonly LaunchStampPayload[];
  /** `verter ui-stamp` observations from the instrumented-client event loop. */
  readonly uiStamps: readonly UiStampPayload[];
  /** Anchor joining the capture's Unix-ms stamp clock to the run timeline clock. */
  readonly clockAnchor: StampClockAnchor;
}

/**
 * The real Lapce host (WSP1L): replays the UI instrumentation actually observed
 * while a driven Lapce build performed the scripted interactions — the stamps
 * come exclusively from the capture session, never from a clock. Construction
 * FAILS CLOSED when the real path is absent: no pinned client identity, no
 * captured launch stamp or no UI observations means no real host, so the
 * fixture can never stand in for one (WSP1L.3: unavailable is recorded, not
 * simulated).
 */
export class RealLapceHost implements LapceHost {
  readonly hostKind = LAPCE_REAL_HOST;
  readonly automationPath = REAL_LAPCE_AUTOMATION_PATH;
  readonly usedSleepForReadiness = false;
  readonly lapceClientVersion: string;
  readonly #launchStamps: readonly LaunchStampPayload[];
  readonly #clockAnchor: StampClockAnchor;
  #uiStamps: readonly UiStampPayload[];
  #released = false;

  constructor(session: RealLapceCaptureSession) {
    if (
      typeof session.lapceClientVersion !== "string" ||
      session.lapceClientVersion.trim() === ""
    ) {
      throw new Error(
        "a real-Lapce host requires the driven build's pinned lapce-client identity; " +
          "an unpinned client cannot back real-client evidence (record it unavailable, do not guess)",
      );
    }
    if (session.launchStamps.length === 0) {
      throw new Error(
        "a real-Lapce host requires at least one captured launch stamp " +
          "(`verter launch-stamp` line); without the session's volt launch marker the run is not proven",
      );
    }
    if (!session.launchStamps.some((stamp) => stamp.phase === "server_launch_issued")) {
      throw new Error(
        "a real-Lapce host requires a captured `server_launch_issued` stamp; " +
          "a refused-only capture never drove a server",
      );
    }
    if (session.uiStamps.length === 0) {
      throw new Error(
        "a real-Lapce host requires UI stage observations from the driven client's event loop " +
          "(`verter ui-stamp` lines); with none captured, GUI instrumentation stays unavailable, not simulated",
      );
    }
    if (
      typeof session.clockAnchor?.recordedAs !== "string" ||
      session.clockAnchor.recordedAs === ""
    ) {
      throw new Error(
        "a real-Lapce host requires a recorded clock anchor joining the stamp clock to the timeline clock",
      );
    }
    this.lapceClientVersion = session.lapceClientVersion;
    this.#launchStamps = [...session.launchStamps];
    this.#uiStamps = [...session.uiStamps];
    this.#clockAnchor = session.clockAnchor;
  }

  /** The captured launch stamps (read-only evidence for run receipts). */
  get launchStamps(): readonly LaunchStampPayload[] {
    return this.#launchStamps;
  }

  runScriptedStep(step: ScriptedStep): UiInteractionRecord {
    if (this.#released) {
      throw new Error("RealLapceHost was released; no further steps may run");
    }
    const observed = this.#uiStamps.filter(
      (stamp) =>
        stamp.kind === step.kind &&
        stamp.requestEpoch === step.requestEpoch &&
        stamp.sourceEpoch === step.sourceEpoch,
    );
    if (observed.length === 0) {
      throw new Error(
        `the real capture holds no UI stamps for step '${step.kind}' at request epoch ` +
          `${step.requestEpoch}/source ${String(step.sourceEpoch)}; the capture must cover every ` +
          `scripted step — a hole is recorded, never synthesized (WSP1L.3)`,
      );
    }
    return {
      step,
      stamps: observed.map((stamp) => ({
        stage: stamp.stage,
        atMs: stampUnixMsToTimelineMs(stamp.atUnixMs, this.#clockAnchor),
      })),
    };
  }

  release(): void {
    if (this.#released) return;
    this.#released = true;
    this.#uiStamps = [];
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
 * Scripted steps carry their own WSP1 request/source epochs and are correlated
 * on exactly those epochs (WSP1L.2); the bare single-step methods synthesize
 * the next unused epoch.
 */
export class LapceInteractionDriver {
  readonly #options: LapceInteractionDriverOptions;
  readonly #timelines: UiTimeline[] = [];
  readonly #usedRequestEpochs = new Set<number>();
  #tornDown = false;

  constructor(options: LapceInteractionDriverOptions) {
    this.#options = options;
  }

  open(label?: string): UiTimeline {
    return this.#step({ kind: "open", label, requestEpoch: 0, sourceEpoch: 0 });
  }

  type(label?: string): UiTimeline {
    return this.#step({ kind: "type", label, requestEpoch: 0, sourceEpoch: 0 });
  }

  complete(label?: string): UiTimeline {
    return this.#step({ kind: "complete", label, requestEpoch: 0, sourceEpoch: 0 });
  }

  navigate(label?: string): UiTimeline {
    return this.#step({ kind: "navigate", label, requestEpoch: 0, sourceEpoch: 0 });
  }

  close(label?: string): UiTimeline {
    return this.#step({ kind: "close", label, requestEpoch: 0, sourceEpoch: 0 });
  }

  /** Run a full script and return the certified-able run record. */
  runScriptedInteraction(script: readonly ScriptedStep[]): LapceUiRun {
    for (const step of script) this.#step(step);
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
    if (this.#tornDown) return;
    this.#tornDown = true;
    this.#options.host.release();
  }

  #step(scripted: ScriptedStep): UiTimeline {
    if (this.#tornDown) {
      throw new Error("LapceInteractionDriver is torn down; no further steps may run");
    }
    const step = this.#withEpoch(scripted);
    const record = this.#options.host.runScriptedStep(step);
    const trace = this.#options.serverTraces.find(
      (candidate) => candidate.requestEpoch === step.requestEpoch,
    );
    if (trace === undefined) {
      throw new Error(
        `no WSP1 server InteractionTrace for request epoch ${step.requestEpoch} (step ` +
          `'${step.kind}'); UI timestamps cannot be correlated without the server timeline (WSP1L.2)`,
      );
    }
    if (trace.sourceEpoch !== step.sourceEpoch) {
      throw new Error(
        `the WSP1 trace for request epoch ${step.requestEpoch} carries sourceEpoch ` +
          `${String(trace.sourceEpoch)} but the step correlates sourceEpoch ` +
          `${String(step.sourceEpoch)}; refusing to bind the wrong trace (WSP1L.2)`,
      );
    }
    const timeline = buildUiTimeline(record, trace, {
      stallThreshold: this.#options.stallThreshold,
      immediacyBound: this.#options.immediacyBound,
    });
    this.#timelines.push(timeline);
    return timeline;
  }

  /**
   * Validate and register the step's epochs: a WSP1 request epoch correlates
   * exactly one UI step, so each epoch may be driven once; the bare single-step
   * methods (requestEpoch 0) are assigned the smallest unused epoch instead.
   */
  #withEpoch(scripted: ScriptedStep): ScriptedStep {
    if (scripted.requestEpoch === 0) {
      const synthesized = this.#nextUnusedEpoch();
      return { ...scripted, requestEpoch: synthesized, sourceEpoch: synthesized };
    }
    if (!Number.isSafeInteger(scripted.requestEpoch) || scripted.requestEpoch < 1) {
      throw new Error(
        `requestEpoch must be a positive integer (or 0 to synthesize); got ${String(
          scripted.requestEpoch,
        )}`,
      );
    }
    if (
      scripted.sourceEpoch !== null &&
      (!Number.isSafeInteger(scripted.sourceEpoch) || scripted.sourceEpoch < 1)
    ) {
      throw new Error(
        `sourceEpoch must be a positive integer or null; got ${String(scripted.sourceEpoch)}`,
      );
    }
    if (this.#usedRequestEpochs.has(scripted.requestEpoch)) {
      throw new Error(
        `request epoch ${scripted.requestEpoch} was already driven; a WSP1 request epoch ` +
          `correlates exactly one UI step (WSP1L.2)`,
      );
    }
    this.#usedRequestEpochs.add(scripted.requestEpoch);
    return scripted;
  }

  #nextUnusedEpoch(): number {
    let candidate = 1;
    while (this.#usedRequestEpochs.has(candidate)) candidate += 1;
    this.#usedRequestEpochs.add(candidate);
    return candidate;
  }
}

/**
 * WSP1L-AC3 and friends: certify a run record. Refusals are explicit — a
 * protocol smoke that passed with no UI timeline is never certified. A
 * certification is hermetic evidence by default; it carries real-client
 * evidence only when the run came from the real host on the real automation
 * path, so a consumer that stops at `certifyRun` can never mistake a fixture
 * run's synthetic timelines for real-client paint evidence.
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
  return { certified: true, run, realClientEvidence: carriesRealClientEvidence(run) };
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
 * A run carries real-client evidence only when BOTH the host kind and the
 * automation path are the real ones — relabeling a fixture run with the real
 * hostKind does not turn its synthesized timestamps into client observations
 * (WSP1L.3 'not simulated', AC-RESOURCE).
 */
export function carriesRealClientEvidence(run: LapceUiRun): boolean {
  return (
    run.hostKind === LAPCE_REAL_HOST && run.automationPath.kind === REAL_LAPCE_AUTOMATION_PATH.kind
  );
}

/**
 * A real-client paint/product claim requires the real Lapce host produced by
 * the real automation path, a pinned lapce-client identity and a measured
 * input-to-paint; the fixture host — or fixture timestamps merely relabeled
 * with the real hostKind — stays explicitly inadmissible and carries the
 * unavailability reason (WSP1L.3, AC-RESOURCE).
 */
export function assertRealLapceProductClaim(run: LapceUiRun): RealClientClaimVerdict {
  if (run.hostKind !== LAPCE_REAL_HOST) {
    const host =
      run.hostKind === LAPCE_FIXTURE_HOST ? "the instrumented fixture host" : run.hostKind;
    return {
      admissible: false,
      hostKind: run.hostKind,
      reason: `${host} cannot carry a real-client paint claim; ${GUI_INSTRUMENTATION_UNAVAILABLE_REASON}`,
    };
  }
  if (run.automationPath.kind !== REAL_LAPCE_AUTOMATION_PATH.kind) {
    return {
      admissible: false,
      hostKind: run.hostKind,
      reason:
        `hostKind '${run.hostKind}' is relabeled onto the '${run.automationPath.kind}' ` +
        `automation path; a real-client claim requires the real path ` +
        `'${REAL_LAPCE_AUTOMATION_PATH.kind}' with client-observed stamps (WSP1L.3: not simulated)`,
    };
  }
  const lapceClient = run.versions.items.find((item) => item.item === "lapce-client");
  if (lapceClient?.status !== "pinned" || typeof lapceClient.version !== "string") {
    return {
      admissible: false,
      hostKind: run.hostKind,
      reason:
        `a real-client claim requires a pinned lapce-client identity; the run manifest records ` +
        `it ${lapceClient === undefined ? "missing" : `'${lapceClient.status}'`} — an unpinned ` +
        `client cannot back real-client paint evidence`,
    };
  }
  const headline = run.timelines.find((timeline) => timeline.inputToPaintMs.status === "measured");
  if (headline !== undefined) {
    return {
      admissible: true,
      hostKind: run.hostKind,
      reason: `real-Lapce host on the real automation path (lapce-client ${lapceClient.version}) with measured input-to-paint evidence`,
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
