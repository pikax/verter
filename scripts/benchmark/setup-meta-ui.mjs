import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";

import {
  getMetaUiCheckoutCommands,
  getMetaUiInstallStrategies,
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

async function main() {
  const args = parseMetaUiSetupArgs(process.argv.slice(2), repoRoot);
  const project = await resolveMetaUiProject();
  const remoteUrl = `https://github.com/${project.repo}.git`;

  mkdirSync(path.dirname(args.targetRoot), { recursive: true });

  if (!existsSync(path.join(args.targetRoot, ".git"))) {
    run("git", ["clone", "--branch", project.branch, remoteUrl, args.targetRoot], repoRoot);
  }

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
