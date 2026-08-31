#!/usr/bin/env node

import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { generateSvelteBindContract } from "./generate-svelte-bind-contract.mjs";
import {
  checkSvelteEntityTable,
  pinnedSvelteEntitiesPresent,
} from "./generate-svelte-entities.mjs";

const SCHEMA = "verter-compiler-generated-artifacts/v1";
const scriptsDir = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = join(scriptsDir, "..");

function failureDetail(error) {
  return error instanceof Error ? error.message : String(error);
}

function checkBindContract() {
  const tempRoot = mkdtempSync(join(tmpdir(), "verter-bind-contract-check-"));
  try {
    const regenerated = join(tempRoot, "bind_contract_data.rs");
    generateSvelteBindContract(repoRoot, regenerated);
    const committed = join(
      repoRoot,
      "crates",
      "verter_compiler",
      "src",
      "svelte",
      "bind_contract_data.rs",
    );
    if (readFileSync(committed, "utf8") !== readFileSync(regenerated, "utf8")) {
      throw new Error(
        "the committed Svelte bind-contract table differs from its generator; run " +
          "`node scripts/generate-svelte-bind-contract.mjs` and commit the result",
      );
    }
    return { name: "svelte-bind-contract", status: "pass", detail: "in-sync" };
  } catch (error) {
    return { name: "svelte-bind-contract", status: "fail", detail: failureDetail(error) };
  } finally {
    rmSync(tempRoot, { recursive: true, force: true });
  }
}

async function checkEntityTable() {
  if (!pinnedSvelteEntitiesPresent()) {
    return {
      name: "svelte-entity-table",
      status: "skip",
      detail: "pinned-svelte-not-installed",
    };
  }
  try {
    await checkSvelteEntityTable();
    return { name: "svelte-entity-table", status: "pass", detail: "in-sync" };
  } catch (error) {
    return { name: "svelte-entity-table", status: "fail", detail: failureDetail(error) };
  }
}

const artifacts = [checkBindContract(), await checkEntityTable()];
process.stdout.write(`${JSON.stringify({ schema: SCHEMA, artifacts })}\n`);
if (artifacts.some((artifact) => artifact.status === "fail")) process.exitCode = 1;
