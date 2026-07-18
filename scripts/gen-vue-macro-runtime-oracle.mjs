#!/usr/bin/env node

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import {
  generateOracle,
  oracleDiff,
  stableOracleJson,
  VUE_MACRO_ORACLE_VERSION,
} from "./vue-macro-runtime-oracle/oracle-lib.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = join(scriptDirectory, "..");
const outputPath = join(
  repositoryRoot,
  "crates/verter_session/tests/fixtures/vue_macro_runtime_oracle.json",
);
const check = process.argv.includes("--check");
const unknown = process.argv.slice(2).filter((arg) => arg !== "--check");
if (unknown.length !== 0) {
  throw new Error(`unknown argument(s): ${unknown.join(", ")}`);
}

const generated = generateOracle();
const generatedText = stableOracleJson(generated);
if (check) {
  if (!existsSync(outputPath)) {
    throw new Error(
      `missing ${relative(repositoryRoot, outputPath)}; regenerate with ` +
        "`pnpm run gen:vue-macro-oracle`",
    );
  }
  const committedText = readFileSync(outputPath, "utf8").replaceAll("\r\n", "\n");
  let committed;
  try {
    committed = JSON.parse(committedText);
  } catch (error) {
    throw new Error(`committed Vue macro oracle is invalid JSON: ${error.message}`);
  }
  const diff = oracleDiff(committed, generated);
  if (diff !== null || committedText !== generatedText) {
    throw new Error(
      `Vue macro runtime oracle drift for @vue/compiler-sfc@${VUE_MACRO_ORACLE_VERSION}: ` +
        `${diff ?? "non-canonical JSON formatting"}. Regenerate with ` +
        "`pnpm run gen:vue-macro-oracle`.",
    );
  }
  process.stdout.write(
    `Vue macro runtime oracle: ${generated.cases.length} case(s) in sync with ` +
      `@vue/compiler-sfc@${VUE_MACRO_ORACLE_VERSION}.\n`,
  );
} else {
  mkdirSync(dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, generatedText);
  process.stdout.write(
    `Vue macro runtime oracle: wrote ${generated.cases.length} case(s) from ` +
      `@vue/compiler-sfc@${VUE_MACRO_ORACLE_VERSION} to ` +
      `${relative(repositoryRoot, outputPath)}.\n`,
  );
}
