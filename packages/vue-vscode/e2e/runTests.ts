import * as path from "path";
import * as fs from "fs";
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
  knownFrameworkContractGapsForRoute,
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
import { installFixtureDeps } from "./lib/fixtureDeps";
import {
  E2E_BASE_SERVER_PROFILE_ENV,
  E2E_SERVER_PROFILE_ENV,
  E2E_SERVER_PROFILE_SLUGS,
  serverProfileKeys,
  serverProfileSettings,
  serverProfilesForSuites,
  type E2eServerProfile,
} from "./lib/serverProfiles";
import { FIXTURE_SUITE_GLOBS } from "./lib/fixtureSuiteMap";
import {
  isProjectlessContractFixture,
  PROJECTLESS_CONTRACT_LOADED_FILES,
  PROJECTLESS_CONTRACT_SUITE_GLOB,
  PROJECTLESS_CONTRACT_TEST_IDS,
} from "./lib/projectlessContractManifest";
import { knownProductGapsForRoute } from "./lib/knownProductGapManifest";
import {
  BARREL_REGRESSION_LOADED_FILES,
  BARREL_REGRESSION_SUITE_GLOB,
  BARREL_REGRESSION_TEST_IDS,
  isBarrelRegressionFixture,
} from "./lib/barrelRegressionManifest";

const EDITOR_ACCEPTANCE_FIXTURE = "editor-owned-project";
const EXTENSION_ACCEPTANCE_FIXTURE = "out-of-tree-monorepo";
/**
 * Fixtures whose workspace must be launched from OUTSIDE this repository.
 *
 * The extension-hosted provider resolves each project's TypeScript with
 * `createRequire` anchored at the project root the LSP DECLARES, and Node walks
 * ancestors. Under `e2e/fixtures/*` every ancestor chain ends in the repo's own
 * `node_modules/typescript`, so a wrongly-declared root (the workspace folder
 * instead of the owning package) still resolves a compiler and the fixture
 * passes against the defect it exists to catch. Materialized into an OS temp
 * directory the chain ends at the filesystem root with no TypeScript, so only
 * the correctly-declared nested package can serve.
 */
const OUT_OF_TREE_FIXTURES: ReadonlySet<string> = new Set([EXTENSION_ACCEPTANCE_FIXTURE]);
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
 * Copy an out-of-tree fixture template into a fresh OS temp workspace.
 *
 * `node_modules` is never copied: the whole point of the workspace is which
 * package has an install and which does not.
 */
function materializeOutOfTreeWorkspace(fixture: string, templateDir: string): string {
  const root = fs.mkdtempSync(path.join(HARNESS_TEMP_ROOT, `verter-e2e-ws-${fixture}-`));
  fs.cpSync(templateDir, root, {
    recursive: true,
    filter: (src) => path.basename(src) !== "node_modules",
  });
  console.log(`  Materialized out-of-tree workspace: ${root}`);
  return root;
}

