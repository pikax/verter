// triage-gate-internals.mjs — pure parsing/classification helpers for gate-failure triage.
//
// WHY THIS EXISTS. The maintainer's ruling: the working branch's gate is green BY INVARIANT, never by
// hypothesis — it is never re-measured. When a BLOCK branch's gate fails, the old habit was to run a
// SECOND full `node scripts/gate.mjs` on the working branch just to decide "did this branch break it, or
// was it already broken?". That is two whole-workspace gate runs (tens of minutes, real memory risk) to
// answer one question a per-test isolation re-run answers directly and far more cheaply. This module (plus
// the CLI in `triage-gate-failure.mjs`) replaces that habit: given a FAILED gate's own captured log, parse
// exactly which tests it named as non-tolerated failures, then re-run EACH ONE ALONE, N times, in true
// process isolation, and classify it — REAL / FLAKY / INTERACTION — never falling back to a second gate.
//
// NO CLI, argv, `process.exit`, or top-level side effect lives here — importing this runs nothing. Mirrors
// the gate.mjs / gate-internals.mjs split so the self-test can exercise every function in-process, and so
// `triage-gate-failure.mjs` stays a thin CLI shell. Reuses `gate-internals.mjs`'s own nextest recap/FAIL[
// parsing (`extractNextestTerminalFailures`, `parseNextestSummary`) — this file does NOT re-implement a
// second nextest-output parser; it only adds gate.mjs-log-specific structure (verdict-block extraction,
// surface segmentation, binary-id recovery, isolation-command construction, REAL/FLAKY/INTERACTION
// classification) on top of it.
//
// ----------------------------------------------------------------------------------------------------
// Classification contract (the three, and only three, outcomes for a real test id):
//   REAL        — every isolated attempt failed (N/N fail). The branch broke it; reproducible alone.
//   FLAKY       — at least one isolated attempt passed AND at least one failed. Genuinely intermittent
//                 even with NO other test sharing its process — a timing/nondeterminism bug in the test
//                 or the code it exercises.
//   INTERACTION — every isolated attempt passed (N/N pass) despite the test having failed under the full
//                 gate. It only fails under concurrency/ordering/shared process state — the cross-process-
//                 or cross-test-state race class. This is NOT collapsed into FLAKY: it is a distinct,
//                 usually MORE serious signal (a real shared-state bug), and the report says so.
// A run that never produced a clean isolated attempt (every attempt aborted — timeout/stall/memory/setup)
// classifies as INCONCLUSIVE, never silently as one of the three above.
// ----------------------------------------------------------------------------------------------------

// ----------------------------------------------------------------------------------------------------
// 1. Parsing the gate's own captured log (the `tee`'d output the operator already has per the repo's
//    long-running-command convention).
// ----------------------------------------------------------------------------------------------------

// gate.mjs's `log()`/`warn()`/`err()` prefix every line with `[gate]` / `[gate][warn]` / `[gate][error]`.
// The FAIL verdict block is printed via `err()`:
//   err(`VERDICT: FAIL — ${failures.length} non-tolerated failure(s):`);
//   for (const f of failures.slice(0, 50)) err(`  [${f.surface}] ${f.name}`);
// — so the block header is `[gate][error] VERDICT: FAIL` and each named failure is
// `[gate][error]   [<surface>] <name>`. `PASS` / `PASS-WITH-TOLERATED` are printed via `log()` (no
// `[error]`), so the verdict-line regex accepts either prefix and the FAIL/PASS branch is decided by the
// captured word.
const VERDICT_LINE = /^\[gate\](?:\[error\])? VERDICT: (FAIL|PASS-WITH-TOLERATED|PASS)\b/;
const FAILURE_LINE = /^\[gate\]\[error\]\s+\[(.+?)\]\s+(.+)$/;

