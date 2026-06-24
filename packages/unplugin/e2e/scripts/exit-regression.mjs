import { spawn } from "node:child_process";
import { existsSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const e2eDir = path.resolve(__dirname, "..");
const unpluginDir = path.resolve(e2eDir, "..");

const localFixtureDir = path.resolve(e2eDir, "fixtures", "vite-lib-exit");
const localViteConfig = path.resolve(localFixtureDir, "vite.config.ts");
const localDistDir = path.resolve(localFixtureDir, "dist");
const localOutputFile = path.resolve(localDistDir, "index.mjs");
const localTimeoutMs = 20_000;
const directStyleRegressionScript = path.resolve(e2eDir, "scripts", "direct-style-exit-repro.mjs");
const directStyleTimeoutMs = 20_000;

// The optional external consumer-repro lives in a local checkout whose path comes
// purely from an env var; no private/local path lives in the tree. The check is
// gated on VERTER_RUN_EXTERNAL_LIB=1 and is a no-op when the root env var is unset.
const externalLibRoot = process.env.VERTER_EXTERNAL_LIB_ROOT;
const externalLibPackageDir = externalLibRoot
  ? path.resolve(externalLibRoot, "packages", "ui")
  : undefined;
const externalLibDistDir = externalLibPackageDir
  ? path.resolve(externalLibPackageDir, "dist")
  : undefined;
const externalLibTimeoutMs = 60_000;
const runExternalLibCheck = process.env.VERTER_RUN_EXTERNAL_LIB === "1";

const viteBin = path.resolve(unpluginDir, "node_modules", "vite", "bin", "vite.js");
const pnpmInvocation = resolvePnpmInvocation();

function resolvePnpmInvocation() {
  const npmExecPath = process.env.npm_execpath;
  if (npmExecPath) {
    const ext = path.extname(npmExecPath).toLowerCase();
    if (ext === ".cmd" || ext === ".exe") {
      return { command: npmExecPath, args: [] };
    }
    if (ext === ".js" || ext === ".cjs" || ext === ".mjs") {
      return { command: process.execPath, args: [npmExecPath] };
    }
  }

  return { command: "pnpm", args: [] };
}

function cleanDir(dir) {
  if (dir && existsSync(dir)) {
    rmSync(dir, { recursive: true, force: true });
  }
}

async function runCommand(spec) {
  cleanDir(spec.cleanupDir);

  const start = Date.now();
  let stdout = "";
  let stderr = "";
  let timedOut = false;

  const child = spawn(spec.command, spec.args, {
    cwd: spec.cwd,
    env: {
      ...process.env,
      NODE_ENV: "production",
    },
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
    detached: process.platform !== "win32",
    shell: process.platform === "win32" && spec.command.endsWith(".cmd"),
  });

  child.stdout?.on("data", (chunk) => {
    stdout += chunk.toString();
  });
  child.stderr?.on("data", (chunk) => {
    stderr += chunk.toString();
  });

  const timeout = setTimeout(async () => {
    timedOut = true;
    await killProcessTree(child.pid);
  }, spec.timeoutMs);

  try {
    const exit = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", (code, signal) => {
        resolve({ code, signal });
      });
    });
    clearTimeout(timeout);

    if (timedOut) {
      return {
        ok: false,
        label: spec.label,
        durationMs: Date.now() - start,
        stdout,
        stderr,
        shortMessage: `timed out after ${spec.timeoutMs}ms`,
        timedOut: true,
      };
    }

    if (exit.code !== 0) {
      return {
        ok: false,
        label: spec.label,
        durationMs: Date.now() - start,
        stdout,
        stderr,
        shortMessage: `exited with code ${exit.code}${exit.signal ? ` (signal: ${exit.signal})` : ""}`,
        timedOut: false,
      };
    }

    if (spec.expectedOutput && !existsSync(spec.expectedOutput)) {
      return {
        ok: false,
        label: spec.label,
        durationMs: Date.now() - start,
        stdout,
        stderr,
        shortMessage: `expected output missing: ${spec.expectedOutput}`,
        timedOut: false,
      };
    }

    return {
      ok: true,
      label: spec.label,
      durationMs: Date.now() - start,
      stdout,
      stderr,
    };
  } catch (error) {
    clearTimeout(timeout);
    return {
      ok: false,
      label: spec.label,
      durationMs: Date.now() - start,
      stdout,
      stderr,
      shortMessage: error instanceof Error ? error.message : String(error),
      timedOut,
    };
  }
}

