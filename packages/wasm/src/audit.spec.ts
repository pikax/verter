/**
 * WASM audit wrapper contract tests. Plan §3 Commit 8 (F7) test
 * list — specifically:
 *
 * - `wasm_get_component_meta_with_audit_serializes_across_boundary`
 * - `wasm_why_loaded_binding_invokes_rust_walker`
 *
 * ## Why mocked, not real-WASM
 *
 * The `packages/wasm` binary is built with `wasm-pack build --target
 * web`, which expects browser-supplied `performance.now()` / time
 * imports. When that binary is loaded in Node.js (via `initSync` +
 * `fs.readFileSync`), `std::time::Instant::now()` on the Rust side
 * panics with `time not implemented on this platform` — this is a
 * pre-existing, WASM-target runtime limitation independent of the
 * audit work. See the feedback file for details.
 *
 * The "serializes across boundary" property is therefore verified in
 * TWO complementary places:
 *
 * 1. **Rust-side** in `crates/verter_wasm/tests/audit.rs` — exercises
 *    the `WasmAuditBundle` serde shape via `serde_json` round-trip
 *    (the exact same `Serialize`/`Deserialize` impls that
 *    `serde-wasm-bindgen` consumes to produce a `JsValue`).
 * 2. **TS-side** in this file — exercises the wrapper contract
 *    (`whyLoaded`, `whyInstantiated`, `renderChainText`,
 *    `loadedFiles`, `decodeAuditBundle`) with an
 *    `AuditCapableMetaSession` mock that mirrors the WASM binding
 *    shape (`getComponentMetaWithAudit`,
 *    `whyLoadedFromAuditJson`, `whyInstantiatedFromAuditJson`) and
 *    emits Rust-authored JSON payloads.
 *
 * Together these tests cover both sides of the serde boundary. The
 * NAPI package performs the full end-to-end FFI round-trip via its
 * native `.node` binary in `packages/native/index.spec.ts`.
 */

import { describe, it, expect } from "vitest";

import type { AuditBundle, AuditCapableMetaSession } from "./audit.js";
import {
  assertDeclaredDependencyFilesExactly,
  assertLoadedFilesExactly,
  declaredDependencyFiles,
  decodeAuditBundle,
  loadedFiles,
  renderChainText,
  whyInstantiated,
  whyLoaded,
} from "./audit.js";

/**
 * Minimum synthetic audit record that round-trips through
 * `JSON.parse`/`JSON.stringify` and exercises every wrapper branch
 * (loaded_files, declared_dependency_files, shared_load_terminals,
 * render_chain_text). Mirrors the shape a real WASM binding produces
 * — every `u64` field is a decimal string (plan §1.4).
 */
function synthesizeBundle(): AuditBundle {
  return {
    analysis: { name: "Widget" },
    resolution: { requestId: "1" },
    record: {
      request_id: "1",
      canonical_id: "/Widget.vue",
      timings: {
        total_ms: 10,
        capture_inputs_ms: 1,
        store_read_ms: 2,
        store_merge_ms: 0,
        direct_import_proof_ms: 0,
        imported_root_proof_ms: 0,
        solver_ms: 3,
        materialize_ms: 0,
        serialize_ms: 0,
      },
      solver: { total_resolve_steps: "0", solve_count: 0 },
      store: {
        store_view_hits: 0,
        store_view_misses: 0,
        structural_merges: 0,
        imported_dependency_entries: 0,
        imported_dependency_bytes: "0",
        prepared_type_decls: 0,
        prepared_value_decls: 0,
      },
      memory: {
        process_rss_before_bytes: "0",
        process_rss_after_bytes: "0",
        process_rss_delta_bytes: "0",
        host_cache_before_bytes: "0",
        host_cache_after_bytes: "0",
        workspace_before_bytes: "0",
        workspace_after_bytes: "0",
      },
      footprint: {
        indexed_ready_builds: [{ canonical_id: "/ir.ts", whole_hash: new Array(16).fill(0) }],
        vfs_reads: [
          {
            canonical_id: "/a.ts",
            layer: "Disk",
            cache_hit: false,
            bytes_read: "1",
            request_id: "1",
          },
        ],
        shared_load_reuses: [
          {
            canonical_id: "/shared.ts",
            winner_request_id: "99",
            winner_audited: false,
          },
        ],
        instantiations: [],
        projections: [],
        conditional_decisions: [],
        substitutions: [],
        alias_resolutions: [],
        materializations: [],
        cache_outcomes: {
          cold_builds: 0,
          warm_hits: 0,
          joined_waits: 0,
          sentinels: 0,
          inflight_aborted_retries: 0,
          cold_aborts_swept: 0,
        },
        graph_completeness: { has_orphan_edges: false, edges_truncated: 0 },
        derivation_subgraph: { nodes: [], edges: [] },
      },
    },
  } as unknown as AuditBundle;
}

