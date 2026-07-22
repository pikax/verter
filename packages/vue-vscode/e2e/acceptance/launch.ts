import * as fs from "fs";
import * as os from "os";
import * as path from "path";

import { runTests } from "@vscode/test-electron";

import {
  copyLspBinaryToTemp,
  findWorkspaceRcTsgoBinary,
  readE2eEnv,
  resolveVscodeExecutablePath,
  writeVsCodeUserSettings,
} from "../sharedLaunch";

/**
 * Launcher for the VS Code acceptance lane.
 *
 * The fixture runner (`e2e/runTests.ts`) can only open workspaces that live
 * inside `e2e/fixtures`. The acceptance lane's whole point is to measure a REAL
 * project, so it takes the workspace path from the environment and never
 * embeds one — the projects it is pointed at are private, and no path, project
 * name, or file name may reach the repository.
 *
 * Everything else is deliberately shared with the fixture runner through
 * `sharedLaunch.ts`: the same LSP binary discovery, the same
 * copy-off-the-build-output step, the same VS Code resolution and the same
 * Windows CLI handling. A second copy of that plumbing would drift.
 *
 * Environment:
 *
 * - `VERTER_ACCEPTANCE_WORKSPACE`  (required) absolute path to the project.
 * - `VERTER_ACCEPTANCE_LABEL`      a short opaque label for the receipt.
 * - `VERTER_ACCEPTANCE_RECEIPT`    where to write the JSON receipt.
 * - `VERTER_ACCEPTANCE_PROVIDER`   `auto` | `tsgo` | `tsserver` | `shared-tsgo` | `off`.
 *                                  `off` is the lane's NEGATIVE CONTROL: with no
 *                                  engine, every TypeScript answer count must be
 *                                  zero, which is what proves the discriminator
 *                                  rejects Verter-native results in the live
 *                                  system rather than only in unit tests.
 */
async function main(): Promise<void> {
  // `__dirname` is `out-test/e2e/acceptance`, so the package root — which is the
  // extension development path — is three levels up, not two. Getting this wrong
  // makes VS Code treat `out-test/e2e` as the extension and the host then reports
  // that the extension is not installed.
  const extensionDevelopmentPath = path.resolve(__dirname, "../../../");
  const extensionTestsPath = path.resolve(__dirname, "./suite/index");

  const workspace = process.env.VERTER_ACCEPTANCE_WORKSPACE;
  if (!workspace) {
    throw new Error(
      "VERTER_ACCEPTANCE_WORKSPACE is required — the acceptance lane measures a real project " +
        "supplied at the shell and never embeds a workspace path",
    );
  }
  if (!fs.existsSync(workspace)) {
    throw new Error("VERTER_ACCEPTANCE_WORKSPACE does not exist");
  }

  const label = process.env.VERTER_ACCEPTANCE_LABEL ?? "unlabelled";
  const provider = process.env.VERTER_ACCEPTANCE_PROVIDER ?? "auto";
  const keepExtensions = process.env.VERTER_ACCEPTANCE_KEEP_EXTENSIONS === "1";
  const receipt =
    process.env.VERTER_ACCEPTANCE_RECEIPT ??
    path.join(os.tmpdir(), `verter-acceptance-${label}-${provider}.json`);

  const vscodeExecutablePath = await resolveVscodeExecutablePath(
    readE2eEnv("VSCODE_VERSION") ?? "stable",
    { explicitExecutablePath: readE2eEnv("VSCODE_EXECUTABLE") },
  );
  const lspBinaryPath = copyLspBinaryToTemp(extensionDevelopmentPath);
  if (!lspBinaryPath) {
    throw new Error("no verter-lsp binary was found — run `pnpm run prepare:e2e` first");
  }
  const rcTsgoBinaryPath =
    provider === "tsgo" || provider === "shared-tsgo"
      ? findWorkspaceRcTsgoBinary(extensionDevelopmentPath)
      : undefined;

  const profileRoot = path.join(
    os.tmpdir(),
    `verter-acceptance-profile-${process.pid}-${label}-${provider}`,
  );
  const extensionsDir = path.join(profileRoot, "extensions");
  const userDataDir = path.join(profileRoot, "user-data");
  fs.mkdirSync(extensionsDir, { recursive: true });
  fs.mkdirSync(userDataDir, { recursive: true });
  writeVsCodeUserSettings(userDataDir, {
    // The lane measures the LSP path, not the analysis sidebar or MCP endpoint.
    "verter.analysis.enabled": false,
    "verter.mcp.enabled": false,
  });

  const logFile = path.join(os.tmpdir(), `verter-acceptance-${label}-${provider}.log`);
  for (const stale of [logFile, `${logFile}.runsummary`, receipt]) {
    fs.rmSync(stale, { force: true });
  }

  console.log(
    `acceptance lane: corpus=${label} provider=${provider} keepExtensions=${keepExtensions}`,
  );
  console.log(`  receipt: ${receipt}`);
  console.log(`  log:     ${logFile}`);

  let failed = false;
  try {
    await runTests({
      vscodeExecutablePath,
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs: [
        workspace,
        "--disable-updates",
        "--disable-workspace-trust",
        "--skip-welcome",
        "--skip-release-notes",
        // `--disable-extensions` keeps the host clean, but it is not what a user
        // runs. On the editor-owned tsserver route Verter defers carrier source
        // features to its TypeScript plugin inside the editor's own tsserver,
        // and that hand-off depends on the built-in TypeScript extension
        // registering providers for the `vue` language from the plugin
        // contribution. `VERTER_ACCEPTANCE_KEEP_EXTENSIONS=1` reproduces the
        // real editor so that hand-off can be measured rather than assumed.
        ...(keepExtensions ? [] : ["--disable-extensions"]),
        `--extensions-dir=${extensionsDir}`,
        `--user-data-dir=${userDataDir}`,
      ],
      extensionTestsEnv: {
        ...process.env,
        VERTER_E2E_TEST: "1",
        VERTER_E2E_LOG_FILE: logFile,
        VERTER_E2E_LSP_PATH: lspBinaryPath,
        VERTER_E2E_TYPE_PROVIDER: provider,
        VERTER_E2E_ACCEPTANCE_RECEIPT: receipt,
        VERTER_E2E_ACCEPTANCE_LABEL: label,
        // `off` is the negative control, so the lane must PROVE it saw and
        // rejected Verter-native answers rather than merely seeing nothing.
        ...(provider === "off" ? { VERTER_E2E_ACCEPTANCE_EXPECT_NATIVE: "1" } : {}),
        VERTER_LOG: "debug",
        ...(rcTsgoBinaryPath ? { VERTER_TSGO_BIN: rcTsgoBinaryPath } : {}),
      },
    });
  } catch (err) {
    failed = true;
    console.error("acceptance lane reported failures:", err instanceof Error ? err.message : err);
  }

  // The receipt is the deliverable even on failure — a run that proves the
  // editor shows nothing is a RESULT, not a lost run. Exit status still
  // reflects the assertions.
  const summaryPath = `${logFile}.runsummary`;
  if (!fs.existsSync(summaryPath)) {
    console.error("no run summary was written — the extension host executed nothing");
    process.exit(1);
  }
  if (fs.existsSync(receipt)) {
    console.log(fs.readFileSync(receipt, "utf8"));
  } else {
    console.error("no receipt was written");
    failed = true;
  }
  process.exit(failed ? 1 : 0);
}

main().catch((err) => {
  console.error("acceptance launcher failed:", err);
  process.exit(1);
});
