/**
 * WSP1L real-Lapce producer (the charter's "instrumented client build"
 * automation path, WSP1L.1).
 *
 * `driveLapceSession` spawns the instrumented Lapce reference build with the
 * production volt and the native `verter-lsp` server, drives the scripted
 * open/type/complete/navigate/close interactions over the client's WSP1L
 * drive channel, and captures exactly what that session observed:
 *
 * - every process output line (the volt's `verter launch-stamp` rendered
 *   through the client's tracing log, and the client's `verter ui-stamp`
 *   lines), and
 * - the server's interaction-trace JSONL dump (`VERTER_LSP_INTERACTION_TRACE`
 *   opt-in), anchored to the Unix clock.
 *
 * Nothing is synthesized: a stage the client did not stamp is a hole that
 * fails the drive loud, and a step the server dump does not cover is an
 * error — except `close`, where the observed absence of any server-side
 * counterpart is recorded as an empty trace (Lapce sends no `didClose` when
 * an editor tab closes; the server metrics derived from that trace stay
 * unknown, never zero).
 *
 * The recorded artifact (`driven-lapce-capture.v1`) seals the session: its
 * content digest detects tampering, and the capture-provenance digest binds
 * any run that claims the capture to exactly the recorded UI stamps
 * (WSP1L.3, AC-RESOURCE: relabeled fixtures and hand-written capture lines
 * cannot carry real-client evidence).
 */

import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import net from "node:net";
import path from "node:path";

import type { InteractionTrace } from "@verter/lsp-test-client";

import {
  LAPCE_REAL_HOST,
  type AutomationPath,
  type CaptureProvenance,
  type LapceVersionManifest,
  type ScriptedStep,
  type ScriptedStepKind,
  type UiInteractionRecord,
  type UiStageStamp,
} from "./types.js";
import type { LapceHost } from "./driver.js";
import {
  collectStampMessages,
  uiStampDigest,
  type LaunchStampPayload,
  type StampClockAnchor,
  type UiStampPayload,
} from "./stamp.js";
import { PINNED_LAPCE_VERSION_MANIFEST } from "./manifest.js";

/** The server method each scripted step correlates with (WSP1L.2 join key). */
export const DRIVEN_STEP_SERVER_METHOD: Record<ScriptedStepKind, string> = {
  open: "textDocument/didOpen",
  type: "textDocument/didChange",
  complete: "textDocument/completion",
  navigate: "textDocument/definition",
  close: "textDocument/didClose",
};

export interface DrivenLapceSessionOptions {
  /** The instrumented Lapce reference build (WSP1L patch applied, release). */
  readonly lapceBin: string;
  /** Workspace folder opened by the client (fixture with `.lapce/settings.toml`). */
  readonly workspaceDir: string;
  /** Volt folder (`volt.toml` + built wasm) passed via `--plugin-path`. */
  readonly voltDir: string;
  /** The script to drive, in order. Every step's epoch must be unique. */
  readonly script: readonly ScriptedStep[];
  /** Extra env for the client tree (inherited by the proxy and the server). */
  readonly extraEnv?: Readonly<Record<string, string>>;
  /** Bound on waiting for the client's ready event (default 30 s). */
  readonly readyTimeoutMs?: number;
  /** Bound on waiting for one step's paint ack (default 120 s). */
  readonly stepTimeoutMs?: number;
}

export interface DrivenLapceSession {
  readonly sessionId: string;
  readonly lapceClientVersion: string;
  readonly startedUnixMs: number;
  /** Every captured stdout/stderr line of the client process, in order. */
  readonly capturedLines: readonly string[];
  /** The server's interaction-trace JSONL dump (anchor line + traces). */
  readonly dumpLines: readonly string[];
  /** The stamps each step's ack carried, keyed by the step's request epoch. */
  readonly stepStamps: ReadonlyMap<number, readonly UiStampPayload[]>;
  /** The script as it was driven. */
  readonly drivenScript: readonly ScriptedStep[];
}

interface DriveAck {
  readonly kind?: string;
  readonly requestEpoch?: number;
  readonly stamps?: { readonly stage: string; readonly atUnixMs: number }[];
  readonly event?: string;
  readonly error?: string;
}

