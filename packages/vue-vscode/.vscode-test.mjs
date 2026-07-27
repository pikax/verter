import { defineConfig } from "@vscode/test-cli";
import path from "path";
import fs from "fs";
import { fileURLToPath } from "url";
import os from "os";

// The SHARED preparation, not a second copy of it. This config used to carry
// its own `node_modules`-exists shortcut; when that shortcut was replaced in
// `runTests.ts` the copy here kept the old behaviour, and this launcher went on
// serving stale trees. It also validates the selector before joining it onto a
// path: `E2E_FIXTURE=../..` is this package, which has a `package.json` and a
// pnpm-managed `node_modules` the install decision would otherwise act on.
// `files` below already requires `out-test`, so importing the compiled module
// adds no prerequisite this config did not have.
const { installFixtureDeps, prepareFixtureWorkspace } =
  await import("./out-test/e2e/lib/fixtureDeps.js");

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const onlyTest = process.env.VERTER_E2E_ONLY || "";
const vscodeVersion = process.env.VERTER_E2E_VSCODE_VERSION || "stable";
const testFiles = onlyTest
  ? onlyTest.includes("*")
    ? onlyTest
    : `out-test/e2e/suite/**/${onlyTest}`
  : "out-test/e2e/suite/**/*.test.js";

/**
 * Find and copy the LSP binary to a temp directory to prevent file locking.
 * On Windows, a running .exe is locked by the OS.
 * Returns the path to the copied binary, or undefined if not found.
 */
function copyLspBinaryToTemp() {
  const ext = process.platform === "win32" ? ".exe" : "";
  const binaryName = `verter-lsp${ext}`;
  const candidates = [];

  // Search upward from extension dir for target/debug or target/release
  let dir = __dirname;
  for (let i = 0; i < 5; i++) {
    for (const profile of ["debug", "release"]) {
      const candidate = path.join(dir, "target", profile, binaryName);
      if (fs.existsSync(candidate)) {
        candidates.push(candidate);
      }
    }
    dir = path.dirname(dir);
  }

  // Also check dist/ and bin/
  const distPath = path.join(__dirname, "dist", binaryName);
  if (fs.existsSync(distPath)) {
    candidates.push(distPath);
  }
  const binPath = path.join(__dirname, "bin", binaryName);
  if (fs.existsSync(binPath)) {
    candidates.push(binPath);
  }

  const sourcePath = candidates
    .map((candidate) => ({
      candidate,
      mtimeMs: fs.statSync(candidate).mtimeMs,
    }))
    .sort((a, b) => b.mtimeMs - a.mtimeMs)
    .map((entry) => entry.candidate)[0];

  if (!sourcePath) return undefined;

  const tempDir = path.join(os.tmpdir(), "verter-e2e-bin");
  fs.mkdirSync(tempDir, { recursive: true });
  const destPath = path.join(tempDir, binaryName);
  fs.copyFileSync(sourcePath, destPath);

  if (process.platform !== "win32") {
    fs.chmodSync(destPath, 0o755);
  }

  console.log(`LSP binary copied: ${sourcePath} → ${destPath}`);
  return destPath;
}

const lspBinaryPath = copyLspBinaryToTemp();

// Validate the selector, derive the workspace from it, and make its
// dependencies current — all before VS Code is launched at it.
const {
  fixture,
  typeProvider,
  workspace: fixtureDir,
} = prepareFixtureWorkspace(path.join(__dirname, "e2e", "fixtures"), {
  rawFixture: process.env.E2E_FIXTURE,
  typeProvider: process.env.E2E_TYPE_PROVIDER,
});
// For monorepo, also install workspace package deps
if (fixture === "monorepo") {
  const packagesDir = path.join(fixtureDir, "packages");
  if (fs.existsSync(packagesDir)) {
    for (const pkg of fs.readdirSync(packagesDir)) {
      installFixtureDeps(path.join(packagesDir, pkg));
    }
  }
}

export default defineConfig({
  files: testFiles,
  version: vscodeVersion,
  extensionDevelopmentPath: __dirname,
  workspaceFolder: fixtureDir,
  launchArgs: ["--disable-extensions", "--disable-updates"],
  env: {
    ...process.env,
    VERTER_E2E_TEST: "1",
    VERTER_E2E_LOG_FILE: path.join(
      os.tmpdir(),
      `verter-e2e-${fixture}${typeProvider ? `-${typeProvider}` : ""}.log`,
    ),
    VERTER_E2E_FIXTURE: fixture,
    VERTER_E2E_TIMING_FILE: path.join(
      os.tmpdir(),
      `verter-e2e-timing-${fixture}${typeProvider ? `-${typeProvider}` : ""}.json`,
    ),
    VERTER_LOG: "debug",
    ...(lspBinaryPath ? { VERTER_E2E_LSP_PATH: lspBinaryPath } : {}),
    ...(typeProvider ? { VERTER_E2E_TYPE_PROVIDER: typeProvider } : {}),
  },
  mocha: {
    ui: "tdd",
    timeout: 90_000,
  },
});
