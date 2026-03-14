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

const fixtureName = process.env.VERTER_COMPLETION_BENCHMARK_FIXTURE ?? "single-project";
const providerKind = process.env.VERTER_COMPLETION_BENCHMARK_PROVIDER ?? "tsserver";
const relativePath =
  process.env.VERTER_COMPLETION_BENCHMARK_FILE ?? "src/App.vue";
const anchor =
  process.env.VERTER_COMPLETION_BENCHMARK_ANCHOR ?? "count.value * 2";
const anchorOffset = readIntEnv("VERTER_COMPLETION_BENCHMARK_OFFSET", 6);
const expectedLabel =
  process.env.VERTER_COMPLETION_BENCHMARK_LABEL ?? "value";
const triggerCharacter = process.env.VERTER_COMPLETION_BENCHMARK_TRIGGER;
const runs = readIntEnv("VERTER_COMPLETION_BENCHMARK_RUNS", 5);
const iterations = readIntEnv("VERTER_COMPLETION_BENCHMARK_ITERATIONS", 10);
const resultsDir = path.join(__dirname, "benchmarks", "results");
const scenario = "script-member-access";

main().catch((error) => {
  console.error("Completion benchmark failed:", error);
  process.exit(1);
});

async function main() {
  fs.mkdirSync(resultsDir, { recursive: true });

  const fixtureDir = path.resolve(__dirname, "fixtures", fixtureName);
  const workspaceFile = createWorkspaceOverlay(fixtureDir);
  const fileToOpen = path.join(fixtureDir, relativePath);
  const vscodeExecutablePath = await downloadAndUnzipVSCode("stable");
  const lspBinaryPath = copyLspBinaryToTemp();

  const reports = [];

  for (let index = 0; index < runs; index += 1) {
    const label = `${fixtureName}-${providerKind}-${scenario}-run-${index + 1}`;
    const userDataDir = path.join(
      os.tmpdir(),
      `vcb-u-${Date.now()}-${index}`,
    );
    const extensionsDir = path.join(
      os.tmpdir(),
      `vcb-e-${Date.now()}-${index}`,
    );
    const logFile = path.join(os.tmpdir(), `${label}.log`);
    const timingFile = path.join(os.tmpdir(), `${label}-timing.json`);
    const reportFile = path.join(os.tmpdir(), `${label}-report.json`);

    fs.rmSync(userDataDir, { recursive: true, force: true });
    fs.rmSync(extensionsDir, { recursive: true, force: true });

    console.log(`Running completion benchmark ${index + 1}/${runs}...`);

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
        VERTER_E2E_ONLY: "completionBenchmark.test.js",
        VERTER_E2E_COMPLETION_BENCHMARK: "1",
        VERTER_E2E_TYPE_PROVIDER: providerKind,
        VERTER_E2E_COMPLETION_FILE: reportFile,
        VERTER_E2E_COMPLETION_BENCHMARK_ITERATIONS: String(iterations),
        VERTER_E2E_COMPLETION_BENCHMARK_FILE: relativePath,
        VERTER_E2E_COMPLETION_BENCHMARK_ANCHOR: anchor,
        VERTER_E2E_COMPLETION_BENCHMARK_OFFSET: String(anchorOffset),
        VERTER_E2E_COMPLETION_BENCHMARK_LABEL: expectedLabel,
        VERTER_E2E_LOG_FILE: logFile,
        VERTER_E2E_TIMING_FILE: timingFile,
        VERTER_LOG: "debug",
        ...(lspBinaryPath ? { VERTER_E2E_LSP_PATH: lspBinaryPath } : {}),
        ...(triggerCharacter
          ? { VERTER_E2E_COMPLETION_BENCHMARK_TRIGGER: triggerCharacter }
          : {}),
      },
    });

    const report = JSON.parse(fs.readFileSync(reportFile, "utf8"));
    reports.push(report);

    fs.writeFileSync(
      path.join(resultsDir, `${label}.json`),
      JSON.stringify(report, null, 2),
    );
  }

  const summary = {
    warmRequestMs: summarizeMetric(
      reports.flatMap((report) =>
        report.warmRequest.samples.map((sample) => sample.latencyMs),
      ),
    ),
    afterEditTypedMs: summarizeMetric(
      reports.flatMap((report) =>
        report.afterEditTyped.samples.map((sample) => sample.latencyMs),
      ),
    ),
  };

  const output = {
    fixture: fixtureName,
    providerKind,
    scenario,
    runs,
    iterations,
    generatedAt: new Date().toISOString(),
    target: {
      relativePath,
      anchor,
      anchorOffset,
      expectedLabel,
      triggerCharacter: triggerCharacter ?? null,
    },
    summary,
    reports,
  };

  const summaryPath = path.join(
    resultsDir,
    `${fixtureName}-${providerKind}-${scenario}-summary.json`,
  );
  fs.writeFileSync(summaryPath, JSON.stringify(output, null, 2));

  console.log(`Summary written to ${summaryPath}`);
  console.log(
    `Warm median=${summary.warmRequestMs.median}ms, ` +
      `after-edit median=${summary.afterEditTypedMs.median}ms`,
  );
}

function createWorkspaceOverlay(fixtureDir) {
  const workspaceFile = path.join(
    os.tmpdir(),
    `verter-completion-benchmark-${Date.now()}.code-workspace`,
  );
  const workspaceConfig = {
    folders: [{ path: fixtureDir }],
    settings: {
      "verter.typeProvider": providerKind,
      "verter.analysis.enabled": false,
      "verter.decorations.bindingColors": false,
      "verter.decorations.vueApiCalls": false,
      "verter.decorations.propConstness": false,
      "verter.mcp.claudeCodeNotification": false,
    },
  };
  fs.writeFileSync(workspaceFile, JSON.stringify(workspaceConfig, null, 2));
  return workspaceFile;
}

function copyLspBinaryToTemp() {
  const sourcePath = findLspBinary();
  if (!sourcePath) {
    return undefined;
  }

  if (process.platform !== "win32") {
    return sourcePath;
  }

  const ext = process.platform === "win32" ? ".exe" : "";
  const tempDir = path.join(os.tmpdir(), "verter-e2e-bin");
  fs.mkdirSync(tempDir, { recursive: true });
  const destinationPath = path.join(tempDir, `verter-lsp${ext}`);
  fs.copyFileSync(sourcePath, destinationPath);
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

function summarizeMetric(values) {
  const sorted = [...values].sort((left, right) => left - right);
  return {
    median: percentile(sorted, 0.5),
    p95: percentile(sorted, 0.95),
    min: sorted[0] ?? 0,
    max: sorted[sorted.length - 1] ?? 0,
    avg:
      sorted.length > 0
        ? Math.round(sorted.reduce((sum, value) => sum + value, 0) / sorted.length)
        : 0,
  };
}

function percentile(sortedValues, percentileValue) {
  if (sortedValues.length === 0) {
    return 0;
  }
  const index = Math.min(
    sortedValues.length - 1,
    Math.max(0, Math.ceil(sortedValues.length * percentileValue) - 1),
  );
  return sortedValues[index];
}

function readIntEnv(name, fallback) {
  const raw = process.env[name];
  if (!raw) {
    return fallback;
  }

  const parsed = Number.parseInt(raw, 10);
  return Number.isFinite(parsed) ? parsed : fallback;
}
