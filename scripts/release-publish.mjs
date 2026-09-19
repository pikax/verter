#!/usr/bin/env node

/**
 * release-publish.mjs — the ONE publish path for a Verter release.
 *
 * release.yml runs the same subcommands a maintainer runs by hand, so a
 * release that CI cannot finish (a red test lane on an otherwise complete
 * build matrix) is published locally from the SAME build artifacts, by the
 * SAME staging and publish logic, with the same fail-closed checks.
 *
 *   node scripts/release-publish.mjs list [--json]
 *       The packages the release publishes, in publish order.
 *
 *   node scripts/release-publish.mjs stage --artifacts <dir>
 *       Copy the downloaded release-workflow artifacts into the tree: every
 *       platform package's binary, the wasm build, the napi loader. Refuses to
 *       stage anything when any platform package cannot be fed.
 *
 *   node scripts/release-publish.mjs prepare
 *       Build the TypeScript packages, generate @verter/native's types and run
 *       its packaging guards.
 *
 *   node scripts/release-publish.mjs publish-npm --dist-tag <tag> [--provenance]
 *       [--otp <code>] [--interactive-otp] [--dry-run]
 *       Pack every package in the publish set (platform packages first, then
 *       the topological order), set the executable bit on shipped binaries
 *       inside the tarball, and publish each tarball. `--interactive-otp`
 *       prompts for a one-time password when the registry asks for one and
 *       re-prompts exactly when a code expires — a batch shares one code.
 *
 *   node scripts/release-publish.mjs tag-npm --tag <dist-tag> [--otp <code>] [--interactive-otp]
 *       Point a dist-tag (e.g. latest) at the workspace version for every
 *       package in the publish set, with the same OTP batching.
 *
 *   node scripts/release-publish.mjs verify-npm [--attempts <n>]
 *       Confirm every package in the publish set exists on the registry at
 *       the workspace version.
 *
 *   node scripts/release-publish.mjs publish-crates [--dry-run]
 *       `cargo publish` the crates.io crates in dependency order.
 *
 *   node scripts/release-publish.mjs local [--run <id>] [--tag v<version>]
 *       [--dir <path>] [--skip-crates] [--dry-run] [--redownload]
 *       [--allow-head-mismatch] [--otp <code>]
 *       The whole local release: preflight, download the tag's release-run
 *       artifacts with `gh`, then stage → prepare → publish-npm (interactive
 *       OTP) → verify-npm → publish-crates.
 *
 * Artifacts directory layout (what `gh run download` and
 * `actions/download-artifact` both produce): `<dir>/<artifact-name>/<files>`.
 */

import { execFileSync, spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createInterface } from "node:readline/promises";
import { dirname, join, relative, resolve } from "node:path";
import { computePublishSet, PUBLISHED_CRATES, scanWorkspacePackages } from "./lib/publish-set.mjs";
import {
  BINARY_FAMILIES,
  invokedAsEntrypoint,
  classifyCargoPublishOutcome,
  distTagForVersion,
  markTarballEntriesExecutable,
  npmPublishArgs,
  parsePlatformDir,
  planCoreStaging,
  planPlatformStaging,
  publishSequentially,
} from "./lib/release-publish.mjs";

const ROOT = resolve(import.meta.dirname, "..");
const PACKAGES_DIR = join(ROOT, "packages");
const DEFAULT_RELEASE_DIR = ".release";
const CRATES_INDEX_PROPAGATION_MS = 30_000;
const VERIFY_RETRY_MS = 15_000;
const DEFAULT_VERIFY_ATTEMPTS = 8;

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const flags = new Map();
  const positional = [];
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (!arg.startsWith("--")) {
      positional.push(arg);
      continue;
    }
    const eq = arg.indexOf("=");
    if (eq !== -1) {
      flags.set(arg.slice(2, eq), arg.slice(eq + 1));
      continue;
    }
    const next = argv[i + 1];
    if (next !== undefined && !next.startsWith("--")) {
      flags.set(arg.slice(2), next);
      i += 1;
    } else {
      flags.set(arg.slice(2), true);
    }
  }
  return { flags, positional };
}