function nowUnixMs(): number {
  return Date.now();
}

function assertSingleUse<T>(seen: Set<number>, step: ScriptedStep): void {
  if (seen.has(step.requestEpoch)) {
    throw new Error(
      `driven script reuses request epoch ${step.requestEpoch}; a WSP1 request epoch correlates exactly one UI step (WSP1L.2)`,
    );
  }
  seen.add(step.requestEpoch);
}

/**
 * Spawn and drive the instrumented client. Fails loud — never partially —
 * when the client exits early, a step's paint is not observed within the
 * bound, or a step reports a hole: a failed drive is recorded as failed,
 * never trimmed into a complete-looking capture.
 */
export async function driveLapceSession(
  options: DrivenLapceSessionOptions,
): Promise<DrivenLapceSession> {
  const readyTimeoutMs = options.readyTimeoutMs ?? 30_000;
  const stepTimeoutMs = options.stepTimeoutMs ?? 120_000;
  const seenEpochs = new Set<number>();
  for (const step of options.script) assertSingleUse(seenEpochs, step);

  const dumpDir = mkdtempSync(path.join(tmpdir(), "verter-wsp1l-dump-"));
  try {
    return await driveLapceSessionInto(options, dumpDir, readyTimeoutMs, stepTimeoutMs);
  } finally {
    // Launch, drive and post-drive validation failures all pass through here:
    // the temporary dump directory never outlives the session.
    rmSync(dumpDir, { recursive: true, force: true });
  }
}

