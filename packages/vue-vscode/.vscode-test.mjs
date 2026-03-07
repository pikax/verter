import { defineConfig } from "@vscode/test-cli";
import path from "path";
import fs from "fs";
import { fileURLToPath } from "url";
import os from "os";
import { execSync } from "child_process";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixture = process.env.E2E_FIXTURE || "single-project";
const typeProvider = process.env.E2E_TYPE_PROVIDER || "";

/**
 * Find and copy the LSP binary to a temp directory to prevent file locking.
 * On Windows, a running .exe is locked by the OS.
 * Returns the path to the copied binary, or undefined if not found.
 */
function copyLspBinaryToTemp() {
  const ext = process.platform === "win32" ? ".exe" : "";
  const binaryName = `verter-lsp${ext}`;

  // Search upward from extension dir for target/debug or target/release
  let dir = __dirname;
  let sourcePath;
  for (let i = 0; i < 5; i++) {
    for (const profile of ["debug", "release"]) {
      const candidate = path.join(dir, "target", profile, binaryName);
      if (fs.existsSync(candidate)) {
        sourcePath = candidate;
        break;
      }
    }
    if (sourcePath) break;
    dir = path.dirname(dir);
  }

  // Also check dist/ and bin/
  if (!sourcePath) {
    const distPath = path.join(__dirname, "dist", binaryName);
    if (fs.existsSync(distPath)) sourcePath = distPath;
  }
  if (!sourcePath) {
    const binPath = path.join(__dirname, "bin", binaryName);
    if (fs.existsSync(binPath)) sourcePath = binPath;
  }

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

/**
 * Install dependencies in a fixture directory if it has a package.json.
 * Skips if node_modules already exists.
 */
function installFixtureDeps(fixtureDir) {
  const pkgJson = path.join(fixtureDir, "package.json");
  const nodeModules = path.join(fixtureDir, "node_modules");

  if (!fs.existsSync(pkgJson) || fs.existsSync(nodeModules)) {
    return;
  }

  console.log(`Installing dependencies in ${fixtureDir}...`);
  try {
    execSync("npm install --no-package-lock --ignore-scripts", {
      cwd: fixtureDir,
      stdio: "pipe",
      timeout: 60_000,
    });
  } catch (err) {
    console.warn(`Warning: npm install failed in ${fixtureDir}:`, err.message);
  }
}

// Install fixture dependencies before launching VS Code
const fixtureDir = path.join(__dirname, "e2e", "fixtures", fixture);
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

export default defineConfig({
  files: "out-test/e2e/suite/**/*.test.js",
  version: "stable",
  extensionDevelopmentPath: __dirname,
  workspaceFolder: path.join(__dirname, "e2e", "fixtures", fixture),
  launchArgs: ["--disable-extensions"],
  env: {
    VERTER_E2E_TEST: "1",
    VERTER_E2E_LOG_FILE: path.join(os.tmpdir(), `verter-e2e-${fixture}${typeProvider ? `-${typeProvider}` : ""}.log`),
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