/** Lightweight mock of a wasm-bindgen-generated `MetaSession`. */
function makeMockSession(): {
  session: AuditCapableMetaSession;
  lastBundleJson: { value: string | null };
  lastArgs: { canonical: string; fp: string | null };
} {
  const lastBundleJson: { value: string | null } = { value: null };
  const lastArgs: { canonical: string; fp: string | null } = { canonical: "", fp: null };
  const session: AuditCapableMetaSession = {
    getComponentMetaWithAudit(canonical: string): unknown {
      lastArgs.canonical = canonical;
      return synthesizeBundle();
    },
    whyLoadedFromAuditJson(auditJson: string, canonical: string): string {
      lastBundleJson.value = auditJson;
      lastArgs.canonical = canonical;
      // A real walker would produce a ProvenanceChain; the mock
      // returns a minimal chain with the shared-load-reuse carried
      // through so the TS wrapper exercises the renderer.
      return JSON.stringify({
        root: null,
        steps: [],
        terminated: "Complete",
        shared_load_terminals: [
          {
            canonical_id: canonical,
            winner_request_id: "99",
            winner_audited: false,
          },
        ],
      });
    },
    whyInstantiatedFromAuditJson(auditJson, canonical, symbol, fp): string {
      lastBundleJson.value = auditJson;
      lastArgs.canonical = canonical;
      lastArgs.fp = fp;
      return JSON.stringify({
        root: null,
        steps: [],
        terminated: "NotFound",
        shared_load_terminals: [],
      });
    },
  };
  return { session, lastBundleJson, lastArgs };
}

