/**
 * Architectural guard for the compat layer's native session call surface.
 *
 * Per the legacy → graph + dispatch migration plan §7.1 (Tier 5a), the
 * `@verter/component-meta/compat` layer must call only a fixed allow-list of
 * methods on `_session.*` and read only a fixed allow-list of properties off
 * `_session.*`. Property writes on `_session` are forbidden. Imports of native
 * modules must be named-symbol imports (no namespace imports).
 *
 * The walker is implemented with the TypeScript compiler API and runs against
 * the actual `packages/component-meta/src/compat/` source tree as well as
 * synthetic fixtures that pin the discriminating predicates of each rule.
 *
 * Authority chain: D24 (compat allow-list = TS compiler API member-call walker),
 * D35 (allow-list contents), D58 (NATIVE_MODULE_GLOBS).
 */

import { describe, expect, it } from "vitest";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  ALLOWED_NATIVE_SESSION_METHODS,
  ALLOWED_NATIVE_SESSION_PROPERTY_READS,
  NATIVE_MODULE_GLOBS,
  walkSourceForViolations,
  walkCompatLayerForViolations,
  type WalkerViolation,
} from "./__arch__/native-call-surface-walker.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const compatDir = resolve(__dirname, "../src/compat");

// =============================================================================
// Self-tests for the walker constants — confirm the brief's pinned values.
// =============================================================================

describe("Walker constants are pinned to plan §7.1 / D35 / D58", () => {
  it("ALLOWED_NATIVE_SESSION_METHODS matches D35", () => {
    expect(new Set(ALLOWED_NATIVE_SESSION_METHODS)).toEqual(
      new Set([
        "getComponentMeta",
        "getEffectiveSource",
        "delete",
        "restoreBaseFile",
        "refreshBaseFile",
        "ensureBaseFile",
      ]),
    );
  });

  it("ALLOWED_NATIVE_SESSION_PROPERTY_READS matches D35", () => {
    expect(new Set(ALLOWED_NATIVE_SESSION_PROPERTY_READS)).toEqual(new Set(["engine"]));
  });

  it("NATIVE_MODULE_GLOBS matches D58", () => {
    expect(NATIVE_MODULE_GLOBS).toEqual(["@verter/native", "@verter/native-*", "../native"]);
  });
});

// =============================================================================
// Test 1 — compat_native_call_surface_allowlist
// Discriminating predicate: walker REJECTS a fixture that calls a non-allowlist
// method on `_session.*` AND ACCEPTS a fixture that calls only allow-listed
// methods.
// =============================================================================

describe("compat_native_call_surface_allowlist", () => {
  it("rejects a fixture calling a forbidden method on _session", () => {
    const violatingSource = `
      class Bad {
        private _session: any = null;
        run() {
          // forbidden: getDeclaredComponentMeta is not in the allow-list
          this._session.getDeclaredComponentMeta("/a.vue");
        }
      }
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-bad-method.ts",
      source: violatingSource,
    });
    const callViolations = violations.filter((v) => v.rule === "native-session-method-allowlist");
    expect(callViolations.length).toBeGreaterThan(0);
    expect(callViolations[0]?.detail).toMatch(/getDeclaredComponentMeta/);
    expect(callViolations[0]?.line).toBeGreaterThan(0);
  });

  it("accepts a fixture calling only allow-listed methods on _session", () => {
    const cleanSource = `
      class Good {
        private _session: any = null;
        run(p: string) {
          this._session.getComponentMeta(p);
          this._session.getEffectiveSource(p);
          this._session.delete(p);
          this._session.restoreBaseFile(p);
          this._session.refreshBaseFile(p);
          this._session.ensureBaseFile(p);
        }
      }
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-good-methods.ts",
      source: cleanSource,
    });
    const callViolations = violations.filter((v) => v.rule === "native-session-method-allowlist");
    expect(callViolations).toEqual([]);
  });

  it("does NOT trigger for member calls on identifiers other than _session", () => {
    const otherIdentifierSource = `
      class Other {
        private session: any = null;
        run() {
          // 'session' (no underscore) is not the gated identifier
          this.session.getDeclaredComponentMeta("/a.vue");
        }
      }
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-other-ident.ts",
      source: otherIdentifierSource,
    });
    const callViolations = violations.filter((v) => v.rule === "native-session-method-allowlist");
    expect(callViolations).toEqual([]);
  });

  it("compat layer source is allow-list compliant for method calls", () => {
    const violations = walkCompatLayerForViolations(compatDir);
    const callViolations = violations.filter((v) => v.rule === "native-session-method-allowlist");
    expect(callViolations, formatViolationsForFailureMessage(callViolations)).toEqual([]);
  });
});

// =============================================================================
// Test 2 — compat_no_namespace_imports_of_native_modules
// Discriminating predicate: walker REJECTS an `import * as foo from '@verter/native'`
// fixture AND ACCEPTS a named-symbol-only import fixture.
// =============================================================================

describe("compat_no_namespace_imports_of_native_modules", () => {
  it("rejects a namespace import of @verter/native", () => {
    const violatingSource = `
      import * as nat from "@verter/native";
      const _ = nat;
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-namespace-import.ts",
      source: violatingSource,
    });
    const importViolations = violations.filter(
      (v) => v.rule === "native-module-no-namespace-import",
    );
    expect(importViolations.length).toBeGreaterThan(0);
    expect(importViolations[0]?.detail).toMatch(/@verter\/native/);
  });

  it("rejects a namespace import matching @verter/native-* pattern", () => {
    const violatingSource = `
      import * as p from "@verter/native-darwin-arm64";
      const _ = p;
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-namespace-import-pattern.ts",
      source: violatingSource,
    });
    const importViolations = violations.filter(
      (v) => v.rule === "native-module-no-namespace-import",
    );
    expect(importViolations.length).toBeGreaterThan(0);
    expect(importViolations[0]?.detail).toMatch(/@verter\/native-darwin-arm64/);
  });

  it("rejects a namespace import of ../native", () => {
    const violatingSource = `
      import * as native from "../native";
      const _ = native;
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-namespace-relative.ts",
      source: violatingSource,
    });
    const importViolations = violations.filter(
      (v) => v.rule === "native-module-no-namespace-import",
    );
    expect(importViolations.length).toBeGreaterThan(0);
    expect(importViolations[0]?.detail).toMatch(/\.\.\/native/);
  });

  it("accepts named-symbol imports of @verter/native", () => {
    const cleanSource = `
      import { Workspace, type MetaSession } from "@verter/native";
      const _: any = Workspace;
      type _T = MetaSession;
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-named-import.ts",
      source: cleanSource,
    });
    const importViolations = violations.filter(
      (v) => v.rule === "native-module-no-namespace-import",
    );
    expect(importViolations).toEqual([]);
  });

  it("does NOT flag namespace imports of unrelated modules", () => {
    const unrelatedSource = `
      import * as path from "node:path";
      const _ = path;
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-unrelated-namespace.ts",
      source: unrelatedSource,
    });
    const importViolations = violations.filter(
      (v) => v.rule === "native-module-no-namespace-import",
    );
    expect(importViolations).toEqual([]);
  });

  it("compat layer source has no namespace imports of native modules", () => {
    const violations = walkCompatLayerForViolations(compatDir);
    const importViolations = violations.filter(
      (v) => v.rule === "native-module-no-namespace-import",
    );
    expect(importViolations, formatViolationsForFailureMessage(importViolations)).toEqual([]);
  });
});

