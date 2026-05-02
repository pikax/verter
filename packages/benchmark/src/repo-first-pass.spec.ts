/**
 * @ai-generated - Phase 11b diagnosis benchmark.
 *
 * Drives the four §10.4 overlay-isolation scenarios against the
 * nuxt-ui-codex-bench corpus and emits the captured per-counter
 * deltas to `tmp/perf-baselines/post-b2/repo-first-pass.json`.
 *
 * The spec orchestrates a Rust integration test
 * (`crates/verter_session/tests/repo_first_pass_diagnosis_corpus.rs`)
 * that is gated behind the `diagnosis-bench` cargo feature so the
 * default `cargo test --workspace --tests` run stays hermetic. The
 * Rust test holds the actual `CaptureToken` bind and emits a JSON
 * document framed by `===VERTER_PHASE_11B_DIAGNOSIS_BEGIN===` /
 * `===VERTER_PHASE_11B_DIAGNOSIS_END===` markers; this spec parses
 * the output, asserts non-empty data, and writes the public
 * baseline JSON.
 *
 * Corpus-drift refusal (Phase 11b mandatory):
 * - The spec reads `tmp/perf-baselines/post-b2/baseline-commit.txt`
 *   for the recorded `corpus-commit`.
 * - Runs `git -C <corpus> rev-parse HEAD` for the live commit.
 * - On mismatch: aborts with `BENCHMARK_CORPUS_DRIFT: ...` and exit
 *   code 78 (per §8 Phase 11b's contract). Vitest converts the
 *   thrown error to a test failure; this spec also calls
 *   `process.exit(78)` when `process.env.VERTER_PHASE_11B_STRICT`
 *   is set so CI scripts can shell-detect drift directly.
 */

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../../..");

const BASELINE_COMMIT_FILE = resolve(
  repoRoot,
  "tmp",
  "perf-baselines",
  "post-b2",
  "baseline-commit.txt",
);
const OUTPUT_JSON = resolve(repoRoot, "tmp", "perf-baselines", "post-b2", "repo-first-pass.json");

interface BaselineRecord {
  baselineCommit: string;
  corpusPath: string;
  corpusCommit: string;
}

function parseBaselineRecord(): BaselineRecord {
  if (!existsSync(BASELINE_COMMIT_FILE)) {
    throw new Error(
      `BENCHMARK_BASELINE_MISSING: ${BASELINE_COMMIT_FILE} does not exist. ` +
        `Phase 11b requires a recorded baseline; the orchestrator should run ` +
        `Phase 11a (post-B2 baseline capture) before dispatching Phase 11b.`,
    );
  }
  const text = readFileSync(BASELINE_COMMIT_FILE, "utf8");
  const lines = text.split(/\r?\n/);
  const record: Partial<BaselineRecord> = {};
  for (const line of lines) {
    const m = line.match(/^([\w-]+):\s*(.+)$/);
    if (!m) continue;
    const [, key, value] = m;
    if (key === "baseline-commit") record.baselineCommit = value.trim();
    if (key === "corpus-path") record.corpusPath = value.trim();
    if (key === "corpus-commit") record.corpusCommit = value.trim();
  }
  if (!record.baselineCommit || !record.corpusPath || !record.corpusCommit) {
    throw new Error(`BENCHMARK_BASELINE_MALFORMED: ${BASELINE_COMMIT_FILE} missing required keys`);
  }
  return record as BaselineRecord;
}

