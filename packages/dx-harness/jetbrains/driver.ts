/**
 * Real-IDE comparison driver and fixture runner (JBT1H.1 / JBT1H.2).
 *
 * Captures come from the pinned WebStorm Platform test-framework (or a robot
 * session against that same build). Mock LSP records are refused. Official and
 * Verter sides run the same workflow ids; UI application/paint is recorded
 * separately from RPC duration.
 */

import { assertRealIdeUiEvidence, classifyUiEvidence } from "./evidence.js";
import {
  compareSwappedPairs,
  labelOutliers,
  recordPair,
  swappedOrder,
  type NoiseBound,
  type PairRecord,
} from "./paired-run.js";
import { evaluateMemoryRow, memoryMetricsFromCapture } from "./process-tree.js";
import {
  COMPARISON_WORKFLOWS,
  REAL_IDE_HOST,
  isBasisKind,
  isCompletenessState,
  isRejectedUiHost,
  isSessionState,
  isSide,
  type ComparisonWorkflowId,
  type CompletenessState,
  type CorrectnessRow,
  type IdeCapture,
  type PaintSample,
  type ProcessTreeSnapshot,
  type ProductReceiptBasis,
  type Side,
  type VersionSet,
} from "./types.js";

export const FIXTURE_CORPUS = {
  id: "dx-harness-hermetic-equal-work",
  path: "packages/dx-harness/fixtures/hermetic",
  equalWorkContract: "tests/workspace-responsiveness/WSP0/products/interactive-slo-catalog.v1.json",
  requiredFeaturesEnabled: true,
  diagnosticsEnabled: true,
} as const;

export const SKELETON_UNSUPPORTED_REASON =
  "plugin skeleton has no semantic features; JBT2 owns lifecycle — workflow stays unsupported, not complete";

export const OFFICIAL_PENDING_REASON =
  "official semantic workflow on the platform-only test IDE is pending bundled Vue/JS plugin capture; not guessed";

export function workflowRow(input: {
  readonly workflow: ComparisonWorkflowId;
  readonly side: Side;
  readonly completeness: CompletenessState;
  readonly reason: string;
}): CorrectnessRow {
  return input;
}

export function defaultWorkflowRows(side: Side): readonly CorrectnessRow[] {
  const completeness: CompletenessState = side === "verter" ? "unsupported" : "pending";
  const reason = side === "verter" ? SKELETON_UNSUPPORTED_REASON : OFFICIAL_PENDING_REASON;
  return COMPARISON_WORKFLOWS.map((workflow) => ({ workflow, side, completeness, reason }));
}

export function versionsFromCaptureFields(fields: VersionSet): VersionSet {
  return fields;
}

export interface DriverRunRequest {
  readonly capture: IdeCapture;
}

export interface DriverRunResult {
  readonly capture: IdeCapture;
  readonly uiAdmissible: boolean;
  readonly memory: ReturnType<typeof evaluateMemoryRow>;
}

export function ingestCapture(capture: IdeCapture): DriverRunResult {
  if (capture.usedSleepForReadiness) {
    throw new Error("driver refused sleep-as-readiness capture");
  }
  if (isRejectedUiHost(capture.hostKind)) {
    throw new Error(`driver refused ${capture.hostKind} as a real-IDE capture (JBT1H-AC1)`);
  }
  if (capture.hostKind !== REAL_IDE_HOST) {
    throw new Error(`driver refused unknown hostKind '${capture.hostKind}'`);
  }
  assertRealIdeUiEvidence(capture);
  return {
    capture,
    uiAdmissible: true,
    memory: evaluateMemoryRow({
      tree: capture.processTree,
      ...memoryMetricsFromCapture(capture),
    }),
  };
}

export function parseIdeCapture(raw: unknown): IdeCapture {
  if (raw === null || typeof raw !== "object") {
    throw new Error("IdeCapture must be an object");
  }
  const record = raw as Record<string, unknown>;
  if (record.hostKind !== REAL_IDE_HOST && !isRejectedUiHost(record.hostKind)) {
    throw new Error(`IdeCapture.hostKind invalid: ${String(record.hostKind)}`);
  }
  if (typeof record.usedSleepForReadiness !== "boolean") {
    throw new Error("IdeCapture.usedSleepForReadiness must be boolean");
  }
  if (!isSide(record.side)) throw new Error("IdeCapture.side invalid");
  if (!isSessionState(record.sessionState)) throw new Error("IdeCapture.sessionState invalid");
  if (!isBasisKind(record.basisKind)) throw new Error("IdeCapture.basisKind invalid");
  return {
    hostKind: record.hostKind,
    usedSleepForReadiness: record.usedSleepForReadiness,
    versions: parseVersions(record.versions),
    paint: parsePaint(record.paint),
    processTree: parseProcessTree(record.processTree),
    workflows: parseWorkflows(record.workflows, record.side),
    receiptBasis: parseBasis(record.receiptBasis),
    side: record.side,
    sessionState: record.sessionState,
    basisKind: record.basisKind,
    ...parseOptionalAggregates(record),
  };
}

