import path from "node:path";
import process from "node:process";

export function parseMetaUiSetupArgs(argv, repoRoot) {
  const args = {
    repoRoot: normalizePath(repoRoot),
    targetRoot: normalizePath(path.join(repoRoot, ".integration-tests", "repos", "nuxt-ui")),
    ref: null,
    allowUnfrozenInstall: false,
  };

  for (const arg of argv) {
    if (arg.startsWith("--target-root=")) {
      args.targetRoot = normalizePath(arg.slice("--target-root=".length));
    } else if (arg.startsWith("--ref=")) {
      args.ref = arg.slice("--ref=".length) || null;
    } else if (arg === "--allow-unfrozen-install") {
      args.allowUnfrozenInstall = true;
    }
  }

  return args;
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
  const commands = [["fetch", "origin", project.branch, "--prune", "--tags"]];

  if (ref) {
    if (isCommitSha(ref)) {
      commands.push(["checkout", "--detach", ref]);
    } else {
      commands.push(["fetch", "origin", ref, "--prune", "--tags"]);
      commands.push(["checkout", "--detach", "FETCH_HEAD"]);
    }
    return commands;
  }

  commands.push(["checkout", project.branch]);
  commands.push(["pull", "--ff-only", "origin", project.branch]);
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
