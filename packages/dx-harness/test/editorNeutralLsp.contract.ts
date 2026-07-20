/**
 * @ai-generated - Drives the complete editor-neutral behavioral contract against
 * real tsserver, managed tsgo, and real relay-backed shared-tsgo routes.
 */
import { execFileSync, spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  createEditorNeutralContractInventory,
  EditorNeutralContractFailure,
  executeEditorNeutralContractCase,
  type EditorNeutralProviderRoute,
} from "@verter/lsp-test-client";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { RawEditorNeutralLspDriver } from "../src/editor-neutral/rawLspDriver.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..", "..", "..");
const WORKSPACE_ROOT = path.join(
  REPO_ROOT,
  "packages",
  "lsp-test-client",
  "fixtures",
  "editor-neutral",
);
const ROUTES: readonly EditorNeutralProviderRoute[] = ["tsserver", "tsgo", "shared-tsgo"];
const INVENTORY = createEditorNeutralContractInventory();
const EXPECTED_EXECUTIONS = ROUTES.reduce(
  (total, route) =>
    total + INVENTORY.filter((testCase) => testCase.providers.includes(route)).length,
  0,
);

interface RunCounters {
  attempted: number;
  passed: number;
  failed: number;
  setupFailures: string[];
}

interface ExecutionOutcome {
  readonly route: EditorNeutralProviderRoute;
  readonly id: string;
  readonly surface: string;
  readonly feature: string;
  readonly status: "passed" | "failed";
  readonly durationMs: number;
  readonly localDefinitionDurationsMs?: readonly [number, number];
  readonly error?: string;
}

interface TypeScriptCliControlOutcome {
  readonly status: "passed" | "failed";
  readonly version?: string;
  readonly diagnosticCodes?: readonly number[];
  readonly output?: string;
  readonly error?: string;
}

function groupedCounts<T>(
  items: readonly T[],
  keyFor: (item: T) => string,
): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const item of items) {
    const key = keyFor(item);
    counts[key] = (counts[key] ?? 0) + 1;
  }
  return Object.fromEntries(
    Object.entries(counts).sort(([left], [right]) => left.localeCompare(right)),
  );
}

const counters: RunCounters = { attempted: 0, passed: 0, failed: 0, setupFailures: [] };
const outcomes: ExecutionOutcome[] = [];
const typeScriptCliControls: Partial<Record<"jsx" | "tsx", TypeScriptCliControlOutcome>> = {};
let laxJavaScriptPolicyControl: TypeScriptCliControlOutcome | undefined;

async function resolveTypeScript7Compiler(): Promise<{ compiler: string; version: string }> {
  const resolver = path.join(REPO_ROOT, "node_modules", "typescript", "lib", "getExePath.js");
  const module = (await import(pathToFileURL(resolver).href)) as {
    default?: () => string;
  };
  if (typeof module.default !== "function") {
    throw new Error(`TypeScript executable resolver has no default function: ${resolver}`);
  }
  const compiler = module.default();
  const version = spawnSync(compiler, ["--version"], { encoding: "utf8" });
  if (version.status !== 0) {
    throw new Error(`TypeScript version command failed: ${version.stderr || version.stdout}`);
  }
  const versionText = version.stdout.trim();
  const major = Number(/^Version\s+(\d+)/.exec(versionText)?.[1]);
  if (!Number.isInteger(major) || major < 7) {
    throw new Error(`TypeScript >=7 is required, got ${JSON.stringify(versionText)}`);
  }
  return { compiler, version: versionText };
}

