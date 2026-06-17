/**
 * The curated semantic-oracle RUNNER.
 *
 * For one `.vue` scenario probe and its paired `.ts` oracle anchor it queries
 * verter on the `.vue` (via the `LspClient`) and tsgo/tsserver on the `.ts`
 * gold standard (via the `verter_dx_baseline` bridge), folds both into the shared
 * normalized facts, and runs the `vueSemanticValidity` diff — producing the same
 * {@link DifferentialOutcome} union the artifact-parity differential emits, so the
 * report layer consumes oracle outcomes identically (distinguished only by
 * `outcome.probe.dimension === "vueSemanticValidity"`).
 *
 * The orchestration depends on NARROW client interfaces ({@link OracleVerterClient}
 * / {@link OracleBridgeClient}) the real `LspClient`/`BridgeClient` structurally
 * satisfy, so the runner's logic is unit-tested with in-memory fakes without
 * spawning either binary; the live env-gated path wires the real clients.
 *
 * The runner takes the oracle `.ts` source + a `.vue` scenario as INPUTS: the full
 * paired corpus and its workspace materialization are supplied separately, so this
 * machinery is corpus-agnostic.
 */

import {
  DocumentPositions,
  type LspPosition,
  type PositionEncoding,
} from "@verter/lsp-test-client";

import { requireAnchor, type AnchorMap } from "../anchors.js";
import type {
  ErrorResponse,
  ProviderName,
  QueryInput,
  QueryResponse,
} from "../baseline/bridgeClient.js";
import {
  classifyOracleCompletion,
  classifyOracleDefinition,
  classifyOracleHover,
} from "../differential/vueSemanticValidity.js";
import {
  skipped,
  type DifferentialOutcome,
  type ProviderInputs,
  type ProviderResult,
} from "../differential/index.js";
import type {
  CompletionResponse,
  DefinitionResponse,
  HoverResponse,
} from "../normalize/lspTypes.js";
import type { Probe } from "../scenario/index.js";
import {
  bridgeCompletionFact,
  bridgeDefinitionFact,
  bridgeHoverFact,
  verterCompletionFact,
  verterDefinitionFact,
  verterHoverFact,
} from "./facts.js";
import { isOracleQueryMethod, type OracleBinding, type SemanticOracle } from "./model.js";
import { requireOracleByteOffset, type PreparedOracleSource } from "./prepare.js";

/** A typed error for oracle authoring faults (wrong dimension, unbound probe). */
export class OracleError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "OracleError";
  }
}

// ── narrow client interfaces ───────────────────────────────────────────────────

/** The slice of `@verter/lsp-test-client`'s `LspClient` the runner queries verter through. */
export interface OracleVerterClient {
  /** The negotiated position encoding the server reported at `initialize`. */
  readonly positionEncoding: PositionEncoding;
  sendRequest<T = unknown>(method: string, params?: unknown, timeout?: number): Promise<T>;
}

/** The slice of the `verter_dx_baseline` `BridgeClient` the runner queries an oracle provider through. */
export interface OracleBridgeClient {
  query(input: QueryInput): Promise<QueryResponse | ErrorResponse>;
}

/** The verter client plus the per-provider oracle bridges (each optional). */
export interface OracleProviders {
  readonly verter: OracleVerterClient;
  readonly tsgo?: OracleBridgeClient;
  readonly tsserver?: OracleBridgeClient;
  /** When set, verter is compared only against this provider (disagreement ignored). */
  readonly authoritativeProvider?: ProviderName;
  /** Per-request verter timeout (ms); the client's default when omitted. */
  readonly requestTimeoutMs?: number;
}

// ── position resolution ─────────────────────────────────────────────────────────

/**
 * Fold a UTF-16 anchor position into an LSP position in the server's negotiated
 * `encoding`. verter-lsp can select UTF-8, in which case the column is a byte count
 * — so the authored UTF-16 column must be re-measured against the document text
 * rather than passed through verbatim.
 */
export function resolveLspPosition(
  text: string,
  anchor: { readonly line: number; readonly character: number },
  encoding: PositionEncoding,
): LspPosition {
  const doc = new DocumentPositions(text);
  const utf16Offset = doc.positionToUtf16(
    { line: anchor.line, character: anchor.character },
    "utf-16",
  );
  return doc.utf16ToPosition(utf16Offset, encoding);
}

