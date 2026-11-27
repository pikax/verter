#!/usr/bin/env node
import { spawn } from "node:child_process";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { mkdtemp } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CORPUS_VARIABLE, Refusal, insideTree } from "./measure.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

/** The git repository the corpus is checked out from. */
export const SOURCE_VARIABLE = "VERTER_PARITY_CORPUS_SOURCE";
/** The branch or tag of the source to check out (default `main`). */
export const REF_VARIABLE = "VERTER_PARITY_CORPUS_REF";
/** The corpus root's directory inside the checkout (default: the checkout itself). */
export const ROOT_VARIABLE = "VERTER_PARITY_CORPUS_ROOT";
/**
 * The steps that prepare a fresh checkout, a JSON array run in order:
 * `{ "argv": [command, ...args], "cwd"?: directory inside the checkout }`,
 * each started directly, without a shell.
 */
export const STEPS_VARIABLE = "VERTER_PARITY_CORPUS_STEPS";

/** The file that marks a checkout whose steps all succeeded, naming its revision. */
const READY = ".verter-corpus-ready";
/** How many prepared checkouts are kept, besides the one the corpus link names. */
const KEEP = 2;
/** How long an unfinished checkout is left for a concurrent provisioning to finish. */
const STALE_MS = 24 * 60 * 60 * 1000;

const usage = `Usage: ${CORPUS_VARIABLE}=<link> ${SOURCE_VARIABLE}=<repository> \\
       [${REF_VARIABLE}=<branch>] [${ROOT_VARIABLE}=<directory>] [${STEPS_VARIABLE}=<JSON>] \\
       node scripts/sfc-projection/parity/provision-corpus.mjs

Prepares the parity corpus that scripts/sfc-projection/parity/measure.mjs reads,
outside the Verter tree. It checks out the newest commit of the source ref into a
directory beside ${CORPUS_VARIABLE} (<link>.builds/), runs the preparation steps
there, and then points the link ${CORPUS_VARIABLE} names at the corpus root inside
that checkout. A checkout already prepared at the same commit is reused; a failed
preparation leaves the link as it was.

Exit 0 means the corpus is ready; exit 2 means it was refused or a step failed.
`;

/** A directory inside the checkout, given relative to it. */
function inCheckout(directory, label) {
  const normal = path.normalize(directory || ".");
  if (path.isAbsolute(normal) || normal === ".." || normal.startsWith(`..${path.sep}`))
    throw new Refusal(`refusing to provision: ${label} lies outside the checkout.`);
  return normal;
}

/** Read and check the provisioning settings; nothing is touched. */
export function planProvision(env, repoRoot = REPO_ROOT) {
  if (!env[CORPUS_VARIABLE])
    throw new Refusal(
      `refusing to provision: ${CORPUS_VARIABLE} is not set; name where the corpus is to appear, outside the Verter tree.`,
    );
  if (!env[SOURCE_VARIABLE])
    throw new Refusal(
      `refusing to provision: ${SOURCE_VARIABLE} is not set; name the repository to check out.`,
    );
  const link = path.resolve(env[CORPUS_VARIABLE]);
  const builds = `${link}.builds`;
  if (insideTree(link, repoRoot) || insideTree(builds, repoRoot))
    throw new Refusal(
      `refusing to provision: the corpus named by ${CORPUS_VARIABLE} is inside the Verter tree.`,
    );
  const malformed = new Refusal(
    `refusing to provision: ${STEPS_VARIABLE} must be a JSON array of steps, each ` +
      `{ "argv": [command, ...args], "cwd"?: directory inside the checkout }.`,
  );
  let steps;
  try {
    steps = JSON.parse(env[STEPS_VARIABLE] || "[]");
  } catch {
    throw malformed;
  }
  if (
    !Array.isArray(steps) ||
    steps.some(
      (step) =>
        !step ||
        !Array.isArray(step.argv) ||
        !step.argv.length ||
        !step.argv[0] ||
        step.argv.some((arg) => typeof arg !== "string") ||
        (step.cwd !== undefined && typeof step.cwd !== "string"),
    )
  )
    throw malformed;
  return {
    link,
    builds,
    source: env[SOURCE_VARIABLE],
    ref: env[REF_VARIABLE] || "main",
    root: inCheckout(env[ROOT_VARIABLE], "the corpus root"),
    steps: steps.map((step, index) => ({
      argv: step.argv,
      cwd: inCheckout(step.cwd, `step ${index + 1}'s directory`),
    })),
  };
}

/** Run one command without a shell; resolve with its stdout when captured. */
function execute(argv, cwd, label, capture = false) {
  return new Promise((resolve, reject) => {
    const child = spawn(argv[0], argv.slice(1), {
      cwd,
      stdio: ["ignore", capture ? "pipe" : "inherit", "inherit"],
    });
    let stdout = "";
    if (capture) child.stdout.setEncoding("utf8").on("data", (chunk) => (stdout += chunk));
    child.on("error", (error) => reject(new Error(`${label} could not start: ${error.code}`)));
    child.on("close", (exit, signal) =>
      exit === 0 ? resolve(stdout) : reject(new Error(`${label} exited ${exit ?? signal}`)),
    );
  });
}