describe.sequential("TypeScript >=7 authority control", () => {
  for (const extension of ["jsx", "tsx"] as const) {
    it(`contextually types the plain .${extension} handler as PointerEvent`, async () => {
      try {
        const { compiler, version } = await resolveTypeScript7Compiler();

        const controlFile = path.join(WORKSPACE_ROOT, "src", `plain-pointer-control.${extension}`);
        const control = spawnSync(
          compiler,
          [
            "--ignoreConfig",
            "--noEmit",
            "--allowJs",
            "--checkJs",
            "--target",
            "es2022",
            "--module",
            "esnext",
            "--moduleResolution",
            "bundler",
            "--lib",
            "es2022,dom",
            "--jsx",
            "preserve",
            controlFile,
          ],
          { cwd: REPO_ROOT, encoding: "utf8" },
        );
        const output = `${control.stdout}${control.stderr}`.trim();
        const diagnosticCodes = [...output.matchAll(/error TS(\d+):/g)].map((match) =>
          Number(match[1]),
        );
        typeScriptCliControls[extension] = {
          status: "passed",
          version,
          diagnosticCodes,
          output,
        };
        expect(control.status, "the deliberate invalid-member control must fail typecheck").toBe(1);
        expect(diagnosticCodes, "the control must report exactly the anti-any diagnostic").toEqual([
          2339,
        ]);
        expect(output, "the diagnostic must expose the concrete contextual event type").toMatch(
          /type 'PointerEvent'/,
        );
      } catch (error) {
        typeScriptCliControls[extension] = {
          status: "failed",
          error: error instanceof Error ? error.message : String(error),
        };
        throw error;
      }
    });
  }

  it("honors the authored lax JavaScript project policy", async () => {
    try {
      const { compiler, version } = await resolveTypeScript7Compiler();
      const project = path.join(WORKSPACE_ROOT, "src", "policy", "lax", "tsconfig.json");
      const control = spawnSync(compiler, ["--project", project], {
        cwd: REPO_ROOT,
        encoding: "utf8",
      });
      const output = `${control.stdout}${control.stderr}`.trim();
      const diagnosticCodes = [...output.matchAll(/error TS(\d+):/g)].map((match) =>
        Number(match[1]),
      );
      laxJavaScriptPolicyControl = {
        status: "passed",
        version,
        diagnosticCodes,
        output,
      };
      expect(control.status, "checkJs:false must keep the invalid JS member diagnostic-free").toBe(
        0,
      );
      expect(diagnosticCodes).toEqual([]);
    } catch (error) {
      laxJavaScriptPolicyControl = {
        status: "failed",
        error: error instanceof Error ? error.message : String(error),
      };
      throw error;
    }
  });
});

for (const route of ROUTES) {
  describe.sequential(`editor-neutral LSP contract [${route}]`, () => {
    let driver: RawEditorNeutralLspDriver;

    beforeAll(async () => {
      try {
        driver = await RawEditorNeutralLspDriver.create({
          route,
          repoRoot: REPO_ROOT,
          workspaceRoot: WORKSPACE_ROOT,
        });
      } catch (error) {
        counters.setupFailures.push(`${route}: ${String(error)}`);
        throw error;
      }
    }, 180_000);

    afterAll(async () => {
      if (driver) {
        await driver.dispose();
      }
    }, 30_000);

    for (const testCase of INVENTORY.filter((candidate) => candidate.providers.includes(route))) {
      it(`${testCase.surface}: ${testCase.id}`, async () => {
        counters.attempted += 1;
        const startedAt = performance.now();
        try {
          const evidence = await executeEditorNeutralContractCase(testCase, driver, driver.sources);
          counters.passed += 1;
          outcomes.push({
            route,
            id: testCase.id,
            surface: testCase.surface,
            feature: testCase.feature,
            status: "passed",
            durationMs: Math.round(performance.now() - startedAt),
            localDefinitionDurationsMs: evidence?.localDefinitionDurationsMs,
          });
        } catch (error) {
          counters.failed += 1;
          outcomes.push({
            route,
            id: testCase.id,
            surface: testCase.surface,
            feature: testCase.feature,
            status: "failed",
            durationMs: Math.round(performance.now() - startedAt),
            localDefinitionDurationsMs:
              error instanceof EditorNeutralContractFailure
                ? error.evidence.localDefinitionDurationsMs
                : undefined,
            error: error instanceof Error ? error.message : String(error),
          });
          throw error;
        }
      }, 60_000);
    }
  });
}

