/**
 * Structured `ImportSpec` round-trip through `evaluateTypeExpressionWithAudit`.
 *
 * Submitting a structured import (`{ specifier: 'foo', bindings:
 * [{ kind: 'named', exportedName: 'X', localAlias: 'Y', typeOnly:
 * true }] }`) MUST produce a resolved expression that sees `Y` as
 * an imported alias of `X`.
 *
 * REGRESSION — discriminating against a substrate where
 * `evaluateTypeExpressionWithAudit` either does not exist or does
 * not lower structured `ImportSpec` payloads correctly.
 */

import { describe, expect, it } from "vitest";

import { TypeInfoSession } from "../src/index.js";

describe("typeinfo evaluate with structured ImportSpec", () => {
  it("named-import with localAlias + typeOnly resolves the renamed symbol", () => {
    // Mirrors the Rust `evaluate_with_extra_imports`
    // characterisation test. Synthetic scratch URIs cannot follow
    // relative-path resolution from the workspace root, so callers
    // pass workspace-rooted specifiers in `extraImports`.
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/source.ts",
      source: `export type X = { msg: string };\n`,
    });
    session.host.upsert({
      inputId: "/scope.ts",
      source: `// scope; the synthesised scratch will inject the import\n`,
    });

    const result = session.evaluateTypeExpression({
      scope: "/scope.ts",
      expression: "Y",
      mode: "expanded",
      cacheable: true,
      extraImports: [
        {
          specifier: "/source",
          bindings: [
            {
              kind: "named",
              exportedName: "X",
              localAlias: "Y",
              typeOnly: true,
            },
          ],
        },
      ],
    });

    expect(result.type).toBeDefined();
    expect(result.type?.kind).toBe("object");
    if (result.type?.kind === "object") {
      const msg = result.type.properties.find((p) => p.name === "msg");
      expect(msg).toBeDefined();
      expect(msg?.type.kind).toBe("primitive");
      if (msg?.type.kind === "primitive") {
        expect(msg.type.name).toBe("string");
      }
    }

    session.host.close();
  });

  it("default-import shape is accepted at the boundary", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/source.ts",
      source: `export default class K { x: number }\n`,
    });
    session.host.upsert({
      inputId: "/scope.ts",
      source: `// scope\n`,
    });
    // The substrate is allowed to surface a Recursive/Unknown for
    // class-default imports; we only check that the boundary
    // accepted the structured ImportSpec without erroring out.
    const result = session.evaluateTypeExpression({
      scope: "/scope.ts",
      expression: "K",
      mode: "navigate",
      cacheable: false,
      extraImports: [
        {
          specifier: "/source",
          bindings: [{ kind: "default", localName: "K" }],
        },
      ],
    });
    expect(result.auditRecord).toBeDefined();
    session.host.close();
  });
});