async function driveLapceSessionInto(
  options: DrivenLapceSessionOptions,
  dumpDir: string,
  readyTimeoutMs: number,
  stepTimeoutMs: number,
): Promise<DrivenLapceSession> {
  const dumpPath = path.join(dumpDir, "interaction-trace.jsonl");
  const sessionId = `${nowUnixMs()}-${randomUUID().slice(0, 8)}`;

  const server = net.createServer();
  const port = await new Promise<number>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (typeof address === "string" || address === null) {
        reject(new Error(`unexpected drive listener address ${String(address)}`));
        return;
      }
      resolve(address.port);
    });
  });

  const capturedLines: string[] = [];
  const acks: DriveAck[] = [];
  const connBox: { current: net.Socket | null } = { current: null };
  server.on("connection", (socket) => {
    connBox.current = socket;
    // The client tree is force-killed at teardown; the resulting reset on the
    // drive socket is expected after the script's last ack, not a failure.
    socket.on("error", (error) => {
      capturedLines.push(`verter drive: socket error after teardown boundary: ${String(error)}`);
    });
    let buffer = "";
    socket.on("data", (chunk) => {
      buffer += chunk.toString("utf8");
      let at = buffer.indexOf("\n");
      while (at !== -1) {
        const line = buffer.slice(0, at);
        buffer = buffer.slice(at + 1);
        if (line.trim() !== "") {
          capturedLines.push(line);
          try {
            acks.push(JSON.parse(line) as DriveAck);
          } catch {
            /* non-JSON client noise is kept in capturedLines, not parsed */
          }
        }
        at = buffer.indexOf("\n");
      }
    });
  });

  const child = spawn(
    options.lapceBin,
    ["--new", "--wait", "--plugin-path", options.voltDir, options.workspaceDir],
    {
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        ...options.extraEnv,
        LAPCE_LOG: "lapce_proxy=debug",
        VERTER_WSP1L_DRIVE_ADDR: `127.0.0.1:${port}`,
        VERTER_LSP_INTERACTION_TRACE: "1",
        VERTER_LSP_INTERACTION_TRACE_DUMP: dumpPath,
      },
    },
  );
  let exited = false;
  child.once("exit", () => {
    exited = true;
  });
  const capture = (stream: NodeJS.ReadableStream) => {
    let buffer = "";
    stream.on("data", (chunk: Buffer) => {
      buffer += chunk.toString("utf8");
      let at = buffer.indexOf("\n");
      while (at !== -1) {
        const line = buffer.slice(0, at);
        buffer = buffer.slice(at + 1);
        if (line.trim() !== "") capturedLines.push(line);
        at = buffer.indexOf("\n");
      }
    });
  };
  capture(child.stdout!);
  capture(child.stderr!);

  const killTree = () => {
    if (!exited) {
      if (process.platform === "win32") {
        spawn("taskkill", ["/PID", String(child.pid), "/T", "/F"]);
      } else {
        child.kill("SIGKILL");
      }
    }
  };

  const waitFor = (predicate: () => boolean, label: string, timeoutMs: number): Promise<void> =>
    new Promise((resolve, reject) => {
      const startedAt = Date.now();
      const timer = setInterval(() => {
        if (predicate()) {
          clearInterval(timer);
          resolve();
        } else if (exited) {
          clearInterval(timer);
          reject(new Error(`the driven Lapce client exited while waiting for ${label}`));
        } else if (Date.now() - startedAt > timeoutMs) {
          clearInterval(timer);
          killTree();
          reject(new Error(`timeout (${timeoutMs} ms) waiting for ${label}`));
        }
      }, 25);
    });

  const ackFor = (epoch: number) =>
    acks.find((ack) => ack.requestEpoch === epoch && ack.stamps !== undefined);

  try {
    await waitFor(
      () => acks.some((ack) => ack.event === "ready"),
      "the client's ready event",
      readyTimeoutMs,
    );
    for (const step of options.script) {
      const socket = connBox.current;
      if (socket === null) throw new Error("drive channel closed before the script finished");
      socket.write(`${JSON.stringify(stepCommand(step))}\n`);
      await waitFor(
        () => ackFor(step.requestEpoch) !== undefined,
        `step '${step.kind}' (epoch ${step.requestEpoch}) paint ack`,
        stepTimeoutMs,
      );
      const ack = ackFor(step.requestEpoch)!;
      if (ack.stamps!.length === 0) {
        throw new Error(
          `step '${step.kind}' (epoch ${step.requestEpoch}) acked with no observed stamps; a hole is never trimmed into a capture`,
        );
      }
    }
  } finally {
    killTree();
    if (!exited) {
      // The exit listener is only useful while the child is still running: a
      // child that already exited would never fire it and the timer alone
      // would hold teardown for its full duration.
      await new Promise<void>((resolve) => {
        child.once("exit", () => resolve());
        setTimeout(resolve, 5_000).unref();
      });
    }
    // server.close() keeps established connections alive; the drive socket is
    // torn down explicitly so the listener can actually close.
    connBox.current?.destroy();
    connBox.current = null;
    server.close();
  }

  const ready = acks.find((ack) => ack.event === "ready") as
    | { readonly lapceVersion?: string }
    | undefined;
  if (ready?.lapceVersion === undefined) {
    throw new Error("the driven client's ready event carried no lapceVersion identity");
  }
  const stepStamps = new Map<number, readonly UiStampPayload[]>();
  for (const step of options.script) {
    const ack = ackFor(step.requestEpoch)!;
    stepStamps.set(
      step.requestEpoch,
      ack.stamps!.map((stamp) => ({
        kind: step.kind,
        requestEpoch: step.requestEpoch,
        sourceEpoch: step.sourceEpoch,
        stage: stamp.stage as UiStampPayload["stage"],
        atUnixMs: stamp.atUnixMs,
      })),
    );
  }

  let dumpLines: string[] = [];
  try {
    dumpLines = readFileSync(dumpPath, "utf8")
      .split("\n")
      .filter((line) => line.trim() !== "");
  } catch {
    dumpLines = [];
  }

  return {
    sessionId,
    lapceClientVersion: ready.lapceVersion,
    startedUnixMs: Number(sessionId.slice(0, sessionId.indexOf("-"))),
    capturedLines,
    dumpLines,
    stepStamps,
    drivenScript: [...options.script],
  };
}

function stepCommand(step: ScriptedStep): Record<string, unknown> {
  const command: Record<string, unknown> = {
    kind: step.kind,
    requestEpoch: step.requestEpoch,
    sourceEpoch: step.sourceEpoch,
  };
  if (step.text !== undefined) command.text = step.text;
  if (step.uri !== undefined) command.path = step.uri;
  if (step.line !== undefined) command.line = step.line;
  if (step.column !== undefined) command.column = step.column;
  return command;
}

