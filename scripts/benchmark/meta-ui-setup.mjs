import path from "node:path";
import process from "node:process";

/**
 * Tier 6 §8.2 / T9.3 — strict-ref enforcement for the nuxt-ui
 * benchmark setup. Per D57, the setup MUST be reproducible: pinning
 * to an explicit ref means every CI run, every developer laptop, and
 * every sub-run sees the same git tree. The previous behavior
 * silently fell back to the branch HEAD when `--ref` was absent,
 * which caused benchmark drift between identical-looking runs that
 * actually compiled different upstream commits.
 *
 * Returned argument shape:
 *   - `repoRoot`          (string, normalized forward-slash)
 *   - `targetRoot`        (string, normalized forward-slash)
 *   - `ref`               (string, REQUIRED post-T9.3)
 *   - `allowUnfrozenInstall` (boolean)
 *   - `allowDirtyTarget`  (boolean) — escape hatch reviewed in
 *     `setup-meta-ui.mjs::assertCleanTargetWorktree`. Off by default.
 *
 * Throws when `--ref` is absent or empty.
 */
export function parseMetaUiSetupArgs(argv, repoRoot) {
  const args = {
    repoRoot: normalizePath(repoRoot),
    targetRoot: normalizePath(path.join(repoRoot, ".integration-tests", "repos", "nuxt-ui")),
    ref: null,
    allowUnfrozenInstall: false,
    allowDirtyTarget: false,
  };

  for (const arg of argv) {
    if (arg.startsWith("--target-root=")) {
      args.targetRoot = normalizePath(arg.slice("--target-root=".length));
    } else if (arg.startsWith("--ref=")) {
      args.ref = arg.slice("--ref=".length) || null;
    } else if (arg === "--allow-unfrozen-install") {
      args.allowUnfrozenInstall = true;
    } else if (arg === "--allow-dirty-target") {
      args.allowDirtyTarget = true;
    }
  }

  if (!args.ref) {
    throw new Error(
      "Tier 6 §8.2 / T9.3 — `--ref=<sha-or-ref>` is required. The meta-ui benchmark setup pins\n" +
        "the prepared `nuxt-ui` checkout to an explicit upstream ref so every CI run and every\n" +
        "developer laptop benchmarks the same git tree. Pass a commit sha (preferred) or a\n" +
        "symbolic ref (e.g., `--ref=v0.5.0` or `--ref=refs/pull/123/head`).",
    );
  }

  return args;
}

/**
 * Predicate: parse `git status --porcelain` output and return the
 * list of dirty entries. Each entry is an object with `xy` (the
 * porcelain status code) and `path`. An empty array means the
 * worktree is clean.
 *
 * Pure helper, exported so the discriminating tests in
 * `meta-ui-setup.spec.ts` can characterize the parser without
 * spawning git.
 */
export function parseGitStatusPorcelain(porcelainOutput) {
  if (!porcelainOutput) {
    return [];
  }
  const dirty = [];
  for (const rawLine of porcelainOutput.split("\n")) {
    const line = rawLine.replace(/\r$/, "");
    if (line.length === 0) {
      continue;
    }
    // Porcelain format: `XY <path>` (X = staged, Y = unstaged).
    if (line.length < 4) {
      continue;
    }
    const xy = line.slice(0, 2);
    const filePath = line.slice(3);
    dirty.push({ xy, path: filePath });
  }
  return dirty;
}

export function getMetaUiInstallStrategies({ allowUnfrozenInstall = false } = {}) {
  const strategies = [["install", "--frozen-lockfile"]];
  if (allowUnfrozenInstall) {
    strategies.push(["install", "--no-frozen-lockfile"]);
  }
  return strategies;
}

export function isCommitSha(value) {
  return typeof value === "string" && /^[0-9a-f]{7,40}$/i.test(value);
}

export function getMetaUiCheckoutCommands(project, { ref = null } = {}) {
  // Tier 6 §8.2 / T9.3 — `ref` is required (the parser throws if it
  // is missing). Defensive: refuse here too so a future caller that
  // bypasses `parseMetaUiSetupArgs` cannot regress to the
  // floating-branch behavior.
  if (!ref) {
    throw new Error(
      "getMetaUiCheckoutCommands: a `ref` is required (Tier 6 §8.2 / T9.3 strict-ref). The\n" +
        "previous floating-branch fallback was retired because it produced silent benchmark\n" +
        "drift between identical-looking runs that actually compiled different commits.",
    );
  }
  const commands = [["fetch", "origin", project.branch, "--prune", "--tags"]];

  if (isCommitSha(ref)) {
    commands.push(["checkout", "--detach", ref]);
  } else {
    commands.push(["fetch", "origin", ref, "--prune", "--tags"]);
    commands.push(["checkout", "--detach", "FETCH_HEAD"]);
  }
  return commands;
}

export async function resolveMetaUiProject() {
  const moduleUrl = new URL("../integration-test/projects.mjs", import.meta.url);
  const { projects } = await import(moduleUrl);
  const project = projects.find((entry) => entry.name === "nuxt-ui");
  if (!project) {
    throw new Error(
      "Could not find the canonical nuxt-ui definition in scripts/integration-test/projects.mjs.",
    );
  }
  return project;
}

export function validatePreparedMetaUiRepo(root, { exists = defaultExists } = {}) {
  const requiredPaths = [
    path.join(root, "src", "runtime", "components"),
    path.join(root, ".nuxt", "tsconfig.app.json"),
    path.join(root, ".nuxt", "tsconfig.shared.json"),
  ];

  for (const requiredPath of requiredPaths) {
    const normalized = normalizePath(requiredPath);
    if (!exists(normalized)) {
      throw new Error(`Prepared nuxt-ui repo is missing required path: ${normalized}`);
    }
  }
}

function defaultExists(_path) {
  return false;
}

function normalizePath(value) {
  return value.replace(/\\/g, "/");
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const args = parseMetaUiSetupArgs(process.argv.slice(2), process.cwd());
  process.stdout.write(`${JSON.stringify(args, null, 2)}\n`);
}