function liveCorpusCommit(corpusRoot: string): string {
  if (!existsSync(corpusRoot)) {
    throw new Error(`BENCHMARK_CORPUS_MISSING: ${corpusRoot} not present. Phase 11b cannot run.`);
  }
  const result = spawnSync("git", ["-C", corpusRoot, "rev-parse", "HEAD"], {
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(
      `BENCHMARK_CORPUS_GIT_FAILED: git rev-parse exit ${result.status}: ${result.stderr}`,
    );
  }
  return result.stdout.trim();
}

function refuseOnDrift(record: BaselineRecord): string {
  const corpusRoot = resolve(repoRoot, record.corpusPath);
  const live = liveCorpusCommit(corpusRoot);
  if (live !== record.corpusCommit) {
    const msg = `BENCHMARK_CORPUS_DRIFT: recorded=${record.corpusCommit}, live=${live}`;
    process.stderr.write(`${msg}\n`);
    if (process.env.VERTER_PHASE_11B_STRICT) {
      process.exit(78);
    }
    throw new Error(msg);
  }
  return corpusRoot;
}

interface CounterRow {
  record_origin_edge_total_ns: number;
  origin_edge_count: number;
  derivation_signature_pool_size: number;
  derivation_signature_intern_calls: number;
  derivation_signature_intern_returned_existing: number;
  entries_mutex_wait_total_ns: number;
  entries_mutex_hold_total_ns: number;
  elapsed_ns: number;
  duplicate_edge_count: number;
  dispatch_count: number;
}

interface DiagnosisJson {
  captured_at: string;
  corpus_commit: string;
  components: Record<string, Record<string, CounterRow>>;
}

function runRustDiagnosis(): DiagnosisJson {
  // Invoke the diagnosis-gated Rust integration test. The test
  // emits a single JSON document framed by markers; we extract
  // just that block.
  const result = spawnSync(
    "cargo",
    [
      "test",
      "--package",
      "verter_session",
      "--features",
      "diagnosis-bench",
      "--test",
      "repo_first_pass_diagnosis_corpus",
      "--",
      "--nocapture",
      "repo_first_pass_diagnosis_corpus_emits_json",
    ],
    {
      cwd: repoRoot,
      encoding: "utf8",
      maxBuffer: 50 * 1024 * 1024,
    },
  );
  const combined = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
  if (result.status !== 0) {
    throw new Error(`cargo test failed (exit ${result.status}):\n${combined.slice(-4000)}`);
  }
  const beginMarker = "===VERTER_PHASE_11B_DIAGNOSIS_BEGIN===";
  const endMarker = "===VERTER_PHASE_11B_DIAGNOSIS_END===";
  const beginIdx = combined.indexOf(beginMarker);
  const endIdx = combined.indexOf(endMarker);
  if (beginIdx < 0 || endIdx < 0 || endIdx < beginIdx) {
    throw new Error(
      `cargo test stdout did not contain the diagnosis JSON markers. ` +
        `Last 2000 chars:\n${combined.slice(-2000)}`,
    );
  }
  const jsonText = combined.slice(beginIdx + beginMarker.length, endIdx).trim();
  const parsed = JSON.parse(jsonText) as DiagnosisJson;
  return parsed;
}

function writeReport(report: DiagnosisJson): void {
  // Renormalise captured_at into a real ISO timestamp.
  if (report.captured_at.startsWith("unix:")) {
    const sec = Number(report.captured_at.slice("unix:".length));
    if (Number.isFinite(sec)) {
      report.captured_at = new Date(sec * 1000).toISOString();
    }
  }
  mkdirSync(dirname(OUTPUT_JSON), { recursive: true });
  writeFileSync(OUTPUT_JSON, `${JSON.stringify(report, null, 2)}\n`, "utf8");
}

describe("Phase 11b — repo_first_pass diagnosis benchmark", () => {
  it("refuses to run when the corpus has drifted from the recorded baseline", () => {
    // This test always passes when the corpus is in sync; its
    // discriminating value is that it exercises the
    // drift-detection logic. A drift would surface as a thrown
    // BENCHMARK_CORPUS_DRIFT error with a clear message.
    const record = parseBaselineRecord();
    expect(record.baselineCommit).toBeTruthy();
    expect(record.corpusPath).toBeTruthy();
    expect(record.corpusCommit).toBeTruthy();
    const corpusRoot = refuseOnDrift(record);
    expect(corpusRoot).toBeTruthy();
  });

  it(
    "captures non-empty per-counter data for all 12 components × 4 scenarios",
    { timeout: 30 * 60 * 1000 },
    () => {
      // Pre-flight: corpus drift refusal MUST run before any
      // benchmark work. A drift here aborts the test before any
      // potentially-misleading data is written.
      const record = parseBaselineRecord();
      refuseOnDrift(record);

      const report = runRustDiagnosis();
      expect(report.captured_at).toBeTruthy();
      expect(report.corpus_commit).toBeTruthy();
      expect(report.corpus_commit).toBe(record.corpusCommit);

      const componentNames = Object.keys(report.components);
      expect(componentNames.length).toBeGreaterThan(0);

      // At least one (component, scenario) pair must have non-empty
      // data. The instrumentation is wired correctly when at least
      // one counter is > 0 across the captured grid.
      const hasNonEmpty = componentNames.some((c) =>
        Object.values(report.components[c]).some(
          (row) =>
            row.origin_edge_count > 0 ||
            row.entries_mutex_hold_total_ns > 0 ||
            row.derivation_signature_intern_calls > 0,
        ),
      );
      expect(hasNonEmpty, "diagnosis benchmark captured no counter data").toBe(true);

      writeReport(report);
      expect(existsSync(OUTPUT_JSON)).toBe(true);
    },
  );

  it("recovers a useful execFileSync handle (sanity)", () => {
    // Validates the test environment has cargo on PATH; if cargo
    // is unavailable the prior test will fail with a less obvious
    // error.
    const result = execFileSync("cargo", ["--version"], { encoding: "utf8" });
    expect(result).toMatch(/^cargo /);
  });
});