async function killProcessTree(pid) {
  if (!pid) {
    return;
  }

  if (process.platform === "win32") {
    await new Promise((resolve) => {
      const killer = spawn("taskkill", ["/PID", String(pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
      });
      killer.once("error", () => resolve());
      killer.once("close", () => resolve());
    });
    return;
  }

  try {
    process.kill(-pid, "SIGKILL");
  } catch {
    try {
      process.kill(pid, "SIGKILL");
    } catch {}
  }
}

function printResult(result) {
  const status = result.ok ? "PASS" : result.timedOut ? "TIMEOUT" : "FAIL";
  console.log(`[exit-regression] ${result.label}: ${status} (${result.durationMs}ms)`);

  if (!result.ok) {
    console.log(`[exit-regression] ${result.label} error: ${result.shortMessage}`);
    if (result.stdout.trim()) {
      console.log(`[exit-regression] ${result.label} stdout:\n${result.stdout}`);
    }
    if (result.stderr.trim()) {
      console.log(`[exit-regression] ${result.label} stderr:\n${result.stderr}`);
    }
  }
}

async function runLocalFixture() {
  return runCommand({
    label: "local vite lib build",
    cwd: unpluginDir,
    command: process.execPath,
    args: [viteBin, "build", "--config", localViteConfig],
    timeoutMs: localTimeoutMs,
    expectedOutput: localOutputFile,
    cleanupDir: localDistDir,
  });
}

async function runDirectStyleRegression() {
  return runCommand({
    label: "direct scss virtual-style repro",
    cwd: unpluginDir,
    command: process.execPath,
    args: [directStyleRegressionScript],
    timeoutMs: directStyleTimeoutMs,
  });
}

async function buildLinkedUnplugin() {
  return runCommand({
    label: "build linked @verter/unplugin dist",
    cwd: unpluginDir,
    command: pnpmInvocation.command,
    args: [...pnpmInvocation.args, "run", "build"],
    timeoutMs: 120_000,
  });
}

async function runExternalLibFallback() {
  // The pnpm workspace filter for the external consumer also comes from the
  // environment (the operator's local package name), so no private package name
  // is hardcoded. Defaults to the package directory's build script.
  const filter = process.env.VERTER_EXTERNAL_LIB_FILTER;
  const args = filter
    ? [...pnpmInvocation.args, "--filter", filter, "build-lib"]
    : [...pnpmInvocation.args, "run", "build-lib"];
  return runCommand({
    label: "external lib build-lib",
    cwd: externalLibRoot,
    command: pnpmInvocation.command,
    args,
    timeoutMs: externalLibTimeoutMs,
    cleanupDir: externalLibDistDir,
    expectedOutput: externalLibDistDir,
  });
}

async function main() {
  console.log("[exit-regression] rebuilding @verter/unplugin dist for the repro");
  const buildResult = await buildLinkedUnplugin();
  printResult(buildResult);
  if (!buildResult.ok) {
    throw new Error("Could not rebuild @verter/unplugin before running the exit regression.");
  }

  console.log("[exit-regression] running direct SCSS virtual-style repro");
  const directResult = await runDirectStyleRegression();
  printResult(directResult);
  if (!directResult.ok) {
    throw new Error(
      "Direct SCSS virtual-style regression failed. Fix the style pipeline before shipping the exit-hang change.",
    );
  }

  console.log("[exit-regression] running local Vite lib-build smoke test");
  const localResult = await runLocalFixture();
  printResult(localResult);

  if (!localResult.ok) {
    throw new Error("Local Vite lib-build smoke test failed after the direct regression passed.");
  }

  if (!runExternalLibCheck || !externalLibPackageDir) {
    console.log(
      "[exit-regression] external-lib validation skipped by default; set VERTER_RUN_EXTERNAL_LIB=1 and VERTER_EXTERNAL_LIB_ROOT to run it manually",
    );
    return;
  }

  if (!existsSync(externalLibPackageDir)) {
    console.log(
      `[exit-regression] external-lib validation skipped: ${externalLibPackageDir} not found`,
    );
    return;
  }

  console.log(
    `[exit-regression] validating the original consumer repro at ${externalLibPackageDir}`,
  );

  const externalResult = await runExternalLibFallback();
  printResult(externalResult);

  if (!externalResult.ok) {
    throw new Error(
      "The CSS-path fix passed locally but the external lib package still did not exit cleanly.",
    );
  }
}

main().catch((error) => {
  console.error(`[exit-regression] ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
});