/** One parsed server-dump trace line (the server's `InteractionTrace` JSON). */
interface ServerDumpTrace {
  readonly requestEpoch: number;
  readonly sourceEpoch?: number;
  readonly method: string;
  readonly status: string;
  readonly stamps: readonly { readonly stage: string; readonly atMs: number }[];
  readonly firstBlockedStage?: string | null;
}

export interface CorrelatedCapture {
  /** The script with each step's source epoch set to the observed doc version. */
  readonly steps: readonly ScriptedStep[];
  /** Server traces keyed by the step epochs, stamps mapped to Unix ms. */
  readonly serverTraces: readonly InteractionTrace[];
  /** Unix-ms mapping of the dump's session-relative stamps. */
  readonly sessionStartUnixMs: number;
}

/**
 * Join the driven steps with the server dump (WSP1L.2): each step claims the
 * latest trace of its expected method whose observed window overlaps the
 * step's [input_dispatched, next step's input_dispatched) span — sequential
 * driving keeps those spans disjoint. Stamps map onto the Unix clock through
 * the dump's recorded session anchor, never by assuming a shared origin.
 */
export function correlateServerTraces(session: DrivenLapceSession): CorrelatedCapture {
  if (session.dumpLines.length === 0) {
    throw new Error(
      "the server interaction-trace dump is empty; the server timeline cannot be correlated (WSP1L.2) — check VERTER_LSP_INTERACTION_TRACE propagation to the server process",
    );
  }
  let sessionStartUnixMs: number | null = null;
  const dumpTraces: ServerDumpTrace[] = [];
  for (const line of session.dumpLines) {
    const parsed = JSON.parse(line) as
      | { readonly anchor?: string; readonly sessionStartUnixMs?: number }
      | ServerDumpTrace;
    if ("anchor" in parsed && parsed.anchor === "session") {
      sessionStartUnixMs = parsed.sessionStartUnixMs ?? null;
      continue;
    }
    dumpTraces.push(parsed as ServerDumpTrace);
  }
  if (sessionStartUnixMs === null) {
    throw new Error(
      "the server dump carries no session anchor; its stamps cannot join the Unix clock",
    );
  }

  const unixOf = (trace: ServerDumpTrace, atMs: number) => sessionStartUnixMs! + atMs;
  const steps = [...session.drivenScript];
  const serverTraces: InteractionTrace[] = [];

  steps.forEach((step, index) => {
    const stamps = session.stepStamps.get(step.requestEpoch);
    if (stamps === undefined || stamps.length === 0) {
      throw new Error(`step '${step.kind}' (epoch ${step.requestEpoch}) has no observed stamps`);
    }
    const windowStart = stamps[0]!.atUnixMs;
    const windowEnd =
      index + 1 < steps.length
        ? session.stepStamps.get(steps[index + 1]!.requestEpoch)![0]!.atUnixMs
        : Number.POSITIVE_INFINITY;
    const expected = DRIVEN_STEP_SERVER_METHOD[step.kind];
    const candidates = dumpTraces
      .filter((trace) => trace.method === expected)
      .filter((trace) => {
        const first = unixOf(trace, trace.stamps[0]?.atMs ?? 0);
        const last = unixOf(trace, trace.stamps[trace.stamps.length - 1]?.atMs ?? 0);
        return first < windowEnd && last >= windowStart;
      });
    if (candidates.length === 0) {
      if (step.kind === "close") {
        // The observed absence of a server counterpart: Lapce sends no
        // didClose when an editor tab closes. Recorded as an empty trace —
        // every server metric derived from it stays unknown, never zero.
        serverTraces.push({
          requestEpoch: step.requestEpoch,
          sourceEpoch: null,
          method: expected,
          status: "complete",
          stamps: [],
          firstBlockedStage: null,
        });
        return;
      }
      throw new Error(
        `no server trace of method '${expected}' overlaps step '${step.kind}' ` +
          `(epoch ${step.requestEpoch}); the server timeline has a hole the capture refuses to fill`,
      );
    }
    // Sequential driving: the step's own interaction is the latest candidate
    // that began inside its window (earlier ones belong to previous steps).
    const claimed = candidates.reduce((latest, trace) =>
      unixOf(trace, trace.stamps[0]?.atMs ?? 0) > unixOf(latest, latest.stamps[0]?.atMs ?? 0)
        ? trace
        : latest,
    );
    serverTraces.push({
      requestEpoch: step.requestEpoch,
      sourceEpoch: claimed.sourceEpoch ?? null,
      method: claimed.method,
      status: claimed.status as InteractionTrace["status"],
      stamps: claimed.stamps.map((stamp) => ({
        stage: stamp.stage as InteractionTrace["stamps"][number]["stage"],
        atMs: unixOf(claimed, stamp.atMs),
      })),
      firstBlockedStage: (claimed.firstBlockedStage ??
        null) as InteractionTrace["firstBlockedStage"],
    });
    steps[index] = { ...step, sourceEpoch: claimed.sourceEpoch ?? null };
  });

  return { steps, serverTraces, sessionStartUnixMs };
}

