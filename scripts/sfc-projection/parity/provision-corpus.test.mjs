import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { CORPUS_VARIABLE } from "./measure.mjs";
import {
  REF_VARIABLE,
  ROOT_VARIABLE,
  SOURCE_VARIABLE,
  STEPS_VARIABLE,
  planProvision,
  provision,
} from "./provision-corpus.mjs";
import { Refusal } from "./measure.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

const git = (cwd, ...args) =>
  execFileSync(
    "git",
    ["-c", "user.name=parity", "-c", "user.email=parity@example.invalid", ...args],
    { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
  ).trim();

/** A source repository whose `members` directory is the corpus root. */
async function sourceRepository() {
  const source = await mkdtemp(path.join(os.tmpdir(), "verter-corpus-source-"));
  git(source, "-c", "init.defaultBranch=main", "init", "--quiet");
  await mkdir(path.join(source, "members"));
  await writeFile(path.join(source, "members", "revision.txt"), "one\n");
  git(source, "add", ".");
  git(source, "commit", "--quiet", "-m", "one");
  return source;
}

// Each step appends to a log in the checkout, so a reused build shows one run.
const node = (script) => [process.execPath, "-e", script];
const steps = (failing = false) =>
  JSON.stringify([
    { argv: node("require('fs').appendFileSync('steps.log', 'install\\n')") },
    {
      cwd: "members",
      argv: node(failing ? "process.exit(3)" : "require('fs').writeFileSync('prepared', '')"),
    },
  ]);

async function workspace() {
  const source = await sourceRepository();
  const home = await mkdtemp(path.join(os.tmpdir(), "verter-corpus-home-"));
  const env = {
    [CORPUS_VARIABLE]: path.join(home, "corpus"),
    [SOURCE_VARIABLE]: source,
    [REF_VARIABLE]: "main",
    [ROOT_VARIABLE]: "members",
    [STEPS_VARIABLE]: steps(),
  };
  return {
    source,
    env,
    cleanup: () =>
      Promise.all([source, home].map((dir) => rm(dir, { recursive: true, force: true }))),
  };
}

const quiet = { log: () => {} };

test("provisioning refuses an unusable plan before touching anything", async () => {
  const { env, cleanup } = await workspace();
  try {
    const refused = (changes, pattern) =>
      assert.throws(
        () => planProvision({ ...env, ...changes }, repoRoot),
        (error) => error instanceof Refusal && pattern.test(error.message),
      );
    refused({ [CORPUS_VARIABLE]: "" }, /VERTER_PARITY_CORPUS is not set/);
    refused({ [SOURCE_VARIABLE]: "" }, /VERTER_PARITY_CORPUS_SOURCE is not set/);
    refused(
      { [CORPUS_VARIABLE]: path.join(repoRoot, "target", "corpus") },
      /inside the Verter tree/,
    );
    refused({ [STEPS_VARIABLE]: "[{" }, /VERTER_PARITY_CORPUS_STEPS/);
    refused({ [STEPS_VARIABLE]: JSON.stringify([{ argv: [] }]) }, /VERTER_PARITY_CORPUS_STEPS/);
    refused(
      { [STEPS_VARIABLE]: JSON.stringify([{ cwd: "../outside", argv: ["true"] }]) },
      /outside the checkout/,
    );
    refused({ [ROOT_VARIABLE]: "../outside" }, /outside the checkout/);
    const plan = planProvision(env, repoRoot);
    assert.equal(plan.ref, "main");
    assert.equal(plan.steps.length, 2);
    assert.equal(existsSync(env[CORPUS_VARIABLE]), false);
  } finally {
    await cleanup();
  }
});

test("a provisioned corpus is reused until its source moves", async () => {
  const { source, env, cleanup } = await workspace();
  try {
    const plan = planProvision(env, repoRoot);
    const first = await provision(plan, quiet);
    assert.equal(first.reused, false);
    assert.equal(first.revision, git(source, "rev-parse", "HEAD"));
    const corpus = realpathSync.native(env[CORPUS_VARIABLE]);
    assert.equal(corpus, realpathSync.native(path.join(first.checkout, "members")));
    assert.equal(readFileSync(path.join(corpus, "revision.txt"), "utf8"), "one\n");
    assert.equal(existsSync(path.join(corpus, "prepared")), true);

    const second = await provision(plan, quiet);
    assert.equal(second.reused, true);
    assert.equal(second.checkout, first.checkout);
    assert.equal(readFileSync(path.join(first.checkout, "steps.log"), "utf8"), "install\n");

    await writeFile(path.join(source, "members", "revision.txt"), "two\n");
    git(source, "commit", "--quiet", "-am", "two");
    const third = await provision(plan, quiet);
    assert.equal(third.reused, false);
    assert.notEqual(third.checkout, first.checkout);
    assert.equal(readFileSync(path.join(env[CORPUS_VARIABLE], "revision.txt"), "utf8"), "two\n");
  } finally {
    await cleanup();
  }
});

test("a failing step leaves the corpus link as it was and is retried next time", async () => {
  const { env, cleanup } = await workspace();
  try {
    await assert.rejects(
      provision(planProvision({ ...env, [STEPS_VARIABLE]: steps(true) }, repoRoot), quiet),
      /step 2 .* exited 3/,
    );
    assert.equal(existsSync(env[CORPUS_VARIABLE]), false);
    const retried = await provision(planProvision(env, repoRoot), quiet);
    assert.equal(retried.reused, false);
    assert.equal(existsSync(path.join(env[CORPUS_VARIABLE], "prepared")), true);
  } finally {
    await cleanup();
  }
});
