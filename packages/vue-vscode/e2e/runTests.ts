import * as path from "path";
import * as fs from "fs";
import { execSync } from "child_process";
import { runTests, downloadAndUnzipVSCode } from "@vscode/test-electron";
import * as os from "os";

const FIXTURES = [
  "single-project",
  "monorepo",
  "tsconfig-extends",
  "tsconfig-references",
  "path-aliases",
  "no-config",
  "single-file",
];

/**
 * Find the verter-lsp binary in the monorepo.
 * Searches: target/debug/, target/release/, dist/, PATH.
 * Returns undefined if not found.
 */
function findLspBinary(extensionPath: string): string | undefined {
  const ext = process.platform === "win32" ? ".exe" : "";
  const binaryName = `verter-lsp${ext}`;

  // Check monorepo target/ (walk upward to find the monorepo root)
  let dir = extensionPath;
  for (let i = 0; i < 5; i++) {
    for (const profile of ["debug", "release"]) {
      const candidate = path.join(dir, "target", profile, binaryName);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
    dir = path.dirname(dir);
  }

  // Check dist/ in extension path
  const distPath = path.join(extensionPath, "dist", binaryName);
  if (fs.existsSync(distPath)) {
    return distPath;
  }

  // Check bin/ in extension path
  const binPath = path.join(extensionPath, "bin", binaryName);
  if (fs.existsSync(binPath)) {
    return binPath;
  }

  return undefined;
}

/**
 * Copy the LSP binary to a temp directory to prevent file locking issues.
 * On Windows, a running .exe is locked and can't be overwritten by cargo build.
 * Returns the path to the copied binary, or undefined if source not found.
 */
function copyLspBinaryToTemp(extensionPath: string): string | undefined {
  const sourcePath = findLspBinary(extensionPath);
  if (!sourcePath) {
    console.warn("Warning: LSP binary not found — tests will use PATH fallback");
    return undefined;
  }

  const ext = process.platform === "win32" ? ".exe" : "";
  const tempDir = path.join(os.tmpdir(), "verter-e2e-bin");
  fs.mkdirSync(tempDir, { recursive: true });

  const destPath = path.join(tempDir, `verter-lsp${ext}`);
  fs.copyFileSync(sourcePath, destPath);

  // Ensure executable permission on Unix
  if (process.platform !== "win32") {
    fs.chmodSync(destPath, 0o755);
  }

  console.log(`LSP binary copied: ${sourcePath} → ${destPath}`);
  return destPath;
}

/**
 * Install dependencies in a fixture directory if it has a package.json.
 * Uses npm (available everywhere) to install deps for Vue type resolution.
 * Skips if node_modules already exists.
 */
function installFixtureDeps(fixtureDir: string): void {
  const pkgJson = path.join(fixtureDir, "package.json");
  const nodeModules = path.join(fixtureDir, "node_modules");

  if (!fs.existsSync(pkgJson) || fs.existsSync(nodeModules)) {
    return;
  }

  console.log(`  Installing dependencies in ${fixtureDir}...`);
  try {
    execSync("npm install --no-package-lock --ignore-scripts", {
      cwd: fixtureDir,
      stdio: "pipe",
      timeout: 60_000,
    });
  } catch (err) {
    console.warn(`  Warning: npm install failed in ${fixtureDir}:`, (err as Error).message);
  }
}

async function main() {
  const extensionDevelopmentPath = path.resolve(__dirname, "../../");
  const extensionTestsPath = path.resolve(__dirname, "./suite/index");

  const fixtureArg = process.argv.find((a) => a.startsWith("--fixture="));
  const fixturesToRun = fixtureArg
    ? [fixtureArg.replace("--fixture=", "")]
    : FIXTURES;

  const vscodeExecutablePath = await downloadAndUnzipVSCode("stable");

  // Copy LSP binary to temp to prevent file locking
  const lspBinaryPath = copyLspBinaryToTemp(extensionDevelopmentPath);

  let totalFailures = 0;

  for (const fixture of fixturesToRun) {
    const fixtureDir = path.resolve(
      __dirname,
      "../fixtures",
      fixture,
    );

    console.log(`\n${"=".repeat(60)}`);
    console.log(`Running E2E tests for fixture: ${fixture}`);
    console.log(`Workspace: ${fixtureDir}`);
    console.log("=".repeat(60));

    // Install fixture dependencies if needed (for Vue type resolution)
    installFixtureDeps(fixtureDir);
    // For monorepo, also install workspace package deps
    if (fixture === "monorepo") {
      const packagesDir = path.join(fixtureDir, "packages");
      if (fs.existsSync(packagesDir)) {
        for (const pkg of fs.readdirSync(packagesDir)) {
          installFixtureDeps(path.join(packagesDir, pkg));
        }
      }
    }

    try {
      await runTests({
        vscodeExecutablePath,
        extensionDevelopmentPath,
        extensionTestsPath,
        launchArgs: ["--disable-extensions", fixtureDir],
        extensionTestsEnv: {
          VERTER_E2E_TEST: "1",
          VERTER_E2E_LOG_FILE: path.join(
            os.tmpdir(),
            `verter-e2e-${fixture}.log`,
          ),
          VERTER_E2E_FIXTURE: fixture,
          VERTER_E2E_TIMING_FILE: path.join(
            os.tmpdir(),
            `verter-e2e-timing-${fixture}.json`,
          ),
          VERTER_LOG: "debug",
          ...(lspBinaryPath ? { VERTER_E2E_LSP_PATH: lspBinaryPath } : {}),
        },
      });
      console.log(`  PASSED: ${fixture}`);
    } catch (err) {
      console.error(`  FAILED: ${fixture}`, err);
      totalFailures++;
    }
  }

  if (totalFailures > 0) {
    console.error(`\n${totalFailures} fixture(s) failed.`);
    process.exit(1);
  }

  console.log("\nAll fixture E2E tests passed.");
}

main().catch((err) => {
  console.error("E2E test runner failed:", err);
  process.exit(1);
});
