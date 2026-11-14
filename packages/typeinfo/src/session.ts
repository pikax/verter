/**
 * `TypeInfoSession` — public surface around the Rust host typeinfo
 * substrate.
 *
 * Wraps `@verter/native`'s `VerterHost` to expose three audited
 * methods:
 *
 * 1. {@link TypeInfoSession.listSymbols} — shallow-state symbol inventory.
 * 2. {@link TypeInfoSession.resolveSymbol} — audited resolution of a
 *    named declaration with optional generic instantiation.
 * 3. {@link TypeInfoSession.evaluateTypeExpression} — audited
 *    evaluation of a synthetic type expression in a file scope.
 *
 * The session does not own a project graph, only a `VerterHost` —
 * consumers populate the host (`upsert`, `withWorkspace`) before
 * calling the session methods. The host is exposed via
 * {@link TypeInfoSession.host} for shared-host integration scenarios
 * (LSP, MCP, bundlers).
 */

import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { VerterHost, type AuditRecordFilter } from "@verter/native";
import {
  GraphOperation,
  TYPEINFO_GRAPH_SCHEMA_VERSION,
  TypeInfoGraphRequestSchema,
  TypeInfoGraphResponseSchema,
  type TypeInfoRequestError,
} from "@verter/proto";
import type { TypeDescriptor } from "@verter/type-ir";

import { decodeResolveResult } from "./decode.js";
import { descriptorToNative } from "./descriptor-to-native.js";
import type {
  EvaluateTypeExpressionRequest,
  EvaluateTypeExpressionResult,
  ResolveSymbolOpts,
  ResolveSymbolResult,
  SymbolEntry,
  TypeInfoSessionConfig,
} from "./types.js";

export class TypeInfoSession {
  /** Underlying native host. Exposed for shared-host integration. */
  public readonly host: VerterHost;

  private readonly config: TypeInfoSessionConfig;

  constructor(config: TypeInfoSessionConfig) {
    this.config = config;
    const auditEnabled = config.auditEnabled !== false;
    this.host = new VerterHost({
      auditEnabled,
      typeinfoScratchCacheCapacity: config.evaluateTypeExpressionCacheSize,
    });
  }

  /**
   * Return the top-level symbol inventory for `canonicalId`, answered by
   * the list-symbols operation of the host's one typeinfo entry. Spans are
   * UTF-8 byte offsets (the schema-12 unit contract).
   *
   * Throws {@link TypeInfoRequestFailure} when the host refuses the request,
   * for example `unknownSource` for a file it does not know.
   */
  listSymbols(canonicalId: string): SymbolEntry[] {
    const request = create(TypeInfoGraphRequestSchema, {
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      operation: GraphOperation.LIST_SYMBOLS,
      payload: { case: "listSymbols", value: { canonicalId } },
    });
    const { response } = this.host.typeInfoWithAudit(
      Buffer.from(toBinary(TypeInfoGraphRequestSchema, request)),
    );
    const kind = fromBinary(TypeInfoGraphResponseSchema, new Uint8Array(response)).kind;
    if (kind.case === "error") {
      throw new TypeInfoRequestFailure(kind.value.kind);
    }
    if (kind.case !== "symbolList") {
      throw new Error(`listSymbols: unexpected response arm '${String(kind.case)}'`);
    }
    return kind.value.entries.map<SymbolEntry>((entry) => ({
      name: entry.name,
      kind: entry.kind as SymbolEntry["kind"],
      spanEncoding: "utf8.v1" as const,
      span: entry.hasSpan ? { start: entry.spanStart, end: entry.spanEnd } : undefined,
      isExported: entry.isExported,
    }));
  }

  /**
   * Resolve `name` in `canonicalId`'s top-level scope, optionally
   * instantiating with `opts.typeArgs`, returning the resolved type
   * descriptor and the per-request audit record.
   */
  resolveSymbol(canonicalId: string, name: string, opts?: ResolveSymbolOpts): ResolveSymbolResult {
    const typeArgsBuf = encodeTypeArgs(opts?.typeArgs);
    const mode = opts?.mode ?? null;
    const result = this.host.resolveSymbolWithAudit(canonicalId, name, typeArgsBuf, mode);
    return decodeResolveResult(result);
  }

  /**
   * Evaluate a synthetic type expression in a file scope and return
   * the resolved type descriptor plus the per-request audit record.
   */
  evaluateTypeExpression(req: EvaluateTypeExpressionRequest): EvaluateTypeExpressionResult {
    const wire = {
      scope: req.scope,
      expression: req.expression,
      extraImports: (req.extraImports ?? []).map((spec) => ({
        specifier: spec.specifier,
        bindings: spec.bindings.map((b) => {
          switch (b.kind) {
            case "default":
              return {
                kind: "default",
                localName: b.localName,
                exportedName: "",
                localAlias: "",
                typeOnly: false,
              };
            case "named":
              return {
                kind: "named",
                localName: "",
                exportedName: b.exportedName,
                localAlias: b.localAlias ?? "",
                typeOnly: b.typeOnly ?? false,
              };
            case "namespace":
              return {
                kind: "namespace",
                localName: b.localName,
                exportedName: "",
                localAlias: "",
                typeOnly: false,
              };
          }
        }),
      })),
      mode: req.mode ?? "expanded",
      cacheable: req.cacheable ?? true,
    };
    const requestBuf = Buffer.from(JSON.stringify(wire), "utf-8");
    const result = this.host.evaluateTypeExpressionWithAudit(requestBuf);
    return decodeResolveResult(result);
  }
}

/** JSON-encode a `TypeRef[]` to the FFI-bound TypeExprList shape. */
function encodeTypeArgs(typeArgs: TypeDescriptor[] | undefined): Buffer | null {
  if (!typeArgs || typeArgs.length === 0) {
    return null;
  }
  const native = typeArgs.map(descriptorToNative);
  return Buffer.from(JSON.stringify(native), "utf-8");
}

/**
 * Re-export for convenience so callers can use a typed filter shape
 * if they want to consult the underlying audit-records store via
 * `session.host.getAuditRecords(filter)`.
 */
export type { AuditRecordFilter };

/** A typed request refusal from the host's typeinfo entry. */
export class TypeInfoRequestFailure extends Error {
  /** The typed wire error, for example `{ case: "unknownSource", value: { path } }`. */
  public readonly error: TypeInfoRequestError["kind"];

  constructor(error: TypeInfoRequestError["kind"]) {
    super(`typeinfo request refused: ${String(error.case)}`);
    this.name = "TypeInfoRequestFailure";
    this.error = error;
  }
}
