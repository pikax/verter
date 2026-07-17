/**
 * @ai-generated - Drives the complete editor-neutral behavioral contract against
 * real tsserver, managed tsgo, and real relay-backed shared-tsgo routes.
 */
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  createEditorNeutralContractInventory,
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

const counters: RunCounters = { attempted: 0, passed: 0, failed: 0, setupFailures: [] };

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
        try {
          await executeEditorNeutralContractCase(testCase, driver, driver.sources);
          counters.passed += 1;
        } catch (error) {
          counters.failed += 1;
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
  writeFileSync(
    receiptPath,
    `${JSON.stringify(
      {
        schemaVersion: 1,
        sourceSha: sha,
        inventoryCases: INVENTORY.length,
        standardLspCases: INVENTORY.filter((testCase) => testCase.surface === "standard-lsp")
          .length,
        routes: ROUTES,
        expectedExecutions: EXPECTED_EXECUTIONS,
        ...counters,
      },
      null,
      2,
    )}\n`,
    "utf8",
  );
  console.log(`editor-neutral LSP receipt: ${receiptPath}`);

  expect(INVENTORY.length, "the complete shared inventory must be discovered").toBe(43);
  expect(EXPECTED_EXECUTIONS, "41 standard + custom on each route, plus shared topology").toBe(127);
  expect(
    counters.setupFailures,
    "every provider route must start; no route may be skipped",
  ).toEqual([]);
  expect(counters.attempted, "all applicable cases must execute; zero/N/A is a failure").toBe(
    EXPECTED_EXECUTIONS,
  );
});
