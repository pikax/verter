/**
 * TDD tests for the parent-owned hard-timeout corpus trace runner.
 *
 * These tests verify that:
 * 1. Each component runs in an isolated child process
 * 2. Timeout enforcement is parent-owned (SIGKILL, not Promise.race)
 * 3. Structured result status classification is correct
 * 4. Component names appear in results
 */

import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  type CorpusTraceResult,
  classifyExitStatus,
  parseStdoutFields,
  runComponentInIsolation,
} from "./corpus-trace-runner.js";

// ---------------------------------------------------------------------------
// classifyExitStatus — pure status classification
// ---------------------------------------------------------------------------

describe("classifyExitStatus", () => {
  it("returns ok when exit code is 0 and Done line was seen", () => {
    expect(
      classifyExitStatus({
        exitCode: 0,
        signal: null,
        timedOut: false,
        sawDoneLine: true,
        sawClosedLine: true,
      }),
    ).toBe("ok");
  });

  it("returns query_timeout when the parent timer fired", () => {
    expect(
      classifyExitStatus({
        exitCode: null,
        signal: "SIGKILL",
        timedOut: true,
        sawDoneLine: false,
        sawClosedLine: false,
      }),
    ).toBe("query_timeout");
  });

  it("returns crash when exit code is nonzero and no timeout", () => {
    expect(
      classifyExitStatus({
        exitCode: 1,
        signal: null,
        timedOut: false,
        sawDoneLine: false,
        sawClosedLine: false,
      }),
    ).toBe("crash");
  });

  it("returns external_signal when killed by a signal without parent timeout", () => {
    expect(
      classifyExitStatus({
        exitCode: null,
        signal: "SIGTERM",
        timedOut: false,
        sawDoneLine: false,
        sawClosedLine: false,
      }),
    ).toBe("external_signal");
  });

  it("returns close_timeout when Done was seen but process did not exit cleanly", () => {
    expect(
      classifyExitStatus({
        exitCode: null,
        signal: "SIGKILL",
        timedOut: true,
        sawDoneLine: true,
        sawClosedLine: false,
      }),
    ).toBe("close_timeout");
  });
});

// ---------------------------------------------------------------------------
// parseStdoutFields — extract structured fields from child stdout
// ---------------------------------------------------------------------------