/** The commit the source's ref names now: a branch, else a tag. */
async function remoteRevision(source, ref) {
  const listing = await execute(["git", "ls-remote", source], undefined, "git ls-remote", true);
  const refs = new Map(
    listing
      .split("\n")
      .filter(Boolean)
      .map((line) => line.split("\t").reverse()),
  );
  const revision =
    refs.get(`refs/heads/${ref}`) ??
    refs.get(`refs/tags/${ref}^{}`) ??
    refs.get(`refs/tags/${ref}`);
  if (!revision)
    throw new Refusal(`refusing to provision: the source has no branch or tag ${ref}.`);
  return revision;
}

/** The prepared checkouts under `builds`, newest first. */
function preparedCheckouts(builds) {
  const prepared = [];
  for (const entry of readdirSync(builds, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const checkout = path.join(builds, entry.name);
    const marker = path.join(checkout, READY);
    if (!existsSync(marker)) continue;
    const { revision } = JSON.parse(readFileSync(marker, "utf8"));
    prepared.push({ checkout, revision, at: statSync(marker).mtimeMs });
  }
  return prepared.sort((left, right) => right.at - left.at);
}

/** Point `link` at `target`, replacing an earlier link in one rename. */
function pointLink(link, target) {
  const pending = `${link}.${process.pid}.pending`;
  rmSync(pending, { force: true });
  symlinkSync(target, pending, process.platform === "win32" ? "junction" : "dir");
  try {
    renameSync(pending, link);
  } catch (error) {
    // Windows will not rename a junction over another; a real directory is never replaced.
    if (!existsSync(link) || !lstatSync(link).isSymbolicLink()) {
      rmSync(pending, { force: true });
      throw error;
    }
    unlinkSync(link);
    renameSync(pending, link);
  }
}

/** Remove prepared checkouts beyond the newest few and long-abandoned unfinished ones. */
function prune(builds, current) {
  const prepared = preparedCheckouts(builds);
  const keep = new Set([current, ...prepared.slice(0, KEEP).map((build) => build.checkout)]);
  for (const entry of readdirSync(builds, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const checkout = path.join(builds, entry.name);
    if (keep.has(checkout)) continue;
    const finished = existsSync(path.join(checkout, READY));
    if (finished || Date.now() - statSync(checkout).mtimeMs > STALE_MS)
      rmSync(checkout, { recursive: true, force: true });
  }
}

/**
 * Make the corpus the plan describes ready and point its link at it. Returns
 * the commit, the checkout, the corpus root and whether a prepared checkout
 * was reused.
 */
export async function provision(plan, { log = (line) => process.stderr.write(`${line}\n`) } = {}) {
  mkdirSync(plan.builds, { recursive: true });
  const wanted = await remoteRevision(plan.source, plan.ref);
  const found = preparedCheckouts(plan.builds).find((build) => build.revision === wanted);
  let checkout = found?.checkout;
  let revision = wanted;
  if (found) log(`provision: reusing the corpus prepared at ${revision}.`);
  else {
    checkout = await mkdtemp(path.join(plan.builds, "checkout-"));
    log(`provision: checking out ${revision}.`);
    await execute(["git", "init", "--quiet"], checkout, "git init");
    await execute(
      ["git", "fetch", "--quiet", "--depth", "1", plan.source, revision],
      checkout,
      "git fetch",
    ).catch(() =>
      // A server that refuses a fetch by commit still serves the ref itself.
      execute(
        ["git", "fetch", "--quiet", "--depth", "1", plan.source, plan.ref],
        checkout,
        "git fetch",
      ),
    );
    await execute(
      ["git", "checkout", "--quiet", "--detach", "FETCH_HEAD"],
      checkout,
      "git checkout",
    );
    revision = (
      await execute(["git", "rev-parse", "HEAD"], checkout, "git rev-parse", true)
    ).trim();
    for (const [index, step] of plan.steps.entries()) {
      const label = `step ${index + 1} (${path.basename(step.argv[0])})`;
      log(`provision: ${label}.`);
      await execute(step.argv, path.join(checkout, step.cwd), label);
    }
    writeFileSync(path.join(checkout, READY), `${JSON.stringify({ revision })}\n`);
  }
  const corpus = path.join(checkout, plan.root);
  if (!existsSync(corpus) || !statSync(corpus).isDirectory())
    throw new Refusal("refusing to provision: the checkout has no corpus root directory.");
  pointLink(plan.link, corpus);
  prune(plan.builds, checkout);
  return { revision, checkout, corpus, reused: Boolean(found) };
}

async function main(env) {
  try {
    const result = await provision(planProvision(env));
    process.stderr.write(`provision: the corpus is ready at ${result.revision}.\n`);
  } catch (error) {
    process.stderr.write(`provision: ${error.message}\n`);
    process.exitCode = 2;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length === 3 && process.argv[2] === "--help") process.stdout.write(usage);
  else if (process.argv.length > 2) {
    process.stderr.write(usage);
    process.exitCode = 2;
  } else main(process.env);
}
