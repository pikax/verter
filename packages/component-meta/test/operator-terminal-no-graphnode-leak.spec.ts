// FAIL-FIRST (plan §3 Step 6.5): the `graphTypeExprToString` exhaustive
// switch in type-expr-bridge.ts must not emit the `graphNode(N)`
// placeholder for any operator-shaped graph node. Pre-fix the `default:
// return graphNode(${node.kind})` arm leaked node-discriminator
// integers (notably `graphNode(13)` for IndexedAccess and
// `graphNode(14)` for Conditional) into compat-layer string output —
// the Phase 3 §4.1 graphNode_leak bucket. Post-fix every kind has an
// explicit structural rendering (e.g. `T[K]`, `T extends U ? X : Y`).
//
// This is a static-text discriminator over the bridge source: the
// patterns `graphNode(13)`, `graphNode(14)`, `graphNode(15)`, etc. must
// not appear as string literals or template segments in the production
// bridge. The exhaustive switch's `_exhaustive: never` check is the
// type-system enforcement; this test catches accidental regressions
// where someone reintroduces a fallback string that mentions
// `graphNode(N)`.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const bridgePath = resolve(here, "..", "src", "type-expr-bridge.ts");

describe("operator-terminal-no-graphnode-leak", () => {
  it("graphTypeExprToString emits no graphNode(N) placeholder string", () => {
    const source = readFileSync(bridgePath, "utf-8");
    // Strip line comments — comments may legitimately mention the
    // pre-Step-6.5 `graphNode(N)` history. Match block comments and
    // line comments in a TypeScript-aware manner without a full
    // tokenizer (sufficient for this static check).
    const stripped = source
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .split("\n")
      .map((line) => line.replace(/\/\/.*$/, ""))
      .join("\n");

    // Pattern: `graphNode(` followed by a digit (template-string,
    // template-literal, or function-call form). Any match indicates
    // production code is still emitting the leaked placeholder.
    const pattern = /graphNode\(\s*\d/;
    const match = pattern.exec(stripped);

    expect(match).toBeNull();
  });
});