// The exact header lines gate.mjs prints via `log()`, used to segment the log so a nextest binary-id
// lookup for a given surface's failures searches ONLY that surface's own raw recap — never a different
// surface's (which could name the same test under a different profile / outcome). Matched against
// gate.mjs's ACTUAL current output, not the deleted SURFACE 2 (direct in-process libtest replay) / old
// package-filtered SURFACE 3 shape — see the maintainer's SINGLE-TEST-UNIVERSE directive. There are only
// two segments in the current gate: SURFACE 1 (the main archive's `cargo nextest run`) and the
// SHIPPED-CFG GUARD's own small package-scoped `cargo nextest run -p verter_shipped_cfg_contract`.
const SURFACE1_HEADER = /^\[gate\] SURFACE 1: nextest run from the archive/;
const SHIPPED_CFG_HEADER = /^\[gate\] SHIPPED-CFG GUARD: cargo nextest run/;

// Find the gate's own verdict in a captured log. Returns:
//   { kind: "fail", failures: [{surface, name}] }   — a FAIL verdict; failures is the exact named list
//                                                       gate.mjs printed (already tolerance-filtered).
//   { kind: "pass" }                                  — a PASS / PASS-WITH-TOLERATED verdict; nothing to
//                                                       triage.
//   { kind: "none" }                                  — no recognizable VERDICT line found at all (a
//                                                       truncated capture, a log from something other than
//                                                       gate.mjs, or a run that aborted before reaching a
//                                                       verdict — TIMEOUT/STALL/MEMORY/LOCK-REFUSED/USAGE).
//                                                       Never guessed at; the caller fails closed.
export function parseGateVerdict(text) {
  const lines = text.split("\n");
  let verdictIdx = -1;
  let kind = null;
  for (let i = 0; i < lines.length; i++) {
    const m = VERDICT_LINE.exec(lines[i]);
    if (!m) continue;
    // The LAST verdict line wins (a log may contain a prior run's output pasted above a fresh one); this
    // mirrors the "last Summary wins" style used elsewhere only where a single well-formed verdict is
    // expected — here we simply prefer the final occurrence, which is what an operator's own tee capture
    // of ONE gate invocation always has (at most one, this is a hardening default for concatenated logs).
    verdictIdx = i;
    kind = m[1] === "FAIL" ? "fail" : "pass";
  }
  if (kind === null) return { kind: "none" };
  if (kind === "pass") return { kind: "pass" };
  const failures = [];
  for (let i = verdictIdx + 1; i < lines.length; i++) {
    const fm = FAILURE_LINE.exec(lines[i]);
    if (!fm) break; // the block ends at the first non-matching line (or EOF)
    failures.push({ surface: fm[1], name: fm[2] });
  }
  return { kind: "fail", failures };
}

// Split a captured gate log into its per-surface raw segments — SURFACE 1's own `cargo nextest run` and
// the SHIPPED-CFG GUARD's own package-scoped `cargo nextest run -p verter_shipped_cfg_contract`, both
// mirrored live to the gate's stdout/stderr by default (`runContainedStep`'s `captureStdoutSeparately`
// defaults to `false` for both), so both raw recaps are present in an operator's `tee` capture and the
// raw recap `extractNextestTerminalFailures` can read binary-ids out of either.
export function splitGateLogSurfaces(text) {
  const lines = text.split("\n");
  const idx = (re) => lines.findIndex((l) => re.test(l));
  const s1 = idx(SURFACE1_HEADER);
  const sc = idx(SHIPPED_CFG_HEADER);
  const verdict = lines.findIndex((l) => VERDICT_LINE.test(l));
  const end = lines.length;
  const seg = (start, stop) =>
    start === -1 ? "" : lines.slice(start, stop === -1 ? end : stop).join("\n");
  const s1Stop = sc !== -1 ? sc : verdict !== -1 ? verdict : -1;
  const scStop = verdict !== -1 ? verdict : -1;
  return { surface1: seg(s1, s1Stop), shippedCfg: seg(sc, scStop) };
}

// A failure `name` wrapped in `<...>` is a SYNTHETIC diagnostic entry gate.mjs itself manufactures when a
// surface's failures cannot be individually named — e.g. `<run did not complete: N of M selected test(s)
// never ran>`, `<abnormal libtest exit 132 (signal/abort)>`, `<tolerance refused: ...>`. These are never
// a real test id: there is no single test to isolate and re-run. `resolveIsolationTargets` routes them to
// `unclassifiable` instead of attempting a reproduction command.
export function isSyntheticFailureName(name) {
  return name.startsWith("<") && name.endsWith(">");
}