function removeOutOfTreeWorkspace(root: string): void {
  assertRemovablePath(root, "verter-e2e-ws-", "workspace");
  fs.rmSync(path.resolve(root), { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
}

/**
 * The shortest REAL (symlink-free) system temp root available on this platform.
 *
 * Two independent constraints make this a real decision rather than `os.tmpdir()`:
 *
 * 1. SYMLINK. The Unix control-socket bind rejects a `--control-dir` grandparent that
 *    is ITSELF a symlink (`crates/verter_tsgo_api/src/control/transport.rs` →
 *    `prepare_unix_socket_parent`). On macOS `/tmp` IS a symlink to `/private/tmp`, so a
 *    `TMPDIR=/tmp` run silently loses the shared-tsgo rail and degrades to managed-tsgo —
 *    a route change that reads as a product failure. Resolving through `realpathSync`
 *    makes the harness independent of how the operator spelled `TMPDIR`.
 * 2. LENGTH. The control socket falls back to `<temp>/vr-ctl-<16 hex>/ctl.sock`, i.e.
 *    the temp root plus 34 bytes, against a 100-byte `sockaddr_un` budget. macOS's
 *    per-user `os.tmpdir()` (`/var/folders/<a>/<b>/T`) realpaths to ~55 bytes, leaving
 *    under 11 bytes for the per-run segment — so a per-run directory rooted there is at
 *    the edge of unbindable. `/private/tmp` is 12 bytes and leaves the budget untouched.
 */
function computeRealSystemTempRoot(): string {
  const candidates = process.platform === "darwin" ? ["/private/tmp"] : [];
  for (const candidate of candidates) {
    try {
      if (fs.statSync(candidate).isDirectory() && fs.realpathSync(candidate) === candidate) {
        return candidate;
      }
    } catch {
      /* fall through to the platform default */
    }
  }
  return fs.realpathSync(os.tmpdir());
}

/**
 * The one temp root every harness-owned directory hangs off — profiles, logs, timing
 * reports, out-of-tree workspaces, and the per-route isolated roots.
 *
 * It is NOT `os.tmpdir()`. On macOS the default per-user temp dir is long enough that VS
 * Code's OWN control socket under an E2E profile
 * (`<profile>/user-data/<version>-main.sock`) blows the 103-byte `sockaddr_un` limit and
 * the editor refuses to start with `listen EINVAL`. Runs only ever succeeded there because
 * the operator happened to export a short `TMPDIR`; an environment-dependent harness is
 * exactly the nondeterminism this lane cannot afford, so the root is derived, not inherited.
 */
const HARNESS_TEMP_ROOT = computeRealSystemTempRoot();

/** Guard a harness-owned path before recursive removal. */
function assertRemovablePath(target: string, prefix: string, kind: string): void {
  const resolved = path.resolve(target);
  if (
    !resolved.startsWith(`${path.resolve(HARNESS_TEMP_ROOT)}${path.sep}`) ||
    !path.basename(resolved).startsWith(prefix)
  ) {
    throw new Error(`Refusing to remove unexpected E2E ${kind} path: ${resolved}`);
  }
}

/**
 * Create the per-route TEMP root handed to the extension host.
 *
 * The LSP derives its on-disk carrier store from `std::env::temp_dir()`:
 * `<temp>/verter-carrier-store/<lsp-package-version>/<blake3(workspace root)>/`
 * (`crates/verter_lsp/src/external_ts/carrier_publish_store.rs`). Both key components are
 * STABLE across runs, so with a shared temp root every run inherits the previous run's
 * blobs and manifest — runs are not independent, and the same tree yields different
 * verdicts depending on what ran before it. Handing each route a fresh temp root makes the
 * store (and every other temp-derived artifact: shim dirs, control dirs, sockets) belong to
 * exactly one route of one run.
 *
 * This makes runs INDEPENDENT. It does NOT make the warm store correct: a real user's
 * second editor session still opens a populated store, and that path currently loses
 * template occurrences from rename/references (see `docs`-side report). Do not read a green
 * isolated run as evidence that the warm path works.
 */
function createIsolatedTempRoot(label: string, index: number): string {
  const safeLabel = label.replace(/[^a-zA-Z0-9]/g, "").slice(0, 8);
  const root = fs.mkdtempSync(path.join(HARNESS_TEMP_ROOT, `vt${index}${safeLabel}-`));
  // Owner-only, matching `mkdtemp`'s own 0700 — the control-socket grandparent ceiling
  // accepts a directory we own with no group/other write bits.
  fs.chmodSync(root, 0o700);
  return root;
}

function removeIsolatedTempRoot(root: string): void {
  assertRemovablePath(root, "vt", "temp root");
  fs.rmSync(path.resolve(root), { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
}

interface E2eProfile {
  root: string;
  extensionsDir: string;
  userDataDir: string;
}

function createE2eProfile(label: string, index: number): E2eProfile {
  const safeLabel = label.replace(/[^a-zA-Z0-9_-]/g, "-");
  const root = path.join(
    HARNESS_TEMP_ROOT,
    `verter-e2e-profile-${process.pid}-${index}-${safeLabel}`,
  );
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
  assertRemovablePath(profile.root, "verter-e2e-profile-", "profile");
  // Electron reports the extension-host exit before Windows has necessarily
  // released every log-file handle. Let Node's recursive remover retry EBUSY /
  // EPERM instead of turning a fully green product run into a harness failure.
  fs.rmSync(path.resolve(profile.root), {
    recursive: true,
    force: true,
    maxRetries: 20,
    retryDelay: 100,
  });
}

/**
 * Remove every server-profile key from a fixture's generated workspace settings.
 *
 * `<fixture>/.vscode/settings.json` is written at runtime by suites that flip
 * VS Code settings and is gitignored, so it outlives the run that produced it.
 * Workspace scope beats the user scope the launcher writes, so a leftover
 * profile key there would decide the server's configuration instead of the
 * declared profile.
 */
function clearProfileKeysFromWorkspaceSettings(fixtureDir: string): void {
  const settingsPath = path.join(fixtureDir, ".vscode", "settings.json");
  if (!fs.existsSync(settingsPath)) return;
  let parsed: Record<string, unknown>;
  try {
    parsed = JSON.parse(fs.readFileSync(settingsPath, "utf8")) as Record<string, unknown>;
  } catch {
    // Unparseable generated file: replacing it is strictly better than leaving
    // VS Code to interpret it.
    fs.rmSync(settingsPath, { force: true });
    return;
  }
  let changed = false;
  for (const key of serverProfileKeys()) {
    if (key in parsed) {
      delete parsed[key];
      changed = true;
    }
  }
  if (changed) {
    fs.writeFileSync(settingsPath, `${JSON.stringify(parsed, null, 2)}\n`);
  }
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
    const routeLabel = e2eRouteLabel(route);
    // The product keeps Verter-native semantic enrichment and hover contribution
    // off by default. Parity fixtures exercise those opt-in surfaces wholesale, so
    // their BASELINE is that profile; every other fixture baselines on the shipped
    // default. Individual suites may declare a different profile
    // (`lib/serverProfiles.ts`), and each distinct profile gets its OWN launch: a
    // restart can adopt current settings, but a separate launch keeps one suite's
    // configuration changes from becoming another suite's baseline.
    // The set is derived from this fixture's suite GLOBS, not from the authored
    // suite files on disk.
    const baseServerProfile: E2eServerProfile =
      fixture in PARITY_FIXTURE_CONFIGS ? "verter-native-semantics" : "default";
    // A `--only` selection narrows which suites load, so it narrows which
    // profiles have anything to run. Launching a profile whose selection is empty
    // would fail the suite runner's zero-selection guard — correctly, since a
    // launch that runs no tests proves nothing.
    const selectableSuiteGlobs = (FIXTURE_SUITE_GLOBS[fixture] ?? []).filter(
      (glob) => !onlyPattern || glob.includes(onlyPattern) || onlyPattern.includes(glob),
    );
    const serverProfilesInUse = serverProfilesForSuites(
      selectableSuiteGlobs.length > 0 ? selectableSuiteGlobs : (FIXTURE_SUITE_GLOBS[fixture] ?? []),
      baseServerProfile,
    );
    const templateDir = path.join(extensionDevelopmentPath, "e2e", "fixtures", fixture);
    const outOfTreeWorkspace = OUT_OF_TREE_FIXTURES.has(fixture)
      ? materializeOutOfTreeWorkspace(fixture, templateDir)
      : undefined;
    const fixtureDir = outOfTreeWorkspace ?? templateDir;

    console.log(`\n${"=".repeat(60)}`);
    console.log(`Running E2E tests for fixture: ${routeLabel}`);
    console.log(`Workspace: ${fixtureDir}`);
    if (typeProvider) console.log(`Type provider override: ${typeProvider}`);
    if (serverProfilesInUse.length > 1) {
      console.log(`Server profiles: ${serverProfilesInUse.join(", ")} (one launch each)`);
    }
    console.log("=".repeat(60));

    // Install fixture dependencies if needed (for Vue type resolution).
    // An out-of-tree workspace installs ONLY inside its packages: its root must
    // stay TypeScript-less, which is the condition under test.
    if (outOfTreeWorkspace) {
      const packagesDir = path.join(fixtureDir, "packages");
      if (fs.existsSync(packagesDir)) {
        for (const pkg of fs.readdirSync(packagesDir)) {
          installFixtureDeps(path.join(packagesDir, pkg));
        }
      }
    } else {
      installFixtureDeps(fixtureDir);
    }
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

    // ONE LAUNCH PER PROFILE. Each gets its own log, VS Code profile directory,
    // isolated temp root and run summary, so a profile's verdict never depends on
    // another's. The label is suffixed only when a route actually runs more than
    // one, keeping every single-profile route's artifact names unchanged.
    for (const [profileIndex, serverProfile] of serverProfilesInUse.entries()) {
      // The slug, not the profile name: this label becomes a VS Code user-data
      // directory whose path becomes a Unix socket path, and the full name blows
      // the ~103-byte limit (`listen EINVAL`, VS Code never starts).
      const label =
        serverProfilesInUse.length > 1
          ? `${routeLabel}#${E2E_SERVER_PROFILE_SLUGS[serverProfile]}`
          : routeLabel;
      const launchIndex = index * serverProfilesInUse.length + profileIndex;
      // Deliberately OUTSIDE the per-route isolated root: the runner reads this log and the
      // run summary beside it AFTER the route's root is removed.
      const logFile = path.join(HARNESS_TEMP_ROOT, `verter-e2e-${label}.log`);
      const profile = createE2eProfile(label, launchIndex);
      // Isolate every temp-derived LSP artifact — above all the carrier store — to THIS
      // route of THIS run, so a run's verdict never depends on what ran before it.
      const tempRoot = createIsolatedTempRoot(label, launchIndex);
      console.log(`  Isolated temp root: ${tempRoot}`);
      // Delete any stale run summary before the run so a prior-run summary can
      // never false-green a current zero-exit crash that writes no fresh summary.
      clearRunArtifacts(logFile);
      // The runner's profile is USER-scope, and several suites write WORKSPACE-scope
      // settings into `<fixture>/.vscode/settings.json` at runtime (the decorations
      // suite, `revealDefinition`). That file is gitignored, so it survives between
      // runs, and workspace scope OUTRANKS user scope — a stale profile key left
      // there silently defeats this launch's profile and the server comes up
      // configured as something nobody declared. Strip exactly the keys the profile
      // owns; everything else in that generated file is the suites' own business.
      clearProfileKeysFromWorkspaceSettings(fixtureDir);
      try {
        // THE single authority: the same table the suite selection resolves each
        // suite's profile from also writes this launch's settings, so a suite's
        // declared profile and the server it actually talks to cannot drift.
        writeVsCodeUserSettings(profile.userDataDir, {
          "verter.experimental.exposeBindingsTesting":
            fixture === "vue-parity" || fixture === "svelte-parity",
          ...serverProfileSettings(serverProfile),
          // The extension spawns the LSP with `VERTER_LOG` set from THIS setting, which
          // OVERWRITES the `VERTER_LOG=debug` the runner puts in `extensionTestsEnv`
          // (`extension.ts` → `buildServerOptions`). Without it the server runs at `info`
          // and the fail-closed signal a test needs to distinguish "the provider gave a
          // wrong answer" from "Verter refused to answer" — the workspace-symbol frontier's
          // `activated N/M carriers` line — is never emitted at all.
          //
          // The value is a `tracing_subscriber` EnvFilter directive, not a bare level:
          // blanket `debug` is 6x the log volume, and every line is mirrored SYNCHRONOUSLY
          // to both the output channel and the E2E log file, which measurably slows the
          // extension host. Raise exactly the one module that owns the signal.
          "verter.server.logLevel": "info,verter_lsp::server::provider_state=debug",
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
        let extensionHostError: unknown;
        try {
          await runTests({
            vscodeExecutablePath,
            extensionDevelopmentPath,
            extensionTestsPath,
            launchArgs,
            extensionTestsEnv: {
              ...process.env,
              // Every temp-derived artifact the extension host and the LSP it spawns create —
              // the carrier store above all — lands under this route's own root. `TMPDIR` is
              // what Node's `os.tmpdir()` and Rust's `std::env::temp_dir()` read on Unix;
              // `TMP`/`TEMP` are the Windows equivalents, so all three are set.
              TMPDIR: tempRoot,
              TMP: tempRoot,
              TEMP: tempRoot,
              VERTER_E2E_TEST: "1",
              VERTER_E2E_PROVIDER_ONLY_COMPLETIONS: "1",
              VERTER_E2E_LOG_FILE: logFile,
              VERTER_E2E_FIXTURE: fixture,
              // What the server was STARTED with. A suite must not infer this from the
              // live settings — a previous suite may have flipped them — and the
              // default-configuration pin refuses to run on anything but `default`.
              [`VERTER_E2E_${E2E_SERVER_PROFILE_ENV}`]: serverProfile,
              [`VERTER_E2E_${E2E_BASE_SERVER_PROFILE_ENV}`]: baseServerProfile,
              VERTER_E2E_TIMING_FILE: path.join(
                HARNESS_TEMP_ROOT,
                `verter-e2e-timing-${label}.json`,
              ),
              VERTER_LOG: "debug",
              ...(lspBinaryPath ? { VERTER_E2E_LSP_PATH: lspBinaryPath } : {}),
              ...(typeProvider ? { VERTER_E2E_TYPE_PROVIDER: typeProvider } : {}),
              ...(rcTsgoBinaryPath && (typeProvider === "tsgo" || typeProvider === "shared-tsgo")
                ? { VERTER_TSGO_BIN: rcTsgoBinaryPath }
                : {}),
              ...(fixture === EDITOR_ACCEPTANCE_FIXTURE
                ? { VERTER_E2E_ONLY: "editor-owned-project.test" }
                : fixture === EXTENSION_ACCEPTANCE_FIXTURE
                  ? { VERTER_E2E_ONLY: "out-of-tree-monorepo.test" }
                  : isProjectlessContractFixture(fixture)
                    ? { VERTER_E2E_ONLY: PROJECTLESS_CONTRACT_SUITE_GLOB }
                    : isBarrelRegressionFixture(fixture)
                      ? { VERTER_E2E_ONLY: BARREL_REGRESSION_SUITE_GLOB }
                      : CONTRACT_FIXTURES[fixture]
                        ? { VERTER_E2E_ONLY: CONTRACT_FIXTURES[fixture].only }
                        : fixture in PARITY_FIXTURE_CONFIGS
                          ? {
                              VERTER_E2E_ONLY:
                                onlyPattern ??
                                PARITY_FIXTURE_CONFIGS[fixture as ParityFixture].only,
                            }
                          : onlyPattern
                            ? { VERTER_E2E_ONLY: onlyPattern }
                            : {}),
            },
          });
        } catch (error) {
          // VS Code exits non-zero when Mocha reports a known product gap. The
          // structured run summary below is the verdict authority: it rejects
          // unexpected failures, missing/duplicate required tests, and missing
          // summaries. Retain the launcher error for diagnostics, but do not let
          // its coarse exit code bypass the route-specific product-gap manifest.
          extensionHostError = error;
        }
        // The @vscode/test-electron process exit code is an UNRELIABLE pass/fail signal
        // on some hosts (Windows: VS Code can exit 0 even when the extension test run
        // rejected). The authoritative oracle is the run summary the mocha runner writes
        // (`suite/index.ts` → `<logFile>.runsummary`): fail on any reported test failure,
        // and on a vacuous 0-test execution or a MISSING summary. Every matrix entry is a
        // required gate; no ordinary fixture is allowed a legacy zero-execution pass.
        const contract = CONTRACT_FIXTURES[fixture];
        const parity = requiredParityRun(fixture, onlyPattern);
        const projectlessContract = isProjectlessContractFixture(fixture);
        const barrelRegression = isBarrelRegressionFixture(fixture);
        const requiredLoadedFiles = contract
          ? [`frameworks/${contract.framework}/contract.test.js`]
          : projectlessContract
            ? PROJECTLESS_CONTRACT_LOADED_FILES
            : barrelRegression
              ? BARREL_REGRESSION_LOADED_FILES
              : parity?.loadedFiles;
        try {
          await enforceRunSummary(logFile, label, {
            expectedFixture: fixture,
            expectedTypeProvider: typeProvider,
            requiredLoadedFiles,
            requiredTestIds: contract
              ? requiredFrameworkContractIds(contract.framework)
              : projectlessContract
                ? PROJECTLESS_CONTRACT_TEST_IDS
                : barrelRegression
                  ? BARREL_REGRESSION_TEST_IDS
                  : parity?.testIds,
            allowedProductGaps: parity
              ? knownProductGapsForRoute(fixture, typeProvider, parity.testIds)
              : contract
                ? knownFrameworkContractGapsForRoute(contract.framework, typeProvider)
                : undefined,
          });
        } catch (summaryError) {
          if (extensionHostError) {
            console.error(
              `  Extension-host process also exited non-zero for ${label}:`,
              extensionHostError,
            );
          }
          throw summaryError;
        }
        if (extensionHostError) {
          console.warn(
            `  Extension-host process exited non-zero for ${label}; the complete run summary ` +
              "contains only statically allowed product gaps.",
          );
        }
        console.log(`  PASSED: ${label}`);
      } catch (err) {
        console.error(`  FAILED: ${label}`, err);
        totalFailures++;
      } finally {
        if (readE2eEnv("KEEP_PROFILE") === "1") {
          console.log(`  Preserved E2E profile: ${profile.root}`);
          console.log(`  Preserved E2E temp root: ${tempRoot}`);
          if (outOfTreeWorkspace) console.log(`  Preserved E2E workspace: ${outOfTreeWorkspace}`);
        } else {
          removeE2eProfile(profile);
          removeIsolatedTempRoot(tempRoot);
          if (outOfTreeWorkspace) removeOutOfTreeWorkspace(outOfTreeWorkspace);
        }
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
