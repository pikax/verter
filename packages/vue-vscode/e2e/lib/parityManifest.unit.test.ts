import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import ts from "typescript";
import { describe, expect, it } from "vitest";

import { SVELTE_MATRIX_CASES, VUE_MATRIX_CASES, type MatrixCase } from "./matrixCases";
import { PARITY_LITERAL_TEST_COUNTS, requiredParitySuiteFiles } from "./parityManifest";
import { buildParityTestInventory } from "./parityTestInventory";

const libRoot = dirname(fileURLToPath(import.meta.url));
const suiteRoot = resolve(libRoot, "../suite/parity");
const fixturesRoot = resolve(libRoot, "../fixtures");

function testSources(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const absolute = join(directory, entry.name);
    if (entry.isDirectory()) return testSources(absolute);
    return entry.name.endsWith(".test.ts") ? [absolute] : [];
  });
}

function literalTestRegistrations(file: string): number {
  const source = ts.createSourceFile(
    file,
    readFileSync(file, "utf8"),
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
  let count = 0;
  const visit = (node: ts.Node): void => {
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      (node.expression.text === "test" || node.expression.text === "registerFrameworkTest")
    ) {
      count += 1;
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return count;
}

function assertAnchor(source: string, token: string, occurrence = 0): void {
  let from = 0;
  let offset = -1;
  for (let index = 0; index <= occurrence; index += 1) {
    offset = source.indexOf(token, from);
    expect(
      offset,
      `missing ${JSON.stringify(token)} occurrence ${occurrence}`,
    ).toBeGreaterThanOrEqual(0);
    from = offset + token.length;
  }
}

function assertMatrixFixture(fixture: string, entry: MatrixCase): void {
  const source = readFileSync(join(fixturesRoot, fixture, entry.file), "utf8");
  if (entry.anchor) {
    const anchorSource = readFileSync(join(fixturesRoot, fixture, entry.anchor.file), "utf8");
    assertAnchor(anchorSource, entry.anchor.token, entry.anchor.occurrence);
  }
  if (entry.target) {
    const targetSource = readFileSync(join(fixturesRoot, fixture, entry.target.file), "utf8");
    assertAnchor(targetSource, entry.target.token, entry.target.occurrence);
  }
  if (entry.completionOffsetNeedle) {
    assertAnchor(source, entry.completionOffsetNeedle);
    expect(entry.completionOffsetExtra ?? 0).toBeLessThanOrEqual(
      entry.completionOffsetNeedle.length,
    );
  }
}

function withTemporaryInventory(
  suiteSource: string,
  matrixSource: string,
  assertion: (options: { suiteRoot: string; matrixCasesFile: string }) => void,
): void {
  const root = mkdtempSync(join(tmpdir(), "verter-parity-inventory-"));
  try {
    const suite = join(root, "suite", "shared");
    mkdirSync(suite, { recursive: true });
    writeFileSync(join(suite, "sample.test.ts"), suiteSource, "utf8");
    const matrixCasesFile = join(root, "matrixCases.ts");
    writeFileSync(matrixCasesFile, matrixSource, "utf8");
    assertion({ suiteRoot: join(root, "suite"), matrixCasesFile });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

const EMPTY_MATRIX_SOURCE = `
export const VUE_MATRIX_CASES = [] as const;
export const SVELTE_MATRIX_CASES = [] as const;
`;

describe("parity inventory", () => {
  it("owns exactly 219 literal tests across the complete 38-file parity suite", () => {
    const discovered = testSources(suiteRoot)
      .map((file) => relative(suiteRoot, file).replace(/\\/g, "/"))
      .sort();
    const accepted = Object.keys(PARITY_LITERAL_TEST_COUNTS).sort();
    expect(discovered).toEqual(accepted);

    let total = 0;
    for (const file of accepted) {
      const count = literalTestRegistrations(join(suiteRoot, file));
      expect(count, file).toBe(
        PARITY_LITERAL_TEST_COUNTS[file as keyof typeof PARITY_LITERAL_TEST_COUNTS],
      );
      total += count;
    }
    expect(total).toBe(219);
  });

  it("attests all 73 unique matrix IDs and every authored anchor", () => {
    const matrix = [...VUE_MATRIX_CASES, ...SVELTE_MATRIX_CASES];
    expect(matrix).toHaveLength(73);
    expect(new Set(matrix.map((entry) => entry.id)).size).toBe(73);
    for (const entry of VUE_MATRIX_CASES) assertMatrixFixture("vue-parity", entry);
    for (const entry of SVELTE_MATRIX_CASES) assertMatrixFixture("svelte-parity", entry);

    const inventory = buildParityTestInventory({
      suiteRoot,
      matrixCasesFile: resolve(libRoot, "matrixCases.ts"),
    });
    const required = Object.values(inventory.testIdsByFixture).flat();
    for (const entry of matrix) expect(required).toContain(entry.id);
  });

  it("derives every applicable literal ID as a unique required test", () => {
    const inventory = buildParityTestInventory({
      suiteRoot,
      matrixCasesFile: resolve(libRoot, "matrixCases.ts"),
    });
    expect(inventory.literalRegistrationCount).toBe(219);
    expect(inventory.matrixCaseCount).toBe(73);
    expect(
      Object.fromEntries(
        Object.entries(inventory.testIdsByFixture).map(([fixture, ids]) => [fixture, ids.length]),
      ),
    ).toEqual({
      "vue-parity": 204,
      "svelte-parity": 181,
      "mixed-parity": 7,
      "multi-root-parity": 6,
      "ecosystem-parity": 9,
    });
    for (const ids of Object.values(inventory.testIdsByFixture)) {
      expect(ids.length).toBeGreaterThan(0);
      expect(new Set(ids).size).toBe(ids.length);
    }
    expect(inventory.testIdsByFixture["vue-parity"]).toContain(
      "shared.code-action.apply.organize-imports",
    );
    expect(inventory.testIdsByFixture["svelte-parity"]).not.toContain(
      "shared.code-action.apply.organize-imports",
    );
    expect(inventory.testIdsByFixture["svelte-parity"]).toContain(
      "strict.svelte.rest-props-opt-in",
    );
    expect(inventory.testIdsByFixture["vue-parity"]).not.toContain(
      "strict.svelte.rest-props-opt-in",
    );
  });

  it("rejects non-literal, skipped, duplicate, and pending accepted tests", () => {
    withTemporaryInventory(`test(makeTitle(), () => {});`, EMPTY_MATRIX_SOURCE, (options) =>
      expect(() => buildParityTestInventory(options)).toThrow(/string literal/),
    );
    withTemporaryInventory(
      `test.skip("shared.skipped", () => {});`,
      EMPTY_MATRIX_SOURCE,
      (options) => expect(() => buildParityTestInventory(options)).toThrow(/skip call/),
    );
    withTemporaryInventory(
      `test("shared.duplicate", () => {}); test("shared.duplicate", () => {});`,
      EMPTY_MATRIX_SOURCE,
      (options) => expect(() => buildParityTestInventory(options)).toThrow(/duplicate required/),
    );
    withTemporaryInventory(
      `test("shared.required", () => {});`,
      `
        export const VUE_MATRIX_CASES = [{ id: "vue.pending", pendingIssue: "ISSUE-pending" }];
        export const SVELTE_MATRIX_CASES = [];
      `,
      (options) => expect(() => buildParityTestInventory(options)).toThrow(/pending/),
    );
  });

  it("declares the exact loaded suite files for every parity fixture", () => {
    expect(requiredParitySuiteFiles("vue-parity")).toHaveLength(29);
    expect(requiredParitySuiteFiles("svelte-parity")).toHaveLength(28);
    expect(requiredParitySuiteFiles("mixed-parity")).toEqual(["parity/mixed/workspace.test.js"]);
    expect(requiredParitySuiteFiles("multi-root-parity")).toEqual([
      "parity/multi-root/workspace.test.js",
    ]);
    expect(requiredParitySuiteFiles("ecosystem-parity")).toEqual([
      "parity/ecosystem/paths.test.js",
    ]);
  });
});