// ── resolved query ──────────────────────────────────────────────────────────────

/** A probe + binding with both anchors resolved into their query coordinates. */
export interface ResolvedOracleQuery {
  readonly probe: Probe;
  readonly binding: OracleBinding;
  /** The verter `.vue` document URI + the anchor position in the server's encoding. */
  readonly vue: { readonly uri: string; readonly position: LspPosition };
  /** The oracle `.ts` document: bridge `uri`/`path`, open `version`, anchor byte offset. */
  readonly oracle: {
    readonly uri: string;
    readonly path: string;
    readonly version: number;
    readonly offset: number;
  };
}

/** The `.vue` + `.ts` source context a {@link SemanticOracle}'s bindings resolve against. */
export interface OracleSourceContext {
  /** The verter `.vue` document URI. */
  readonly vueUri: string;
  /** The stripped `.vue` source text (for encoding-correct position resolution). */
  readonly vueText: string;
  /** The `.vue` anchor map (stripped positions). */
  readonly vueAnchors: AnchorMap;
  /** The oracle `.ts` document URI the bridge correlates. */
  readonly oracleUri: string;
  /** The oracle `.ts` path the bridge addresses. */
  readonly oraclePath: string;
  /** The bridge-open version of the oracle `.ts`. */
  readonly oracleVersion: number;
  /** The prepared oracle `.ts` (anchor byte offsets). */
  readonly oracle: PreparedOracleSource;
}

/** Resolve one probe + binding's anchors into query coordinates. */
export function resolveOracleQuery(
  probe: Probe,
  binding: OracleBinding,
  ctx: OracleSourceContext,
  encoding: PositionEncoding,
): ResolvedOracleQuery {
  const vueAnchor = requireAnchor(ctx.vueAnchors, probe.anchor);
  const position = resolveLspPosition(ctx.vueText, vueAnchor, encoding);
  const offset = requireOracleByteOffset(ctx.oracle, binding.oracleAnchor);
  return {
    probe,
    binding,
    vue: { uri: ctx.vueUri, position },
    oracle: { uri: ctx.oracleUri, path: ctx.oraclePath, version: ctx.oracleVersion, offset },
  };
}

// ── execution ───────────────────────────────────────────────────────────────────

/** Query both present oracle bridges at one `QueryInput`, folding each to a `ProviderResult`. */
async function queryBothBridges<B>(
  providers: OracleProviders,
  input: QueryInput,
  extract: (response: QueryResponse | ErrorResponse) => ProviderResult<B>,
): Promise<ProviderInputs<B>> {
  // The two baseline providers are independent bridge sessions, so query them
  // concurrently — the live path pays one round-trip, not two in series.
  const [tsgo, tsserver] = await Promise.all([
    providers.tsgo?.query(input),
    providers.tsserver?.query(input),
  ]);
  const inputs: { tsgo?: ProviderResult<B>; tsserver?: ProviderResult<B> } = {};
  if (tsgo !== undefined) inputs.tsgo = extract(tsgo);
  if (tsserver !== undefined) inputs.tsserver = extract(tsserver);
  return inputs;
}

/**
 * Run one resolved oracle query: fetch verter's `.vue` fact and the oracle
 * providers' `.ts` facts, then classify through the `vueSemanticValidity` diff.
 *
 * @throws {OracleError} if the probe is not `vueSemanticValidity` — the dimension
 *   contract is a hard authoring invariant, not a silent reclassification.
 */