function parseVersions(raw: unknown): VersionSet {
  if (raw === null || typeof raw !== "object") throw new Error("versions missing");
  const record = raw as Record<string, unknown>;
  if (typeof record.ideProduct !== "string" || record.ideProduct.length === 0) {
    throw new Error("versions.ideProduct required");
  }
  if (typeof record.ideBuild !== "string" || record.ideBuild.length === 0) {
    throw new Error("versions.ideBuild required");
  }
  const pluginVersion =
    record.pluginVersion === null
      ? null
      : typeof record.pluginVersion === "string"
        ? record.pluginVersion
        : invalid("versions.pluginVersion");
  return {
    ideProduct: record.ideProduct,
    ideBuild: record.ideBuild,
    pluginVersion,
    engine: parseMetric(record.engine, "versions.engine"),
  };
}

function parsePaint(raw: unknown): PaintSample {
  if (raw === null || typeof raw !== "object") throw new Error("paint missing");
  const record = raw as Record<string, unknown>;
  return {
    uiApplyPaintMs: parseMetric(record.uiApplyPaintMs, "paint.uiApplyPaintMs"),
    rpcDurationMs: parseMetric(record.rpcDurationMs, "paint.rpcDurationMs"),
  };
}

function parseProcessTree(raw: unknown): ProcessTreeSnapshot {
  if (raw === null || typeof raw !== "object") throw new Error("processTree missing");
  const record = raw as Record<string, unknown>;
  if (typeof record.rootPid !== "number" || !Number.isSafeInteger(record.rootPid)) {
    throw new Error("processTree.rootPid invalid");
  }
  const status = record.typeProviderStatus;
  if (
    status !== "observed" &&
    status !== "missing" &&
    status !== "excluded" &&
    status !== "unknown"
  ) {
    throw new Error("processTree.typeProviderStatus invalid");
  }
  if (typeof record.typeProviderReason !== "string") {
    throw new Error("processTree.typeProviderReason required");
  }
  const pids = Array.isArray(record.typeProviderPids)
    ? record.typeProviderPids.map((pid) => {
        if (typeof pid !== "number" || !Number.isSafeInteger(pid)) {
          throw new Error("processTree.typeProviderPids invalid");
        }
        return pid;
      })
    : invalid("processTree.typeProviderPids");
  const members = Array.isArray(record.members)
    ? record.members.map((member, index) => parseMember(member, index))
    : invalid("processTree.members");
  return {
    rootPid: record.rootPid,
    members,
    typeProviderStatus: status,
    typeProviderPids: pids,
    typeProviderReason: record.typeProviderReason,
  };
}

function parseMember(raw: unknown, index: number): ProcessTreeSnapshot["members"][number] {
  if (raw === null || typeof raw !== "object")
    throw new Error(`processTree.members[${index}] invalid`);
  const record = raw as Record<string, unknown>;
  if (typeof record.pid !== "number" || !Number.isSafeInteger(record.pid)) {
    throw new Error(`processTree.members[${index}].pid invalid`);
  }
  const parentPid =
    record.parentPid === null
      ? null
      : typeof record.parentPid === "number" && Number.isSafeInteger(record.parentPid)
        ? record.parentPid
        : invalid(`processTree.members[${index}].parentPid`);
  if (typeof record.image !== "string")
    throw new Error(`processTree.members[${index}].image invalid`);
  if (record.role !== "ide" && record.role !== "provider" && record.role !== "descendant") {
    throw new Error(`processTree.members[${index}].role invalid`);
  }
  return {
    pid: record.pid,
    parentPid,
    image: record.image,
    role: record.role,
    rssBytes: parseMetric(record.rssBytes, `processTree.members[${index}].rssBytes`),
  };
}

function parseWorkflows(raw: unknown, side: Side): readonly CorrectnessRow[] {
  if (!Array.isArray(raw)) throw new Error("workflows must be an array");
  return raw.map((row, index) => {
    if (row === null || typeof row !== "object") throw new Error(`workflows[${index}] invalid`);
    const record = row as Record<string, unknown>;
    if (!(COMPARISON_WORKFLOWS as readonly string[]).includes(record.workflow as string)) {
      throw new Error(`workflows[${index}].workflow unknown`);
    }
    if (record.side !== side) throw new Error(`workflows[${index}].side must match capture side`);
    if (!isCompletenessState(record.completeness)) {
      throw new Error(`workflows[${index}].completeness invalid`);
    }
    if (typeof record.reason !== "string") throw new Error(`workflows[${index}].reason required`);
    return {
      workflow: record.workflow as ComparisonWorkflowId,
      side,
      completeness: record.completeness,
      reason: record.reason,
    };
  });
}

