/**
 * WASM audit binding integration tests — specifically:
 *
 * - `wasm_get_component_meta_with_audit_serializes_across_boundary`
 * - `wasm_why_loaded_binding_invokes_rust_walker`
 *
 * Loads the real built WASM binary (`wasm/verter_wasm_bg.wasm`) via
 * `initSync` with disk bytes (bypassing the browser-only `fetch` code
 * path) and drives the synchronous `MetaProject` / `MetaSession`
 * surface. The time primitives that previously panicked on
 * `wasm32-unknown-unknown` ("time not implemented on this platform")
 * are now routed through `verter_session::time` → `web_time`, so the
 * binary initializes cleanly in Node.js as well as the browser.
 *
 * Unlike `index.spec.ts`, this file does NOT mock `verter_wasm.js` —
 * it drives the real bindings through an in-memory audit request.
 */

import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import { initSync, MetaProject } from "../wasm/verter_wasm.js";
import type { AuditBundle } from "./audit.js";
import { type AuditCapableMetaSession, loadedFiles, renderChainText, whyLoaded } from "./audit.js";

const WASM_BINARY_PATH = resolve(import.meta.dirname, "../wasm/verter_wasm_bg.wasm");

const SIMPLE_SFC =
  '<script setup lang="ts">\nconst n: number = 1\n</script>\n' +
  "<template><div>{{ n }}</div></template>";

function ensureWasmInitialized(): void {
  if (!existsSync(WASM_BINARY_PATH)) {
    throw new Error(
      `WASM binary missing at ${WASM_BINARY_PATH}. Run \`pnpm --filter @verter/wasm build:wasm\` first.`,
    );
  }
  const bytes = readFileSync(WASM_BINARY_PATH);
  initSync({ module: bytes });
}

