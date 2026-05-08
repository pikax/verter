/**
 * `TypeInfoSession` — public surface around the Rust host typeinfo
 * substrate.
 *
 * Wraps `@verter/native`'s `VerterHost` to expose three audited
 * methods (per typeinfo plan §5):
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

import { VerterHost, type AuditRecordFilter } from "@verter/native";
import type { TypeDescriptor } from "@verter/type-ir";

import { descriptorToNative } from "./descriptor-to-native.js";
import { nativeToDescriptor } from "./native-to-descriptor.js";
import type {
  AuditRecord,
  EvaluateTypeExpressionRequest,
  EvaluateTypeExpressionResult,
  ResolveSymbolOpts,
  ResolveSymbolResult,
  SymbolEntry,
  TypeInfoSessionConfig,
} from "./types.js";

/**
 * Pre-existing extensions the substrate adds to `VerterHost` for
 * Phase 4. Declared here as a structural augment so the package
 * builds against the published `@verter/native` types whether or not
 * the JS-side declaration has been refreshed.
 */
interface VerterHostTypeinfoExtensions {
  listSymbols(canonicalId: string): Buffer;
  resolveSymbolWithAudit(
    canonicalId: string,
    name: string,
    typeArgs: Buffer | null,
    mode: string | null,
  ): { typeExpr: Buffer | null; auditRecord: Buffer | null };
  evaluateTypeExpressionWithAudit(request: Buffer): {
    typeExpr: Buffer | null;
    auditRecord: Buffer | null;
  };
}

type VerterHostWithTypeinfo = VerterHost & VerterHostTypeinfoExtensions;

export class TypeInfoSession {
  /** Underlying native host. Exposed for shared-host integration. */
  public readonly host: VerterHostWithTypeinfo;

  private readonly config: TypeInfoSessionConfig;

  constructor(config: TypeInfoSessionConfig) {
    this.config = config;
    const auditEnabled = config.auditEnabled !== false;
    this.host = new VerterHost({
      auditEnabled,
      typeinfoScratchCacheCapacity: config.evaluateTypeExpressionCacheSize,
    }) as VerterHostWithTypeinfo;
  }

  /**
   * Return the top-level symbol inventory for `canonicalId`.
   *
   * Pure shallow read; no audit record is emitted.
   */
  listSymbols(canonicalId: string): SymbolEntry[] {
    const buf = this.host.listSymbols(canonicalId);
    if (!buf || buf.length === 0) {
      return [];
    }
    const raw = JSON.parse(buf.toString("utf-8")) as Array<{
      name: string;
      kind: string;
      spanStart: number;
      spanEnd: number;
      hasSpan: boolean;
      isExported: boolean;
    }>;
    return raw.map<SymbolEntry>((r) => ({
      name: r.name,
      kind: r.kind as SymbolEntry["kind"],
      span: r.hasSpan ? { start: r.spanStart, end: r.spanEnd } : undefined,
      isExported: r.isExported,
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

function decodeResolveResult(result: {
  typeExpr: Buffer | null;
  auditRecord: Buffer | null;
}): ResolveSymbolResult {
  let descriptor: TypeDescriptor | undefined;
  if (result.typeExpr && result.typeExpr.length > 0) {
    const native = JSON.parse(result.typeExpr.toString("utf-8"));
    descriptor = nativeToDescriptor(native);
  }
  let audit: AuditRecord | undefined;
  if (result.auditRecord && result.auditRecord.length > 0) {
    audit = JSON.parse(result.auditRecord.toString("utf-8")) as AuditRecord;
  }
  return { type: descriptor, auditRecord: audit };
}

/**
 * Re-export for convenience so callers can use a typed filter shape
 * if they want to consult the underlying audit-records store via
 * `session.host.getAuditRecords(filter)`.
 */
export type { AuditRecordFilter };
