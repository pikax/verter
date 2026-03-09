import fs from "fs";
import os from "os";
import path from "path";
import { fileURLToPath } from "url";
import { downloadAndUnzipVSCode, runTests } from "@vscode/test-electron";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const extensionDevelopmentPath = path.resolve(__dirname, "..");
const extensionTestsPath = path.resolve(
  extensionDevelopmentPath,
  "out-test",
  "e2e",
  "suite",
  "index",
);

const fixtureName = "single-project";
const providerKind = "tsserver";
const appVueRelativePath = "src/App.vue";
const runs = Number.parseInt(process.env.VERTER_STARTUP_BENCHMARK_RUNS ?? "5", 10);
const scenario = process.env.VERTER_STARTUP_BENCHMARK_SCENARIO ?? "default";
const benchmarkDir = path.resolve(__dirname, "benchmarks");
const resultsDir = path.join(benchmarkDir, "results");
const baselinePath = path.join(
  benchmarkDir,
  `${fixtureName}-${providerKind}-baseline.json`,
);

main().catch((error) => {
  console.error("Startup benchmark failed:", error);
  process.exit(1);
});

async function main() {
  fs.mkdirSync(resultsDir, { recursive: true });

  const fixtureDir = path.resolve(__dirname, "fixtures", fixtureName);
  const workspaceFile = createWorkspaceOverlay(fixtureDir);
  const fileToOpen = path.join(fixtureDir, appVueRelativePath);
  const vscodeExecutablePath = await downloadAndUnzipVSCode("stable");
  const lspBinaryPath = copyLspBinaryToTemp();

  const reports = [];
  for (let index = 0; index < runs; index += 1) {
    const label = `${fixtureName}-${providerKind}-${scenario}-run-${index + 1}`;
    const userDataDir = path.join(
      os.tmpdir(),
      `verter-startup-benchmark-user-${Date.now()}-${index}`,
    );
    const extensionsDir = path.join(
      os.tmpdir(),
      `verter-startup-benchmark-ext-${Date.now()}-${index}`,
    );
    const logFile = path.join(os.tmpdir(), `${label}.log`);
    const timingFile = path.join(os.tmpdir(), `${label}.json`);

    fs.rmSync(userDataDir, { recursive: true, force: true });
    fs.rmSync(extensionsDir, { recursive: true, force: true });

    console.log(`Running cold start ${index + 1}/${runs}...`);

    await runTests({
      vscodeExecutablePath,
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs: [
        "--disable-extensions",
        "--user-data-dir",
        userDataDir,
        "--extensions-dir",
        extensionsDir,
        workspaceFile,
        fileToOpen,
      ],
      extensionTestsEnv: {
        ...process.env,
        VERTER_E2E_TEST: "1",
        VERTER_E2E_FIXTURE: fixtureName,
        VERTER_E2E_ONLY: "startupBenchmark.test.js",
        VERTER_E2E_STARTUP_BENCHMARK: "1",
        VERTER_E2E_TYPE_PROVIDER: providerKind,
        VERTER_E2E_STARTUP_PROBE: JSON.stringify({
          relativePath: appVueRelativePath,
          completionAnchor: "{{ count }}",
          completionLabel: "count",
        }),
        VERTER_E2E_LOG_FILE: logFile,
        VERTER_E2E_TIMING_FILE: timingFile,
        VERTER_LOG: "debug",
        ...(lspBinaryPath ? { VERTER_E2E_LSP_PATH: lspBinaryPath } : {}),
      },
    });

    const report = JSON.parse(fs.readFileSync(timingFile, "utf8"));
    reports.push(report);
    fs.writeFileSync(
      path.join(resultsDir, `${label}.json`),
      JSON.stringify(report, null, 2),
    );
  }

  const summary = summarizeReports(reports);
  const baselineComparison = compareAgainstBaseline(summary);
  const output = {
    fixture: fixtureName,
    providerKind,
    scenario,
    runs,
    generatedAt: new Date().toISOString(),
    summary,
    baselineComparison,
    reports,
  };

  const summaryPath = path.join(
    resultsDir,
    `${fixtureName}-${providerKind}-${scenario}-summary.json`,
  );
  fs.writeFileSync(summaryPath, JSON.stringify(output, null, 2));

  console.log(`Summary written to ${summaryPath}`);
  if (baselineComparison?.status === "warn") {
    console.warn(baselineComparison.message);
  }
}

