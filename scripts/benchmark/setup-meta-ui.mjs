import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";

import {
  getMetaUiCheckoutCommands,
  getMetaUiInstallStrategies,
  parseGitStatusPorcelain,
  parseMetaUiSetupArgs,
  resolveMetaUiProject,
  validatePreparedMetaUiRepo,
} from "./meta-ui-setup.mjs";

const repoRoot = path.resolve(import.meta.dirname, "../..");

function run(command, args, cwd) {
  execFileSync(command, args, {
    cwd,
    stdio: "inherit",
  });
}

function capture(command, args, cwd) {
  return execFileSync(command, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  }).trim();
}

/**
 * Tier 6 §8.2 / T9.3 — refuse to proceed if the target repo has
 * local modifications, untracked files, or files staged for
 * deletion. The setup script's `git checkout --detach <ref>` step
 * would otherwise silently destroy uncommitted developer work or,
 * worse, mix a stale post-build artefact into a fresh checkout
 * (causing benchmark drift the strict-ref enforcement was meant to
 * prevent).
 *
 * Escape hatch: `--allow-dirty-target` opts out for one-off manual
 * debugging. The flag is intentionally verbose so it shows up in
 * shell history and PR diffs.
 */
function assertCleanTargetWorktree(targetRoot, { allowDirtyTarget = false } = {}) {
  if (!existsSync(path.join(targetRoot, ".git"))) {
    // Fresh clone path — no worktree yet, nothing to clobber.
    return;
  }
  const porcelain = capture("git", ["status", "--porcelain"], targetRoot);
  const dirty = parseGitStatusPorcelain(porcelain);
  if (dirty.length === 0) {
    return;
  }
  if (allowDirtyTarget) {
    process.stderr.write(
      `[setup-meta-ui] warning: target worktree at ${targetRoot} has ${dirty.length} dirty entries; proceeding because --allow-dirty-target was passed.\n`,
    );
    return;
  }
  const summary = dirty
    .slice(0, 20)
    .map((entry) => `  ${entry.xy} ${entry.path}`)
    .join("\n");
  const overflow = dirty.length > 20 ? `\n  ... and ${dirty.length - 20} more` : "";
  throw new Error(
    `Tier 6 §8.2 / T9.3 — target worktree at ${targetRoot} is not clean.\n` +
      `Refusing to proceed; the strict-ref checkout would silently delete or overwrite\n` +
      `${dirty.length} local entries:\n${summary}${overflow}\n\n` +
      `Resolve by stashing/committing the changes, or pass --allow-dirty-target to opt\n` +
      `into the destructive behavior for one-off manual debugging.`,
  );
}

async function main() {
  const args = parseMetaUiSetupArgs(process.argv.slice(2), repoRoot);
  const project = await resolveMetaUiProject();
  const remoteUrl = `https://github.com/${project.repo}.git`;

  mkdirSync(path.dirname(args.targetRoot), { recursive: true });

  if (!existsSync(path.join(args.targetRoot, ".git"))) {
    run("git", ["clone", "--branch", project.branch, remoteUrl, args.targetRoot], repoRoot);
  }

  // Tier 6 §8.2 / T9.3 — refuse to clobber dirty target.
  assertCleanTargetWorktree(args.targetRoot, {
    allowDirtyTarget: args.allowDirtyTarget,
  });

  for (const command of getMetaUiCheckoutCommands(project, args)) {
    run("git", command, args.targetRoot);
  }

  const installStrategies = getMetaUiInstallStrategies(args);
  for (let index = 0; index < installStrategies.length; index++) {
    try {
      run("pnpm", installStrategies[index], args.targetRoot);
      break;
    } catch (error) {
      if (index === installStrategies.length - 1) {
        throw error;
      }
      console.warn(
        "Frozen install failed, retrying without --frozen-lockfile because --allow-unfrozen-install was set.",
      );
    }
  }
  run("pnpm", ["dev:prepare"], args.targetRoot);

  validatePreparedMetaUiRepo(args.targetRoot, { exists: existsSync });
  const resolvedSha = capture("git", ["rev-parse", "HEAD"], args.targetRoot);

  process.stdout.write(
    `${JSON.stringify(
      {
        project: project.name,
        repo: project.repo,
        branch: project.branch,
        root: args.targetRoot,
        resolvedSha,
      },
      null,
      2,
    )}\n`,
  );
}

main().catch((error) => {
  console.error(error instanceof Error ? (error.stack ?? error.message) : error);
  process.exitCode = 1;
});
