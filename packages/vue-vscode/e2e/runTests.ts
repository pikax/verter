import * as path from "path";
import * as fs from "fs";
import { execSync } from "child_process";
import { resolveCliArgsFromVSCodeExecutablePath, runTests } from "@vscode/test-electron";
import * as os from "os";
import {
  copyLspBinaryToTemp,
  findWorkspaceRcTsgoBinary,
  provisionVsCodeExtension,
  readE2eEnv,
  resolveVscodeExecutablePath,
  writeVsCodeUserSettings,
} from "./sharedLaunch";
import { clearRunArtifacts, enforceRunSummary } from "../src/runSummaryOracle";
import {
  requiredFrameworkContractIds,
  type ContractFramework,
} from "./lib/frameworkContractManifest";
import {
  selectParityTestInventory,
  type ParityFixture,
  type ParityTestInventory,
} from "./lib/parityTestInventory";
import {
  e2eRouteLabel,
  parseE2eRouteLabel,
  selectE2eRoutes,
  type E2eRoute,
} from "./lib/routeInventory";

const EDITOR_ACCEPTANCE_FIXTURE = "editor-owned-project";
const NATIVE_PREVIEW_EXTENSION = "TypeScriptTeam.native-preview@0.20260708.2";
const CONTRACT_FIXTURES: Readonly<Record<string, { framework: ContractFramework; only: string }>> =
  {
    "vue-contract": { framework: "vue", only: "frameworks/vue/contract.test" },
    "svelte-contract": { framework: "svelte", only: "frameworks/svelte/contract.test" },
  };
/** Focused parity fixtures: only the parity suite tree runs. */
const PARITY_FIXTURE_CONFIGS: Readonly<Record<ParityFixture, { only: string }>> = {
  "vue-parity": { only: "parity/" },
  "svelte-parity": { only: "parity/" },
  "mixed-parity": { only: "parity/" },
  "multi-root-parity": { only: "parity/" },
  "ecosystem-parity": { only: "parity/" },
};

function requiredParityRun(
  fixture: string,
  onlyPattern?: string,
): { readonly testIds: readonly string[]; readonly loadedFiles: readonly string[] } | undefined {
  if (!(fixture in PARITY_FIXTURE_CONFIGS)) return undefined;
  const manifestPath = path.resolve(__dirname, "../e2e-suite-build-manifest.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8")) as {
    version?: number;
    parity?: Partial<ParityTestInventory>;
  };
  const parity = manifest.parity;
  if (
    manifest.version !== 4 ||
    !parity ||
    !Number.isSafeInteger(parity.literalRegistrationCount) ||
    (parity.literalRegistrationCount ?? 0) <= 0 ||
    !Number.isSafeInteger(parity.matrixCaseCount) ||
    (parity.matrixCaseCount ?? 0) <= 0
  ) {
    throw new Error(
      `E2E parity manifest has an unsupported or incomplete inventory: ${manifestPath}`,
    );
  }
  const value = parity.testIdsByFixture?.[fixture as ParityFixture];
  const loadedFiles = parity.suiteFilesByFixture?.[fixture as ParityFixture];
  const bySuiteFile = parity.testIdsBySuiteFileByFixture?.[fixture as ParityFixture];
  if (!Array.isArray(value) || value.length === 0 || !value.every((id) => typeof id === "string")) {
    throw new Error(`E2E parity manifest has no stable test-ID inventory for ${fixture}`);
  }
  if (
    !Array.isArray(loadedFiles) ||
    loadedFiles.length === 0 ||
    !loadedFiles.every((file) => typeof file === "string")
  ) {
    throw new Error(`E2E parity manifest has no stable suite-file inventory for ${fixture}`);
  }
  if (!bySuiteFile || typeof bySuiteFile !== "object" || Array.isArray(bySuiteFile)) {
    throw new Error(`E2E parity manifest has no suite-to-test-ID inventory for ${fixture}`);
  }
  if (new Set(value).size !== value.length) {
    throw new Error(`E2E parity manifest contains duplicate test IDs for ${fixture}`);
  }
  if (new Set(loadedFiles).size !== loadedFiles.length) {
    throw new Error(`E2E parity manifest contains duplicate suite files for ${fixture}`);
  }
  return selectParityTestInventory(
    parity as ParityTestInventory,
    fixture as ParityFixture,
    onlyPattern,
  );
}

/**
 * Select a non-empty subset of the canonical route inventory. A fixture-only
 * selector expands to every applicable provider instead of inventing an auto route.
 */