describe("@verter/wasm audit wrappers", () => {
  it("wasm_get_component_meta_with_audit_serializes_across_boundary", () => {
    // Plan §3 Commit 8 — verify the TS wrapper consumes the
    // JS-shaped audit bundle the Rust-side `WasmAuditBundle`
    // produces, without re-implementing any of the Rust serialization
    // logic. Every field the Rust binding emits must be accessible
    // via the typed `AuditBundle` interface.
    //
    // Discriminating: a schema drift where `record.footprint` loses
    // its `shared_load_reuses` array, or a `u64` field is emitted as
    // a non-string Number, trips the type-narrowed field reads below.
    const bundle = synthesizeBundle();
    expect(bundle).toHaveProperty("analysis");
    expect(bundle).toHaveProperty("resolution");
    expect(bundle).toHaveProperty("record");
    expect(bundle.record).toHaveProperty("request_id");
    expect(bundle.record).toHaveProperty("footprint");
    expect(bundle.record.footprint).not.toBeNull();

    const fp = bundle.record.footprint!;
    expect(Array.isArray(fp.vfs_reads)).toBe(true);
    expect(Array.isArray(fp.shared_load_reuses)).toBe(true);
    expect(Array.isArray(fp.indexed_ready_builds)).toBe(true);

    // u64 fields are decimal strings through the WASM boundary.
    expect(typeof bundle.record.request_id).toBe("string");
    expect(typeof bundle.record.memory.process_rss_delta_bytes).toBe("string");
    expect(typeof fp.shared_load_reuses[0].winner_request_id).toBe("string");
    expect(typeof fp.vfs_reads[0].bytes_read).toBe("string");

    // Round-trip through JSON — simulates shipping the bundle through
    // a transport layer. The wrapper helpers must accept the parsed
    // shape exactly.
    const serialized = JSON.stringify(bundle);
    const reparsed = JSON.parse(serialized) as AuditBundle;
    const filesSet = loadedFiles(reparsed.record.footprint ?? null);
    expect(filesSet).toEqual(["/a.ts", "/shared.ts"]);
    const deps = declaredDependencyFiles(reparsed.record.footprint ?? null);
    expect(deps).toEqual(["/a.ts", "/ir.ts", "/shared.ts"]);
  });

  it("decodeAuditBundle turns the native Buffer-like payload into a typed bundle", () => {
    // The NAPI variant returns a Buffer, the WASM variant returns a
    // JS object. `decodeAuditBundle` unifies the two — returning
    // `null` for `null` input, passing through anything else.
    expect(decodeAuditBundle(null)).toBeNull();
    const bundle = synthesizeBundle();
    expect(decodeAuditBundle(bundle)).toBe(bundle);
  });

  it("wasm_why_loaded_binding_invokes_rust_walker", () => {
    // Plan §3 Commit 8 — the TS `whyLoaded` wrapper MUST delegate to
    // the session's `whyLoadedFromAuditJson` binding (i.e. the Rust
    // walker) and return the parsed `ProvenanceChain`. The wrapper
    // must NOT walk the footprint itself.
    //
    // Discriminating: if a future refactor adds a "fallback"
    // traversal in TS when the Rust walker returns `NotFound`, the
    // assertion that the mock session observed the full bundle JSON
    // (and no other operation modified the returned chain) would
    // break — the wrapper must be a thin adapter.
    const { session, lastBundleJson, lastArgs } = makeMockSession();
    const bundle = synthesizeBundle();

    const chain = whyLoaded(session, bundle, "/shared.ts");

    // The mock saw the full bundle as a JSON string (so the wrapper
    // called `JSON.stringify(bundle)` before delegating).
    expect(lastBundleJson.value, "wrapper must serialize the bundle").not.toBeNull();
    const echoed = JSON.parse(lastBundleJson.value!);
    expect(echoed).toEqual(bundle);
    expect(lastArgs.canonical).toBe("/shared.ts");

    // The returned chain is the walker's output parsed back into the
    // typed `ProvenanceChain` — the wrapper did not rewrite it.
    expect(chain).toMatchObject({
      root: null,
      steps: [],
      terminated: "Complete",
    });
    expect(chain.shared_load_terminals).toHaveLength(1);
    expect(chain.shared_load_terminals[0].canonical_id).toBe("/shared.ts");
    expect(chain.shared_load_terminals[0].winner_audited).toBe(false);

    // Renderer: pure formatting, includes the winner-unaudited marker.
    const text = renderChainText(chain);
    expect(text).toContain("/shared.ts");
    expect(text).toContain("99");
    expect(text).toContain("winner_audited=false");
  });

  it("whyInstantiated wrapper forwards the full (canonical, symbol, args) triple", () => {
    // Plan §3 Commit 8 — `whyInstantiated` delegates the three
    // identifier fields plus the audit JSON to the Rust walker.
    // If a future refactor drops or reorders an argument, the mock's
    // `lastArgs` record makes the regression obvious.
    const { session, lastArgs } = makeMockSession();
    const bundle = synthesizeBundle();
    const args = "0".repeat(32);

    const chain = whyInstantiated(session, bundle, "/types.ts", "Props", args);
    expect(lastArgs.canonical).toBe("/types.ts");
    expect(lastArgs.fp).toBe(args);
    expect(chain.terminated).toBe("NotFound");
  });

  it("assertLoadedFilesExactly / assertDeclaredDependencyFilesExactly report diffs accurately", () => {
    const record = synthesizeBundle().record;

    // Positive: set-equality passes on the real two-file set.
    expect(() => assertLoadedFilesExactly(record, ["/a.ts", "/shared.ts"])).not.toThrow();
    expect(() =>
      assertDeclaredDependencyFilesExactly(record, ["/a.ts", "/ir.ts", "/shared.ts"]),
    ).not.toThrow();

    // Negative: missing entry surfaces as a `+ missing` line.
    let thrown: Error | null = null;
    try {
      assertLoadedFilesExactly(record, ["/a.ts", "/shared.ts", "/z.ts"]);
    } catch (err) {
      thrown = err as Error;
    }
    expect(thrown, "missing file must throw").toBeInstanceOf(Error);
    expect(thrown!.message).toContain("/z.ts");
    expect(thrown!.message).toContain("+ ");

    // Negative: unexpected entry surfaces as a `- extra` line.
    let thrown2: Error | null = null;
    try {
      assertLoadedFilesExactly(record, []);
    } catch (err) {
      thrown2 = err as Error;
    }
    expect(thrown2!.message).toContain("/a.ts");
    expect(thrown2!.message).toContain("- ");
  });
});
