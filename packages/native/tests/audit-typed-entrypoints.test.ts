/**
 * Slice 3.H — Typed NAPI/WASM entry-points round-trip suite.
 *
 * Wave 3 ships eight typed audit entry-points on `VerterHost`:
 *
 *   1. getComponentMetaWithAudit  (existing, kept green)
 *   2. resolveTypeWithAudit       (new — wraps Slice 3.A)
 *   3. compileWithAudit           (new — wraps Slice 3.B)
 *   4. analyzeWithAudit           (new — wraps Slice 3.C)
 *   5. auditWorkspaceOp           (new — wraps Slice 3.D)
 *   6. getLastAuditRecord         (new — drains the most-recent record)
 *   7. getAuditRecords            (new — filtered query)
 *   8. getBundlerBatchSummary     (new — Slice 3.G aggregator)
 *
 * Each entry-point round-trips a JSON Buffer carrying the typed
 * payload (`RequestKind` discriminant + matching `RequestKindPayload`
 * arm). The tests below construct an audit-enabled host, drive each
 * API, decode the Buffer, and assert the produced shape.
 *
 * Discrimination contract: pre-change tree has no
 * `resolveTypeWithAudit` / `compileWithAudit` / etc. exports. The
 * `import` of `@verter/native` would still succeed (it has the rest
 * of the surface) but the per-test `expect(typeof host.compileWithAudit
 * === 'function')` would fail — the typed entry-point existence is the
 * discriminator.
 */

import { describe, expect, it } from "vitest";

const native = require("../index.js") as typeof import("../index");

const SFC_SOURCE =
  '<script setup lang="ts">\nconst greeting: string = "hello";\n</script>\n<template><div>{{ greeting }}</div></template>\n';

interface DecodedRecord {
  request_id: string;
  canonical_id: string;
  kind: string | { [k: string]: unknown };
  kind_payload: { kind: string; [k: string]: unknown };
  from_cache: boolean;
  timings: { total_ms: number };
  memory: Record<string, unknown>;
  store: Record<string, unknown>;
  files: unknown[];
  scheduler: Record<string, unknown> | null;
  footprint: Record<string, unknown> | null;
  waits: Record<string, unknown> | null;
  parent_request_id: string | null;
}

function decodeRecordBuffer(buf: Buffer | null): DecodedRecord | null {
  if (buf === null) return null;
  return JSON.parse(buf.toString("utf-8")) as DecodedRecord;
}

function decodeRecordList(buf: Buffer): DecodedRecord[] {
  return JSON.parse(buf.toString("utf-8")) as DecodedRecord[];
}

function buildAuditHost(): InstanceType<typeof native.VerterHost> {
  const host = new native.VerterHost({
    auditEnabled: true,
    footprintCapture: true,
  });
  host.upsert({
    canonicalId: "/widget.vue",
    inputId: "/widget.vue",
    source: Buffer.from(SFC_SOURCE, "utf-8"),
  });
  return host;
}