function fail(message) {
  console.error(`release-publish: ${message}`);
  process.exit(1);
}

function log(line = "") {
  console.log(line);
}

function heading(title) {
  log("");
  log(`=== ${title} ===`);
}

// ---------------------------------------------------------------------------
// Process helpers
// ---------------------------------------------------------------------------

// npm and pnpm are `.cmd` shims on Windows and need a shell; every other tool
// used here (git, gh, cargo, node) is a real executable.
const SHELL_TOOLS = new Set(["npm", "pnpm"]);

export function quoteForShell(arg) {
  return /[\s"]/.test(arg) ? `"${arg.replace(/"/g, '\\"')}"` : arg;
}

/** Run a command, streaming its output. Throws on a non-zero exit. */
function run(cmd, args, options = {}) {
  const useShell = process.platform === "win32" && SHELL_TOOLS.has(cmd);
  const result = spawnSync(
    useShell ? [cmd, ...args.map(quoteForShell)].join(" ") : cmd,
    useShell ? [] : args,
    {
      cwd: options.cwd ?? ROOT,
      stdio: "inherit",
      shell: useShell,
      env: { ...process.env, ...(options.env ?? {}) },
    },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${cmd} ${args.join(" ")} exited with ${result.status}`);
  }
}

/** Run a command, capturing stdout+stderr. Never throws on a non-zero exit. */
function capture(cmd, args, options = {}) {
  const useShell = process.platform === "win32" && SHELL_TOOLS.has(cmd);
  const result = spawnSync(
    useShell ? [cmd, ...args.map(quoteForShell)].join(" ") : cmd,
    useShell ? [] : args,
    {
      cwd: options.cwd ?? ROOT,
      encoding: "utf8",
      shell: useShell,
      env: { ...process.env, ...(options.env ?? {}) },
      maxBuffer: 64 * 1024 * 1024,
    },
  );
  if (result.error) {
    return {
      exitCode: 127,
      stdout: "",
      stderr: String(result.error),
      output: String(result.error),
    };
  }
  return {
    exitCode: result.status ?? 1,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    output: `${result.stdout ?? ""}${result.stderr ?? ""}`,
  };
}

function git(args) {
  return execFileSync("git", args, { cwd: ROOT, encoding: "utf8" }).trim();
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// ---------------------------------------------------------------------------
// Workspace facts
// ---------------------------------------------------------------------------

/** The release version: `[workspace.package] version` in Cargo.toml, which every crate and package holds. */
function workspaceVersion() {
  const cargoToml = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const section = /\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/.exec(cargoToml);
  const match = section && /^version\s*=\s*"([^"]+)"/m.exec(section[1]);
  if (!match) fail("could not read the workspace version from Cargo.toml [workspace.package]");
  return match[1];
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function toPosix(p) {
  return p.split("\\").join("/");
}

/** Platform packages in the publish set with their manifest `files`. */
function platformPackages(publishSet) {
  return publishSet.platform.map((dir) => {
    const pkg = readJson(join(ROOT, dir, "package.json"));
    return { dir: toPosix(dir), name: pkg.name, files: pkg.files ?? [] };
  });
}

/**
 * Every package the release publishes, in publish order: platform packages
 * first (so a launcher's optionalDependencies resolve the moment the launcher
 * lands), then the main packages in dependency order.
 */
function publishTargets() {
  const publishSet = computePublishSet();
  const workspace = scanWorkspacePackages(PACKAGES_DIR);
  const targets = [];
  for (const entry of platformPackages(publishSet)) {
    const family = BINARY_FAMILIES[parsePlatformDir(entry.dir)?.family];
    targets.push({
      label: entry.name,
      dir: join(ROOT, entry.dir),
      executableFiles: family?.executable ? entry.files : [],
    });
  }
  for (const name of publishSet.order) {
    const entry = workspace.get(name);
    if (!entry) fail(`publish set names "${name}" but it is not a workspace package`);
    targets.push({ label: name, dir: entry.dir, executableFiles: [] });
  }
  return { publishSet, targets };
}

/**
 * `list --json`: the publish targets as data, so a guard can execute the same
 * derivation the publish runs — platform packages first, then the main
 * packages in dependency order — instead of inferring it from source text.
 */
function listTargets(flags) {
  const version = workspaceVersion();
  const { targets } = publishTargets();
  const rows = targets.map((target) => ({
    name: target.label,
    dir: toPosix(relative(ROOT, target.dir)),
    kind: parsePlatformDir(toPosix(relative(ROOT, target.dir))) ? "platform" : "package",
    executableFiles: target.executableFiles,
  }));
  if (flags.get("json") === true) {
    process.stdout.write(
      `${JSON.stringify({ version, distTag: distTagForVersion(version), targets: rows }, null, 2)}\n`,
    );
    return;
  }
  log(
    `${rows.length} packages @ ${version} (dist-tag ${distTagForVersion(version)}), in publish order:`,
  );
  for (const row of rows) log(`  [${row.kind}] ${row.name}  ${row.dir}`);
}

/** Recursively list an artifacts dir as artifact name → posix-relative files. */
function listArtifacts(artifactsDir) {
  if (!existsSync(artifactsDir) || !statSync(artifactsDir).isDirectory()) {
    fail(`artifacts directory does not exist: ${artifactsDir}`);
  }
  const artifacts = {};
  for (const entry of readdirSync(artifactsDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const files = [];
    const walk = (dir) => {
      for (const child of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, child.name);
        if (child.isDirectory()) walk(full);
        else files.push(toPosix(relative(join(artifactsDir, entry.name), full)));
      }
    };
    walk(join(artifactsDir, entry.name));
    artifacts[entry.name] = files.sort();
  }
  return artifacts;
}

// ---------------------------------------------------------------------------
// stage
// ---------------------------------------------------------------------------

function stage(flags) {
  const artifactsDir = resolve(
    ROOT,
    flags.get("artifacts") ?? fail("stage requires --artifacts <dir>"),
  );
  heading(`Stage release artifacts from ${artifactsDir}`);

  const publishSet = computePublishSet();
  const artifacts = listArtifacts(artifactsDir);
  log(`Artifacts found: ${Object.keys(artifacts).length}`);

  const platform = planPlatformStaging(platformPackages(publishSet), artifacts);
  const core = planCoreStaging(artifacts);
  const problems = [...platform.problems, ...core.problems];
  if (problems.length > 0) {
    for (const problem of problems) console.error(`  MISSING: ${problem}`);
    fail(`${problems.length} staging problem(s) — refusing to stage a partial release`);
  }

  for (const copy of [...platform.copies, ...core.copies]) {
    const source = join(artifactsDir, copy.artifact, copy.source);
    const destination = join(ROOT, copy.destination);
    mkdirSync(dirname(destination), { recursive: true });
    copyFileSync(source, destination);
    if (copy.executable) chmodSync(destination, 0o755);
    log(`  ${copy.artifact}/${copy.source} -> ${copy.destination}`);
  }
  log(`Staged ${platform.copies.length} platform binaries and ${core.copies.length} core files`);
}

// ---------------------------------------------------------------------------
// prepare
// ---------------------------------------------------------------------------

function prepare() {
  heading("Build the TypeScript packages");
  run("pnpm", ["run", "build:ts"]);

  heading("Generate @verter/native types");
  run("pnpm", ["--filter", "@verter/native", "run", "build:types"]);

  // The packaging guards only: they build their own fake install layouts and
  // use `npm pack --dry-run`, so they need no `.node` in the main package's
  // binary-free `dist/`. The Rust-backed suite belongs to CI, not the publish.
  heading("Guard @verter/native pack shape + loader fallback");
  run("pnpm", [
    "--filter",
    "@verter/native",
    "exec",
    "vitest",
    "run",
    "loader-fallback.spec.ts",
    "pack-shape.spec.ts",
    "clean-dist.spec.ts",
  ]);
}

// ---------------------------------------------------------------------------
// publish-npm
// ---------------------------------------------------------------------------

/**
 * Pack one package with pnpm (which rewrites `workspace:` ranges to the real
 * versions) and return the tarball path.
 */
export function packTarball(target, packDir) {
  const result = capture("pnpm", ["pack", "--pack-destination", packDir, "--json"], {
    cwd: target.dir,
  });
  if (result.exitCode !== 0) {
    throw new Error(`pnpm pack failed for ${target.label}:\n${result.output}`);
  }
  // pnpm may print warnings before the JSON document.
  const start = result.stdout.indexOf("{");
  if (start === -1)
    throw new Error(`pnpm pack printed no JSON for ${target.label}:\n${result.output}`);
  const info = JSON.parse(result.stdout.slice(start));
  const filename = info.filename ?? info.path;
  if (!filename) throw new Error(`pnpm pack reported no filename for ${target.label}`);
  return resolve(target.dir, filename);
}

/**
 * Give the shipped binaries the executable bit inside the tarball. A tarball
 * packed on Windows records 0644 for every file — the platform package then
 * installs fine and cannot be spawned. Fail closed when a declared binary is
 * not in the archive.
 */
export function fixExecutableBits(target, tarball) {
  if (target.executableFiles.length === 0) return;
  const { bytes, patched } = markTarballEntriesExecutable(
    readFileSync(tarball),
    target.executableFiles,
  );
  const missing = target.executableFiles.filter((f) => !patched.includes(f));
  if (missing.length > 0) {
    throw new Error(
      `${target.label}: tarball is missing declared binary file(s): ${missing.join(", ")}`,
    );
  }
  writeFileSync(tarball, bytes);
}

async function promptOtpInteractively(reason) {
  const rl = createInterface({ input: process.stdin, output: process.stderr });
  try {
    for (;;) {
      const answer = (
        await rl.question(`\n${reason}\nnpm one-time password (empty to abort): `)
      ).trim();
      if (answer === "") return null;
      if (/^\d{6,8}$/.test(answer)) return answer;
      console.error("  a one-time password is 6 to 8 digits");
    }
  } finally {
    rl.close();
  }
}

async function publishNpm(flags) {
  const version = workspaceVersion();
  const distTag = flags.get("dist-tag") ?? fail("publish-npm requires --dist-tag <tag>");
  const provenance = flags.get("provenance") === true;
  const dryRun = flags.get("dry-run") === true;
  const interactive = flags.get("interactive-otp") === true;
  const initialOtp = typeof flags.get("otp") === "string" ? flags.get("otp") : null;
  const packDir = resolve(
    ROOT,
    flags.get("pack-dir") ?? join(DEFAULT_RELEASE_DIR, `v${version}`, "tarballs"),
  );
  mkdirSync(packDir, { recursive: true });

  const { targets } = publishTargets();
  heading(
    `Publish ${targets.length} npm packages @ ${version} (dist-tag ${distTag}${provenance ? ", provenance" : ""}${dryRun ? ", DRY RUN" : ""})`,
  );

  const publish = async (target, { otp, provenance: withProvenance }) => {
    log("");
    log(`--- ${target.label} ---`);
    const tarball = packTarball(target, packDir);
    fixExecutableBits(target, tarball);
    const args = npmPublishArgs(tarball, { distTag, provenance: withProvenance, otp, dryRun });
    const result = capture("npm", args);
    return { exitCode: result.exitCode, output: result.output };
  };

  const summary = await publishSequentially(targets, {
    publish,
    provenance,
    initialOtp,
    promptOtp: interactive ? promptOtpInteractively : async () => null,
    log,
  });

  heading("Publish summary");
  log(`  Published: ${summary.published.length}`);
  log(`  Skipped:   ${summary.skipped.length} (already on the registry)`);
  log(`  Failed:    ${summary.failed.length}`);
  if (summary.otpPrompts > 0) log(`  OTP prompts: ${summary.otpPrompts}`);
  for (const failure of summary.failed) {
    log("");
    log(`FAILED ${failure.label}:`);
    log(failure.output);
  }
  if (summary.failed.length > 0) fail(`${summary.failed.length} package(s) failed to publish`);
  if (summary.published.length === 0 && summary.skipped.length === 0) {
    fail("no package was published or found on the registry");
  }
}

// ---------------------------------------------------------------------------
// tag-npm
// ---------------------------------------------------------------------------

/**
 * Point a dist-tag (e.g. `latest`) at the workspace version for every package
 * in the publish set. Same target list and OTP loop as publish-npm.
 */
async function tagNpm(flags) {
  const version = workspaceVersion();
  const tag = flags.get("tag") ?? fail("tag-npm requires --tag <dist-tag>");
  const interactive = flags.get("interactive-otp") === true;
  const initialOtp = typeof flags.get("otp") === "string" ? flags.get("otp") : null;
  const { targets } = publishTargets();
  heading(`Point dist-tag "${tag}" at ${version} for ${targets.length} packages`);

  const summary = await publishSequentially(targets, {
    publish: async (target, { otp }) => {
      log("");
      log(`--- ${target.label} ---`);
      const args = [
        "dist-tag",
        "add",
        `${target.label}@${version}`,
        tag,
        ...(otp ? ["--otp", otp] : []),
      ];
      const result = capture("npm", args);
      return { exitCode: result.exitCode, output: result.output };
    },
    initialOtp,
    promptOtp: interactive ? promptOtpInteractively : async () => null,
    log,
  });

  heading("Dist-tag summary");
  log(`  Tagged:  ${summary.published.length}`);
  log(`  Failed:  ${summary.failed.length}`);
  for (const failure of summary.failed) {
    log("");
    log(`FAILED ${failure.label}:`);
    log(failure.output);
  }
  if (summary.failed.length > 0) fail(`${summary.failed.length} package(s) could not be tagged`);
}

// ---------------------------------------------------------------------------
// verify-npm
// ---------------------------------------------------------------------------

async function verifyNpm(flags) {
  const version = workspaceVersion();
  const attempts = Number(flags.get("attempts") ?? DEFAULT_VERIFY_ATTEMPTS);
  const { targets } = publishTargets();
  heading(`Verify ${targets.length} packages @ ${version} on the registry`);

  let missing = targets.map((t) => t.label);
  for (let attempt = 1; attempt <= attempts && missing.length > 0; attempt++) {
    if (attempt > 1) {
      log(
        `  ${missing.length} not visible yet — waiting ${VERIFY_RETRY_MS / 1000}s for registry propagation (attempt ${attempt}/${attempts})`,
      );
      await sleep(VERIFY_RETRY_MS);
    }
    const still = [];
    for (const name of missing) {
      const result = capture("npm", ["view", `${name}@${version}`, "version", "--json"]);
      let found = null;
      try {
        found = result.exitCode === 0 ? JSON.parse(result.stdout) : null;
      } catch {
        found = null;
      }
      if (found === version || (Array.isArray(found) && found.includes(version))) {
        log(`  OK: ${name}@${version}`);
      } else {
        still.push(name);
      }
    }
    missing = still;
  }

  if (missing.length > 0) {
    for (const name of missing) console.error(`  MISSING: ${name}@${version}`);
    fail(`${missing.length} package(s) not found on the registry`);
  }
  log("All packages verified on the registry");
}

// ---------------------------------------------------------------------------
// publish-crates
// ---------------------------------------------------------------------------

async function publishCrates(flags) {
  const version = workspaceVersion();
  const dryRun = flags.get("dry-run") === true;
  heading(`Publish crates @ ${version}${dryRun ? " (DRY RUN)" : ""}`);

  for (let i = 0; i < PUBLISHED_CRATES.length; i++) {
    const crate = PUBLISHED_CRATES[i];
    log("");
    log(`--- ${crate} ---`);
    // `--allow-dirty`: the staged release binaries sit in the tree (all
    // gitignored); cargo's cleanliness check must not refuse the publish.
    const args = ["publish", "-p", crate, "--allow-dirty", ...(dryRun ? ["--dry-run"] : [])];
    const result = capture("cargo", args);
    process.stdout.write(result.output);
    const outcome = classifyCargoPublishOutcome(result.exitCode, result.output);
    if (outcome === "failed") fail(`cargo publish failed for ${crate}`);
    if (outcome === "already-published") {
      log(`  SKIP: ${crate} ${version} already on crates.io`);
      continue;
    }
    log(`  OK: ${crate} ${version} published`);
    if (!dryRun && i < PUBLISHED_CRATES.length - 1) {
      log(
        `  waiting ${CRATES_INDEX_PROPAGATION_MS / 1000}s for the crates.io index before the dependent crate`,
      );
      await sleep(CRATES_INDEX_PROPAGATION_MS);
    }
  }
}

// ---------------------------------------------------------------------------
// local — the whole release from a tag's release-run artifacts
// ---------------------------------------------------------------------------

function ghJson(args) {
  const result = capture("gh", args);
  if (result.exitCode !== 0) fail(`gh ${args.join(" ")} failed:\n${result.output}`);
  return JSON.parse(result.stdout);
}

/** The completed release.yml run for the tag's commit, newest first. */
function findReleaseRun(tag, tagSha) {
  const runs = ghJson([
    "run",
    "list",
    "--workflow",
    "release.yml",
    "--branch",
    tag,
    "--limit",
    "20",
    "--json",
    "databaseId,headSha,status,createdAt",
  ]);
  const candidates = runs.filter((r) => r.headSha === tagSha && r.status === "completed");
  if (candidates.length === 0) {
    fail(
      `no completed release.yml run found for ${tag} at ${tagSha} — pass --run <id> once the build jobs have finished`,
    );
  }
  candidates.sort((a, b) => (a.createdAt < b.createdAt ? 1 : -1));
  return candidates[0].databaseId;
}

async function local(flags) {
  const version = workspaceVersion();
  const tag = flags.get("tag") ?? `v${version}`;
  const dryRun = flags.get("dry-run") === true;
  const skipCrates = flags.get("skip-crates") === true;
  const releaseDir = resolve(ROOT, flags.get("dir") ?? join(DEFAULT_RELEASE_DIR, tag));
  const artifactsDir = join(releaseDir, "artifacts");
  const distTag = distTagForVersion(version);

  heading(`Local release ${tag} (version ${version}, dist-tag ${distTag})`);

  // --- Preflight: the tree is the tagged tree, the version is complete, the
  // credentials are there. Every check here is one that would otherwise fail
  // the release half way through, after some packages already landed.
  if (tag !== `v${version}`) {
    fail(
      `tag ${tag} does not match the workspace version ${version} — check out the tagged commit first`,
    );
  }
  const tagSha = (() => {
    try {
      return git(["rev-parse", `${tag}^{commit}`]);
    } catch {
      return fail(`tag ${tag} does not exist locally (git fetch --tags?)`);
    }
  })();
  const headSha = git(["rev-parse", "HEAD"]);
  if (headSha !== tagSha) {
    const message = `HEAD ${headSha.slice(0, 10)} is not the tagged commit ${tagSha.slice(0, 10)} — the TypeScript packages would be built from a different tree than the binaries`;
    if (flags.get("allow-head-mismatch") === true) console.warn(`WARNING: ${message}`);
    else fail(`${message} (git checkout ${tag}, or pass --allow-head-mismatch)`);
  }
  const dirty = git(["status", "--porcelain", "--untracked-files=no"]);
  if (dirty) fail(`the working tree has uncommitted changes:\n${dirty}`);
  log(`Tree: ${headSha.slice(0, 10)} (${tag})`);

  run(process.execPath, [join(ROOT, "scripts", "set-version.mjs"), "--check", version]);

  if (!dryRun) {
    const who = capture("npm", ["whoami"]);
    if (who.exitCode !== 0) fail(`not logged in to npm (run \`npm login\` first):\n${who.output}`);
    log(`npm user: ${who.stdout.trim()}`);
  }

  // --- Download the tag's release-run artifacts (the exact binaries CI built
  // for this tag), unless a previous invocation already did.
  const runId = flags.get("run") ?? findReleaseRun(tag, tagSha);
  const runInfo = ghJson([
    "run",
    "view",
    String(runId),
    "--json",
    "headSha,status,conclusion,workflowName",
  ]);
  if (runInfo.headSha !== tagSha) {
    fail(`run ${runId} built ${runInfo.headSha}, not the tagged commit ${tagSha}`);
  }
  log(`Release run: ${runId} (${runInfo.workflowName}, ${runInfo.status}/${runInfo.conclusion})`);

  const haveArtifacts =
    existsSync(artifactsDir) &&
    readdirSync(artifactsDir, { withFileTypes: true }).some((d) => d.isDirectory());
  if (haveArtifacts && flags.get("redownload") !== true) {
    log(`Reusing downloaded artifacts in ${artifactsDir} (pass --redownload to fetch again)`);
  } else {
    heading(`Download run ${runId} artifacts to ${artifactsDir}`);
    mkdirSync(artifactsDir, { recursive: true });
    run("gh", ["run", "download", String(runId), "-D", artifactsDir]);
  }

  // --- The shared publish path.
  stage(new Map([["artifacts", artifactsDir]]));
  prepare();
  await publishNpm(
    new Map([
      ["dist-tag", distTag],
      ["interactive-otp", true],
      ["pack-dir", join(releaseDir, "tarballs")],
      ...(typeof flags.get("otp") === "string" ? [["otp", flags.get("otp")]] : []),
      ...(dryRun ? [["dry-run", true]] : []),
    ]),
  );
  if (!dryRun) await verifyNpm(new Map());
  if (skipCrates) {
    log("");
    log("Skipping crates.io (--skip-crates)");
  } else {
    await publishCrates(new Map(dryRun ? [["dry-run", true]] : []));
  }

  heading("Done");
  log(`npm (${distTag}) and crates.io are published for ${tag}.`);
  log("Not covered here (release.yml owns them): the GitHub Release with its staged assets,");
  log("the CHANGELOG commit, the platform VSIXes and the Marketplace publish.");
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

const invoked = invokedAsEntrypoint(process.argv[1], import.meta.url);
if (invoked) {
  const { flags, positional } = parseArgs(process.argv.slice(2));
  const command = positional[0];

  const COMMANDS = {
    list: async () => listTargets(flags),
    stage: async () => stage(flags),
    prepare: async () => prepare(),
    "publish-npm": () => publishNpm(flags),
    "tag-npm": () => tagNpm(flags),
    "verify-npm": () => verifyNpm(flags),
    "publish-crates": () => publishCrates(flags),
    local: () => local(flags),
  };

  if (!command || !COMMANDS[command]) {
    console.error(
      `usage: node scripts/release-publish.mjs <${Object.keys(COMMANDS).join("|")}> [options]\n` +
        "see the header comment of scripts/release-publish.mjs for each subcommand's options",
    );
    process.exit(2);
  }

  try {
    await COMMANDS[command]();
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error));
  }
}