// =============================================================================
// Test 3 — compat_no_property_writes_on_session
// Discriminating predicate: walker REJECTS a `_session.foo = bar` write AND
// ACCEPTS reads of allow-listed properties.
// =============================================================================

describe("compat_no_property_writes_on_session", () => {
  it("rejects a property write on _session", () => {
    const violatingSource = `
      class Bad {
        private _session: any = null;
        run() {
          // forbidden: writing to a property on _session
          this._session.engine = null;
        }
      }
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-property-write.ts",
      source: violatingSource,
    });
    const writeViolations = violations.filter((v) => v.rule === "native-session-no-property-write");
    expect(writeViolations.length).toBeGreaterThan(0);
    expect(writeViolations[0]?.detail).toMatch(/engine/);
  });

  it("rejects a compound assignment to _session property", () => {
    const violatingSource = `
      class Bad {
        private _session: any = null;
        run() {
          this._session.counter += 1;
        }
      }
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-compound-assign.ts",
      source: violatingSource,
    });
    const writeViolations = violations.filter((v) => v.rule === "native-session-no-property-write");
    expect(writeViolations.length).toBeGreaterThan(0);
    expect(writeViolations[0]?.detail).toMatch(/counter/);
  });

  it("accepts a read of an allow-listed property", () => {
    const cleanSource = `
      class Good {
        private _session: any = null;
        run() {
          const e = this._session.engine;
          return e;
        }
      }
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-property-read-allowed.ts",
      source: cleanSource,
    });
    const readViolations = violations.filter(
      (v) => v.rule === "native-session-property-read-allowlist",
    );
    const writeViolations = violations.filter((v) => v.rule === "native-session-no-property-write");
    expect(readViolations).toEqual([]);
    expect(writeViolations).toEqual([]);
  });

  it("rejects a read of a non-allow-listed property on _session", () => {
    const violatingSource = `
      class Bad {
        private _session: any = null;
        run() {
          // forbidden: 'closed' is not in the property-read allow-list
          if (this._session.closed) return;
        }
      }
    `;
    const violations = walkSourceForViolations({
      fileName: "fixture-property-read-bad.ts",
      source: violatingSource,
    });
    const readViolations = violations.filter(
      (v) => v.rule === "native-session-property-read-allowlist",
    );
    expect(readViolations.length).toBeGreaterThan(0);
    expect(readViolations[0]?.detail).toMatch(/closed/);
  });

  it("compat layer source has no property writes on _session", () => {
    const violations = walkCompatLayerForViolations(compatDir);
    const writeViolations = violations.filter((v) => v.rule === "native-session-no-property-write");
    expect(writeViolations, formatViolationsForFailureMessage(writeViolations)).toEqual([]);
  });

  it("compat layer source reads only allow-listed properties on _session", () => {
    const violations = walkCompatLayerForViolations(compatDir);
    const readViolations = violations.filter(
      (v) => v.rule === "native-session-property-read-allowlist",
    );
    expect(readViolations, formatViolationsForFailureMessage(readViolations)).toEqual([]);
  });
});

// =============================================================================
// Helper — pretty-format violations for failure messages
// =============================================================================

function formatViolationsForFailureMessage(violations: WalkerViolation[]): string {
  if (violations.length === 0) return "no violations";
  const lines = violations.map((v) => `${v.rule}: ${v.file}:${v.line} — ${v.detail}`);
  return ["Walker violations:", ...lines].join("\n  ");
}
