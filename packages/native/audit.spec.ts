/**
 * Single-walker rule inspection tests.
 *
 * The Rust walker at `verter_session::component_meta_audit::assertions`
 * (via `RustAuditRecord::why_loaded` / `why_instantiated`) is the sole
 * implementation of the provenance walk. TS helpers parse the
 * Rust-produced `ProvenanceChain` JSON and render it — they MUST NOT
 * re-walk `footprint.derivation_subgraph` or compute transitive hops
 * from `footprint.shared_load_reuses` themselves.
 *
 * These tests grep both TS helper files (`packages/native/audit.ts`
 * and `packages/wasm/src/audit.ts`) for forbidden patterns that
 * would signal a second walker implementation leaking into JS.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const NATIVE_AUDIT_TS = resolve(import.meta.dirname, "audit.ts");
const WASM_AUDIT_TS = resolve(import.meta.dirname, "../wasm/src/audit.ts");

interface HelperFile {
  label: string;
  path: string;
  content: string;
}

const FILES: HelperFile[] = [NATIVE_AUDIT_TS, WASM_AUDIT_TS].map((path) => ({
  label: path,
  path,
  content: readFileSync(path, "utf8"),
}));

describe("ts_helpers_do_not_reimplement_walker_logic", () => {
  it("neither audit.ts touches footprint.derivation_subgraph", () => {
    // The derivation subgraph is the walker's private data structure.
    // Consumers receive a `ProvenanceChain` JSON from the Rust walker
    // and must not peek inside the derivation tables. If a TS helper
    // references `derivation_subgraph`, it is almost certainly about
    // to walk edges — which is precisely the duplication the
    // single-walker rule forbids.
    for (const file of FILES) {
      expect(
        file.content.includes("derivation_subgraph"),
        `${file.label} must not reference footprint.derivation_subgraph — the walker lives in Rust`,
      ).toBe(false);
    }
  });

  it("neither audit.ts iterates derivation nodes or edges", () => {
    // Catches a sneak-past of the derivation_subgraph check via
    // structural typing (e.g. `const nodes = fp.edges; for (...) { ... }`).
    // The loop patterns `fp.edges`, `fp.nodes`, `chain.edges`, and
    // `record.footprint.derivation_subgraph` in any spelling are
    // forbidden in the TS helpers.
    const FORBIDDEN_PATTERNS = [
      /\.nodes\b/,
      /\.edges\b/,
      /EdgeRecord|NodeRecord\b/i,
      /for\s*\([^)]*\b(?:node|edge)s?\b/i,
    ];
    for (const file of FILES) {
      for (const pat of FORBIDDEN_PATTERNS) {
        expect(
          pat.test(file.content),
          `${file.label} matches the forbidden pattern ${pat} — TS helpers must not iterate derivation nodes/edges`,
        ).toBe(false);
      }
    }
  });

  it("any access to shared_load_reuses is a set-accumulation, never a transitive walk", () => {
    // `shared_load_reuses` is consulted by `loadedFiles()` and
    // `declaredDependencyFiles()` to compute the `vfs_reads ∪
    // shared_load_reuses` set. No other helper may touch it, and
    // those set-accumulations must not recurse or cross-reference
    // any derivation-graph structure.
    //
    // Review F13: the previous check required the single-line form
    // `for (const r of ...) set.add(r.canonical_id);`. That broke
    // on benign reformatting (prettier expanding the body to a
    // brace block). This broader check accepts multi-line forms
    // but forbids any sub-property access on iterator items beyond
    // `.canonical_id` — the core safety invariant (TS must not
    // peek at shared-load-graph structure).
    const ALLOWED_LINE_PATTERNS = [
      // Single-line: `for (const r of ...) set.add(r.canonical_id);`
      /for\s*\(const\s+r\s+of\s+(?:footprint\.|fp\.)?shared_load_reuses\)\s*set\.add\(r\.canonical_id\);?/,
      // Multi-line body: `for (const r of ...shared_load_reuses) {`
      // — the opening line. The `set.add(r.canonical_id)` body line
      // is covered by the property-access check below.
      /for\s*\(const\s+r\s+of\s+(?:footprint\.|fp\.)?shared_load_reuses\)\s*\{?\s*$/,
      // The body line `set.add(r.canonical_id);` inside a for-of
      // block. Accepted standalone because the preceding loop
      // opener was already validated.
      /^\s*set\.add\(r\.canonical_id\);?\s*$/,
      // The loop closing brace on its own line.
      /^\s*\}\s*$/,
    ];
    for (const file of FILES) {
      const lines = file.content.split(/\r?\n/);
      for (const [idx, line] of lines.entries()) {
        if (!line.includes("shared_load_reuses")) continue;
        // Permit comment / jsdoc references in any spelling.
        const trimmed = line.trim();
        if (
          trimmed.startsWith("*") ||
          trimmed.startsWith("//") ||
          trimmed.startsWith("/**") ||
          trimmed.startsWith("/*")
        ) {
          continue;
        }
        // Permit the import list / type-level references.
        if (/^\s*(?:import\b|type\b|interface\b|export\s+(?:type|interface))/.test(line)) continue;
        // The remaining code line must match one of the allowed
        // patterns OR pass the forbidden-property-access check.
        const matchesAllowed = ALLOWED_LINE_PATTERNS.some((p) => p.test(trimmed));
        expect(
          matchesAllowed,
          `${file.label}:${idx + 1} reads shared_load_reuses outside the set-accumulation helpers: ${trimmed}`,
        ).toBe(true);
      }
    }
  });

  it("why-loaded / why-instantiated helpers delegate to the Rust walker via JSON round-trip only", () => {
    // The whyLoaded / whyInstantiated TS helpers must compose of:
    //   1. JSON.stringify(bundle) — serialize the audit record
    //   2. session.whyLoadedFromAuditJson(...) — delegate to Rust
    //   3. JSON.parse(chainJson) — return a typed chain
    // Any extra logic walking the footprint (e.g. fallback traversal
    // when the Rust walker returns NotFound) is forbidden — TS helpers
    // are pure adapters.
    const REQUIRED_SUBSTRINGS = ["JSON.stringify(bundle)", "JSON.parse(chainJson)"];
    const REQUIRED_BINDING_CALLS = [
      "whyLoadedFromAuditJson(auditJson",
      "whyInstantiatedFromAuditJson(",
    ];
    for (const file of FILES) {
      for (const s of REQUIRED_SUBSTRINGS) {
        expect(
          file.content.includes(s),
          `${file.label} is missing expected walker-adapter pattern: ${s}`,
        ).toBe(true);
      }
      for (const s of REQUIRED_BINDING_CALLS) {
        expect(
          file.content.includes(s),
          `${file.label} is missing required Rust-binding invocation: ${s}`,
        ).toBe(true);
      }
    }
  });

  it("renderChainText is pure string formatting — no iteration over derivation-graph primitives", () => {
    // The renderer may walk `chain.steps` (a flat array produced by
    // the Rust walker) and `chain.shared_load_terminals` (also
    // produced by the walker). Anything else would imply the TS
    // helper is reconstructing graph structure from footprint data.
    for (const file of FILES) {
      const rendererStart = file.content.indexOf("export function renderChainText");
      if (rendererStart < 0) {
        expect.fail(`${file.label} is missing renderChainText export`);
      }
      // Slice the function body — from the export keyword to the
      // next top-level `function` / `export` marker.
      const tail = file.content.slice(rendererStart);
      const nextTop = tail.slice(1).search(/\n(?:export |function )/);
      const body = nextTop >= 0 ? tail.slice(0, nextTop + 1) : tail;
      expect(
        body.includes("derivation_subgraph"),
        `${file.label}.renderChainText must not touch derivation_subgraph`,
      ).toBe(false);
      expect(
        body.includes("footprint"),
        `${file.label}.renderChainText must not dereference footprint — it operates on ProvenanceChain only`,
      ).toBe(false);
    }
  });
});