afterAll(() => {
  const receiptPath =
    process.env.VERTER_EDITOR_NEUTRAL_RECEIPT ??
    path.join(tmpdir(), `verter-editor-neutral-lsp-${process.pid}.json`);
  const sha = execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: REPO_ROOT,
    encoding: "utf8",
  }).trim();
  const inventoryGroups = {
    bySurface: groupedCounts(INVENTORY, (testCase) => testCase.surface),
    byFeature: groupedCounts(INVENTORY, (testCase) => testCase.feature),
    byFrameworkLanguage: groupedCounts(
      INVENTORY,
      (testCase) => `${testCase.framework ?? "none"}:${testCase.language ?? "none"}`,
    ),
    byRoute: Object.fromEntries(
      ROUTES.map((route) => [
        route,
        INVENTORY.filter((testCase) => testCase.providers.includes(route)).length,
      ]),
    ),
  };
  const executionGroups = Object.fromEntries(
    ROUTES.map((route) => {
      const routeOutcomes = outcomes.filter((outcome) => outcome.route === route);
      return [
        route,
        {
          attempted: routeOutcomes.length,
          passed: routeOutcomes.filter((outcome) => outcome.status === "passed").length,
          failed: routeOutcomes.filter((outcome) => outcome.status === "failed").length,
          bySurface: groupedCounts(routeOutcomes, (outcome) => outcome.surface),
          byFeature: groupedCounts(routeOutcomes, (outcome) => outcome.feature),
        },
      ];
    }),
  );
  writeFileSync(
    receiptPath,
    `${JSON.stringify(
      {
        schemaVersion: 2,
        sourceSha: sha,
        inventoryCases: INVENTORY.length,
        standardLspCases: INVENTORY.filter((testCase) => testCase.surface === "standard-lsp")
          .length,
        routes: ROUTES,
        expectedExecutions: EXPECTED_EXECUTIONS,
        authorityControls: {
          typeScriptCli: typeScriptCliControls,
          authoredPolicy: { laxJavaScript: laxJavaScriptPolicyControl },
        },
        inventoryGroups,
        executionGroups,
        ...counters,
        outcomes,
      },
      null,
      2,
    )}\n`,
    "utf8",
  );
  console.log(`editor-neutral LSP receipt: ${receiptPath}`);

  expect(INVENTORY.length, "the complete shared inventory must be discovered").toBe(89);
  expect(
    EXPECTED_EXECUTIONS,
    "standard + custom on each applicable route, plus shared topology",
  ).toBe(265);
  expect(
    counters.setupFailures,
    "every provider route must start; no route may be skipped",
  ).toEqual([]);
  expect(counters.attempted, "all applicable cases must execute; zero/N/A is a failure").toBe(
    EXPECTED_EXECUTIONS,
  );
  expect(outcomes, "every execution must have an auditable receipt outcome").toHaveLength(
    EXPECTED_EXECUTIONS,
  );
  expect(
    new Set(outcomes.map((outcome) => `${outcome.route}:${outcome.id}`)).size,
    "receipt outcomes must be unique by route and case",
  ).toBe(EXPECTED_EXECUTIONS);
  expect(outcomes.filter((outcome) => outcome.status === "passed")).toHaveLength(counters.passed);
  expect(outcomes.filter((outcome) => outcome.status === "failed")).toHaveLength(counters.failed);
  expect(inventoryGroups.byRoute).toEqual({ tsserver: 88, tsgo: 88, "shared-tsgo": 89 });
  expect(
    typeScriptCliControls,
    "both TypeScript >=7 authority controls must execute",
  ).toMatchObject({ jsx: { status: "passed" }, tsx: { status: "passed" } });
  expect(
    laxJavaScriptPolicyControl?.status,
    "the authored lax-JavaScript TypeScript >=7 control must execute",
  ).toBe("passed");
  for (const route of ROUTES) {
    expect(executionGroups[route]?.attempted, `${route} receipt group must be complete`).toBe(
      inventoryGroups.byRoute[route],
    );
  }
});
