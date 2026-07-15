/**
 * Pure decoders for the native typeinfo result DTOs.
 *
 * Kept free of any `@verter/native` import so the dispatch-fault
 * channel can be unit-tested without loading the native host binary.
 */

import type { TypeDescriptor } from "@verter/type-ir";

import { nativeToDescriptor } from "./native-to-descriptor.js";
import type { AuditRecord, ResolveSymbolResult } from "./types.js";

/**
 * Thrown when a typeinfo resolution faults inside the native dispatch
 * engine (`BudgetExceeded` / `UnstableState` / `AliasCycle` /
 * `UnsupportedIntrinsic` / `Other`). This is the public end of the
 * native carrier's `Err` channel: a genuine dispatch fault, distinct
 * from a well-formed request that resolved nothing (which returns
 * `{ type: undefined }`).
 *
 * The native `AuditedResult` carrier preserves the per-request audit
 * record on BOTH the `Ok` AND the `Err` arm (the Rust layer's
 * `split_resolve_outcome` keeps the record on faults). This TS layer
 * MUST NOT drop that envelope: the decoded record rides on
 * {@link TypeResolutionFaultError.auditRecord} so an audited fault
 * stays audited.
 */
export class TypeResolutionFaultError extends Error {
  /**
   * Per-request audit record decoded from the native carrier's `Err`
   * arm. `undefined` when audit was disabled / filtered for the
   * faulting request (empty native buffer).
   */
  readonly auditRecord?: AuditRecord;

  constructor(message: string, auditRecord?: AuditRecord) {
    super(message);
    this.name = "TypeResolutionFaultError";
    this.auditRecord = auditRecord;
  }
}

/**
 * Decode the optional audit-record buffer into an {@link AuditRecord}.
 * Returns `undefined` for an absent / empty buffer (audit disabled or
 * filtered).
 */
function decodeAuditRecord(buffer: Buffer | null): AuditRecord | undefined {
  if (buffer && buffer.length > 0) {
    return JSON.parse(buffer.toString("utf-8")) as AuditRecord;
  }
  return undefined;
}

/**
 * Decode a native `resolveSymbolWithAudit` /
 * `evaluateTypeExpressionWithAudit` result into the public
 * {@link ResolveSymbolResult}.
 *
 * A genuine dispatch fault (`BudgetExceeded` / `UnstableState` /
 * `AliasCycle` / `UnsupportedIntrinsic` / `Other`) rides the native
 * `error` channel. It is exceptional — distinct from a well-formed
 * request that simply resolved nothing (`type: undefined`) — so it
 * throws {@link TypeResolutionFaultError} rather than projecting to a
 * silent `undefined` type.
 */
export function decodeResolveResult(result: {
  typeExpr: Buffer | null;
  auditRecord: Buffer | null;
  // Optional so a native binding compiled before the `error` channel
  // landed still typechecks; absent / null both mean "no fault".
  error?: string | null;
}): ResolveSymbolResult {
  // Decode the audit envelope FIRST so it survives the fault path:
  // the native carrier preserves the record on the `Err` arm and the
  // public fault error must carry it through.
  const audit = decodeAuditRecord(result.auditRecord);
  if (result.error) {
    throw new TypeResolutionFaultError(result.error, audit);
  }
  let descriptor: TypeDescriptor | undefined;
  if (result.typeExpr && result.typeExpr.length > 0) {
    const native = JSON.parse(result.typeExpr.toString("utf-8"));
    descriptor = nativeToDescriptor(native);
  }
  return { type: descriptor, auditRecord: audit };
}
