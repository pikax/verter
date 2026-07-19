import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const sourceRoot = join(packageRoot, "e2e", "suite");
const compiledRoot = join(packageRoot, "out-test", "e2e", "suite");
const output = join(packageRoot, "out-test", "e2e-suite-build-manifest.json");
const require = createRequire(import.meta.url);
const { buildParityTestInventory } = require(
  join(packageRoot, "out-test", "e2e", "lib", "parityTestInventory.js"),
);
const ACCEPTED_SUITE_COUNT = 73;
const ACCEPTED_PARITY_LITERAL_COUNT = 242;
const ACCEPTED_MATRIX_CASE_COUNT = 73;

function discover(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const absolute = join(directory, entry.name);
    if (entry.isDirectory()) return discover(absolute);
    return entry.name.endsWith(".test.ts") ? [absolute] : [];
  });
}

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

const sources = discover(sourceRoot).sort();
if (sources.length === 0) throw new Error(`no authored E2E suites found under ${sourceRoot}`);
if (sources.length !== ACCEPTED_SUITE_COUNT) {
  throw new Error(
    `accepted E2E suite inventory mismatch: expected ${ACCEPTED_SUITE_COUNT}, got ${sources.length}`,
  );
}

const entries = sources.map((source) => {
  const sourceRelative = relative(packageRoot, source).replace(/\\/g, "/");
  const suiteRelative = relative(sourceRoot, source).replace(/\.ts$/, ".js");
  const compiled = join(compiledRoot, suiteRelative);
  if (!existsSync(compiled)) {
    throw new Error(`authored E2E suite was not compiled: ${sourceRelative}`);
  }
  return {
    source: sourceRelative,
    sourceSha256: sha256(source),
    compiled: relative(packageRoot, compiled).replace(/\\/g, "/"),
    compiledSha256: sha256(compiled),
  };
});

mkdirSync(dirname(output), { recursive: true });
const parity = buildParityTestInventory({
  suiteRoot: join(sourceRoot, "parity"),
  matrixCasesFile: join(packageRoot, "e2e", "lib", "matrixCases.ts"),
});
if (
  parity.literalRegistrationCount !== ACCEPTED_PARITY_LITERAL_COUNT ||
  parity.matrixCaseCount !== ACCEPTED_MATRIX_CASE_COUNT
) {
  throw new Error(
    `accepted parity inventory mismatch: expected ${ACCEPTED_PARITY_LITERAL_COUNT} literal registrations + ${ACCEPTED_MATRIX_CASE_COUNT} matrix cases, got ${parity.literalRegistrationCount} + ${parity.matrixCaseCount}`,
  );
}
writeFileSync(output, `${JSON.stringify({ version: 4, entries, parity }, null, 2)}\n`, "utf8");
console.log(
  `attested ${entries.length} E2E suite files, ${parity.literalRegistrationCount} parity registrations, and ${parity.matrixCaseCount} matrix cases in ${output}`,
);
