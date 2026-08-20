#!/usr/bin/env node
// Repeat-execution flake probe.
//
// Static prohibitions (see crates/*/clippy.toml) only catch flake CLASSES we
// already know about. The general control is empirical: run a test
// selection several times and require every run to pass. This is the
// natural partner to a future "which tests does this change affect"
// selector — point this probe at exactly that selection to catch a newly
// authored flake at authoring time, rather than four gate runs later.
//
// Deliberately NOT nextest retries (`.config/nextest.toml` keeps
// `retries` unset, i.e. 0, on purpose): a retry lets a later pass supersede
// an earlier failure, which HIDES a flake instead of catching one. This
// probe requires every run in the window to pass and reports every run that
// did not.
//
// Standalone: intentionally NOT wired into scripts/gate.mjs or
// scripts/gate-internals.mjs (owned by another workstream at the time this
// was written — see the flake-prevention report for where it should be
// wired once that ownership frees up). It also does not implement
// gate.mjs's memory-ceiling wrapper; a wide/unfiltered selection carries the
// same OOM risk documented for bare `cargo test`/`cargo nextest run` — keep
// the filterset narrow (exactly the tests a change touched), not
// `--workspace` with no filter.
//
// Usage:
//   node scripts/flake-probe.mjs [--runs=N] -- <args passed to `cargo nextest run`>
//
// Examples:
//   node scripts/flake-probe.mjs --runs=5 -- -E 'test(store_view_build_touches_no_owner_at_any_host_size)'
//   node scripts/flake-probe.mjs -- -p verter_semantic -E 'test(unique_temp_dir)'
//
// Zero-selection is a FAILED verification, never a pass: if the filterset
// selects no tests, this script exits non-zero rather than reporting a
// vacuous "3/3 passed".

import { spawnSync } from "node:child_process";

function parseArgs(argv) {
  const sepIndex = argv.indexOf("--");
  if (sepIndex === -1) {
    console.error(
      "flake-probe: missing `--` separator before the nextest args.\n" +
        "Usage: node scripts/flake-probe.mjs [--runs=N] -- <cargo nextest run args>",
    );
    process.exit(2);
  }
  const ownArgs = argv.slice(0, sepIndex);
  const nextestArgs = argv.slice(sepIndex + 1);

  let runs = 3;
  for (const arg of ownArgs) {
    const match = /^--runs=(\d+)$/.exec(arg);
    if (match) {
      runs = Number.parseInt(match[1], 10);
      continue;
    }
    console.error(`flake-probe: unrecognized flag before \`--\`: ${arg}`);
    process.exit(2);
  }

  if (!Number.isInteger(runs) || runs < 1) {
    console.error(`flake-probe: --runs must be a positive integer, got ${runs}`);
    process.exit(2);
  }
  if (nextestArgs.length === 0) {
    console.error("flake-probe: no `cargo nextest run` args given after `--`.");
    process.exit(2);
  }

  return { runs, nextestArgs };
}

// eslint-disable-next-line no-control-regex
const ANSI_ESCAPE_PATTERN = /\x1b\[[0-9;]*m/g;

/// Extract nextest's own summary count ("Summary [   0.123s] N tests run: P
/// passed, F failed, ...") from captured output. Returns null if the summary
/// line was not found (an unexpected output shape — treated as a failure by
/// the caller rather than silently assumed-zero or assumed-nonzero).
///
/// Strips ANSI color codes first: nextest colorizes its summary line even
/// under a piped (non-TTY) `stdio`, which otherwise splits `1` and `passed`
/// across escape sequences and breaks a naive regex.
function parseSummary(output) {
  const plain = output.replace(ANSI_ESCAPE_PATTERN, "");
  const match = /(\d+) tests? run: (\d+) passed(?:, (\d+) failed)?/.exec(plain);
  if (!match) {
    return null;
  }
  return {
    total: Number.parseInt(match[1], 10),
    passed: Number.parseInt(match[2], 10),
    failed: match[3] ? Number.parseInt(match[3], 10) : 0,
  };
}

function main() {
  const { runs, nextestArgs } = parseArgs(process.argv.slice(2));

  console.log(
    `flake-probe: running \`cargo nextest run ${nextestArgs.join(" ")}\` ${runs} time(s), ` +
      `requiring every run to pass.`,
  );

  const results = [];
  for (let attempt = 1; attempt <= runs; attempt += 1) {
    console.log(`\n=== flake-probe run ${attempt}/${runs} ===`);
    const proc = spawnSync("cargo", ["nextest", "run", ...nextestArgs], {
      stdio: ["ignore", "pipe", "pipe"],
      encoding: "utf8",
    });
    const combined = `${proc.stdout ?? ""}\n${proc.stderr ?? ""}`;
    process.stdout.write(proc.stdout ?? "");
    process.stderr.write(proc.stderr ?? "");

    const summary = parseSummary(combined);
    const exitOk = proc.status === 0;
    results.push({ attempt, exitOk, summary });

    if (summary && summary.total === 0) {
      console.error(
        `\nflake-probe: run ${attempt} selected ZERO tests. A zero-selection filterset is a ` +
          "failed verification, not a vacuous pass — fix the filter expression.",
      );
      process.exit(1);
    }
  }

  const failures = results.filter((r) => !r.exitOk);
  console.log("\n=== flake-probe summary ===");
  for (const r of results) {
    const shape = r.summary
      ? `${r.summary.passed}/${r.summary.total} passed, ${r.summary.failed} failed`
      : "(could not parse nextest summary — treated as failed)";
    console.log(`  run ${r.attempt}/${runs}: ${r.exitOk ? "PASS" : "FAIL"} — ${shape}`);
  }

  if (failures.length > 0) {
    console.error(
      `\nflake-probe: FAILED — ${failures.length}/${runs} run(s) failed. A flaky test failed at ` +
        `least once across ${runs} runs; nextest retries are intentionally OFF (see ` +
        ".config/nextest.toml) so this could not have been hidden by a retry.",
    );
    process.exit(1);
  }

  console.log(`\nflake-probe: PASSED — ${runs}/${runs} runs green.`);
}

main();
