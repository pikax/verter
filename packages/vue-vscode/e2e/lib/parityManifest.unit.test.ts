import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import ts from "typescript";
import { describe, expect, it } from "vitest";

import { SVELTE_MATRIX_CASES, VUE_MATRIX_CASES, type MatrixCase } from "./matrixCases";
import { buildParityTestInventory, selectParityTestInventory } from "./parityTestInventory";

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
  it("derives every parity suite and literal registration from the authored tree", () => {
    const discovered = testSources(suiteRoot)
      .map((file) => relative(suiteRoot, file).replace(/\\/g, "/"))
      .sort();
    const inventory = buildParityTestInventory({
      suiteRoot,
      matrixCasesFile: resolve(libRoot, "matrixCases.ts"),
    });
    const ownedSuiteFiles = new Set(
      Object.values(inventory.suiteFilesByFixture)
        .flat()
        .map((file) => file.replace(/^parity\//, "").replace(/\.js$/, ".ts")),
    );
    expect([...ownedSuiteFiles].sort()).toEqual(discovered);

    const derivedLiteralCount = discovered.reduce(
      (total, file) => total + literalTestRegistrations(join(suiteRoot, file)),
      0,
    );
    expect(inventory.literalRegistrationCount).toBe(derivedLiteralCount);
    expect(discovered).toHaveLength(40);
    expect(derivedLiteralCount).toBe(247);
  });

  it("attests all 73 unique matrix IDs and every authored anchor", () => {
    const matrix = [...VUE_MATRIX_CASES, ...SVELTE_MATRIX_CASES];
    expect(matrix).toHaveLength(73);
    expect(new Set(matrix.map((entry) => entry.id)).size).toBe(matrix.length);
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
    expect(inventory.literalRegistrationCount).toBe(247);
    expect(inventory.matrixCaseCount).toBe(73);
    expect(inventory.matrixCaseCount).toBe(VUE_MATRIX_CASES.length + SVELTE_MATRIX_CASES.length);
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

  it("derives the exact loaded suite files for every parity fixture", () => {
    const inventory = buildParityTestInventory({
      suiteRoot,
      matrixCasesFile: resolve(libRoot, "matrixCases.ts"),
    });
    expect(inventory.suiteFilesByFixture["vue-parity"]).toContain("parity/vue/matrix.test.js");
    expect(inventory.suiteFilesByFixture["svelte-parity"]).toContain(
      "parity/svelte/matrix.test.js",
    );
    expect(inventory.suiteFilesByFixture["mixed-parity"]).toEqual([
      "parity/mixed/workspace.test.js",
    ]);
    expect(inventory.suiteFilesByFixture["multi-root-parity"]).toEqual([
      "parity/multi-root/workspace.test.js",
    ]);
    expect(inventory.suiteFilesByFixture["ecosystem-parity"]).toEqual([
      "parity/ecosystem/paths.test.js",
    ]);
  });

  it("selects an exact, non-empty test-ID inventory for a focused suite", () => {
    const inventory = buildParityTestInventory({
      suiteRoot,
      matrixCasesFile: resolve(libRoot, "matrixCases.ts"),
    });

    const focused = selectParityTestInventory(
      inventory,
      "vue-parity",
      "shared/dom-event-inference.test",
    );

    expect(focused.loadedFiles).toEqual(["parity/shared/dom-event-inference.test.js"]);
    expect(focused.testIds).toEqual([
      "shared.js-jsdoc.dom-event.member-completion",
      "shared.js-jsdoc.dom-event.member-definition",
      "shared.js-jsdoc.dom-event.parameter-hover-concrete",
      "shared.js-lax.dom-event.diagnostics-follow-config",
      "shared.js-lax.dom-event.plain-js-control-hover",
      "shared.js-lax.dom-event.unannotated-remains-any",
      "shared.js.dom-event.checked-diagnostics-follow-config",
      "shared.js.dom-event.classic-or-legacy-not-contextual",
      "shared.js.dom-event.unannotated-checked-remains-any",
      "shared.ts.dom-event.classic-or-legacy-not-contextual",
      "shared.ts.dom-event.invalid-member-expect-error-consumed",
      "shared.ts.dom-event.member-completion",
      "shared.ts.dom-event.member-definition",
      "shared.ts.dom-event.parameter-hover-concrete",
    ]);
    expect(() => selectParityTestInventory(inventory, "vue-parity", "missing-suite.test")).toThrow(
      /selected no suite files/,
    );
  });
});