// ----------------------------------------------------------------------------------------------------
// 2. Turning each named failure into an isolation target: an exact nextest filter expression + the cargo
//    profile that produced it, recovered from the gate's own reused parsing (`extractNextestTerminalFailures`
//    on the surface's raw segment) wherever the raw recap is available, or degrading to a name-only filter
//    with an explicit caveat when it is not (a `libtest:<binary-id>` verdict tag, if one is ever produced,
//    already carries binary-id directly; a truncated/partial log degrades gracefully rather than refusing
//    outright).
// ----------------------------------------------------------------------------------------------------

// nextest's filterset DSL accepts a bare unquoted string for `test(=..)`/`binary_id(..)` when it contains
// only characters that cannot be mistaken for DSL syntax; anything else is wrapped in double quotes with
// internal quotes/backslashes escaped. Rust test/binary identifiers are overwhelmingly `[A-Za-z0-9_:./-]`,
// so this only ever engages on the rare pathological name.
const NEXTEST_BARE_SAFE = /^[A-Za-z0-9_:./-]+$/;
export function quoteNextestFilterValue(value) {
  if (NEXTEST_BARE_SAFE.test(value)) return value;
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

// Build the exact nextest filterset expression (`-E` value) that selects ONE test: the named test inside
// its owning binary when the binary-id is known, or the name alone (may select same-named tests across
// distinct binaries — always noted in the target's `caveat`) when it is not.
export function buildIsolationFilter(binaryId, name) {
  const nameFilter = `test(=${quoteNextestFilterValue(name)})`;
  if (!binaryId) return nameFilter;
  return `binary_id(${quoteNextestFilterValue(binaryId)}) & ${nameFilter}`;
}

// The exact `cargo nextest run ...` argv that reproduces ONE isolated attempt — printed in the report so a
// human can paste it directly, and reused verbatim by the CLI's own rerun driver. `packageScope`, when
// given, adds `-p <package>` — required for the shipped-cfg guard's failures REGARDLESS of whether
// binary-id recovery succeeded: that guard's live invocation is itself package-scoped
// (`cargo nextest run -p verter_shipped_cfg_contract --cargo-profile no-debug-assertions`), and a
// name-only rerun with no `-p` resolves against the whole workspace instead, where feature unification
// can change which candidates even compile — risking a genuine package-scoped failure misclassified as
// an INTERACTION.
export function buildIsolationRunArgs({
  filter,
  cargoProfile,
  packageScope = null,
  extraArgs = [],
}) {
  const args = ["nextest", "run"];
  if (packageScope) args.push("-p", packageScope);
  args.push("-E", filter, "--test-threads", "1", ...extraArgs);
  if (cargoProfile) args.push("--cargo-profile", cargoProfile);
  return args;
}

// Recover binaryId (or null) for a `{surface, name}` failure from the gate's own recap parsing, applied to
// ONLY that failure's owning surface segment (`extractFn` is `extractNextestTerminalFailures`, injected so
// this stays a pure function with no import-order coupling to gate-internals.mjs). Returns `null` when the
// segment is empty (not captured — e.g. libtest, or a truncated log) or the name is not found in it.
function recoverBinaryId(segmentText, name, extractFn) {
  if (!segmentText) return null;
  const { failures } = extractFn(segmentText);
  const hit = failures.find((f) => f.name === name);
  return hit ? hit.binaryId || null : null;
}

// The full resolution: turn a gate verdict's `failures` list into isolation targets + an unclassifiable
// bucket, given the log's per-surface raw segments and the injected nextest-recap extractor.
//
// Returns { targets: [...], unclassifiable: [...] }.
//   target: { surface, name, binaryId, cargoProfile, packageScope, filter, runArgs, caveat }
//   unclassifiable: { surface, name, reason }
export function resolveIsolationTargets({ failures, surfaces, extractNextestTerminalFailures }) {
  const targets = [];
  const unclassifiable = [];
  for (const f of failures) {
    if (isSyntheticFailureName(f.name)) {
      unclassifiable.push({
        surface: f.surface,
        name: f.name,
        reason:
          "synthetic diagnostic entry (gate.mjs's own summary of an unaccounted/crash/tolerance-refused " +
          "condition), not a single named test — there is nothing to isolate and re-run",
      });
      continue;
    }
    let binaryId = null;
    let cargoProfile = null;
    let packageScope = null;
    let caveat = "";
    if (f.surface === "nextest" || f.surface.startsWith("nextest:")) {
      binaryId = recoverBinaryId(surfaces.surface1, f.name, extractNextestTerminalFailures);
      cargoProfile = null; // dev profile (SURFACE 1's own archive)
    } else if (
      f.surface === "shipped-cfg/nextest" ||
      f.surface.startsWith("shipped-cfg/nextest:")
    ) {
      binaryId = recoverBinaryId(surfaces.shippedCfg, f.name, extractNextestTerminalFailures);
      cargoProfile = "no-debug-assertions"; // the shipped-cfg guard's own profile — see Cargo.toml
      // Package-scoped REGARDLESS of binary-id recovery: the live guard itself runs
      // `-p verter_shipped_cfg_contract`, so the rerun must match that exactly, not degrade to a
      // whole-workspace name-only selection when the raw recap segment was not present in the log.
      packageScope = "verter_shipped_cfg_contract";
    } else if (f.surface.startsWith("libtest:")) {
      binaryId = f.surface.slice("libtest:".length);
      cargoProfile = null; // runs from the SAME dev archive as SURFACE 1
    } else {
      unclassifiable.push({
        surface: f.surface,
        name: f.name,
        reason: `unrecognized surface tag '${f.surface}' — cannot determine which archive/profile produced it`,
      });
      continue;
    }
    if (!binaryId) {
      caveat =
        "binary-id could not be recovered (the raw nextest recap for this surface was not present in the " +
        "captured log, or the name was not found in it) — this filter matches by test name ALONE, which " +
        "may select more than one test if the same name exists in multiple binaries" +
        (packageScope ? ` (rerun stays package-scoped to ${packageScope} regardless)` : "");
    }
    const filter = buildIsolationFilter(binaryId, f.name);
    targets.push({
      surface: f.surface,
      name: f.name,
      binaryId,
      cargoProfile,
      packageScope,
      filter,
      runArgs: buildIsolationRunArgs({ filter, cargoProfile, packageScope }),
      caveat,
    });
  }
  return { targets, unclassifiable };
}

// ----------------------------------------------------------------------------------------------------
// 3. Classification from N isolated-attempt outcomes.
// ----------------------------------------------------------------------------------------------------

// One isolated attempt's outcome, as produced by the CLI driver from a single `cargo nextest run` child:
//   { outcome: "pass" | "fail" | "abort", detail }
// "abort" means the attempt itself did not produce a trustworthy pass/fail (watchdog reason, zero
// selection, unparseable summary) — it is excluded from the classification vote but reported.
//
// classify() implements the exact three-way contract from the module doc-comment, plus the fourth,
// explicitly-named non-outcome: INCONCLUSIVE when there is no valid attempt to classify from at all. It
// NEVER returns a bare guess — a target with zero valid attempts is INCONCLUSIVE, not silently REAL/FLAKY.
export function classifyAttempts(attempts) {
  const valid = attempts.filter((a) => a.outcome === "pass" || a.outcome === "fail");
  const aborted = attempts.filter((a) => a.outcome === "abort");
  const passes = valid.filter((a) => a.outcome === "pass").length;
  const fails = valid.filter((a) => a.outcome === "fail").length;
  let classification;
  if (valid.length === 0) classification = "INCONCLUSIVE";
  else if (fails === valid.length) classification = "REAL";
  else if (passes === valid.length) classification = "INTERACTION";
  else classification = "FLAKY";
  return {
    classification,
    validAttempts: valid.length,
    totalAttempts: attempts.length,
    passes,
    fails,
    aborted: aborted.length,
    complete: aborted.length === 0 && attempts.length === valid.length,
  };
}