function selectRoutes(options: {
  readonly fixtureArg?: string;
  readonly envFixture?: string;
  readonly envTypeProvider?: string;
}): E2eRoute[] {
  if (options.fixtureArg?.includes("@")) return [parseE2eRouteLabel(options.fixtureArg)];
  return selectE2eRoutes({
    fixture: options.fixtureArg ?? options.envFixture,
    typeProvider: options.envTypeProvider,
  });
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
  const onlyArg = process.argv.find((a) => a.startsWith("--only="));
  const onlyPattern = onlyArg?.slice("--only=".length) || readE2eEnv("ONLY");
  const envFixture = readE2eEnv("FIXTURE");
  const envTypeProvider = readE2eEnv("TYPE_PROVIDER");
  const routesToRun = selectRoutes({
    fixtureArg: fixtureArg?.replace("--fixture=", ""),
    envFixture,
    envTypeProvider,
  });

  const vscodeExecutablePath = await resolveVscodeExecutablePath(vscodeVersion, {
    explicitExecutablePath: readE2eEnv("VSCODE_EXECUTABLE"),
  });
  const requiresRcTsgo = routesToRun.some(
    ({ typeProvider }) => typeProvider === "tsgo" || typeProvider === "shared-tsgo",
  );
  const rcTsgoBinaryPath = requiresRcTsgo
    ? findWorkspaceRcTsgoBinary(extensionDevelopmentPath)
    : undefined;
  if (requiresRcTsgo && !rcTsgoBinaryPath) {
    throw new Error(
      "E2E requested tsgo but no @typescript/typescript-<platform> RC binary was found; " +
        "install the pinned workspace dependency or set VERTER_TSGO_BIN",
    );
  }

  // Copy LSP binary to temp to prevent file locking
  const lspBinaryPath = copyLspBinaryToTemp(extensionDevelopmentPath);

  let totalFailures = 0;

  for (const [index, route] of routesToRun.entries()) {
    const { fixture, typeProvider } = route;
    const label = e2eRouteLabel(route);
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
    // Multi-root: each folder is its own package with node_modules.
    if (fixture === "multi-root-parity") {
      for (const pkg of ["pkg-a", "pkg-b"]) {
        installFixtureDeps(path.join(fixtureDir, pkg));
      }
    }

    const logFile = path.join(os.tmpdir(), `verter-e2e-${label}.log`);
    const profile = createE2eProfile(label, index);
    // Delete any stale run summary before the run so a prior-run summary can
    // never false-green a current zero-exit crash that writes no fresh summary.
    clearRunArtifacts(logFile);
    try {
      writeVsCodeUserSettings(profile.userDataDir, {
        "verter.experimental.exposeBindingsTesting":
          fixture === "vue-parity" || fixture === "svelte-parity",
      });
      if (typeProvider === "shared-tsgo") {
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

      const workspaceLaunchPath =
        fixture === "multi-root-parity"
          ? path.join(fixtureDir, "multi-root-parity.code-workspace")
          : fixtureDir;
      const launchArgs = [
        workspaceLaunchPath,
        "--disable-updates",
        "--disable-workspace-trust",
        "--skip-welcome",
        "--skip-release-notes",
        `--extensions-dir=${profile.extensionsDir}`,
        `--user-data-dir=${profile.userDataDir}`,
      ];
      if (typeProvider !== "shared-tsgo") {
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
          VERTER_E2E_PROVIDER_ONLY_COMPLETIONS: "1",
          VERTER_E2E_LOG_FILE: logFile,
          VERTER_E2E_FIXTURE: fixture,
          VERTER_E2E_TIMING_FILE: path.join(os.tmpdir(), `verter-e2e-timing-${label}.json`),
          VERTER_LOG: "debug",
          ...(lspBinaryPath ? { VERTER_E2E_LSP_PATH: lspBinaryPath } : {}),
          ...(typeProvider ? { VERTER_E2E_TYPE_PROVIDER: typeProvider } : {}),
          ...(rcTsgoBinaryPath && (typeProvider === "tsgo" || typeProvider === "shared-tsgo")
            ? { VERTER_TSGO_BIN: rcTsgoBinaryPath }
            : {}),
          ...(fixture === EDITOR_ACCEPTANCE_FIXTURE
            ? { VERTER_E2E_ONLY: "editor-owned-project.test" }
            : CONTRACT_FIXTURES[fixture]
              ? { VERTER_E2E_ONLY: CONTRACT_FIXTURES[fixture].only }
              : fixture in PARITY_FIXTURE_CONFIGS
                ? {
                    VERTER_E2E_ONLY:
                      onlyPattern ?? PARITY_FIXTURE_CONFIGS[fixture as ParityFixture].only,
                  }
                : onlyPattern
                  ? { VERTER_E2E_ONLY: onlyPattern }
                  : {}),
        },
      });
      // The @vscode/test-electron process exit code is an UNRELIABLE pass/fail signal
      // on some hosts (Windows: VS Code can exit 0 even when the extension test run
      // rejected). The authoritative oracle is the run summary the mocha runner writes
      // (`suite/index.ts` → `<logFile>.runsummary`): fail on any reported test failure,
      // and on a vacuous 0-test execution or a MISSING summary. Every matrix entry is a
      // required gate; no ordinary fixture is allowed a legacy zero-execution pass.
      const contract = CONTRACT_FIXTURES[fixture];
      const parity = requiredParityRun(fixture, onlyPattern);
      const requiredLoadedFiles = contract
        ? [`frameworks/${contract.framework}/contract.test.js`]
        : parity?.loadedFiles;
      await enforceRunSummary(logFile, label, {
        expectedFixture: fixture,
        expectedTypeProvider: typeProvider,
        requiredLoadedFiles,
        requiredTestIds: contract
          ? requiredFrameworkContractIds(contract.framework)
          : parity?.testIds,
      });
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