/** ─── Recorded capture artifacts ─────────────────────────────────────────── */

export interface DrivenCaptureArtifact {
  readonly schema: "driven-lapce-capture.v1";
  readonly node: "WSP1L";
  readonly recordedAs: string;
  readonly session: {
    readonly sessionId: string;
    readonly startedUnixMs: number;
    readonly machine: string;
  };
  readonly lapceClient: {
    readonly version: string;
    readonly source: string;
    readonly patch: string;
    readonly patchSha256: string;
  };
  readonly automationPath: AutomationPath;
  readonly drivenScript: readonly ScriptedStep[];
  readonly capturedLines: readonly string[];
  readonly serverTraceDumpLines: readonly string[];
  readonly correlated: {
    readonly steps: readonly ScriptedStep[];
    readonly serverTraces: readonly InteractionTrace[];
  };
  readonly clockAnchor: StampClockAnchor;
  readonly contentSha256: string;
}

function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, v]) => v !== undefined)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonicalJson(v)}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

function contentDigest(artifact: Omit<DrivenCaptureArtifact, "contentSha256">): string {
  return createHash("sha256").update(canonicalJson(artifact)).digest("hex");
}

export function buildDrivenCaptureArtifact(input: {
  readonly session: DrivenLapceSession;
  readonly correlated: CorrelatedCapture;
  readonly recordedAs: string;
  readonly lapceClientSource: string;
  /** Local path of the instrumentation patch, read for its digest only. */
  readonly patchPath: string;
  /** Repository-relative path recorded as the patch provenance. */
  readonly patchRecordedAs: string;
  readonly automationPath: AutomationPath;
}): DrivenCaptureArtifact {
  const { launchStamps, uiStamps } = collectStampMessages(input.session.capturedLines);
  if (launchStamps.length === 0) {
    throw new Error("the capture holds no `verter launch-stamp` line; the volt launch is unproven");
  }
  if (!launchStamps.some((stamp) => stamp.phase === "server_launch_issued")) {
    throw new Error(
      "the capture holds no issued launch stamp; the session never launched the server",
    );
  }
  if (uiStamps.length === 0) {
    throw new Error(
      "the capture holds no `verter ui-stamp` observations (WSP1L.3: unavailable, not simulated)",
    );
  }
  const patchSha256 = createHash("sha256").update(readFileSync(input.patchPath)).digest("hex");
  const first = uiStamps[0]!.atUnixMs;
  const body = {
    schema: "driven-lapce-capture.v1" as const,
    node: "WSP1L" as const,
    recordedAs: input.recordedAs,
    session: {
      sessionId: input.session.sessionId,
      startedUnixMs: input.session.startedUnixMs,
      machine: `${process.platform}/${process.arch}`,
    },
    lapceClient: {
      version: input.session.lapceClientVersion,
      source: input.lapceClientSource,
      patch: input.patchRecordedAs,
      patchSha256,
    },
    automationPath: input.automationPath,
    drivenScript: input.session.drivenScript,
    capturedLines: [...input.session.capturedLines],
    serverTraceDumpLines: [...input.session.dumpLines],
    correlated: {
      steps: input.correlated.steps,
      serverTraces: input.correlated.serverTraces,
    },
    clockAnchor: {
      stampUnixMs: first,
      timelineMs: first,
      recordedAs:
        "identity: the driven capture's timeline clock is the Unix-ms wall clock both the client stamps and the anchored server dump already use",
    },
  };
  return { ...body, contentSha256: contentDigest(body) };
}

