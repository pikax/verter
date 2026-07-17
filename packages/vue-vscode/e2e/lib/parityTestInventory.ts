import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";

import ts from "typescript";

export const PARITY_FIXTURES = [
  "vue-parity",
  "svelte-parity",
  "mixed-parity",
  "multi-root-parity",
  "ecosystem-parity",
] as const;

export type ParityFixture = (typeof PARITY_FIXTURES)[number];

export interface ParityTestInventory {
  readonly literalRegistrationCount: number;
  readonly matrixCaseCount: number;
  readonly testIdsByFixture: Readonly<Record<ParityFixture, readonly string[]>>;
  readonly suiteFilesByFixture: Readonly<Record<ParityFixture, readonly string[]>>;
  readonly testIdsBySuiteFileByFixture: Readonly<
    Record<ParityFixture, Readonly<Record<string, readonly string[]>>>
  >;
}

export interface SelectedParityTestInventory {
  readonly testIds: readonly string[];
  readonly loadedFiles: readonly string[];
}

function testSources(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const absolute = join(directory, entry.name);
    if (entry.isDirectory()) return testSources(absolute);
    return entry.name.endsWith(".test.ts") ? [absolute] : [];
  });
}

function unwrapExpression(expression: ts.Expression): ts.Expression {
  if (
    ts.isAsExpression(expression) ||
    ts.isSatisfiesExpression(expression) ||
    ts.isParenthesizedExpression(expression)
  ) {
    return unwrapExpression(expression.expression);
  }
  return expression;
}

function literal(node: ts.Node | undefined, context: string): string {
  if (!node || (!ts.isStringLiteral(node) && !ts.isNoSubstitutionTemplateLiteral(node))) {
    throw new Error(`${context} must be a string literal`);
  }
  return node.text;
}

function fixturesForSuite(relativeFile: string): readonly ParityFixture[] {
  if (relativeFile.startsWith("vue/")) return ["vue-parity"];
  if (relativeFile.startsWith("svelte/")) return ["svelte-parity"];
  if (relativeFile.startsWith("shared/") || relativeFile === "lsp-extras.test.ts") {
    return ["vue-parity", "svelte-parity"];
  }
  if (relativeFile.startsWith("mixed/")) return ["mixed-parity"];
  if (relativeFile.startsWith("multi-root/")) return ["multi-root-parity"];
  if (relativeFile.startsWith("ecosystem/")) return ["ecosystem-parity"];
  throw new Error(`parity suite has no fixture ownership: ${relativeFile}`);
}

function matrixIds(sourceFile: ts.SourceFile, exportName: string): string[] {
  let found: string[] | undefined;
  const visit = (node: ts.Node): void => {
    if (
      !ts.isVariableDeclaration(node) ||
      !ts.isIdentifier(node.name) ||
      node.name.text !== exportName
    ) {
      ts.forEachChild(node, visit);
      return;
    }
    if (!node.initializer) throw new Error(`${exportName} has no initializer`);
    const initializer = unwrapExpression(node.initializer);
    if (!ts.isArrayLiteralExpression(initializer)) {
      throw new Error(`${exportName} must be an array literal`);
    }
    found = initializer.elements.map((element, index) => {
      const value = unwrapExpression(element);
      if (!ts.isObjectLiteralExpression(value)) {
        throw new Error(`${exportName}[${index}] must be an object literal`);
      }
      let id: string | undefined;
      for (const property of value.properties) {
        if (!ts.isPropertyAssignment(property)) continue;
        const name = property.name;
        const key =
          ts.isIdentifier(name) || ts.isStringLiteral(name) || ts.isNumericLiteral(name)
            ? name.text
            : undefined;
        if (key === "pendingIssue") {
          throw new Error(
            `${exportName}[${index}] is pending; accepted parity cases must hard-fail`,
          );
        }
        if (key === "id") id = literal(property.initializer, `${exportName}[${index}].id`);
      }
      if (!id) throw new Error(`${exportName}[${index}] has no literal id`);
      return id;
    });
  };
  visit(sourceFile);
  if (!found) throw new Error(`missing ${exportName}`);
  return found;
}

/**
 * Derive the release-required parity IDs from TypeScript syntax trees. Every
 * accepted registration must have a stable literal title, framework-specific
 * registrations are assigned only to their applicable fixture, and all matrix
 * cases are required hard-fail cases.
 */
