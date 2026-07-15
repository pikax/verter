import * as path from "path";
import * as fs from "fs";
import { execSync } from "child_process";
import { resolveCliArgsFromVSCodeExecutablePath, runTests } from "@vscode/test-electron";
import * as os from "os";
import {
  copyLspBinaryToTemp,
  provisionVsCodeExtension,
  readE2eEnv,
  resolveVscodeExecutablePath,
  writeVsCodeUserSettings,
} from "./sharedLaunch";
import { clearRunArtifacts, enforceRunSummary } from "../src/runSummaryOracle";

const EDITOR_ACCEPTANCE_FIXTURE = "editor-owned-project";
const NATIVE_PREVIEW_EXTENSION = "TypeScriptTeam.native-preview@0.20260708.2";

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
  `${EDITOR_ACCEPTANCE_FIXTURE}@tsserver`,
  `${EDITOR_ACCEPTANCE_FIXTURE}@shared-tsgo`,
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
  execSync("npm install --no-package-lock --ignore-scripts", {
    cwd: fixtureDir,
    stdio: "pipe",
    timeout: 60_000,
  });
}

interface E2eProfile {
  root: string;
  extensionsDir: string;
  userDataDir: string;
}

function createE2eProfile(label: string, index: number): E2eProfile {
  const safeLabel = label.replace(/[^a-zA-Z0-9_-]/g, "-");
  const root = path.join(os.tmpdir(), `verter-e2e-profile-${process.pid}-${index}-${safeLabel}`);
  const profile = {
    root,
    extensionsDir: path.join(root, "extensions"),
    userDataDir: path.join(root, "user-data"),
  };
  fs.mkdirSync(profile.extensionsDir, { recursive: true });
  fs.mkdirSync(profile.userDataDir, { recursive: true });
  return profile;
}

function removeE2eProfile(profile: E2eProfile): void {
  const target = path.resolve(profile.root);
  const tempRoot = path.resolve(os.tmpdir());
  if (
    !target.startsWith(`${tempRoot}${path.sep}`) ||
    !path.basename(target).startsWith("verter-e2e-profile-")
  ) {
    throw new Error(`Refusing to remove unexpected E2E profile path: ${target}`);
  }
  fs.rmSync(target, { recursive: true, force: true });
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

  const vscodeExecutablePath = await resolveVscodeExecutablePath(vscodeVersion, {
    explicitExecutablePath: readE2eEnv("VSCODE_EXECUTABLE"),
  });

  // Copy LSP binary to temp to prevent file locking
  const lspBinaryPath = copyLspBinaryToTemp(extensionDevelopmentPath);

  let totalFailures = 0;

  for (const [index, entry] of fixturesToRun.entries()) {
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

    const logFile = path.join(os.tmpdir(), `verter-e2e-${label}.log`);
    const profile = createE2eProfile(label, index);
    // Delete any stale run summary before the run so a prior-run summary can
    // never false-green a current zero-exit crash that writes no fresh summary.
    clearRunArtifacts(logFile);
    try {
      if (fixture === EDITOR_ACCEPTANCE_FIXTURE && typeProvider === "shared-tsgo") {
        const extension = readE2eEnv("NATIVE_PREVIEW_EXTENSION") ?? NATIVE_PREVIEW_EXTENSION;
        console.log(`  Provisioning ${extension} into the isolated test profile...`);
        provisionVsCodeExtension({
          cliArgs: resolveCliArgsFromVSCodeExecutablePath(vscodeExecutablePath),
          extension,
          extensionsDir: profile.extensionsDir,
          userDataDir: profile.userDataDir,
        });
        // Native Preview's restart/API-session commands exist only after its
        // enabled server starts. Seed the isolated profile before first activation
        // so this acceptance exercises the real editor-owned lifecycle.
        writeVsCodeUserSettings(profile.userDataDir, {
          "js/ts.experimental.useTsgo": true,
        });
      }

      const launchArgs = [
        fixtureDir,
        "--disable-updates",
        "--disable-workspace-trust",
        "--skip-welcome",
        "--skip-release-notes",
        `--extensions-dir=${profile.extensionsDir}`,
        `--user-data-dir=${profile.userDataDir}`,
      ];
      if (!(fixture === EDITOR_ACCEPTANCE_FIXTURE && typeProvider === "shared-tsgo")) {
        launchArgs.push("--disable-extensions");
      }
      await runTests({
        vscodeExecutablePath,
        extensionDevelopmentPath,
        extensionTestsPath,
        launchArgs,
        extensionTestsEnv: {
          ...process.env,
          VERTER_E2E_TEST: "1",
          VERTER_E2E_LOG_FILE: logFile,
          VERTER_E2E_FIXTURE: fixture,
          VERTER_E2E_TIMING_FILE: path.join(os.tmpdir(), `verter-e2e-timing-${label}.json`),
          VERTER_LOG: "debug",
          ...(lspBinaryPath ? { VERTER_E2E_LSP_PATH: lspBinaryPath } : {}),
          ...(typeProvider ? { VERTER_E2E_TYPE_PROVIDER: typeProvider } : {}),
          ...(fixture === EDITOR_ACCEPTANCE_FIXTURE
            ? { VERTER_E2E_ONLY: "editor-owned-project.test" }
            : {}),
        },
      });
      // The @vscode/test-electron process exit code is an UNRELIABLE pass/fail signal
      // on some hosts (Windows: VS Code can exit 0 even when the extension test run
      // rejected). The authoritative oracle is the run summary the mocha runner writes
      // (`suite/index.ts` → `<logFile>.runsummary`): fail on any reported test failure,
      // and on a vacuous 0-test execution or a MISSING summary. Every matrix entry is a
      // required gate; no ordinary fixture is allowed a legacy zero-execution pass.
      await enforceRunSummary(logFile, label, {});
      console.log(`  PASSED: ${label}`);
    } catch (err) {
      console.error(`  FAILED: ${label}`, err);
      totalFailures++;
    } finally {
      if (readE2eEnv("KEEP_PROFILE") === "1") {
        console.log(`  Preserved E2E profile: ${profile.root}`);
      } else {
        removeE2eProfile(profile);
      }
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
