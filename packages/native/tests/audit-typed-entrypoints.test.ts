/**
 * Typed NAPI audit entry-points round-trip suite.
 *
 * `VerterHost` ships typed audit entry-points:
 *
 *   1. getComponentMetaWithAudit  (on ComponentMetaSession)
 *   2. resolveTypeWithAudit
 *   3. compileWithAudit
 *   4. analyzeWithAudit
 *   5. auditWorkspaceOp
 *   6. getLastAuditRecord         (drains the most-recent record)
 *   7. getAuditRecords            (filtered query)
 *   8. getBundlerBatchSummary     (bundler-batch aggregator)
 *
 * Records exist only in a binding built with `semantic-observe`
 * (`captureAvailability() === "Available"`). With capture, each
 * entry-point round-trips a JSON Buffer carrying the typed payload
 * (`RequestKind` discriminant + matching `RequestKindPayload` arm) and
 * the tests below decode and assert the produced shape. Without
 * capture, the same entry-points still run but must report unavailable
 * capture — no record, or a thrown "audit capture is unavailable" error —
 * never an empty or zeroed record that reads as a measurement.
 */

import { describe, expect, it } from "vitest";

const native = require("../index.js") as typeof import("../index");

const captured = native.captureAvailability() === "Available";
const UNAVAILABLE = /audit capture is unavailable/;

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
  capture_state?: string;
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

describe("typed audit entry-points are declared on every build", () => {
  it("captureAvailability names one of the two capture states", () => {
    expect(["Available", "Unavailable"]).toContain(native.captureAvailability());
  });

  it("VerterHost prototype declares all typed audit functions", () => {
    const proto = Object.getOwnPropertyNames(native.VerterHost.prototype).sort();
    expect(proto).toContain("resolveTypeWithAudit");
    expect(proto).toContain("compileWithAudit");
    expect(proto).toContain("analyzeWithAudit");
    expect(proto).toContain("auditWorkspaceOp");
    expect(proto).toContain("getLastAuditRecord");
    expect(proto).toContain("getAuditRecords");
    expect(proto).toContain("getBundlerBatchSummary");
  });

  it("ComponentMetaSession keeps the audit bundle and walker bindings", () => {
    const proto = Object.getOwnPropertyNames(native.ComponentMetaSession.prototype).sort();
    expect(proto).toContain("getComponentMetaWithAudit");
    expect(proto).toContain("whyLoadedFromAuditJson");
    expect(proto).toContain("whyInstantiatedFromAuditJson");
  });
});

describe.runIf(captured)(
  "typed audit entry-points publish typed records (capture available)",
  () => {
    it("compileWithAudit returns a Buffer with kind=Compile and a populated CompilePayload", () => {
      const host = buildAuditHost();
      const buf = host.compileWithAudit("/widget.vue", "BUNDLER");
      expect(buf).toBeInstanceOf(Buffer);
      const record = decodeRecordBuffer(buf);
      expect(record).not.toBeNull();
      expect(record!.canonical_id).toBe("/widget.vue");
      // RequestKind::Compile { products, backend } serializes as
      // { "Compile": { products: {...}, backend: "Vdom" } } — the
      // CompileTargetTag `target` string this used to assert on was
      // replaced by the product-set/backend audit fields.
      expect(typeof record!.kind).toBe("object");
      expect(record!.kind).toHaveProperty("Compile");
      expect(record!.kind_payload.kind).toBe("Compile");
      // CompilePayload fields are present.
      expect(record!.kind_payload).toHaveProperty("products");
      expect(record!.kind_payload).toHaveProperty("backend");
      expect(record!.kind_payload).toHaveProperty("output_bytes");
      expect(record!.kind_payload).toHaveProperty("code_transform_ops");
      host.close();
    });

    it("compileWithAudit IDE target tags the kind's product set as ide_companion-only", () => {
      const host = buildAuditHost();
      const buf = host.compileWithAudit("/widget.vue", "IDE");
      const record = decodeRecordBuffer(buf);
      expect(record).not.toBeNull();
      const kindObj = record!.kind as {
        Compile: { products: Record<string, boolean>; backend: string };
      };
      expect(kindObj.Compile.products.ide_companion).toBe(true);
      expect(kindObj.Compile.products.runtime_client).toBe(false);
      expect(kindObj.Compile.products.runtime_server).toBe(false);
      expect(kindObj.Compile.products.declarations).toBe(false);
      expect(kindObj.Compile.products.analysis).toBe(false);
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
  },
);

describe.runIf(!captured)(
  "typed audit entry-points report unavailable capture (default build)",
  () => {
    it("record-returning entry-points run but return no record", () => {
      const host = buildAuditHost();
      // A captured build returns a Buffer from every one of these on an
      // audit-enabled host; `null` here is the unavailable-capture outcome.
      expect(host.compileWithAudit("/widget.vue", "BUNDLER")).toBeNull();
      expect(host.analyzeWithAudit("/widget.vue")).toBeNull();
      host.upsert({
        canonicalId: "/types.ts",
        inputId: "/types.ts",
        source: Buffer.from("export type Greeting = { value: string };\n", "utf-8"),
      });
      expect(host.resolveTypeWithAudit("/types.ts", "Greeting")).toBeNull();
      host.close();
    });

    it("resolveSymbolWithAudit still resolves the type while carrying no record", () => {
      const host = buildAuditHost();
      host.upsert({
        canonicalId: "/types.ts",
        inputId: "/types.ts",
        source: Buffer.from("export type Greeting = { value: string };\n", "utf-8"),
      });
      const result = host.resolveSymbolWithAudit("/types.ts", "Greeting", null, "expanded");
      expect(result.error ?? null).toBeNull();
      expect(result.typeExpr).toBeInstanceOf(Buffer);
      expect(result.typeExpr!.toString("utf-8")).toContain("value");
      // An absent `Option<Buffer>` object field crosses NAPI as `undefined`.
      expect(result.auditRecord ?? null).toBeNull();
      host.close();
    });

    it("auditWorkspaceOp returns a Workspace record marked Unavailable, with no footprint", () => {
      const host = buildAuditHost();
      const record = decodeRecordBuffer(
        host.auditWorkspaceOp({
          type: "AuditResolve",
          specifier: "./widget.vue",
          from: "/widget.vue",
        }),
      );
      expect(record).not.toBeNull();
      expect(record!.kind).toHaveProperty("Workspace");
      expect(record!.capture_state).toBe("Unavailable");
      expect(record!.footprint).toBeNull();
      host.close();
    });

    it("store-reading methods throw the unavailable-capture error instead of returning empty results", () => {
      const host = buildAuditHost();
      host.compileWithAudit("/widget.vue", "BUNDLER");
      expect(() => host.getLastAuditRecord()).toThrow(UNAVAILABLE);
      expect(() => host.getAuditRecords({ kind: "Compile" })).toThrow(UNAVAILABLE);
      expect(() => host.getBundlerBatchSummary({ kind: "Vite" })).toThrow(UNAVAILABLE);
      host.close();
    });
  },
);