describe("@verter/wasm audit bindings", () => {
  beforeAll(() => {
    ensureWasmInitialized();
  });

  it("wasm_get_component_meta_with_audit_serializes_across_boundary", () => {
    // Exercise the real WASM boundary:
    // `MetaProject` constructor with audit enabled, upsert source,
    // open a session, call `getComponentMetaWithAudit`, and assert
    // the JS value round-trip produces an `AuditBundle` shape.
    //
    // Discriminating: if the FFI serialization regresses (a new
    // record field without `#[serde]`, a camelCase/snake_case
    // mismatch, a non-string `u64`, etc.), one of the structural
    // assertions below fails — either the bundle keys are missing
    // or the types of the scalar fields are wrong.
    const project = new MetaProject({ auditEnabled: true, footprintCapture: true });
    project.upsertBase("/Widget.vue", SIMPLE_SFC);
    const session = project.openSession();

    const raw = session.getComponentMetaWithAudit("/Widget.vue");
    expect(raw, "audit bundle must be non-null when audit is enabled").not.toBeNull();
    expect(typeof raw).toBe("object");

    const bundle = raw as unknown as AuditBundle;
    expect(bundle).toHaveProperty("analysis");
    expect(bundle).toHaveProperty("resolution");
    expect(bundle).toHaveProperty("record");
    expect(bundle.record).toHaveProperty("request_id");
    expect(bundle.record).toHaveProperty("canonical_id");
    expect(bundle.record).toHaveProperty("timings");
    expect(bundle.record).toHaveProperty("footprint");

    // u64 transport invariant — every u64 field is a decimal string.
    // This is the load-bearing property that makes JSON
    // round-trips through JS lossless at 2^53+1.
    expect(typeof bundle.record.request_id).toBe("string");
    expect(bundle.record.request_id).toMatch(/^[0-9]+$/);

    // i64 transport invariant.
    expect(typeof bundle.record.memory.process_rss_delta_bytes).toBe("string");

    // Footprint shape — every record-vector present as an array.
    const fp = bundle.record.footprint;
    expect(fp, "footprint must be attached when footprint_capture is on").not.toBeNull();
    if (fp) {
      expect(Array.isArray(fp.vfs_reads)).toBe(true);
      expect(Array.isArray(fp.shared_load_reuses)).toBe(true);
      expect(Array.isArray(fp.indexed_ready_builds)).toBe(true);
      expect(Array.isArray(fp.instantiations)).toBe(true);
      expect(Array.isArray(fp.projections)).toBe(true);
      expect(Array.isArray(fp.conditional_decisions)).toBe(true);
      expect(Array.isArray(fp.derivation_subgraph.nodes)).toBe(true);
      expect(Array.isArray(fp.derivation_subgraph.edges)).toBe(true);
    }

    expect(bundle.record.canonical_id).toBe("/Widget.vue");

    // Exercise the TS helper — confirms the serde-wasm-bindgen
    // output shape conforms to the ts-rs contract the helpers
    // consume.
    const files = loadedFiles(fp ?? null);
    expect(Array.isArray(files)).toBe(true);

    session.close();
    project.shutdown();
  });

  it("wasm_why_loaded_binding_invokes_rust_walker", () => {
    // End-to-end exercise of the Rust
    // walker through the WASM binding. The TS wrapper `whyLoaded`
    // stringifies the bundle, hands it to `whyLoadedFromAuditJson`,
    // and `JSON.parse`s the returned chain.
    //
    // Discriminating: if the walker binding regresses (e.g. the JSON
    // deserialization fails on a new record field, or
    // `ChainTermination` variant is renamed without updating the
    // generated TS types), the `whyLoaded` helper either throws or
    // returns a malformed `ProvenanceChain` that fails the
    // structural assertions below.
    const project = new MetaProject({ auditEnabled: true, footprintCapture: true });
    project.upsertBase("/Widget.vue", SIMPLE_SFC);
    const session = project.openSession();

    const raw = session.getComponentMetaWithAudit("/Widget.vue");
    expect(raw).not.toBeNull();
    const bundle = raw as unknown as AuditBundle;

    // The WASM `MetaSession` satisfies `AuditCapableMetaSession`
    // structurally — the wrapper delegates via the exact
    // `whyLoadedFromAuditJson` method name the binding exposes.
    const walkerSession: AuditCapableMetaSession = session as unknown as AuditCapableMetaSession;
    const chain = whyLoaded(walkerSession, bundle, "/Widget.vue");

    expect(chain).toBeTruthy();
    expect(chain).toHaveProperty("steps");
    expect(chain).toHaveProperty("terminated");
    expect(chain).toHaveProperty("shared_load_terminals");
    expect(Array.isArray(chain.steps)).toBe(true);
    expect(Array.isArray(chain.shared_load_terminals)).toBe(true);

    // Renderer is pure formatting; must run without throwing on
    // whatever chain the walker produced.
    const text = renderChainText(chain);
    expect(typeof text).toBe("string");
    expect(text.length).toBeGreaterThan(0);

    session.close();
    project.shutdown();
  });

  it("wasm_binding_returns_typed_request_id_distinct_across_calls", () => {
    // Discrimination: the audit binding MUST assign
    // a fresh request_id per call. A regression where the session
    // reuses a request_id across `getComponentMetaWithAudit` calls
    // would break downstream correlation (audit_records storage,
    // joiner reuse tracking). Assert strictly-increasing ids.
    const project = new MetaProject({ auditEnabled: true, footprintCapture: true });
    project.upsertBase("/A.vue", SIMPLE_SFC);
    project.upsertBase("/B.vue", SIMPLE_SFC);
    const session = project.openSession();

    const first = session.getComponentMetaWithAudit("/A.vue") as unknown as AuditBundle | null;
    const second = session.getComponentMetaWithAudit("/B.vue") as unknown as AuditBundle | null;
    expect(first).not.toBeNull();
    expect(second).not.toBeNull();

    const firstId = BigInt(first!.record.request_id as string);
    const secondId = BigInt(second!.record.request_id as string);
    expect(secondId > firstId, "request_id must strictly increase across calls").toBe(true);

    session.close();
    project.shutdown();
  });
});