function parseBasis(raw: unknown): ProductReceiptBasis {
  if (raw === null || typeof raw !== "object") throw new Error("receiptBasis missing");
  const record = raw as Record<string, unknown>;
  const field = (name: keyof ProductReceiptBasis): string => {
    const value = record[name];
    if (typeof value !== "string" || value.length === 0)
      throw new Error(`receiptBasis.${name} required`);
    return value;
  };
  const completenessState = record.completenessState;
  if (!isCompletenessState(completenessState)) {
    throw new Error("receiptBasis.completenessState invalid");
  }
  return {
    sourceRevisions: field("sourceRevisions"),
    projectConfiguration: field("projectConfiguration"),
    engineIdentity: field("engineIdentity"),
    hostIdentity: field("hostIdentity"),
    completenessState,
  };
}

function parseOptionalAggregates(
  record: Record<string, unknown>,
): Pick<IdeCapture, "retainedMemory" | "providerCpu" | "providerWall" | "outboundBytes"> {
  const out: {
    retainedMemory?: IdeCapture["retainedMemory"];
    providerCpu?: IdeCapture["providerCpu"];
    providerWall?: IdeCapture["providerWall"];
    outboundBytes?: IdeCapture["outboundBytes"];
  } = {};
  if (record.retainedMemory !== undefined) {
    out.retainedMemory = parseMetric(record.retainedMemory, "retainedMemory");
  }
  if (record.providerCpu !== undefined) {
    out.providerCpu = parseMetric(record.providerCpu, "providerCpu");
  }
  if (record.providerWall !== undefined) {
    out.providerWall = parseMetric(record.providerWall, "providerWall");
  }
  if (record.outboundBytes !== undefined) {
    out.outboundBytes = parseMetric(record.outboundBytes, "outboundBytes");
  }
  return out;
}

function parseMetric(raw: unknown, path: string): PaintSample["uiApplyPaintMs"] {
  if (raw === null || typeof raw !== "object") throw new Error(`${path} missing`);
  const record = raw as Record<string, unknown>;
  if (record.status === "unknown") {
    if (typeof record.reason !== "string" || record.reason.length === 0) {
      throw new Error(`${path}.reason required for unknown metrics`);
    }
    if ("value" in record && record.value !== undefined) {
      throw new Error(
        `${path}: unknown metric must not carry a value (guessed numbers are forbidden)`,
      );
    }
    return { status: "unknown", reason: record.reason };
  }
  if (record.status !== "measured") throw new Error(`${path}.status invalid`);
  if (typeof record.value !== "number" || !Number.isFinite(record.value)) {
    throw new Error(`${path}.value invalid`);
  }
  if (typeof record.unit !== "string" || record.unit.length === 0) {
    throw new Error(`${path}.unit required`);
  }
  return { status: "measured", value: record.value, unit: record.unit };
}

function invalid(path: string): never {
  throw new Error(`${path} invalid`);
}

export function runPairedCampaign(input: {
  readonly seed: number;
  readonly sessionState: IdeCapture["sessionState"];
  readonly first: { readonly official: IdeCapture; readonly verter: IdeCapture };
  readonly swapped: { readonly official: IdeCapture; readonly verter: IdeCapture };
  readonly noiseBound: NoiseBound;
}): {
  readonly first: PairRecord;
  readonly swapped: PairRecord;
  readonly comparable: ReturnType<typeof compareSwappedPairs>;
} {
  const firstCaptures = pairOrderCaptures(input.seed, input.first.official, input.first.verter);
  const first = labelOutliers(
    recordPair({ seed: input.seed, sessionState: input.sessionState, captures: firstCaptures }),
    input.noiseBound,
  );
  const swappedSeed = input.seed ^ 1;
  const swappedCaptures = pairOrderCaptures(
    swappedSeed,
    input.swapped.official,
    input.swapped.verter,
  );
  const swapped = labelOutliers(
    recordPair({ seed: swappedSeed, sessionState: input.sessionState, captures: swappedCaptures }),
    input.noiseBound,
  );
  if (swapped.order[0] !== swappedOrder(first.order)[0]) {
    throw new Error("internal: swapped seed did not invert pair order");
  }
  return {
    first,
    swapped,
    comparable: compareSwappedPairs(first, swapped, input.noiseBound),
  };
}

function pairOrderCaptures(
  seed: number,
  official: IdeCapture,
  verter: IdeCapture,
): readonly IdeCapture[] {
  return (seed & 1) === 0 ? [official, verter] : [verter, official];
}

export function rejectRpcOnlyAsPaint(capture: IdeCapture): string | null {
  const ui = classifyUiEvidence(capture);
  if (ui.admissible) return null;
  return ui.reason;
}