export async function runResolvedOracleQuery(
  resolved: ResolvedOracleQuery,
  providers: OracleProviders,
): Promise<DifferentialOutcome[]> {
  const { probe, binding } = resolved;
  if (probe.dimension !== "vueSemanticValidity") {
    throw new OracleError(
      `oracle probe "${probe.id}" must carry dimension "vueSemanticValidity", got "${probe.dimension}"`,
    );
  }
  // Verter-side diagnostics are push-delivered (publishDiagnostics), not a pull
  // request/response like completion/hover/definition, so the dedicated diagnostics
  // collector drives them in the signal-collectors layer — a deliberate runner/
  // collector boundary, not an unimplemented method. The per-query runner drives the
  // three request/response query methods only; any other method skips with its reason.
  if (!isOracleQueryMethod(probe.method)) {
    const reason =
      probe.method === "diagnostics"
        ? "diagnostics are push-delivered (publishDiagnostics), not a pull query; the " +
          "diagnostics collector drives them in the signal-collectors layer"
        : `the live oracle query runner does not drive method "${probe.method}"`;
    return [skipped(probe, { reason })];
  }

  const timeout = providers.requestTimeoutMs;
  const authoritativeProvider = providers.authoritativeProvider;
  const verterTarget = { textDocument: { uri: resolved.vue.uri }, position: resolved.vue.position };
  const queryInput: QueryInput = {
    method: probe.method,
    uri: resolved.oracle.uri,
    path: resolved.oracle.path,
    offset: resolved.oracle.offset,
    version: resolved.oracle.version,
    ...(binding.triggerCharacter !== undefined
      ? { triggerCharacter: binding.triggerCharacter }
      : {}),
  };

  switch (probe.method) {
    case "hover": {
      const raw = await providers.verter.sendRequest<HoverResponse>(
        "textDocument/hover",
        verterTarget,
        timeout,
      );
      const inputs = await queryBothBridges(providers, queryInput, bridgeHoverFact);
      return classifyOracleHover({
        probe,
        verter: verterHoverFact(raw),
        providers: inputs,
        ...(binding.requiredSnippets !== undefined
          ? { requiredSnippets: binding.requiredSnippets }
          : {}),
        ...(authoritativeProvider !== undefined ? { authoritativeProvider } : {}),
      });
    }
    case "completion": {
      const context =
        binding.triggerCharacter !== undefined
          ? { triggerKind: 2, triggerCharacter: binding.triggerCharacter }
          : { triggerKind: 1 };
      const raw = await providers.verter.sendRequest<CompletionResponse>(
        "textDocument/completion",
        { ...verterTarget, context },
        timeout,
      );
      const inputs = await queryBothBridges(providers, queryInput, bridgeCompletionFact);
      return classifyOracleCompletion({
        probe,
        verter: verterCompletionFact(raw),
        providers: inputs,
        ...(binding.requiredLabels !== undefined ? { requiredLabels: binding.requiredLabels } : {}),
        ...(authoritativeProvider !== undefined ? { authoritativeProvider } : {}),
      });
    }
    case "definition": {
      // A definition oracle MUST carry an authored Vue identity: the oracle `.ts`
      // resolves into a different file, so without `expected` any resolved `.vue`
      // target would pass as a false agreement. Absence is an authoring fault, raised
      // before verter is queried — never a silent agreement.
      const expected = binding.expected;
      if (expected === undefined) {
        throw new OracleError(
          `definition oracle probe "${probe.id}" requires an \`expected\` authored Vue identity`,
        );
      }
      const raw = await providers.verter.sendRequest<DefinitionResponse>(
        "textDocument/definition",
        verterTarget,
        timeout,
      );
      const inputs = await queryBothBridges(providers, queryInput, bridgeDefinitionFact);
      return classifyOracleDefinition({
        probe,
        verter: verterDefinitionFact(raw),
        providers: inputs,
        expected,
        ...(authoritativeProvider !== undefined ? { authoritativeProvider } : {}),
      });
    }
  }
}

/**
 * Run every binding of a {@link SemanticOracle}: resolve each `.vue` probe + `.ts`
 * anchor against the source context, then run the resolved query. The probe map is
 * the paired `.vue` scenario's probes, keyed by id.
 *
 * @throws {OracleError} if a binding references an unknown probe id.
 */
export async function runSemanticOracle(
  oracle: SemanticOracle,
  probes: ReadonlyMap<string, Probe>,
  ctx: OracleSourceContext,
  providers: OracleProviders,
): Promise<DifferentialOutcome[]> {
  const outcomes: DifferentialOutcome[] = [];
  for (const binding of oracle.bindings) {
    const probe = probes.get(binding.probeId);
    if (probe === undefined) {
      throw new OracleError(
        `oracle "${oracle.family}" binds unknown probe id "${binding.probeId}"`,
      );
    }
    const resolved = resolveOracleQuery(probe, binding, ctx, providers.verter.positionEncoding);
    outcomes.push(...(await runResolvedOracleQuery(resolved, providers)));
  }
  return outcomes;
}
