/**
 * The `editor-neovim` job's verdict guard, executed against REAL plenary transcripts.
 *
 * The job runs the busted suite, captures its output, and decides pass/fail by
 * pattern-matching that output. plenary COLOURISES its summary counters, so the
 * bytes are `\033[32mSuccess: \033[0m<TAB>43` — and ESC is not `[[:space:]]`.
 * A guard written against the plain text therefore matches nothing, which makes
 * the success proof unsatisfiable (the job can never go green) and leaves the
 * failure proof resting on whichever token happens to be uncoloured.
 *
 * This test does not restate the patterns — restating them would only prove the
 * copy is self-consistent. It EXTRACTS the shipped verification tail out of both
 * workflow files and runs it under `bash` with `out` bound to a committed
 * transcript captured from a real CI run, so a regression in the workflow text
 * is what fails here.
 *
 * The three transcripts are the three outcomes the guard must separate:
 *   - all-pass                      → accepted (43 successes, 0 failures)
 *   - failure with runner token     → rejected via `Tests Failed. Exit: 1`
 *   - failure WITHOUT runner token  → rejected via the coloured `Failed : 1`
 *
 * The third is the case the workflow comment names in prose ("plenary builds that
 * do not propagate a nonzero exit code on test failure"): the runner-level literal
 * is absent and the summary counter is the only evidence there is.
 */

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../../", import.meta.url));
const fixtures = path.join(repoRoot, "scripts", "editor-contracts", "fixtures");

const WORKFLOWS = [
  path.join(repoRoot, ".github", "workflows", "ci.yml"),
  path.join(repoRoot, ".github", "workflows", "release.yml"),
];

/** The step whose `run:` block owns the verdict. */
const STEP_NAME = "Run plenary busted suite";
/**
 * First line of the verification tail. Everything above it captures the suite
 * output (it spawns `nvim`); everything from it down is the pure decision, which
 * needs nothing but `out` in scope.
 */
const TAIL_ANCHOR = "rc=${rc:-0}";

/**
 * Pull the `editor-neovim` → {@link STEP_NAME} → `run: |` block out of a workflow
 * and return it dedented. Deliberately not a YAML parse: no YAML dependency is
 * available to these scripts, and a block scalar's extent is unambiguous from
 * indentation alone.
 */
function extractRunBlock(workflowPath) {
  const lines = readFileSync(workflowPath, "utf-8").split("\n");

  const jobAt = lines.findIndex((line) => line === "  editor-neovim:");
  assert.notEqual(jobAt, -1, `${workflowPath}: no \`editor-neovim\` job`);

  const stepAt = lines.findIndex(
    (line, i) => i > jobAt && line.trimEnd().endsWith(`: ${STEP_NAME}`),
  );
  assert.notEqual(stepAt, -1, `${workflowPath}: \`editor-neovim\` has no \`${STEP_NAME}\` step`);

  const runAt = lines.findIndex((line, i) => i > stepAt && /^\s*run: \|\s*$/.test(line));
  assert.notEqual(runAt, -1, `${workflowPath}: \`${STEP_NAME}\` has no \`run: |\` block`);

  const runIndent = lines[runAt].length - lines[runAt].trimStart().length;
  const body = [];
  for (let i = runAt + 1; i < lines.length; i++) {
    const line = lines[i];
    if (line.trim() === "") {
      body.push("");
      continue;
    }
    const indent = line.length - line.trimStart().length;
    if (indent <= runIndent) break;
    body.push(line);
  }
  while (body.length > 0 && body[body.length - 1] === "") body.pop();
  assert.ok(body.length > 0, `${workflowPath}: \`${STEP_NAME}\` run block is empty`);

  const dedent = Math.min(
    ...body.filter((l) => l !== "").map((l) => l.length - l.trimStart().length),
  );
  return body.map((l) => (l === "" ? "" : l.slice(dedent)));
}

/** The verdict half of the run block: from {@link TAIL_ANCHOR} to the end. */
function extractVerificationTail(workflowPath) {
  const body = extractRunBlock(workflowPath);
  const anchorAt = body.indexOf(TAIL_ANCHOR);
  assert.notEqual(
    anchorAt,
    -1,
    `${workflowPath}: \`${STEP_NAME}\` no longer contains the \`${TAIL_ANCHOR}\` anchor this test ` +
      `slices the verdict at — re-anchor the test rather than deleting it`,
  );
  return body.slice(anchorAt).join("\n");
}

