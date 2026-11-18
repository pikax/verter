import type { HostCompileResponse, HostCompileVirtualNode, HostMissingInput } from "@verter/native";
import { requireRuntimeMain, runtimeProduct } from "../compiler";
import type { CompiledStyle, PublishedMain } from "../host-session/catalog";
import { encodeFramed } from "../host-session/identity";

/** What one compile response yields for the bundler. */
export interface AdmittedCompile {
  readonly main: PublishedMain;
  /** Indexed by style block ordinal; no holes. */
  readonly styles: readonly CompiledStyle[];
  /** Nodes that are neither main nor style. */
  readonly nodes: readonly HostCompileVirtualNode[];
}

/** Why a pipeline step published nothing. */
export type PipelineRefusal =
  /** The host needs inputs the embedder has not supplied (or supplied for another basis). */
  | {
      readonly kind: "need-inputs";
      readonly inputs: readonly HostMissingInput[];
      readonly message: string;
    }
  /** The work belongs to an older epoch or a superseded revision. */
  | { readonly kind: "stale"; readonly message: string }
  /** The carrier cannot be read. */
  | { readonly kind: "carrier-unavailable"; readonly carrier: string }
  /** The carrier has no such block. */
  | { readonly kind: "module-not-published"; readonly request: string };

/** Thrown by the pipeline when a step refuses. `refusal` says why. */
export class PipelineRefusalError extends Error {
  constructor(
    readonly refusal: PipelineRefusal,
    /** The native failure the refusal was read from, when there was one. */
    readonly cause?: unknown,
  ) {
    super(`[verter] ${refusal.kind}: ${refusalSubject(refusal)}`);
    this.name = "PipelineRefusalError";
  }
}

/** What a refusal is about, for its message: the request, the carrier, or the host's text. */
function refusalSubject(refusal: PipelineRefusal): string {
  switch (refusal.kind) {
    case "need-inputs":
    case "stale":
      return refusal.message;
    case "carrier-unavailable":
      return refusal.carrier;
    case "module-not-published":
      return refusal.request;
  }
}

/**
 * The only reader of a thrown native compile failure. Returns `need-inputs`,
 * with the inputs the host names, when the error's `kind` is
 * `"dependenciesIncomplete"`; `null` for every other error. Reads typed
 * properties only, never the message.
 */
export function refusalFromCompileError(error: unknown): PipelineRefusal | null {
  if (error === null || typeof error !== "object") return null;
  const { kind, message, missingInputs } = error as {
    kind?: unknown;
    message?: unknown;
    missingInputs?: unknown;
  };
  if (kind !== "dependenciesIncomplete") return null;
  return {
    kind: "need-inputs",
    inputs: Array.isArray(missingInputs) ? (missingInputs as HostMissingInput[]) : [],
    message: typeof message === "string" ? message : "",
  };
}

/** Stable text identifying one missing input, for the per-demand supplied set. */
export function missingInputKey(input: HostMissingInput): string {
  switch (input.kind) {
    case "route":
      return encodeFramed([input.kind, input.importer, input.specifier]);
    case "file":
      return encodeFramed([input.kind, input.canonicalId]);
    case "lookup":
      return encodeFramed([input.kind, input.importSource, input.typeName]);
  }
}

/**
 * The inputs of one report the embedder can supply now: `file`, and `route`
 * that is not refused. Returns `"terminal"` when there is none, or when any of
 * them is already in `supplied` (the host asked again for what it was given).
 */
export function fillableInputs(
  inputs: readonly HostMissingInput[],
  supplied: ReadonlySet<string>,
): readonly HostMissingInput[] | "terminal" {
  const fillable = inputs.filter(
    (input) => input.kind === "file" || (input.kind === "route" && !input.refused),
  );
  if (fillable.length === 0) return "terminal";
  if (fillable.some((input) => supplied.has(missingInputKey(input)))) return "terminal";
  return fillable;
}

/**
 * The only reader of the native compile response.
 *
 * Throws when the response has no runtime main for the requested target.
 * Main and styles come from the nodes of the response's runtime product: the
 * main node, each style by block index, every other node as it is. Nothing is
 * inferred from diagnostic codes or messages.
 */
export function admitCompile(response: HostCompileResponse, ssr: boolean): AdmittedCompile {
  const mainNode = requireRuntimeMain(response, ssr, response.canonicalId);
  const styles: CompiledStyle[] = [];
  const nodes: HostCompileVirtualNode[] = [];
  for (const node of runtimeProduct(response, ssr)?.nodes ?? []) {
    if (node.node.kind === "main") continue;
    if (node.node.kind !== "style") {
      nodes.push(node);
      continue;
    }
    const index = node.node.index;
    if (index === undefined || index === null) {
      throw new Error(`[verter] ${response.canonicalId}: a compiled style has no block index`);
    }
    if (styles[index] !== undefined) {
      throw new Error(
        `[verter] ${response.canonicalId}: compiled style ${index} is published twice`,
      );
    }
    styles[index] = { code: node.code, map: node.sourceMap ?? null, lang: node.lang ?? "css" };
  }
  for (let index = 0; index < styles.length; index += 1) {
    if (styles[index] === undefined) {
      throw new Error(`[verter] ${response.canonicalId}: compiled style ${index} is missing`);
    }
  }
  return {
    main: { code: mainNode.code, map: mainNode.sourceMap ?? null, lang: mainNode.lang ?? null },
    styles,
    nodes,
  };
}