export function buildParityTestInventory(options: {
  readonly suiteRoot: string;
  readonly matrixCasesFile: string;
}): ParityTestInventory {
  const ids = Object.fromEntries(
    PARITY_FIXTURES.map((fixture) => [fixture, [] as string[]]),
  ) as Record<ParityFixture, string[]>;
  const suiteFiles = Object.fromEntries(
    PARITY_FIXTURES.map((fixture) => [fixture, [] as string[]]),
  ) as Record<ParityFixture, string[]>;
  const testIdsBySuiteFile = Object.fromEntries(
    PARITY_FIXTURES.map((fixture) => [fixture, {} as Record<string, string[]>]),
  ) as Record<ParityFixture, Record<string, string[]>>;
  let literalRegistrationCount = 0;

  for (const file of testSources(options.suiteRoot).sort()) {
    const relativeFile = relative(options.suiteRoot, file).replace(/\\/g, "/");
    const sourceFile = ts.createSourceFile(
      file,
      readFileSync(file, "utf8"),
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TS,
    );
    const ownedFixtures = fixturesForSuite(relativeFile);
    const compiledSuiteFile = `parity/${relativeFile.replace(/\.ts$/, ".js")}`;
    for (const fixture of ownedFixtures) {
      suiteFiles[fixture].push(compiledSuiteFile);
      testIdsBySuiteFile[fixture][compiledSuiteFile] = [];
    }
    const visit = (node: ts.Node): void => {
      if (ts.isCallExpression(node)) {
        if (
          ts.isPropertyAccessExpression(node.expression) &&
          node.expression.name.text === "skip"
        ) {
          throw new Error(
            `${relativeFile} contains a skip call; accepted parity tests must hard-fail`,
          );
        }
        if (ts.isIdentifier(node.expression) && node.expression.text === "test") {
          const id = literal(node.arguments[0], `${relativeFile} test title`);
          literalRegistrationCount += 1;
          for (const fixture of ownedFixtures) {
            ids[fixture].push(id);
            testIdsBySuiteFile[fixture][compiledSuiteFile].push(id);
          }
        }
        if (ts.isIdentifier(node.expression) && node.expression.text === "registerFrameworkTest") {
          const framework = literal(node.arguments[0], `${relativeFile} framework`);
          const id = literal(node.arguments[1], `${relativeFile} framework test title`);
          if (framework !== "vue" && framework !== "svelte") {
            throw new Error(
              `${relativeFile} has unsupported framework ${JSON.stringify(framework)}`,
            );
          }
          const fixture = `${framework}-parity` as ParityFixture;
          if (!ownedFixtures.includes(fixture)) {
            throw new Error(`${relativeFile} registers ${id} outside the ${fixture} suite`);
          }
          literalRegistrationCount += 1;
          ids[fixture].push(id);
          testIdsBySuiteFile[fixture][compiledSuiteFile].push(id);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(sourceFile);
  }

  const matrixSource = ts.createSourceFile(
    options.matrixCasesFile,
    readFileSync(options.matrixCasesFile, "utf8"),
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
  const vueMatrixIds = matrixIds(matrixSource, "VUE_MATRIX_CASES");
  const svelteMatrixIds = matrixIds(matrixSource, "SVELTE_MATRIX_CASES");
  ids["vue-parity"].push(...vueMatrixIds);
  ids["svelte-parity"].push(...svelteMatrixIds);
  if (vueMatrixIds.length > 0) {
    const fileIds = testIdsBySuiteFile["vue-parity"]["parity/vue/matrix.test.js"];
    if (!fileIds) throw new Error("VUE_MATRIX_CASES has no parity/vue/matrix.test.ts suite");
    fileIds.push(...vueMatrixIds);
  }
  if (svelteMatrixIds.length > 0) {
    const fileIds = testIdsBySuiteFile["svelte-parity"]["parity/svelte/matrix.test.js"];
    if (!fileIds) throw new Error("SVELTE_MATRIX_CASES has no parity/svelte/matrix.test.ts suite");
    fileIds.push(...svelteMatrixIds);
  }

  for (const fixture of PARITY_FIXTURES) {
    const duplicateIds = [
      ...new Set(ids[fixture].filter((id, index, all) => all.indexOf(id) !== index)),
    ];
    if (duplicateIds.length > 0) {
      throw new Error(
        `${fixture} contains duplicate required test IDs: ${duplicateIds.join(", ")}`,
      );
    }
    ids[fixture].sort();
    suiteFiles[fixture].sort();
    for (const file of suiteFiles[fixture]) {
      const fileIds = testIdsBySuiteFile[fixture][file];
      if (!fileIds || fileIds.length === 0) {
        throw new Error(`${fixture} suite ${file} has no required test IDs`);
      }
      fileIds.sort();
    }
  }

  return {
    literalRegistrationCount,
    matrixCaseCount: vueMatrixIds.length + svelteMatrixIds.length,
    testIdsByFixture: ids,
    suiteFilesByFixture: suiteFiles,
    testIdsBySuiteFileByFixture: testIdsBySuiteFile,
  };
}

/**
 * Resolve the exact file and test-ID contract for a parity run. A focused
 * selector is allowed only when it matches a non-empty subset of the attested
 * suite inventory, so narrowing cannot turn a typo or stale build into a pass.
 */
export function selectParityTestInventory(
  inventory: ParityTestInventory,
  fixture: ParityFixture,
  onlyPattern?: string,
): SelectedParityTestInventory {
  const normalizedPattern = onlyPattern?.replace(/\\/g, "/");
  const loadedFiles = inventory.suiteFilesByFixture[fixture].filter(
    (file) => !normalizedPattern || file.includes(normalizedPattern),
  );
  if (loadedFiles.length === 0) {
    throw new Error(
      `${fixture} parity selector ${JSON.stringify(onlyPattern)} selected no suite files`,
    );
  }

  const bySuiteFile = inventory.testIdsBySuiteFileByFixture[fixture];
  const testIds = loadedFiles.flatMap((file) => bySuiteFile[file] ?? []);
  if (testIds.length === 0) {
    throw new Error(
      `${fixture} parity selector ${JSON.stringify(onlyPattern)} selected no test IDs`,
    );
  }
  if (new Set(testIds).size !== testIds.length) {
    throw new Error(
      `${fixture} parity selector ${JSON.stringify(onlyPattern)} selected duplicate test IDs`,
    );
  }
  return { testIds: [...testIds].sort(), loadedFiles };
}