/**
 * Run a workflow's verdict against a transcript, exactly as the job would.
 *
 * `out` is bound before the tail runs, which is the only thing the tail needs
 * from the half above it. Returns the exit status plus the combined output, so a
 * caller can assert on WHICH failure fired rather than only that one did — an
 * exit code alone cannot tell "detected the failure" from "could not find the
 * success proof".
 */
function runVerdict(workflowPath, fixtureName) {
  const script = [
    "set -euo pipefail",
    'out=$(cat "$1")',
    extractVerificationTail(workflowPath),
  ].join("\n");

  try {
    const stdout = execFileSync("bash", ["-c", script, "bash", path.join(fixtures, fixtureName)], {
      encoding: "utf-8",
      stdio: ["ignore", "pipe", "pipe"],
    });
    return { status: 0, output: stdout };
  } catch (error) {
    return {
      status: error.status ?? -1,
      output: `${error.stdout ?? ""}${error.stderr ?? ""}`,
    };
  }
}

const ACCEPTED = "plenary-all-pass.ansi.txt";
const REJECTED_WITH_TOKEN = "plenary-failure.ansi.txt";
const REJECTED_SUMMARY_ONLY = "plenary-failure-without-runner-token.ansi.txt";

const FAILURE_VERDICT = "Neovim plenary suite reported failures";
const NO_PROOF_VERDICT = "did not prove that any assertion-bearing test ran";

let bashTestOptions = {};
try {
  execFileSync("bash", ["--version"], { stdio: "ignore" });
} catch {
  bashTestOptions = { skip: "requires a working bash executable" };
}

test("the committed transcripts really are ANSI-coloured and really differ", () => {
  // Guards the rest of the file against passing vacuously. If the fixtures ever
  // lose their escape sequences, every assertion below would still hold while
  // testing nothing about the colour blindness that motivated the guard.
  const pass = readFileSync(path.join(fixtures, ACCEPTED), "latin1");
  const fail = readFileSync(path.join(fixtures, REJECTED_WITH_TOKEN), "latin1");
  const summaryOnly = readFileSync(path.join(fixtures, REJECTED_SUMMARY_ONLY), "latin1");

  assert.match(pass, /\x1b\[32mSuccess: \x1b\[0m\t\d+/, "all-pass transcript lost its colour");
  assert.match(fail, /\x1b\[31mFailed : \x1b\[0m\t[1-9]/, "failure transcript lost its colour");
  assert.match(
    summaryOnly,
    /\x1b\[31mFailed : \x1b\[0m\t[1-9]/,
    "summary-only transcript lost its colour",
  );

  // The plain-text spellings the guard must NOT be allowed to rely on.
  assert.doesNotMatch(
    pass,
    /(^|\n)Success: *\t?\d/,
    "all-pass transcript has an uncoloured summary",
  );
  assert.ok(fail.includes("Tests Failed. Exit: 1"), "failure transcript lost the runner token");
  assert.ok(
    !summaryOnly.includes("Tests Failed"),
    "summary-only transcript still carries the runner token it is defined by not having",
  );
});

for (const workflow of WORKFLOWS) {
  const label = path.basename(workflow);

  test(`${label}: an all-pass suite is accepted`, bashTestOptions, () => {
    const { status, output } = runVerdict(workflow, ACCEPTED);
    assert.equal(
      status,
      0,
      `a 43/43 green suite was rejected — the verdict cannot match coloured output:\n${output}`,
    );
    assert.doesNotMatch(output, new RegExp(FAILURE_VERDICT));
  });

  test(`${label}: a failing suite is rejected via the runner token`, bashTestOptions, () => {
    const { status, output } = runVerdict(workflow, REJECTED_WITH_TOKEN);
    assert.equal(status, 1);
    assert.match(output, new RegExp(FAILURE_VERDICT));
  });

  test(
    `${label}: a failing suite is rejected on the coloured counter alone`,
    bashTestOptions,
    () => {
      const { status, output } = runVerdict(workflow, REJECTED_SUMMARY_ONLY);
      assert.equal(status, 1);
      assert.match(
        output,
        new RegExp(FAILURE_VERDICT),
        `the failure was not detected from the coloured \`Failed : 1\` counter. ` +
          `Rejecting for any other reason is not detection:\n${output}`,
      );
      assert.doesNotMatch(
        output,
        new RegExp(NO_PROOF_VERDICT),
        `the job rejected the run for the wrong reason — it could not find the success ` +
          `proof, which it also cannot find on a GREEN run:\n${output}`,
      );
    },
  );
}
