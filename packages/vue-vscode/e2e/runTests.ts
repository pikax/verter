import * as path from "path";
import * as fs from "fs";
import { execSync } from "child_process";
import { runTests } from "@vscode/test-electron";
import * as os from "os";
import { copyLspBinaryToTemp, readE2eEnv, resolveVscodeExecutablePath } from "./sharedLaunch";

/**
 * Fixture entries: plain name uses auto type provider,
 * "name@provider" forces a specific type provider (tsserver or tsgo).
 * Every fixture runs with both providers to ensure full coverage.
 */
const FIXTURES = [
  "single-project@tsserver",
  "single-project@tsgo",
  "monorepo@tsserver",
  "monorepo@tsgo",
  "tsconfig-extends@tsserver",
  "tsconfig-extends@tsgo",
  "tsconfig-references@tsserver",
  "tsconfig-references@tsgo",
  "path-aliases@tsserver",
  "path-aliases@tsgo",
  "composite-paths@tsserver",
  "composite-paths@tsgo",
  "no-config@tsserver",
  "no-config@tsgo",
  "single-file@tsserver",
  "single-file@tsgo",
  "barrel-exports@tsserver",
  "barrel-exports@tsgo",
];

/**
 * Parse a fixture entry into fixture name and optional type provider override.
 * "composite-paths" → { fixture: "composite-paths", typeProvider: undefined }
 * "composite-paths@tsgo" → { fixture: "composite-paths", typeProvider: "tsgo" }
 */
function parseFixtureEntry(entry: string): { fixture: string; typeProvider?: string } {
  const atIndex = entry.indexOf("@");
  if (atIndex === -1) return { fixture: entry };
  return {
    fixture: entry.slice(0, atIndex),
    typeProvider: entry.slice(atIndex + 1),
  };
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
  const vscodeVersion = readE2eEnv("VSCODE_VERSION") ?? "stable";

  const fixtureArg = process.argv.find((a) => a.startsWith("--fixture="));
  const envFixture = readE2eEnv("FIXTURE");
  const envTypeProvider = readE2eEnv("TYPE_PROVIDER");
  const fixturesToRun = fixtureArg
    ? [fixtureArg.replace("--fixture=", "")]
    : envFixture
      ? [envTypeProvider ? `${envFixture}@${envTypeProvider}` : envFixture]
      : envTypeProvider
        ? FIXTURES.filter((entry) => parseFixtureEntry(entry).typeProvider === envTypeProvider)
        : FIXTURES;

  const vscodeExecutablePath = await resolveVscodeExecutablePath(vscodeVersion);

  // Copy LSP binary to temp to prevent file locking
  const lspBinaryPath = copyLspBinaryToTemp(extensionDevelopmentPath);

  let totalFailures = 0;

  for (const entry of fixturesToRun) {
    const { fixture, typeProvider } = parseFixtureEntry(entry);
    const label = typeProvider ? `${fixture}@${typeProvider}` : fixture;
    const fixtureDir = path.join(extensionDevelopmentPath, "e2e", "fixtures", fixture);

    console.log(`\n${"=".repeat(60)}`);
    console.log(`Running E2E tests for fixture: ${label}`);
    console.log(`Workspace: ${fixtureDir}`);
    if (typeProvider) console.log(`Type provider override: ${typeProvider}`);
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
        launchArgs: ["--disable-extensions", "--disable-updates", fixtureDir],
        extensionTestsEnv: {
          ...process.env,
          VERTER_E2E_TEST: "1",
          VERTER_E2E_LOG_FILE: path.join(os.tmpdir(), `verter-e2e-${label}.log`),
          VERTER_E2E_FIXTURE: fixture,
          VERTER_E2E_TIMING_FILE: path.join(os.tmpdir(), `verter-e2e-timing-${label}.json`),
          VERTER_LOG: "debug",
          ...(lspBinaryPath ? { VERTER_E2E_LSP_PATH: lspBinaryPath } : {}),
          ...(typeProvider ? { VERTER_E2E_TYPE_PROVIDER: typeProvider } : {}),
        },
      });
      console.log(`  PASSED: ${label}`);
    } catch (err) {
      console.error(`  FAILED: ${label}`, err);
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