export function writeDrivenCaptureArtifact(artifact: DrivenCaptureArtifact, outPath: string): void {
  writeFileSync(outPath, `${JSON.stringify(artifact, null, 2)}\n`);
}

export interface RecordedLapceCapture {
  readonly session: {
    readonly lapceClientVersion: string;
    readonly launchStamps: readonly LaunchStampPayload[];
    readonly uiStamps: readonly UiStampPayload[];
    readonly clockAnchor: StampClockAnchor;
  };
  readonly steps: readonly ScriptedStep[];
  readonly serverTraces: readonly InteractionTrace[];
  readonly provenance: CaptureProvenance;
  /** The pinned manifest with `lapce-client` bound to the recorded build. */
  readonly versions: LapceVersionManifest;
  readonly artifact: DrivenCaptureArtifact;
}

/**
 * Load and verify a recorded capture artifact. Fails loud on a tampered or
 * stale artifact (content digest), on stamps that do not match the sealed
 * provenance digest, or on a capture that lacks the launch/UI observations a
 * real-client claim requires.
 *
 * The two digests are corruption and drift detection, not authentication:
 * `contentSha256` is recomputed from the artifact body it sits in, so a
 * deliberate edit that recomputes the field passes, and `captureSha256` binds
 * a run's observed stamps to the artifact's own stamp lines. What the committed
 * capture proves is pinned outside the artifact by the tests that load it.
 */
export function loadRecordedLapceCapture(artifactPath: string): RecordedLapceCapture {
  const artifact = JSON.parse(readFileSync(artifactPath, "utf8")) as DrivenCaptureArtifact;
  if (artifact.schema !== "driven-lapce-capture.v1" || artifact.node !== "WSP1L") {
    throw new Error(`'${artifactPath}' is not a driven-lapce-capture.v1 artifact`);
  }
  const { contentSha256, ...body } = artifact;
  const recomputed = contentDigest(body as Omit<DrivenCaptureArtifact, "contentSha256">);
  if (recomputed !== contentSha256) {
    throw new Error(
      `capture artifact '${artifactPath}' content digest mismatch (recorded ${contentSha256.slice(0, 12)}, recomputed ${recomputed.slice(0, 12)}); a tampered capture is not real-client evidence`,
    );
  }
  const { launchStamps, uiStamps } = collectStampMessages(artifact.capturedLines);
  if (launchStamps.length === 0 || uiStamps.length === 0) {
    throw new Error(
      `capture artifact '${artifactPath}' holds no launch/ui stamps; it cannot back a real-client claim`,
    );
  }
  const provenance: CaptureProvenance = {
    schema: "driven-lapce-capture.v1",
    sessionId: artifact.session.sessionId,
    recordedAs: artifact.recordedAs,
    captureSha256: uiStampDigest(uiStamps),
    lapceClientVersion: artifact.lapceClient.version,
  };
  return {
    session: {
      lapceClientVersion: artifact.lapceClient.version,
      launchStamps,
      uiStamps,
      clockAnchor: artifact.clockAnchor,
    },
    steps: artifact.correlated.steps,
    serverTraces: artifact.correlated.serverTraces,
    provenance,
    versions: {
      ...PINNED_LAPCE_VERSION_MANIFEST,
      items: PINNED_LAPCE_VERSION_MANIFEST.items.map((item) =>
        item.item === "lapce-client"
          ? { item: "lapce-client", version: artifact.lapceClient.version, status: "pinned" }
          : item,
      ),
    },
    artifact,
  };
}

/** ─── The driven host ───────────────────────────────────────────────────── */

/**
 * The real Lapce host as driven by `driveLapceSession`: it launches and
 * drives the instrumented build up front (the drive performs every scripted
 * step against the launched client's event loop, in order), then serves each
 * step's observed record synchronously. `runScriptedStep` returns exactly
 * the stamps the driven client's loop produced for that step — mapped
 * through the capture's recorded clock anchor, never synthesized.
 */