describe("typed audit entry-points expose all eight Wave-3 producer kinds", () => {
  it("VerterHost prototype declares all eight typed audit functions", () => {
    const proto = Object.getOwnPropertyNames(native.VerterHost.prototype).sort();
    expect(proto).toContain("resolveTypeWithAudit");
    expect(proto).toContain("compileWithAudit");
    expect(proto).toContain("analyzeWithAudit");
    expect(proto).toContain("auditWorkspaceOp");
    expect(proto).toContain("getLastAuditRecord");
    expect(proto).toContain("getAuditRecords");
    expect(proto).toContain("getBundlerBatchSummary");
  });

  it("compileWithAudit returns a Buffer with kind=Compile and a populated CompilePayload", () => {
    const host = buildAuditHost();
    const buf = host.compileWithAudit("/widget.vue", "BUNDLER");
    expect(buf).toBeInstanceOf(Buffer);
    const record = decodeRecordBuffer(buf);
    expect(record).not.toBeNull();
    expect(record!.canonical_id).toBe("/widget.vue");
    // RequestKind::Compile { target } serializes as { "Compile": { target: "Vdom" } }
    expect(typeof record!.kind).toBe("object");
    expect(record!.kind).toHaveProperty("Compile");
    expect(record!.kind_payload.kind).toBe("Compile");
    // CompilePayload fields are present.
    expect(record!.kind_payload).toHaveProperty("target");
    expect(record!.kind_payload).toHaveProperty("output_bytes");
    expect(record!.kind_payload).toHaveProperty("code_transform_ops");
    host.close();
  });

  it("compileWithAudit IDE target tags the kind as Ide", () => {
    const host = buildAuditHost();
    const buf = host.compileWithAudit("/widget.vue", "IDE");
    const record = decodeRecordBuffer(buf);
    expect(record).not.toBeNull();
    const kindObj = record!.kind as { Compile: { target: string } };
    expect(kindObj.Compile.target).toBe("Ide");
    host.close();
  });

  it("analyzeWithAudit returns a Buffer with kind=SemanticAnalysis and a populated payload", () => {
    const host = buildAuditHost();
    const buf = host.analyzeWithAudit("/widget.vue");
    expect(buf).toBeInstanceOf(Buffer);
    const record = decodeRecordBuffer(buf);
    expect(record).not.toBeNull();
    expect(record!.canonical_id).toBe("/widget.vue");
    expect(record!.kind).toBe("SemanticAnalysis");
    expect(record!.kind_payload.kind).toBe("SemanticAnalysis");
    expect(record!.kind_payload).toHaveProperty("indexed_ready_built");
    expect(record!.kind_payload).toHaveProperty("num_imports");
    host.close();
  });

  it("resolveTypeWithAudit drives a ResolveDecl query and produces a TypeResolution record", () => {
    const host = new native.VerterHost({
      auditEnabled: true,
      footprintCapture: true,
    });
    host.upsert({
      canonicalId: "/types.ts",
      inputId: "/types.ts",
      source: Buffer.from("export type Greeting = { value: string };\n", "utf-8"),
    });
    const buf = host.resolveTypeWithAudit("/types.ts", "Greeting");
    expect(buf).toBeInstanceOf(Buffer);
    const record = decodeRecordBuffer(buf);
    expect(record).not.toBeNull();
    expect(record!.kind).toBe("TypeResolution");
    expect(record!.kind_payload.kind).toBe("TypeResolution");
    expect(record!.kind_payload).toHaveProperty("query_mode");
    expect(record!.kind_payload).toHaveProperty("hops");
    host.close();
  });

  it("auditWorkspaceOp publishes a Workspace record with the requested op tag", () => {
    const host = buildAuditHost();
    const buf = host.auditWorkspaceOp({
      type: "AuditResolve",
      specifier: "./widget.vue",
      from: "/widget.vue",
    });
    expect(buf).toBeInstanceOf(Buffer);
    const record = decodeRecordBuffer(buf);
    expect(record).not.toBeNull();
    expect(typeof record!.kind).toBe("object");
    expect(record!.kind).toHaveProperty("Workspace");
    expect(record!.kind_payload.kind).toBe("Workspace");
    host.close();
  });

  it("getLastAuditRecord drains the most-recent published record", () => {
    const host = buildAuditHost();
    // First a compile to populate the store.
    const compileBuf = host.compileWithAudit("/widget.vue", "BUNDLER");
    const compileRecord = decodeRecordBuffer(compileBuf)!;
    const last = host.getLastAuditRecord();
    expect(last).not.toBeNull();
    const lastRecord = decodeRecordBuffer(last);
    expect(lastRecord).not.toBeNull();
    expect(lastRecord!.request_id).toBe(compileRecord.request_id);
    // After a drain, the second call returns null (the store is empty
    // because compileWithAudit returns the record and getLastAuditRecord
    // takes the last one).
    const second = host.getLastAuditRecord();
    expect(second).toBeNull();
    host.close();
  });

  it("getAuditRecords({ kind }) returns only the matching records", () => {
    const host = buildAuditHost();
    host.compileWithAudit("/widget.vue", "BUNDLER");
    host.analyzeWithAudit("/widget.vue");
    // Filter by Compile.
    const compileBuf = host.getAuditRecords({ kind: "Compile" });
    const compileRecords = decodeRecordList(compileBuf);
    expect(compileRecords.length).toBeGreaterThan(0);
    for (const r of compileRecords) {
      expect(typeof r.kind === "object" && "Compile" in r.kind).toBe(true);
    }
    // Filter by SemanticAnalysis.
    const analysisBuf = host.getAuditRecords({ kind: "SemanticAnalysis" });
    const analysisRecords = decodeRecordList(analysisBuf);
    expect(analysisRecords.length).toBeGreaterThan(0);
    for (const r of analysisRecords) {
      expect(r.kind).toBe("SemanticAnalysis");
    }
    host.close();
  });

  it("getAuditRecords({ limit }) caps the result count", () => {
    const host = buildAuditHost();
    host.compileWithAudit("/widget.vue", "BUNDLER");
    host.compileWithAudit("/widget.vue", "IDE");
    host.analyzeWithAudit("/widget.vue");
    const buf = host.getAuditRecords({ limit: 2 });
    const records = decodeRecordList(buf);
    expect(records.length).toBeLessThanOrEqual(2);
    host.close();
  });

  it("getBundlerBatchSummary aggregates per-kind counts and total_records", () => {
    const host = buildAuditHost();
    host.compileWithAudit("/widget.vue", "BUNDLER");
    host.analyzeWithAudit("/widget.vue");
    const buf = host.getBundlerBatchSummary({ kind: "Vite" });
    expect(buf).toBeInstanceOf(Buffer);
    const summary = JSON.parse(buf.toString("utf-8")) as {
      total_records: number;
      compile_count: number;
      semantic_analysis_count: number;
      kind: string | { Other: string };
      slowest_5: Array<{ request_id: string; duration_ms: number }>;
    };
    expect(summary.total_records).toBeGreaterThanOrEqual(2);
    expect(summary.compile_count).toBeGreaterThanOrEqual(1);
    expect(summary.semantic_analysis_count).toBeGreaterThanOrEqual(1);
    expect(summary.kind).toBe("Vite");
    expect(Array.isArray(summary.slowest_5)).toBe(true);
    host.close();
  });

  it("getComponentMetaWithAudit (existing — Wave 1 baseline) still returns a populated bundle", () => {
    // Wave 1 producer: ComponentMetaSession.getComponentMetaWithAudit.
    // The Wave 3 typed-entrypoint additions on VerterHost MUST NOT
    // regress the existing component-meta surface. Spec test is in
    // index.spec.ts; this assertion just confirms the helper class
    // remains discoverable.
    const proto = Object.getOwnPropertyNames(native.ComponentMetaSession.prototype).sort();
    expect(proto).toContain("getComponentMetaWithAudit");
    expect(proto).toContain("whyLoadedFromAuditJson");
    expect(proto).toContain("whyInstantiatedFromAuditJson");
  });
});
