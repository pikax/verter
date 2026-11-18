import { hashBlockContent } from "@verter/native";
import type {
  HostBlockContentCapturedEchoFields,
  HostBlockOverrideRefusalCode,
  HostPreprocessorDiagnostic,
  HostPreprocessorRequest,
  HostUpdateResult,
  NativeBlockOverrideEntry,
  VerterHost,
} from "@verter/native";
import type { ProcessedStageId } from "../host-session/identity";
import type { SessionBinding } from "../host-session/session";
import { copyCapturedBlockContentEcho, preprocessBlock } from "../preprocessor";
import { PipelineRefusalError } from "./admission";
import type { DemandContext } from "./carrier-demand";
import type { PipelineCounters } from "./counters";

/**
 * One externally processed block, qualified by what it was made from and by
 * what made it.
 */
export interface ProcessedContentEnvelope {
  /** The host's stamps for the request, returned unchanged. */
  readonly echo: HostBlockContentCapturedEchoFields;
  readonly sourceSpaceToken: HostPreprocessorRequest["sourceSpaceToken"];
  /** Hash of the authored content the stage consumed. */
  readonly inputHash: HostPreprocessorRequest["contentHash"];
  readonly stage: ProcessedStageId;
  /** The authored `lang`. */
  readonly dialect: string;
  /** The tool that ran, e.g. `sass`, `vite:preprocessCSS`, `custom:<type>`. */
  readonly processorIdentity: string;
  readonly processorVersion: string | null;
  /** The options the tool ran with, as text; equal options give equal text. */
  readonly configFingerprint: string;
  readonly code: string;
  readonly codeHash: NativeBlockOverrideEntry["codeHash"];
  readonly sourceMap: string | null;
  readonly sourceMapHash: NativeBlockOverrideEntry["codeHash"] | null;
  /** Files the stage read besides the block itself. */
  readonly dependencies: readonly string[];
  readonly diagnostics: readonly HostPreprocessorDiagnostic[];
}

export function stageIdOf(request: HostPreprocessorRequest): ProcessedStageId {
  return `${request.contentClass}:${request.lang}` as ProcessedStageId;
}

/**
 * Run the external stage for one request. `null` when no tool is available for
 * the dialect. Throws when a tool is available and fails.
 */
export async function processBlock(
  request: HostPreprocessorRequest,
  carrier: string,
  binding: SessionBinding,
): Promise<ProcessedContentEnvelope | null> {
  const result = await preprocessBlock(
    request,
    carrier,
    binding.hostConfig,
    binding.options.customBlocks,
  );
  if (!result) return null;
  const sourceMap = result.sourceMap ?? null;
  return {
    echo: copyCapturedBlockContentEcho(request),
    sourceSpaceToken: request.sourceSpaceToken,
    inputHash: request.contentHash,
    stage: stageIdOf(request),
    dialect: request.lang,
    processorIdentity: result.processor,
    processorVersion: result.version,
    configFingerprint: JSON.stringify(result.config),
    code: result.code,
    codeHash: hashBlockContent(result.code),
    sourceMap,
    sourceMapHash: sourceMap === null ? null : hashBlockContent(sourceMap),
    dependencies: result.dependencies,
    diagnostics: [],
  };
}

/** The wire form of an envelope. */
export function toBlockOverride(envelope: ProcessedContentEnvelope): NativeBlockOverrideEntry {
  return {
    ...envelope.echo,
    sourceSpaceToken: envelope.sourceSpaceToken,
    code: envelope.code,
    codeHash: envelope.codeHash,
    sourceMap: envelope.sourceMap ?? undefined,
    sourceMapHash: envelope.sourceMapHash ?? undefined,
    dependencies: [...envelope.dependencies],
    diagnostics: [...envelope.diagnostics],
    processorIdentity: envelope.processorIdentity,
    processorVersion: envelope.processorVersion ?? undefined,
    configFingerprint: envelope.configFingerprint,
    suppliedProvenance: "@verter/unplugin",
  };
}

/**
 * The pipeline's refusal for a host error that refused supplied content, from
 * the error's typed `code`; `null` for any other error.
 */
function refusalFromOverrideError(error: unknown, carrier: string): PipelineRefusalError | null {
  if (error === null || typeof error !== "object") return null;
  const { code, message } = error as { code?: unknown; message?: unknown };
  const text = typeof message === "string" ? message : "";
  switch (code as HostBlockOverrideRefusalCode | undefined) {
    // The stamps no longer describe the block: an older revision or basis, or a
    // block the host no longer holds.
    case "BlockContentStale":
    case "BlockContentBasisMismatch":
    case "BlockContentCorrelationMismatch":
    case "BlockContentOwnerMissing":
      return new PipelineRefusalError({
        kind: "stale",
        message: text || `${carrier}: processed content was made for an older revision`,
      });
    case "BlockContentUnavailable":
      return new PipelineRefusalError({
        kind: "need-inputs",
        inputs: [],
        message: text || `${carrier}: the host requires content it was not given`,
      });
    default:
      return null;
  }
}

/**
 * Process every request of `upsert` that this host owns (style requests are
 * the bundler's when it has a CSS pipeline), hand the results to the native
 * host in one call, and register their dependencies with the bundler.
 * A host refusal rejects with `PipelineRefusalError` (`need-inputs` or `stale`).
 */
export async function applyProcessedContent(
  host: VerterHost,
  carrier: string,
  upsert: HostUpdateResult,
  binding: SessionBinding,
  ctx: DemandContext,
  counters: PipelineCounters,
): Promise<readonly ProcessedContentEnvelope[]> {
  const envelopes: ProcessedContentEnvelope[] = [];
  for (const request of upsert.preprocessorRequests ?? []) {
    // The bundler's own CSS pipeline handles styles.
    if (binding.hostCssPipeline && request.contentClass === "style") continue;
    const envelope = await processBlock(request, carrier, binding);
    if (envelope) envelopes.push(envelope);
  }
  if (envelopes.length === 0) return envelopes;

  try {
    host.applyBlockOverrides({
      canonicalId: carrier,
      overrides: envelopes.map(toBlockOverride),
    });
  } catch (error) {
    throw refusalFromOverrideError(error, carrier) ?? error;
  }
  for (const envelope of envelopes) {
    counters.preparedInputBytes += Buffer.byteLength(envelope.code);
    for (const dependency of envelope.dependencies) ctx.addWatchFile?.(dependency);
  }
  return envelopes;
}