export class DrivenLapceHost implements LapceHost {
  readonly hostKind = LAPCE_REAL_HOST;
  readonly automationPath: AutomationPath;
  readonly usedSleepForReadiness = false;
  readonly lapceClientVersion: string;
  readonly provenance: CaptureProvenance;
  readonly uiStamps: readonly UiStampPayload[];
  readonly #anchor: StampClockAnchor;
  #records = new Map<string, UiInteractionRecord>();
  #released = false;

  private constructor(input: {
    readonly automationPath: AutomationPath;
    readonly session: DrivenLapceSession;
    readonly anchor: StampClockAnchor;
  }) {
    this.automationPath = input.automationPath;
    this.lapceClientVersion = input.session.lapceClientVersion;
    const stamps = input.session.drivenScript.flatMap(
      (step) => input.session.stepStamps.get(step.requestEpoch) ?? [],
    );
    this.uiStamps = stamps;
    this.provenance = {
      schema: "driven-lapce-capture.v1",
      sessionId: input.session.sessionId,
      recordedAs: `live driven session ${input.session.sessionId} (driveLapceSession)`,
      captureSha256: uiStampDigest(stamps),
      lapceClientVersion: input.session.lapceClientVersion,
    };
    this.#anchor = input.anchor;
    for (const step of input.session.drivenScript) {
      const observed = input.session.stepStamps.get(step.requestEpoch) ?? [];
      if (observed.length === 0) {
        throw new Error(
          `the driven session holds no stamps for step '${step.kind}' (epoch ${step.requestEpoch})`,
        );
      }
      const stampsForStep: UiStageStamp[] = observed.map((stamp) => ({
        stage: stamp.stage,
        atMs: stampUnixMsToTimeline(stamp.atUnixMs, input.anchor),
      }));
      this.#records.set(`${step.kind}#${step.requestEpoch}`, { step, stamps: stampsForStep });
    }
  }

  /** Launch and drive the instrumented client for `script`. */
  static async launch(
    options: DrivenLapceSessionOptions & { readonly automationPath: AutomationPath },
  ): Promise<DrivenLapceHost> {
    const session = await driveLapceSession(options);
    const { launchStamps, uiStamps } = collectStampMessages(session.capturedLines);
    const first = launchStamps[0] ?? uiStamps[0];
    if (first === undefined) {
      // The acks carried stamps but the captured lines hold none: the child's
      // streams closed before the stamp lines arrived. That is a capture hole,
      // reported as such rather than as a failed property access.
      throw new Error(
        "the driven session's captured lines hold no launch/ui stamp; the capture has a hole and cannot anchor a clock",
      );
    }
    const anchor: StampClockAnchor = {
      stampUnixMs: first.atUnixMs,
      timelineMs: first.atUnixMs,
      recordedAs: "identity: the driven session's timeline clock is the Unix-ms wall clock",
    };
    return new DrivenLapceHost({ automationPath: options.automationPath, session, anchor });
  }

  runScriptedStep(step: ScriptedStep): UiInteractionRecord {
    if (this.#released) {
      throw new Error("DrivenLapceHost was released; no further steps may run");
    }
    const record = this.#records.get(`${step.kind}#${step.requestEpoch}`);
    if (record === undefined) {
      throw new Error(
        `the driven session did not drive step '${step.kind}' at request epoch ${step.requestEpoch}; ` +
          `the driven record set is exactly what was performed against the launched client — never extended`,
      );
    }
    return record;
  }

  /** The capture binding a driver run needs for provenance-gated claims. */
  get capture(): {
    readonly provenance: CaptureProvenance;
    readonly uiStamps: readonly UiStampPayload[];
  } {
    return { provenance: this.provenance, uiStamps: this.uiStamps };
  }

  release(): void {
    if (this.#released) return;
    this.#released = true;
    this.#records = new Map();
  }
}

function stampUnixMsToTimeline(atUnixMs: number, anchor: StampClockAnchor): number {
  return anchor.timelineMs + (atUnixMs - anchor.stampUnixMs);
}