function createWorkspaceOverlay(fixtureDir) {
  const workspaceFile = path.join(
    os.tmpdir(),
    `verter-startup-benchmark-${Date.now()}.code-workspace`,
  );
  const workspaceConfig = {
    folders: [{ path: fixtureDir }],
    settings: {
      "verter.typeProvider": providerKind,
      "verter.analysis.enabled": false,
      "verter.decorations.bindingColors": true,
      "verter.decorations.vueApiCalls": true,
      "verter.decorations.propConstness": false,
      "verter.mcp.claudeCodeNotification": false,
    },
  };
  fs.writeFileSync(workspaceFile, JSON.stringify(workspaceConfig, null, 2));
  return workspaceFile;
}

function copyLspBinaryToTemp() {
  const binaryPath = findLspBinary();
  if (!binaryPath) {
    return undefined;
  }

  const ext = process.platform === "win32" ? ".exe" : "";
  const tempDir = path.join(os.tmpdir(), "verter-e2e-bin");
  fs.mkdirSync(tempDir, { recursive: true });
  const destinationPath = path.join(tempDir, `verter-lsp${ext}`);
  fs.copyFileSync(binaryPath, destinationPath);
  if (process.platform !== "win32") {
    fs.chmodSync(destinationPath, 0o755);
  }
  return destinationPath;
}

function findLspBinary() {
  const ext = process.platform === "win32" ? ".exe" : "";
  const binaryName = `verter-lsp${ext}`;
  let dir = extensionDevelopmentPath;
  for (let index = 0; index < 5; index += 1) {
    for (const profile of ["debug", "release"]) {
      const candidate = path.join(dir, "target", profile, binaryName);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
    dir = path.dirname(dir);
  }
  return undefined;
}

function summarizeReports(reports) {
  const activationToReadyMs = reports
    .map((report) => report.startup.activationToReadyMs)
    .filter(isNumber);
  const activationToFirstTypedCompletionMs = reports
    .map((report) => report.startup.activationToFirstTypedCompletionMs)
    .filter(isNumber);
  const readyToFirstTypedCompletionMs = reports
    .map((report) => report.startup.readyToFirstTypedCompletionMs)
    .filter(isNumber);
  const activationToTypeProviderStartedMs = reports
    .map((report) => report.startup.activationToTypeProviderStartedMs)
    .filter(isNumber);
  const typeProviderStartedToFirstTypedCompletionMs = reports
    .map((report) => report.startup.typeProviderStartedToFirstTypedCompletionMs)
    .filter(isNumber);
  const typeProviderStartedToReadyMs = reports
    .map((report) => report.startup.typeProviderStartedToReadyMs)
    .filter(isNumber);

  return {
    activationToReadyMs: summarizeMetric(activationToReadyMs),
    activationToFirstTypedCompletionMs: summarizeMetric(
      activationToFirstTypedCompletionMs,
    ),
    readyToFirstTypedCompletionMs: summarizeMetric(readyToFirstTypedCompletionMs),
    activationToTypeProviderStartedMs: summarizeMetric(
      activationToTypeProviderStartedMs,
    ),
    typeProviderStartedToFirstTypedCompletionMs: summarizeMetric(
      typeProviderStartedToFirstTypedCompletionMs,
    ),
    typeProviderStartedToReadyMs: summarizeMetric(typeProviderStartedToReadyMs),
  };
}

function summarizeMetric(values) {
  if (values.length === 0) {
    return null;
  }

  const sorted = [...values].sort((left, right) => left - right);
  return {
    median: percentile(sorted, 0.5),
    p95: percentile(sorted, 0.95),
    min: sorted[0],
    max: sorted[sorted.length - 1],
  };
}

function compareAgainstBaseline(summary) {
  if (!fs.existsSync(baselinePath)) {
    return null;
  }

  const baseline = JSON.parse(fs.readFileSync(baselinePath, "utf8"));
  const currentMedian = summary.activationToFirstTypedCompletionMs?.median;
  const baselineMedian = baseline.summary?.activationToFirstTypedCompletionMs?.median;

  if (!isNumber(currentMedian) || !isNumber(baselineMedian)) {
    return null;
  }

  const toleranceMs = Math.max(Math.round(baselineMedian * 0.2), 300);
  if (currentMedian > baselineMedian + toleranceMs) {
    return {
      status: "warn",
      baselineMedian,
      currentMedian,
      toleranceMs,
      message:
        `Startup benchmark warning: median typed completion regressed from ` +
        `${baselineMedian}ms to ${currentMedian}ms (tolerance ${toleranceMs}ms).`,
    };
  }

  return {
    status: "ok",
    baselineMedian,
    currentMedian,
    toleranceMs,
  };
}

function percentile(sortedValues, fraction) {
  const index = Math.min(
    sortedValues.length - 1,
    Math.max(0, Math.ceil(sortedValues.length * fraction) - 1),
  );
  return sortedValues[index];
}

function isNumber(value) {
  return typeof value === "number" && Number.isFinite(value);
}