describe("parseStdoutFields", () => {
  it("parses a normal Done line with props and payload", () => {
    const stdout =
      "Done in 96ms (13 props) payload=22.3KB mem=28.6KB setup=74ms setup heap=5MB rss=73MB->heap=9MB rss=93MB query heap=9MB rss=93MB->heap=9MB rss=1870MB\nClosed heap=9MB rss=1870MB\n";

    const fields = parseStdoutFields(stdout);
    expect(fields.queryMsFromStdout).toBe(96);
    expect(fields.sawDoneLine).toBe(true);
    expect(fields.sawClosedLine).toBe(true);
  });

  it("parses a TIMEOUT line", () => {
    const stdout = "TIMEOUT after 14000ms setup=74ms setup heap=5MB rss=73MB->heap=9MB rss=93MB\n";
    const fields = parseStdoutFields(stdout);
    expect(fields.queryMsFromStdout).toBeNull();
    expect(fields.sawDoneLine).toBe(false);
    expect(fields.sawClosedLine).toBe(false);
  });

  it("returns nulls for empty stdout", () => {
    const fields = parseStdoutFields("");
    expect(fields.queryMsFromStdout).toBeNull();
    expect(fields.sawDoneLine).toBe(false);
    expect(fields.sawClosedLine).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// runComponentInIsolation — integration test with a real child process
// ---------------------------------------------------------------------------

describe("runComponentInIsolation", () => {
  // 15s vitest timeout on the happy path — the default 5s is tight
  // on Windows when the suite runs in parallel (`pnpm -r --parallel
  // run test` saturates the CPU during the spawn). The other
  // spawn-based tests in this file already use 15s; matching them
  // removes the flake without weakening any assertion.
  it("returns ok result for a child that exits cleanly", async () => {
    const tmpDir = mkdtempSync(resolve(tmpdir(), "verter-corpus-runner-"));
    const childScript = resolve(tmpDir, "child-ok.mjs");
    writeFileSync(
      childScript,
      [
        'console.log("Done in 42ms (5 props) payload=1.2KB mem=0.5KB setup=10ms setup heap=1MB rss=10MB->heap=2MB rss=20MB query heap=2MB rss=20MB->heap=2MB rss=20MB");',
        'console.log("Closed heap=2MB rss=20MB");',
        "process.exit(0);",
      ].join("\n"),
      "utf8",
    );

    const result = await runComponentInIsolation({
      component: "src/runtime/components/TestOk.vue",
      command: process.execPath,
      args: [childScript],
      timeoutMs: 5_000,
      outputDir: tmpDir,
    });

    expect(result.status).toBe("ok");
    expect(result.component).toBe("src/runtime/components/TestOk.vue");
    expect(result.query_ms_from_stdout).toBe(42);
    expect(result.saw_done_line).toBe(true);
    expect(result.saw_closed_line).toBe(true);
    expect(result.exit_code).toBe(0);
    expect(result.signal).toBeNull();
    expect(result.wall_ms).toBeGreaterThanOrEqual(0);
  }, 15_000);

  it("returns query_timeout for a child that hangs and is killed by the parent", async () => {
    const tmpDir = mkdtempSync(resolve(tmpdir(), "verter-corpus-runner-"));
    const childScript = resolve(tmpDir, "child-hang.mjs");
    writeFileSync(
      childScript,
      // Busy wait forever — cannot be interrupted by in-process timeout
      "setInterval(() => {}, 100_000);",
      "utf8",
    );

    const result = await runComponentInIsolation({
      component: "src/runtime/components/Hang.vue",
      command: process.execPath,
      args: [childScript],
      timeoutMs: 200,
      outputDir: tmpDir,
    });

    expect(result.status).toBe("query_timeout");
    expect(result.wall_ms).toBeGreaterThanOrEqual(200);
    expect(result.saw_done_line).toBe(false);
    expect(result.exit_code).toBeNull();
    // Upper bound is an "approximate promptness" guard, not a
    // deterministic SLA. On Windows under `pnpm -r --parallel run
    // test` saturation, child-process teardown can take ~2.5s+
    // (the old 2s ceiling flaked). 5s is a generous envelope that
    // still catches a regression where the parent-kill path is
    // broken entirely (e.g. the busy-wait survives until the test
    // harness times out).
    expect(result.wall_ms).toBeLessThan(5_000);
  }, 15_000);

  it("returns crash for a child that exits with nonzero code", async () => {
    const tmpDir = mkdtempSync(resolve(tmpdir(), "verter-corpus-runner-"));
    const childScript = resolve(tmpDir, "child-crash.mjs");
    writeFileSync(childScript, "process.exit(2);", "utf8");

    const result = await runComponentInIsolation({
      component: "src/runtime/components/Crash.vue",
      command: process.execPath,
      args: [childScript],
      timeoutMs: 5_000,
      outputDir: tmpDir,
    });

    expect(result.status).toBe("crash");
    expect(result.exit_code).toBe(2);
    expect(result.signal).toBeNull();
  });

  it("populates stdout_path and stderr_path in results", async () => {
    const tmpDir = mkdtempSync(resolve(tmpdir(), "verter-corpus-runner-"));
    const childScript = resolve(tmpDir, "child-paths.mjs");
    writeFileSync(
      childScript,
      [
        'console.log("Done in 1ms (0 props) payload=0B mem=0B setup=0ms setup heap=0MB rss=0MB->heap=0MB rss=0MB query heap=0MB rss=0MB->heap=0MB rss=0MB");',
        'console.log("Closed heap=0MB rss=0MB");',
        "process.exit(0);",
      ].join("\n"),
      "utf8",
    );

    const result = await runComponentInIsolation({
      component: "src/runtime/components/Paths.vue",
      command: process.execPath,
      args: [childScript],
      timeoutMs: 5_000,
      outputDir: tmpDir,
    });

    expect(result.stdout_path).toBeTruthy();
    expect(result.stderr_path).toBeTruthy();
  });

  it("provisions a result artifact path for normalized component-meta output", async () => {
    const tmpDir = mkdtempSync(resolve(tmpdir(), "verter-corpus-runner-"));
    const childScript = resolve(tmpDir, "child-result.mjs");
    writeFileSync(
      childScript,
      [
        'import { writeFileSync } from "node:fs";',
        "const resultPath = process.env.VERTER_COMPONENT_META_RESULT_PATH;",
        'if (!resultPath) throw new Error("missing VERTER_COMPONENT_META_RESULT_PATH");',
        'writeFileSync(resultPath, JSON.stringify({ ok: true }), "utf8");',
        'console.log("Done in 3ms (0 props) payload=0B mem=0B setup=0ms setup heap=0MB rss=0MB->heap=0MB rss=0MB query heap=0MB rss=0MB->heap=0MB rss=0MB");',
        'console.log("Closed heap=0MB rss=0MB");',
        "process.exit(0);",
      ].join("\n"),
      "utf8",
    );

    const result = await runComponentInIsolation({
      component: "src/runtime/components/Result.vue",
      command: process.execPath,
      args: [childScript],
      timeoutMs: 5_000,
      outputDir: tmpDir,
    });

    expect(result.status).toBe("ok");
    expect(result.result_path).toBeTruthy();
    expect(result.result_path.endsWith("src__runtime__components__Result__vue.result.json")).toBe(
      true,
    );
    expect(existsSync(result.result_path)).toBe(true);
    expect(JSON.parse(readFileSync(result.result_path, "utf8"))).toEqual({ ok: true });
  });

  it("corpus_trace_runner_emits_audit_and_analysis_json_reads_audit_only", async () => {
    // The runner passes
    // `VERTER_COMPONENT_META_AUDIT_PATH` and
    // `VERTER_COMPONENT_META_ANALYSIS_PATH` to the child (consumed by
    // `_audit-component.ts`) and reports the emitted-file state on
    // the returned result. The analyzer invoked downstream is
    // `audit-validator.ts` — not the deleted `trace-validator.ts` —
    // enforced via grep over the runner source.
    //
    // Discriminating: a regression that drops the audit path env
    // wiring surfaces as `audit_emitted === false` below. A
    // regression that reintroduces the regex validator surfaces as
    // the grep hit.
    const tmpDir = mkdtempSync(resolve(tmpdir(), "verter-corpus-runner-"));
    const childScript = resolve(tmpDir, "child-audit-emit.mjs");
    writeFileSync(
      childScript,
      [
        'import { writeFileSync } from "node:fs";',
        "const auditPath = process.env.VERTER_COMPONENT_META_AUDIT_PATH;",
        "const analysisPath = process.env.VERTER_COMPONENT_META_ANALYSIS_PATH;",
        'if (!auditPath) throw new Error("missing VERTER_COMPONENT_META_AUDIT_PATH");',
        'if (!analysisPath) throw new Error("missing VERTER_COMPONENT_META_ANALYSIS_PATH");',
        // Write a minimal audit record + analysis side-car so the
        // runner's post-condition (`audit_emitted && analysis_emitted`)
        // reports both lanes populated.
        "const auditJson = JSON.stringify({",
        '  request_id: "1", canonical_id: "/C.vue",',
        "  timings: {}, solver: {}, store: {}, memory: {}, footprint: null,",
        "});",
        'writeFileSync(auditPath, auditJson, "utf8");',
        'writeFileSync(analysisPath, JSON.stringify({ props: [] }), "utf8");',
        'console.log("Done in 5ms (0 props) audit=true setup=0ms");',
        'console.log("Closed");',
        "process.exit(0);",
      ].join("\n"),
      "utf8",
    );

    const result = await runComponentInIsolation({
      component: "src/runtime/components/C.vue",
      command: process.execPath,
      args: [childScript],
      timeoutMs: 5_000,
      outputDir: tmpDir,
    });

    expect(result.status).toBe("ok");
    // Both emission paths were pre-allocated.
    expect(result.audit_path).toBeTruthy();
    expect(result.analysis_path).toBeTruthy();
    expect(result.audit_path).toMatch(/\.audit\.json$/);
    expect(result.analysis_path).toMatch(/\.analysis\.json$/);
    // And both files were actually written by the child.
    expect(result.audit_emitted).toBe(true);
    expect(result.analysis_emitted).toBe(true);
    expect(existsSync(result.audit_path)).toBe(true);
    expect(existsSync(result.analysis_path)).toBe(true);
    // The emitted audit payload parses as JSON with the expected
    // top-level shape a downstream audit-validator consumer will
    // process.
    const emittedAudit = JSON.parse(readFileSync(result.audit_path, "utf8"));
    expect(emittedAudit).toHaveProperty("request_id");
    expect(emittedAudit).toHaveProperty("canonical_id");
    const emittedAnalysis = JSON.parse(readFileSync(result.analysis_path, "utf8"));
    expect(emittedAnalysis).toHaveProperty("props");
  });

  it("runner source invokes audit-validator, not the deleted trace-validator", async () => {
    // Exit criterion: the
    // runner's module must not reference the deleted regex
    // validator. A regression that re-introduces `trace-validator`,
    // `trace-check`, or `trace-specs/component-meta` trips here.
    const runnerPath = resolve(import.meta.dirname, "corpus-trace-runner.ts");
    const source = await import("node:fs/promises").then((fs) => fs.readFile(runnerPath, "utf-8"));
    for (const forbidden of ["trace-validator", "trace-check", "trace-specs/component-meta"]) {
      expect(
        source.includes(forbidden),
        `corpus-trace-runner.ts must not reference ${forbidden}`,
      ).toBe(false);
    }
    // The audit path fields (`audit_path`, `analysis_path`,
    // `audit_emitted`, `analysis_emitted`) must be present — they're
    // the runner's audit-flow contract.
    expect(source).toContain("audit_path:");
    expect(source).toContain("analysis_path:");
    expect(source).toContain("audit_emitted:");
    expect(source).toContain("analysis_emitted:");
  });
});
