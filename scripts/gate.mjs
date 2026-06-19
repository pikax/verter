#!/usr/bin/env node
// gate.mjs — canonical agent Rust gate runner.
//
// PURPOSE
//   Builds the whole workspace test universe ONCE (via `cargo nextest archive`) and runs BOTH
//   verification surfaces from the SAME archived artifacts:
//     1. nextest run (per-test PROCESS ISOLATION) — surfaces nothing that survives a fork, catches the
//        ordinary regression set.
//     2. the verter_session libtest binaries executed DIRECTLY (in-process / multi-test-per-process) —
//        surfaces shared-process state bugs the isolated path cannot.
//   Because the only build command issued is the single `--workspace` archive build, the gate NEVER
//   issues the package-scoped `cargo test -p verter_session` resolution and so structurally cannot incur
//   the recompile that resolution caused (see "Canonical feature set" below).
//
// CANONICAL FEATURE SET (why no `-p verter_session`)
//   `cargo nextest run --workspace` and `cargo test --workspace` SHARE Cargo feature unification, which
//   activates `verter_session`'s `session_metrics` feature (a downstream crate — `verter_lsp` — depends on
//   `verter_session` with `features = ["session_metrics"]`, and the real LSP binary + napi binding force it
//   ON). The package-scoped `cargo test -p verter_session` resolution builds `verter_session` with
//   `session_metrics` OFF (its default) and a different dev-dep closure ⇒ a different unit hash ⇒ an
//   artifact-reuse miss ⇒ a full recompile of the verter_session reverse-dependency chain on the very next
//   gate command. This gate deliberately tests the workspace-unified (`session_metrics` ON) configuration —
//   the PRODUCTION-REACHABLE one (it is what the shipping LSP/napi build uses) — which is exactly why it
//   never issues the package-scoped resolution. It does NOT use `--all-features` (the repo has slow/external
//   feature gates) and does NOT mutate any Cargo.toml.
//
// EQUIVALENCE TO THE TWO-COMMAND GATE
//   The legacy gate was: `cargo nextest run --workspace` then `cargo test -p verter_session --tests`.
//   Here: the nextest run from the archive == surface 1; the direct execution of every `verter_session`
//   suite whose kind is `lib` or `test` (i.e. the lib unit-test binary + every `tests/*.rs` integration
//   binary — exactly what `cargo test --tests` builds; `bin`/`bench` excluded) == surface 2. Surface 2 runs
//   with cwd = the verter_session package manifest dir (what Cargo sets) and the runtime Cargo env those
//   tests actually read (CARGO_MANIFEST_DIR + CARGO_TARGET_DIR — verified complete for this suite), modulo
//   the `session_metrics` cfg (ON here, the production configuration).
//
// SAFETY MODEL (pure Node + OS-native tools; ZERO new compiled binaries)
//   1. Runner-owned target dir: every cargo step runs with CARGO_TARGET_DIR + --target-dir forced to
//      <repo>/target/gate-runner (override via --target-dir / VERTER_GATE_TARGET_DIR), so the gate's
//      .cargo-lock is fully runner-owned and cleanup can never hit a developer's cargo / rust-analyzer
//      (which write the default target/debug). User target overrides are scrubbed.
//   2. Single-flight mutex: an atomic mkdir lockdir with owner.json + start-identity. A LIVE holder =>
//      REFUSE (LOCK-REFUSED). A dead/stale holder => reclaim via atomic rename (never bare rm of a live
//      holder's dir), defeating PID reuse via process start identity.
//   3. Process containment: POSIX => the step is spawned detached (its own process group, PGID==PID) and
//      reaped with a negative-PGID SIGTERM→grace→SIGKILL (the whole cargo→rustc→test-binary tree inherits
//      the PGID). Windows => `taskkill /PID <pid> /T /F` (tree kill). This is NOT a hostile-code sandbox: a
//      build script that deliberately setsid/daemonizes can escape — the provenance sweep is the backstop
//      (the bash runner has the same limitation).
//   4. Provenance sweep: after any abnormal termination, TERM→KILL any cargo/rustc/cargo-nextest/nextest
//      process whose command line references the RUNNER-OWNED target dir (NOT the repo root), so a
//      developer's interactive cargo / rust-analyzer (which carry the repo root but write target/debug) is
//      never touched.
//   5. Whole-gate hard timeout (default 50m, --timeout) — a deadline for the ENTIRE gate, not per-step. On
//      expiry the active step's tree is reaped + a sweep runs; exit 124.
//   6. Stall detector with SEPARATE build vs test phases:
//        BUILD phase (the archive build): progress = stdout/stderr byte growth OR runner-owned target-tree
//          artifact growth (file-count + newest-mtime, bounded scan). A long silent rustc is NOT a stall.
//        TEST phase (the nextest run + the direct libtest execs): progress = stdout/stderr byte growth
//          ONLY. Target-tree growth is NOT a valid test liveness signal; a silent test binary IS a hang.
//      Default stall 12m (--stall). On stall: reap + sweep; exit 125.
//   7. Spotlight marker (macOS): a <runnerTarget>/.metadata_never_index file is written so Spotlight does
//      not index the build tree (a harmless no-op file on Linux/Windows).
//
// USAGE
//   node scripts/gate.mjs [--timeout 50m] [--stall 12m] [--target-dir <DIR>] [--no-fail-fast]
//                         [--test-threads N]
//   node scripts/gate.mjs --prepare           # warm-pass: archive + list (+ a one-shot warm of the
//                                             # macOS first-launch assessment), NOT counted in a timed gate
//   node scripts/gate.mjs -- <cmd...>         # run an arbitrary bounded command under the same
//                                             # mutex/containment/timeout/stall/teardown (from the repo root)
//     durations: s/m/h suffix or bare seconds (e.g. 50m, 12m, 5s, 90).
//
// EXIT CODES (distinct, documented)
//   0   PASS / PASS-WITH-TOLERATED
//   1   FAIL          (a build/test command failed / a non-tolerated test failed)
//   124 TIMEOUT       (whole-gate wallclock deadline tripped)
//   125 STALL         (no progress within the stall window)
//   126 LOCK-REFUSED  (another gate holds the single-flight mutex and is alive / lock uninspectable)
//   127 USAGE/SETUP   (bad arguments, repo root not found, archive/list setup failure)
//
// ENV VARS HONORED
//   VERTER_GATE_LOCK / MOM_GATE_LOCK   lockdir path (default: OS temp dir keyed by repo realpath)
//   VERTER_GATE_TARGET_DIR             runner-owned target dir (default <repo>/target/gate-runner)
//   CARGO_TARGET_DIR / CARGO_BUILD_TARGET_DIR / CARGO_BUILD_BUILD_DIR are SCRUBBED and forced to the
//     runner-owned dir.

import { spawn, spawnSync } from "node:child_process";
import {
  mkdirSync,
  rmSync,
  writeFileSync,
  readFileSync,
  existsSync,
  renameSync,
  statSync,
  readdirSync,
  realpathSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, basename, sep, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

// ----------------------------------------------------------------------------------------------------
// Exit-code constants
// ----------------------------------------------------------------------------------------------------
const EXIT_PASS = 0;
const EXIT_FAIL = 1;
const EXIT_TIMEOUT = 124;
const EXIT_STALL = 125;
const EXIT_LOCK_REFUSED = 126;
const EXIT_USAGE = 127;

const IS_WINDOWS = process.platform === "win32";
const IS_MAC = process.platform === "darwin";

// ----------------------------------------------------------------------------------------------------
// Tolerated-failure allowlist — EXACT nextest test names (the env-only typeinfo freshness pair). A test
// whose EXACT name is in this set is tolerated; ANY other failure fails the gate. Matched against the
// EXACT name (the final whitespace token of a `FAIL [   …s] <binary> <test::path::name>` line), NOT a
// substring of the line — so a real regression in a differently-named test that merely CONTAINS one of
// these tokens still FAILS, and a name equal to an allowlisted one PLUS a suffix still FAILS.
// ----------------------------------------------------------------------------------------------------
const TOLERATED_TEST_NAMES = new Set([
  // nextest renders "<suite>::<path>"; the suite is its own binary so the bare free-function name is the
  // path. Both the qualified and bare forms are tolerated for the SAME env-only test.
  "typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output",
  "typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output",
  "typeinfo_proto_ts_freshness::proto_ts_bindings_byte_pinned_repo_wide",
  "proto_ts_bindings_byte_pinned_repo_wide",
]);

// ----------------------------------------------------------------------------------------------------
// Logging helpers (all to stderr so a piped JSON capture stays clean).
// ----------------------------------------------------------------------------------------------------
function log(msg) {
  process.stderr.write(`[gate] ${msg}\n`);
}
function warn(msg) {
  process.stderr.write(`[gate][warn] ${msg}\n`);
}
function err(msg) {
  process.stderr.write(`[gate][error] ${msg}\n`);
}

function nowMs() {
  return Date.now();
}
function delay(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// Duration parser: "50m" / "8m" / "5s" / "2h" / bare seconds -> integer seconds.
function parseDuration(d) {
  const s = String(d);
  let n;
  let mult;
  if (s.endsWith("h")) {
    n = s.slice(0, -1);
    mult = 3600;
  } else if (s.endsWith("m")) {
    n = s.slice(0, -1);
    mult = 60;
  } else if (s.endsWith("s")) {
    n = s.slice(0, -1);
    mult = 1;
  } else {
    n = s;
    mult = 1;
  }
  if (!/^\d+$/.test(n)) {
    throw new Error(`invalid duration: '${d}'`);
  }
  return parseInt(n, 10) * mult;
}

// ----------------------------------------------------------------------------------------------------
// Process start-identity (defeats PID reuse). POSIX: `ps -o lstart=`. Windows: CIM CreationDate (or wmic).
// Returns a normalized non-empty string, or "" if the identity is uncheckable (the caller FAILs CLOSED on
// an alive-but-uncheckable holder).
// ----------------------------------------------------------------------------------------------------
function procIdentity(pid) {
  if (!/^\d+$/.test(String(pid))) return "";
  if (IS_WINDOWS) {
    // PowerShell CIM creation date (preferred), falling back to wmic.
    let r = spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        `(Get-CimInstance Win32_Process -Filter 'ProcessId=${pid}').CreationDate`,
      ],
      { encoding: "utf8", windowsHide: true },
    );
    let out = (r.stdout || "").trim();
    if (!out) {
      r = spawnSync("wmic", ["process", "where", `ProcessId=${pid}`, "get", "CreationDate", "/value"], {
        encoding: "utf8",
        windowsHide: true,
      });
      out = (r.stdout || "").trim();
    }
    return out.replace(/\s+/g, " ").trim();
  }
  const r = spawnSync("ps", ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8" });
  return (r.stdout || "").trim().replace(/\s+/g, " ");
}

// Is a pid alive? EPERM ⇒ alive (a process we cannot signal but that exists).
function pidAlive(pid) {
  if (!/^\d+$/.test(String(pid))) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === "EPERM";
  }
}

// ----------------------------------------------------------------------------------------------------
// Negative-PGID reap (POSIX) / taskkill tree (Windows). TERM, grace, KILL. Safe on an already-dead group.
// ----------------------------------------------------------------------------------------------------
async function reapTree(pid, graceMs) {
  if (!pid) return;
  if (IS_WINDOWS) {
    spawnSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], {
      windowsHide: true,
      stdio: "ignore",
    });
    return;
  }
  const pgid = pid;
  const sig = (s) => {
    try {
      process.kill(-pgid, s);
    } catch (e) {
      if (e.code !== "ESRCH") {
        /* swallow EPERM/other: best-effort reap */
      }
    }
  };
  sig("SIGTERM");
  // grace loop
  const deadline = nowMs() + graceMs;
  while (nowMs() < deadline) {
    let alive = false;
    try {
      process.kill(-pgid, 0);
      alive = true;
    } catch (e) {
      alive = e.code === "EPERM";
    }
    if (!alive) return;
    await delay(200);
  }
  sig("SIGKILL");
}

// ----------------------------------------------------------------------------------------------------
// Provenance-filtered sweep: lingering cargo/rustc/cargo-nextest/nextest that reference the runner-owned
// target dir, TERM->KILL. The provenance gate is SOLELY the runner-owned target dir — NOT the repo root,
// because a developer's interactive cargo / rust-analyzer / rustc all carry the repo root in argv but write
// the DEFAULT target/debug, never the gate-runner dir. Conservative: only runner-owned target-dir processes.
// ----------------------------------------------------------------------------------------------------
function listProcesses() {
  // Returns [{ pid, cmd }]. POSIX: `ps -axww -o pid=,command=`. Windows: `wmic process get ...` or CIM.
  if (IS_WINDOWS) {
    let r = spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId)`t$($_.CommandLine)\" }",
      ],
      { encoding: "utf8", windowsHide: true, maxBuffer: 64 * 1024 * 1024 },
    );
    let out = r.stdout || "";
    if (!out.trim()) {
      r = spawnSync("wmic", ["process", "get", "ProcessId,CommandLine", "/format:csv"], {
        encoding: "utf8",
        windowsHide: true,
        maxBuffer: 64 * 1024 * 1024,
      });
      out = r.stdout || "";
    }
    const rows = [];
    for (const line of out.split(/\r?\n/)) {
      const tabIdx = line.indexOf("\t");
      if (tabIdx > 0) {
        const pid = line.slice(0, tabIdx).trim();
        const cmd = line.slice(tabIdx + 1).trim();
        if (/^\d+$/.test(pid)) rows.push({ pid: parseInt(pid, 10), cmd });
        continue;
      }
      // wmic CSV fallback: Node,CommandLine,ProcessId
      const parts = line.split(",");
      if (parts.length >= 3) {
        const pid = parts[parts.length - 1].trim();
        const cmd = parts.slice(1, parts.length - 1).join(",").trim();
        if (/^\d+$/.test(pid)) rows.push({ pid: parseInt(pid, 10), cmd });
      }
    }
    return rows;
  }
  const r = spawnSync("ps", ["-axww", "-o", "pid=,command="], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  const out = r.stdout || "";
  const rows = [];
  for (const line of out.split("\n")) {
    const trimmed = line.replace(/^\s+/, "");
    if (!trimmed) continue;
    const sp = trimmed.indexOf(" ");
    if (sp < 0) continue;
    const pidTok = trimmed.slice(0, sp);
    if (!/^\d+$/.test(pidTok)) continue;
    const cmd = trimmed.slice(sp + 1);
    rows.push({ pid: parseInt(pidTok, 10), cmd });
  }
  return rows;
}

function isBuildTool(cmd) {
  // cargo / rustc / cargo-nextest / nextest — word-ish boundaries so "cargo-nextest" and "/usr/bin/cargo"
  // both match but an unrelated path containing "cargocult" does not. An optional `.exe` suffix is matched
  // so real Windows command lines (`C:\Users\…\.cargo\bin\cargo.exe`, `rustc.exe`, `cargo-nextest.exe`,
  // `nextest.exe`) are recognized. The argv is lowercased first so a mixed-case Windows path matches.
  const c = cmd.toLowerCase();
  return (
    /(^|[\s/\\])cargo-nextest(\.exe)?([\s]|$)/.test(c) ||
    /(^|[\s/\\])cargo(\.exe)?([\s]|$)/.test(c) ||
    /(^|[\s/\\])rustc(\.exe)?([\s]|$)/.test(c) ||
    /(^|[\s/\\])nextest(\.exe)?([\s]|$)/.test(c)
  );
}

// Does a process command line reference the runner-owned target dir? On Windows, command lines and the
// target path can differ in case and in slash direction (`\` vs `/`); normalize both to a lowercase,
// forward-slash form before the containment check so the sweep's "only the runner-owned target dir"
// scoping holds on Windows. On POSIX, paths are case- and separator-stable, so this is the identity.
// `windows` is parameterized (defaulting to the live platform) so the matcher's Windows branch is unit-
// testable on a POSIX host.
function targetDirMatches(cmd, targetDir, windows) {
  if (!targetDir) return false;
  if (windows) {
    const norm = (s) => s.toLowerCase().replace(/\\/g, "/");
    return norm(cmd).includes(norm(targetDir));
  }
  return cmd.includes(targetDir);
}
function cmdReferencesTargetDir(cmd, targetDir) {
  return targetDirMatches(cmd, targetDir, IS_WINDOWS);
}

async function provenanceSweep(targetDir, graceMs) {
  if (!targetDir) return;
  const self = process.pid;
  const term = (pid) => {
    if (IS_WINDOWS) {
      // /T tears down the whole tree (a swept cargo.exe may have spawned rustc.exe children), /F forces it.
      spawnSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], { windowsHide: true, stdio: "ignore" });
    } else {
      try {
        process.kill(pid, "SIGTERM");
      } catch {
        /* ignore */
      }
    }
  };
  const kill = (pid) => {
    if (IS_WINDOWS) {
      spawnSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], { windowsHide: true, stdio: "ignore" });
    } else {
      try {
        process.kill(pid, "SIGKILL");
      } catch {
        /* ignore */
      }
    }
  };
  const matches = () =>
    listProcesses().filter(
      (p) => p.pid !== self && isBuildTool(p.cmd) && cmdReferencesTargetDir(p.cmd, targetDir),
    );
  // TERM pass.
  for (const p of matches()) term(p.pid);
  await delay(Math.min(graceMs, 1500));
  // KILL pass.
  for (const p of matches()) kill(p.pid);
}

// ----------------------------------------------------------------------------------------------------
// Mutex: mkdir lockdir + owner.json + atomic-rename reclaim. NO bare rm of a live holder's dir.
// owner.json = { token, pid, repoRealpath, targetDir, createdAtMs, processStartIdentity }.
// ----------------------------------------------------------------------------------------------------
class Mutex {
  constructor(lockdir, token, ctx) {
    this.lockdir = lockdir;
    this.ownerFile = join(lockdir, "owner.json");
    this.token = token;
    this.ctx = ctx; // { pid, repoRealpath, targetDir }
    this.held = false;
    this.refuseDetail = "";
    this.INIT_GRACE_MS = 5000;
    this.RECLAIM_RACE_RETRIES = 8;
    this.RECLAIM_RACE_BACKOFF_MS = 200;
    this.KILL_GRACE_MS = 5000;
  }

  _writeOwner() {
    const owner = {
      token: this.token,
      pid: this.ctx.pid,
      repoRealpath: this.ctx.repoRealpath,
      targetDir: this.ctx.targetDir,
      createdAtMs: nowMs(),
      processStartIdentity: procIdentity(this.ctx.pid),
    };
    // Heartbeat-style: temp-write + atomic rename so a reader never sees a half-written owner.json.
    const tmp = join(this.lockdir, `owner.json.tmp.${process.pid}`);
    writeFileSync(tmp, JSON.stringify(owner, null, 0));
    renameSync(tmp, this.ownerFile);
  }

  _readOwner() {
    try {
      return JSON.parse(readFileSync(this.ownerFile, "utf8"));
    } catch {
      return null;
    }
  }

  _lockdirBirthMs() {
    try {
      return statSync(this.lockdir).mtimeMs;
    } catch {
      return nowMs(); // un-inspectable => treat as fresh (SAFE side: do not reclaim)
    }
  }

  // Reclaim a dead/stale lock via atomic rename. Returns true if we won the reclaim.
  _reclaim() {
    const stale = `${this.lockdir}.stale.${this.token}`;
    try {
      renameSync(this.lockdir, stale);
    } catch {
      return false; // lost the race to a concurrent reclaimer
    }
    try {
      rmSync(stale, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
    return true;
  }

  async acquire() {
    let reclaimRaces = 0;
    for (;;) {
      // Try to win the slot. mkdir with recursive:false is atomic (EEXIST on contention).
      try {
        mkdirSync(this.lockdir, { recursive: false });
        this._writeOwner();
        this.held = true;
        return true;
      } catch (e) {
        if (e.code !== "EEXIST") {
          // e.g. parent dir missing — make the parent and retry once.
          if (e.code === "ENOENT") {
            mkdirSync(dirname(this.lockdir), { recursive: true });
            continue;
          }
          throw e;
        }
      }
      // Held. Classify the holder.
      const owner = this._readOwner();
      if (!owner) {
        // Lockdir exists but no readable owner.json yet => still INITIALIZING. Refuse until past the init
        // grace; only then (if STILL unreadable) is it a crashed mid-init lock to reclaim.
        const ageMs = nowMs() - this._lockdirBirthMs();
        if (ageMs < this.INIT_GRACE_MS) {
          this.refuseDetail = `initializing, no owner.json, age=${Math.round(ageMs / 1000)}s < ${Math.round(this.INIT_GRACE_MS / 1000)}s grace`;
          return false;
        }
        // Past grace, still no owner.json => crashed mid-init. Reclaim.
        if (this._reclaim()) continue;
        reclaimRaces++;
        if (reclaimRaces >= this.RECLAIM_RACE_RETRIES) {
          this.refuseDetail = `could not reclaim a crashed mid-init lock after ${reclaimRaces} attempts`;
          return false;
        }
        await delay(this.RECLAIM_RACE_BACKOFF_MS);
        continue;
      }
      const holderPid = owner.pid;
      const holderIdent = owner.processStartIdentity || "";
      if (holderPid && pidAlive(holderPid)) {
        // FAIL CLOSED: an alive holder PID is reclaimed ONLY when PID reuse is PROVEN — i.e. BOTH the
        // stored start-identity and the live start-identity are non-empty AND they differ (the old PID
        // exited and the OS handed its number to an unrelated process). In every other alive case —
        // matching identities, a missing/empty stored identity, an uncheckable live identity, or any
        // identity we cannot positively distinguish — we REFUSE. Reclaiming a live lock would let two gates
        // run concurrently, which is worse than a manual cleanup, so an empty/uncheckable identity is
        // NEVER treated as evidence of PID reuse.
        const liveIdent = procIdentity(holderPid);
        const proveReuse = holderIdent && liveIdent && holderIdent !== liveIdent;
        if (!proveReuse) {
          const ageS = Math.round((nowMs() - (owner.createdAtMs || this._lockdirBirthMs())) / 1000);
          if (holderIdent && liveIdent) {
            // Identities both present and equal => genuinely the same live holder.
            this.refuseDetail = `live holder pid=${holderPid} age=${ageS}s targetDir=${owner.targetDir || "?"}`;
          } else {
            // One or both identities empty/uncheckable while the PID is alive => fail-closed refusal.
            this.refuseDetail =
              `holder pid=${holderPid} appears alive but PID reuse cannot be ruled out ` +
              `(stored-identity=${holderIdent ? "present" : "missing"}, ` +
              `live-identity=${liveIdent ? "present" : "uncheckable"}) — refusing (fail-closed)`;
          }
          return false;
        }
        // Both identities present and DIFFERENT => proven PID reuse; treat as stale and reclaim.
        warn(`lock pid=${holderPid} reused by an unrelated process (identity mismatch) => reclaiming`);
      } else {
        warn(`lock holder pid=${holderPid} is dead/stale => reclaiming`);
      }
      if (this._reclaim()) continue;
      reclaimRaces++;
      if (reclaimRaces >= this.RECLAIM_RACE_RETRIES) {
        this.refuseDetail = `could not acquire lock after ${reclaimRaces} reclaim-race attempts`;
        return false;
      }
      await delay(this.RECLAIM_RACE_BACKOFF_MS);
    }
  }

  release() {
    if (!this.held) return;
    const owner = this._readOwner();
    if (owner && owner.token === this.token) {
      const rel = `${this.lockdir}.release.${this.token}`;
      try {
        renameSync(this.lockdir, rel);
        rmSync(rel, { recursive: true, force: true });
      } catch {
        /* ignore */
      }
    }
    this.held = false;
  }
}

// ----------------------------------------------------------------------------------------------------
// Artifact-progress signature: a cheap fingerprint of the runner-owned target tree that CHANGES while a
// cold build lands .o/.rlib/.d artifacts even when the log emits zero bytes. "<file-count>:<newest-mtime>"
// over files modified in the last ~2 minutes, bounded so the probe is O(seconds). BUILD-phase signal ONLY.
// ----------------------------------------------------------------------------------------------------
function artifactSignature(dir) {
  if (!dir || !existsSync(dir)) return "0:0";
  const cutoff = nowMs() - 2 * 60 * 1000;
  let count = 0;
  let newest = 0;
  const MAX = 5000;
  const stack = [dir];
  while (stack.length && count < MAX) {
    const cur = stack.pop();
    let entries;
    try {
      entries = readdirSync(cur, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const ent of entries) {
      if (count >= MAX) break;
      const full = join(cur, ent.name);
      if (ent.isDirectory()) {
        stack.push(full);
      } else if (ent.isFile()) {
        let st;
        try {
          st = statSync(full);
        } catch {
          continue;
        }
        if (st.mtimeMs >= cutoff) {
          count++;
          if (st.mtimeMs > newest) newest = st.mtimeMs;
        }
      }
    }
  }
  return `${count}:${Math.floor(newest)}`;
}

// ----------------------------------------------------------------------------------------------------
// runContainedStep — launch one external command in a NEW process group (POSIX) / job-tree (Windows) under
// the whole-gate deadline + the phase-appropriate stall detector, capturing combined stdout+stderr to a
// growing buffer (also mirrored to our stderr). Returns { code, reason, durationMs, stdout, stderr }.
//   reason: "TIMEOUT" | "STALL" | "" (empty when not a watchdog kill).
//
//   phase: "build" => progress is byte growth OR target-tree artifact growth.
//          "test"  => progress is byte growth ONLY (a silent test binary is a hang).
//
//   deadlineMs: the WHOLE-GATE absolute deadline (ms epoch). The step is bounded by it; when it passes the
//               step is reaped as TIMEOUT. (The same deadline is shared across every step so the budget is
//               whole-gate, not per-step.)
// ----------------------------------------------------------------------------------------------------
async function runContainedStep(opts) {
  const {
    cmd,
    args,
    cwd,
    env,
    phase,
    deadlineMs,
    stallMs,
    targetDir,
    killGraceMs = 5000,
    captureStdoutSeparately = false,
  } = opts;

  const child = spawn(cmd, args, {
    cwd,
    env,
    shell: false,
    detached: !IS_WINDOWS, // POSIX: new process group (setsid). Windows: taskkill /T is the tree primitive.
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });

  let stdoutBuf = "";
  let stderrBuf = "";
  let totalBytes = 0;
  let lastGrowthMs = nowMs();
  let lastSize = -1;
  let lastArtifact = "";

  child.stdout.on("data", (d) => {
    const s = d.toString();
    totalBytes += d.length;
    if (captureStdoutSeparately) {
      stdoutBuf += s;
    } else {
      stdoutBuf += s;
      process.stderr.write(s);
    }
  });
  child.stderr.on("data", (d) => {
    const s = d.toString();
    totalBytes += d.length;
    stderrBuf += s;
    process.stderr.write(s);
  });

  let reason = "";
  let reaped = false;
  const startMs = nowMs();

  const reapNow = async (why) => {
    if (reaped) return;
    reaped = true;
    reason = why;
    await reapTree(child.pid, killGraceMs);
    await provenanceSweep(targetDir, killGraceMs);
  };

  // Watchdog: owns BOTH the whole-gate deadline and the phase stall detector.
  const watchdog = setInterval(() => {
    const cur = nowMs();
    // Whole-gate hard deadline.
    if (deadlineMs > 0 && cur >= deadlineMs) {
      void reapNow("TIMEOUT");
      return;
    }
    // Stall.
    if (stallMs > 0) {
      const size = totalBytes;
      let artifact = "";
      if (phase === "build") artifact = artifactSignature(targetDir);
      if (size !== lastSize || artifact !== lastArtifact) {
        lastSize = size;
        lastArtifact = artifact;
        lastGrowthMs = cur;
      } else if (cur - lastGrowthMs >= stallMs) {
        void reapNow("STALL");
      }
    }
  }, 1000);

  const code = await new Promise((resolve) => {
    child.on("error", () => resolve(127));
    child.on("close", (c, signal) => {
      if (c === null && signal) resolve(128);
      else resolve(c === null ? 1 : c);
    });
  });

  clearInterval(watchdog);
  // One-tick race: the 1s watchdog can set `reason` (TIMEOUT/STALL) in the same tick the child cleanly
  // exits 0, before `close` resolves. A genuine deadline/stall reap kills with SIGKILL, so the close code
  // is non-zero/null (128 via signal); a clean exit-0 means the step actually completed in time. Only
  // honor `reason` when the close code is non-zero/null, so a step that finished cleanly is never mapped
  // to a spurious TIMEOUT/STALL.
  if (reason && code === 0) {
    reason = "";
  }
  // If the watchdog tripped (a real reap), make sure the tree + strays are gone before we return.
  if (reason) {
    await reapTree(child.pid, killGraceMs);
    await provenanceSweep(targetDir, killGraceMs);
  }

  const durationMs = nowMs() - startMs;
  return { code, reason, durationMs, stdout: stdoutBuf, stderr: stderrBuf };
}

// ----------------------------------------------------------------------------------------------------
// nextest result-line parsing.
//
// nextest prints one terminal status per test: "    <STATUS> [   0.123s] <binary> <test::path::name>".
// A plain assertion failure is `FAIL`, but a CRASH renders under a DIFFERENT status — a signal abort
// (`SIGABRT` / `SIGSEGV` / `SIGBUS` / `SIGILL` / `SIGFPE` / `ABORT`), a leak (`LEAK` under
// leak-fail-mode / `LEAK-FAIL`), or a `TIMEOUT`. Those are NOT `FAIL` lines yet nextest still counts them
// in its summary `failed` total and exits non-zero. Parsing only `FAIL [` would let an aborting/leaking
// test in ANY crate pass the gate green, so the live SURFACE-1 path treats the summary `failed` count +
// the run exit code as authoritative (see runGate), and the classifier below recognizes the non-`FAIL`
// failure statuses too so the testable `--selftest-classify-nextest` hook agrees with the live path.
// ----------------------------------------------------------------------------------------------------

// Terminal status tokens nextest uses for a FAILED test (anything that is not PASS and counts toward the
// summary `failed` total). Informational prefixes (SLOW / TRY / RETRY / START / SETUP) are NOT terminal
// failure statuses and are excluded.
const NEXTEST_FAILURE_STATUSES = new Set([
  "FAIL",
  "ABORT",
  "SIGABRT",
  "SIGSEGV",
  "SIGBUS",
  "SIGILL",
  "SIGFPE",
  "SIGHUP",
  "SIGINT",
  "SIGQUIT",
  "SIGTERM",
  "SIGKILL",
  "LEAK",
  "LEAK-FAIL",
  "TIMEOUT",
]);

// All failed-test names from a nextest log, across EVERY terminal failure status (not just `FAIL`).
// Returns the EXACT test name (final whitespace token after the timing bracket) for each failure line.
function extractNextestFailureStatusNames(text) {
  const names = [];
  for (const line of text.split("\n")) {
    const m = /^\s*([A-Z][A-Z-]*) \[/.exec(line);
    if (!m) continue;
    if (!NEXTEST_FAILURE_STATUSES.has(m[1])) continue;
    const idx = line.indexOf("] ");
    if (idx < 0) continue;
    const after = line.slice(idx + 2).trim();
    if (!after) continue;
    const parts = after.split(/\s+/);
    names.push(parts[parts.length - 1]);
  }
  return names;
}

// The EXACT failed-test names from the plain `FAIL [` lines only — the names the tolerated-allowlist
// accounting operates over on the live path. A crash status (SIGABRT/LEAK/…) is intentionally NOT in this
// set: a crash is never tolerated, and it is surfaced via the summary-count tripwire.
function extractNextestFailedNames(text) {
  const names = [];
  for (const line of text.split("\n")) {
    if (!/^\s*FAIL \[/.test(line)) continue;
    // Drop everything up to and including the "] " that closes the timing bracket, then take the LAST
    // whitespace token.
    const idx = line.indexOf("] ");
    if (idx < 0) continue;
    const after = line.slice(idx + 2).trim();
    if (!after) continue;
    const parts = after.split(/\s+/);
    names.push(parts[parts.length - 1]);
  }
  return names;
}

// Classify a nextest log's failures (used by the `--selftest-classify-nextest` hook so the testable path
// mirrors the live SURFACE-1 verdict). It recognizes the SAME non-`FAIL` failure statuses + summary-count
// tripwire the live path uses:
//   "regression" — >=1 NON-`FAIL` failure status line (a crash/leak/timeout is never tolerated), OR the
//                  summary `failed` count exceeds the accounted `FAIL` names (an unaccounted failure
//                  class), OR >=1 parsed `FAIL` name is not allowlisted.
//   "tolerated"  — >=1 `FAIL` line, EVERY parsed `FAIL` name is an EXACT allowlisted name, NO non-`FAIL`
//                  failure status line, and the summary count does not exceed the accounted names.
//   "none"       — no failure status lines parsed AND the summary reports zero failures.
function classifyNextestFailures(text) {
  const failNames = extractNextestFailedNames(text);
  const allFailureNames = extractNextestFailureStatusNames(text);
  // A non-`FAIL` failure status (SIGABRT/SIGSEGV/LEAK/TIMEOUT/…) is present whenever the broader scan
  // finds more failure lines than the `FAIL`-only scan — those extras are crashes, never tolerated.
  const nonFailFailures = allFailureNames.length - failNames.length;
  const summary = parseNextestSummary(text);
  const unaccounted = summary.failed - failNames.length;
  if (nonFailFailures > 0 || unaccounted > 0) return "regression";
  if (failNames.length === 0) return "none";
  for (const nm of failNames) {
    if (!TOLERATED_TEST_NAMES.has(nm)) return "regression";
  }
  return "tolerated";
}

// The SHARED SURFACE-1 verdict logic. The live gate (runGate) and the `--selftest-classify-nextest-run`
// hook both call this so the testable path is byte-identical to the live aggregation. Given a nextest log
// + the run exit code, it returns the non-tolerated `failures` (each {surface,name}), the tolerated count,
// and the parsed summary. The load-bearing rule: a crash renders under a NON-`FAIL` status and a nextest
// setup/harness error exits non-zero with NO `FAIL [` line — both would pass green if only `FAIL [` lines
// were trusted. The summary `failed` total counts every failure class, so any shortfall vs the accounted
// `FAIL` names is an unaccounted failure; trip a hard failure when the run exited non-zero AND (there is
// such a shortfall OR no `FAIL` name parsed at all). The tolerated env pair has summary.failed == the two
// accounted `FAIL` names, so unaccounted == 0 and this never fires for it.
function analyzeNextestSurface(text, code) {
  const failures = [];
  let toleratedCount = 0;
  const failNames = [...new Set(extractNextestFailedNames(text))];
  const summary = parseNextestSummary(text);
  for (const nm of failNames) {
    if (TOLERATED_TEST_NAMES.has(nm)) toleratedCount++;
    else failures.push({ surface: "nextest", name: nm });
  }
  const unaccounted = summary.failed - failNames.length;
  if (code !== 0 && (unaccounted > 0 || failNames.length === 0)) {
    failures.push({
      surface: "nextest",
      name: `<run exit ${code}; ${unaccounted > 0 ? unaccounted : "0"} non-FAIL-status failure(s); summary failed=${summary.failed}>`,
    });
  }
  return { failures, toleratedCount, summary, namedCount: failNames.length, unaccounted };
}

// ----------------------------------------------------------------------------------------------------
// libtest stdout parsing — the EXACT failed-test names from a direct `cargo test`-style binary run.
// libtest prints a trailing "failures:\n    <name>\n    <name>\n" block; also each failing test emits
// "test <name> ... FAILED". We parse the "test … FAILED" lines (stable across libtest versions).
// ----------------------------------------------------------------------------------------------------
function extractLibtestFailedNames(text) {
  const names = [];
  for (const line of text.split("\n")) {
    const m = /^test\s+(.+?)\s+\.\.\.\s+FAILED\s*$/.exec(line);
    if (m) names.push(m[1]);
  }
  return names;
}

// ----------------------------------------------------------------------------------------------------
// Resolve a suite binary path from a nextest archive listing. With `--extract-to <dir>`, nextest rewrites
// `binary-path` to the extract location. We defend against either layout: if the listed path exists, use
// it; else try rebasing the `target-directory`-relative tail under the extract dir.
// ----------------------------------------------------------------------------------------------------
function resolveSuiteBinary(binaryPath, buildMetaTargetDir, extractDir) {
  if (binaryPath && existsSync(binaryPath)) return binaryPath;
  // Rebase: binaryPath is typically "<target-directory>/debug/deps/<bin>"; strip the leading
  // target-directory and re-root under <extractDir>/target.
  if (buildMetaTargetDir && binaryPath && binaryPath.startsWith(buildMetaTargetDir)) {
    let tail = binaryPath.slice(buildMetaTargetDir.length);
    if (tail.startsWith(sep) || tail.startsWith("/") || tail.startsWith("\\")) tail = tail.slice(1);
    const candidate = join(extractDir, "target", tail);
    if (existsSync(candidate)) return candidate;
    const candidate2 = join(extractDir, tail);
    if (existsSync(candidate2)) return candidate2;
  }
  // Last resort: search the extract dir for a deps binary with the same basename.
  if (binaryPath) {
    const want = basename(binaryPath);
    const found = findFileByName(extractDir, want, 8);
    if (found) return found;
  }
  return binaryPath; // give back the original; the exec will fail loudly if it does not exist
}

function findFileByName(root, name, maxDepth) {
  if (!existsSync(root)) return null;
  const stack = [{ dir: root, depth: 0 }];
  while (stack.length) {
    const { dir, depth } = stack.pop();
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const ent of entries) {
      const full = join(dir, ent.name);
      if (ent.isFile() && ent.name === name) return full;
      if (ent.isDirectory() && depth < maxDepth) stack.push({ dir: full, depth: depth + 1 });
    }
  }
  return null;
}

// ----------------------------------------------------------------------------------------------------
// Extract the trailing JSON object from a nextest `--message-format json` stdout capture. nextest emits a
// single JSON object on stdout (build/compile progress goes to STDERR), but a defensive parse handles a
// capture that prepended log noise: find the first '{' at column 0 (or the first '{'), parse to EOF.
// ----------------------------------------------------------------------------------------------------
function parseNextestListJson(stdout) {
  const trimmed = stdout.trim();
  // Fast path: the whole capture is the JSON object.
  try {
    return JSON.parse(trimmed);
  } catch {
    /* fall through to a tolerant scan */
  }
  // Tolerant: find the first '{' and parse the balanced object from there.
  const start = trimmed.indexOf("{");
  if (start < 0) throw new Error("no JSON object found in nextest list output");
  // Walk braces honoring strings to find the matching close.
  let depth = 0;
  let inStr = false;
  let escape = false;
  for (let i = start; i < trimmed.length; i++) {
    const ch = trimmed[i];
    if (inStr) {
      if (escape) escape = false;
      else if (ch === "\\") escape = true;
      else if (ch === '"') inStr = false;
      continue;
    }
    if (ch === '"') inStr = true;
    else if (ch === "{") depth++;
    else if (ch === "}") {
      depth--;
      if (depth === 0) {
        const obj = trimmed.slice(start, i + 1);
        return JSON.parse(obj);
      }
    }
  }
  throw new Error("unbalanced JSON object in nextest list output");
}

// ----------------------------------------------------------------------------------------------------
// Setup: repo root, runner target dir, lock path, env.
// ----------------------------------------------------------------------------------------------------
function resolveRepoRoot(scriptDir) {
  const r = spawnSync("git", ["-C", scriptDir, "rev-parse", "--show-toplevel"], { encoding: "utf8" });
  const top = (r.stdout || "").trim();
  if (top) {
    try {
      return realpathSync(top);
    } catch {
      return top;
    }
  }
  return "";
}

function defaultLockDir(repoRealpath) {
  // OS temp dir keyed by repo realpath hash (cross-platform, stable per checkout).
  const h = createHash("sha256").update(repoRealpath).digest("hex").slice(0, 16);
  return join(tmpdir(), `verter-gate.lock.${h}.d`);
}

// ----------------------------------------------------------------------------------------------------
// Build the cargo env: scrub user target overrides, force the runner-owned dir + non-TTY output.
// ----------------------------------------------------------------------------------------------------
function buildCargoEnv(baseEnv, runnerTarget) {
  const env = { ...baseEnv };
  delete env.CARGO_TARGET_DIR;
  delete env.CARGO_BUILD_TARGET_DIR;
  delete env.CARGO_BUILD_BUILD_DIR;
  env.CARGO_TARGET_DIR = runnerTarget;
  // Force non-TTY / CI-style output so progress lands in the captured log, not a TTY spinner.
  env.CARGO_TERM_COLOR = "never";
  env.CARGO_TERM_PROGRESS_WHEN = "never";
  env.NEXTEST_HIDE_PROGRESS_BAR = "1";
  return env;
}

// ----------------------------------------------------------------------------------------------------
// Per-suite package identity, derived ENTIRELY from the nextest archive list JSON we already parsed inside
// the contained/watchdogged list step — `package-name` and `package-id` (the part after `#` is the
// semver). This deliberately avoids a SEPARATE `cargo metadata` subprocess: a second synchronous cargo
// call would run OUTSIDE the whole-gate watchdog, the process containment, and the scrubbed/runner-owned
// cargo env, so a hang in it would bypass the gate deadline. The list JSON already carries everything the
// direct-libtest env needs.
// ----------------------------------------------------------------------------------------------------
function deriveSuitePkgInfo(suite) {
  const name = suite["package-name"] || "";
  // package-id forms: "path+file:///…/crates/verter_session#0.0.1-beta.1" (version after the LAST '#'),
  // or the older "verter_session 0.0.1-beta.1 (path+file://…)" form. Extract the semver defensively.
  const id = suite["package-id"] || "";
  let version = "";
  const hash = id.lastIndexOf("#");
  if (hash >= 0) {
    const tail = id.slice(hash + 1);
    // "name@version" or just "version".
    const at = tail.lastIndexOf("@");
    version = at >= 0 ? tail.slice(at + 1) : tail;
  } else {
    const m = /\s(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)\s/.exec(` ${id} `);
    if (m) version = m[1];
  }
  return { name, version };
}

// ----------------------------------------------------------------------------------------------------
// SURFACE-2 suite selection + integrity gate (shared by runGate and the `--selftest-surface2` hook). The
// filter mirrors `cargo test -p verter_session --tests`: the lib unit-test binary + every `tests/*.rs`
// integration binary, i.e. kind ∈ {lib, test}; bins/benches are excluded. SURFACE 2 IS the shared-process
// surface — the whole reason this gate exists — so a filter/archive regression that finds NOTHING must NOT
// let the gate quietly pass on surface 1 alone. Returns `{ suites, lib, test, error }`: `error` is a
// non-null setup-failure message when zero suites are found OR a kind is missing (verter_session always
// has exactly one `lib` plus its integration `test` targets, so we assert >=1 of EACH — a partial filter
// that keeps only one kind is surfaced as a regression, not passed as a half-covered surface).
// ----------------------------------------------------------------------------------------------------
function selectSessionSuites(allSuites) {
  const suites = (allSuites || []).filter(
    (s) => s["package-name"] === "verter_session" && (s.kind === "lib" || s.kind === "test"),
  );
  const lib = suites.filter((s) => s.kind === "lib").length;
  const test = suites.filter((s) => s.kind === "test").length;
  let error = null;
  if (suites.length === 0) {
    error =
      "zero verter_session lib/test suites found in the archive listing — the shared-process surface " +
      "would be silently skipped. Refusing to pass on surface 1 alone.";
  } else if (lib < 1 || test < 1) {
    error =
      `verter_session suite filter is incomplete (lib=${lib}, test=${test}; expected >=1 of each). ` +
      "A partial filter would under-cover the shared-process surface. Refusing to pass.";
  }
  return { suites, lib, test, error };
}

// Per-package Cargo env for a DIRECTLY-executed test binary. This injects the runtime Cargo env the
// verter_session integration tests ACTUALLY read — CARGO_MANIFEST_DIR and CARGO_TARGET_DIR — verified
// complete for this suite (the only runtime `std::env::var(_os)` Cargo lookups in the verter_session test
// sources are `CARGO_MANIFEST_DIR` and `CARGO_TARGET_DIR`; `CARGO_TARGET_DIR` is already forced on the base
// cargo env to the runner-owned dir, and the manifest dir IS the suite cwd). It does NOT claim to
// reproduce the FULL env Cargo passes (it omits e.g. dynamic-library search-path setup and per-test
// tmp/bin vars) — only the subset this suite reads. The CARGO_PKG_NAME/VERSION pair is a faithful extra
// derived from the same archive list JSON (NOT a subprocess); it is not load-bearing for this suite.
function buildSuiteEnv(baseCargoEnv, manifestDir, pkgInfo, crateName) {
  const env = { ...baseCargoEnv };
  // Load-bearing: the package manifest dir Cargo sets for the test process (tests read it via
  // std::env::var("CARGO_MANIFEST_DIR") to resolve the repo root + corpus fixtures). cwd IS the manifest
  // dir. CARGO_TARGET_DIR is already present on baseCargoEnv (forced to the runner-owned target).
  env.CARGO_MANIFEST_DIR = manifestDir;
  if (crateName) env.CARGO_CRATE_NAME = crateName.replace(/-/g, "_");
  if (pkgInfo) {
    if (pkgInfo.name) env.CARGO_PKG_NAME = pkgInfo.name;
    if (pkgInfo.version) {
      env.CARGO_PKG_VERSION = pkgInfo.version;
      const m = /^(\d+)\.(\d+)\.(\d+)(?:[-+](.*))?$/.exec(String(pkgInfo.version));
      env.CARGO_PKG_VERSION_MAJOR = m ? m[1] : "";
      env.CARGO_PKG_VERSION_MINOR = m ? m[2] : "";
      env.CARGO_PKG_VERSION_PATCH = m ? m[3] : "";
      env.CARGO_PKG_VERSION_PRE = m && m[4] ? m[4] : "";
    }
  }
  return env;
}

// ----------------------------------------------------------------------------------------------------
// MAIN
// ----------------------------------------------------------------------------------------------------
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

// Self-test classify hook (cargo-free): `--selftest-classify-nextest <fixture>` prints exactly one of
// PASS | PASS-WITH-TOLERATED | FAIL for a canned nextest-style log. Drives the REAL classifier + verdict
// mapping the canonical loop uses. Pure test scaffolding: touches NO mutex/process-group/target dir.
{
  const av = process.argv.slice(2);
  if (av[0] === "--selftest-classify-nextest") {
    const fixture = av[1];
    if (!fixture || !existsSync(fixture)) {
      process.stdout.write("FAIL\n");
      process.exit(1);
    }
    const text = readFileSync(fixture, "utf8");
    // Route EVERY fixture through the real classifier — do NOT pre-gate on the presence of a `FAIL [`
    // line. A crash/leak/timeout (SIGABRT/SIGSEGV/LEAK/TIMEOUT/…) or an unaccounted summary failure carries
    // no `FAIL [` line yet must classify as a regression, exactly as the live SURFACE-1 path treats it.
    const cls = classifyNextestFailures(text);
    if (cls === "regression") {
      process.stdout.write("FAIL\n");
      process.exit(1);
    }
    if (cls === "tolerated") {
      process.stdout.write("PASS-WITH-TOLERATED\n");
      process.exit(0);
    }
    process.stdout.write("PASS\n");
    process.exit(0);
  }
  // Self-test LIVE-aggregation hook: `--selftest-classify-nextest-run <exitCode> <fixture>` drives the
  // EXACT `analyzeNextestSurface(text, code)` the live SURFACE-1 path calls and prints PASS |
  // PASS-WITH-TOLERATED | FAIL. Unlike the classifier hook, this takes the run exit code so the
  // non-zero-exit-with-no-FAIL tripwire is testable. Pure scaffolding: no mutex/process-group/target dir.
  if (av[0] === "--selftest-classify-nextest-run") {
    const code = parseInt(av[1], 10);
    const fixture = av[2];
    if (!fixture || !existsSync(fixture) || Number.isNaN(code)) {
      process.stdout.write("FAIL\n");
      process.exit(1);
    }
    const text = readFileSync(fixture, "utf8");
    const r = analyzeNextestSurface(text, code);
    if (r.failures.length > 0) {
      process.stdout.write("FAIL\n");
      process.exit(1);
    }
    if (r.toleratedCount > 0) {
      process.stdout.write("PASS-WITH-TOLERATED\n");
      process.exit(0);
    }
    process.stdout.write("PASS\n");
    process.exit(0);
  }
  // Self-test SURFACE-2 integrity hook: `--selftest-surface2 <suites.json>` runs the REAL
  // selectSessionSuites() gate over a canned `allSuites` array and exits with the SAME contract the live
  // path uses — 127 (USAGE/SETUP) when the integrity gate trips (zero suites / missing kind), 0 with an
  // `OK lib=<n> test=<n>` line otherwise. Pure scaffolding: no mutex/process-group/target dir/cargo.
  if (av[0] === "--selftest-surface2") {
    const fixture = av[1];
    if (!fixture || !existsSync(fixture)) {
      process.stderr.write("missing suites.json\n");
      process.exit(EXIT_USAGE);
    }
    let allSuites;
    try {
      allSuites = JSON.parse(readFileSync(fixture, "utf8"));
    } catch (e) {
      process.stderr.write(`bad suites.json: ${e.message}\n`);
      process.exit(EXIT_USAGE);
    }
    const sel = selectSessionSuites(allSuites);
    if (sel.error) {
      process.stderr.write(`SETUP FAILURE: ${sel.error}\n`);
      process.exit(EXIT_USAGE);
    }
    process.stdout.write(`OK lib=${sel.lib} test=${sel.test}\n`);
    process.exit(0);
  }
  // Self-test provenance-sweep matcher hook (pure regex/classify; NO Windows host needed):
  // `--selftest-sweep-match <posix|windows> <targetDir> -- <command line…>` prints MATCH | NOMATCH for the
  // REAL sweep predicate `isBuildTool(cmd) && targetDirMatches(cmd, targetDir, windows)`. Lets the harness
  // assert that a Windows `cargo.exe`/`rustc.exe` command line referencing the runner target dir MATCHES,
  // while a repo-root-only dev `cargo.exe` does NOT. Pure scaffolding: no mutex/process-group/cargo.
  if (av[0] === "--selftest-sweep-match") {
    const plat = av[1];
    const targetDir = av[2];
    const dd = av.indexOf("--");
    const cmd = dd >= 0 ? av.slice(dd + 1).join(" ") : "";
    if ((plat !== "posix" && plat !== "windows") || !targetDir || dd < 0) {
      process.stderr.write("usage: --selftest-sweep-match <posix|windows> <targetDir> -- <cmd…>\n");
      process.exit(EXIT_USAGE);
    }
    const windows = plat === "windows";
    const swept = isBuildTool(cmd) && targetDirMatches(cmd, targetDir, windows);
    process.stdout.write(swept ? "MATCH\n" : "NOMATCH\n");
    process.exit(0);
  }
}

function parseArgs(argv) {
  const opts = {
    mode: "gate", // gate | prepare | custom
    timeoutSecs: parseDuration("50m"),
    stallSecs: parseDuration("12m"),
    targetDir: process.env.VERTER_GATE_TARGET_DIR || "",
    noFailFast: true,
    testThreads: null,
    customCmd: [],
  };
  let i = 0;
  while (i < argv.length) {
    const a = argv[i];
    if (a === "--timeout") {
      opts.timeoutSecs = parseDuration(argv[++i]);
    } else if (a === "--stall") {
      opts.stallSecs = parseDuration(argv[++i]);
    } else if (a === "--target-dir") {
      opts.targetDir = argv[++i];
    } else if (a === "--no-fail-fast") {
      opts.noFailFast = true;
    } else if (a === "--test-threads") {
      opts.testThreads = argv[++i];
    } else if (a === "--prepare") {
      opts.mode = "prepare";
    } else if (a === "--") {
      opts.mode = "custom";
      opts.customCmd = argv.slice(i + 1);
      break;
    } else if (a === "-h" || a === "--help") {
      opts.mode = "help";
      break;
    } else {
      throw new Error(`unknown argument: '${a}' (did you mean to put a command after --?)`);
    }
    i++;
  }
  return opts;
}

async function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (e) {
    err(e.message);
    process.exit(EXIT_USAGE);
  }

  if (opts.mode === "help") {
    process.stderr.write(
      readFileSync(fileURLToPath(import.meta.url), "utf8")
        .split("\n")
        .filter((l) => l.startsWith("//"))
        .map((l) => l.replace(/^\/\/ ?/, ""))
        .join("\n") + "\n",
    );
    process.exit(EXIT_PASS);
  }

  const repoRealpath = resolveRepoRoot(SCRIPT_DIR);
  if (!repoRealpath) {
    err(`could not determine repo root (git rev-parse failed from ${SCRIPT_DIR})`);
    process.exit(EXIT_USAGE);
  }

  const runnerTarget = opts.targetDir
    ? isAbsolute(opts.targetDir)
      ? opts.targetDir
      : join(repoRealpath, opts.targetDir)
    : join(repoRealpath, "target", "gate-runner");

  // Gate work dir (archive, list JSON, extract) lives under the runner target dir.
  const gateDir = join(runnerTarget, "gate-work");

  const lockdir =
    process.env.VERTER_GATE_LOCK || process.env.MOM_GATE_LOCK || defaultLockDir(repoRealpath);

  const token = `${process.pid}.${nowMs()}.${Math.floor(Math.random() * 1e9)}`;
  const cargoEnv = buildCargoEnv(process.env, runnerTarget);

  // Ensure the runner target dir exists + drop the Spotlight marker (macOS) — harmless no-op file elsewhere.
  mkdirSync(runnerTarget, { recursive: true });
  try {
    writeFileSync(join(runnerTarget, ".metadata_never_index"), "");
  } catch {
    /* ignore */
  }

  const mutex = new Mutex(lockdir, token, {
    pid: process.pid,
    repoRealpath,
    targetDir: runnerTarget,
  });

  // Teardown — idempotent. Release the mutex (token-checked) + a final provenance sweep.
  let teardownDone = false;
  const teardown = async () => {
    if (teardownDone) return;
    teardownDone = true;
    try {
      await provenanceSweep(runnerTarget, mutex.KILL_GRACE_MS);
    } catch {
      /* ignore */
    }
    mutex.release();
  };
  const installSignalTraps = () => {
    process.on("SIGINT", async () => {
      await teardown();
      process.exit(130);
    });
    process.on("SIGTERM", async () => {
      await teardown();
      process.exit(143);
    });
  };
  installSignalTraps();

  // Acquire the single-flight mutex FIRST.
  let acquired = false;
  try {
    acquired = await mutex.acquire();
  } catch (e) {
    err(`mutex error: ${e.message}`);
    await teardown();
    process.exit(EXIT_USAGE);
  }
  if (!acquired) {
    err(`LOCK-REFUSED: ${mutex.refuseDetail} (lockdir=${lockdir})`);
    await teardown();
    process.exit(EXIT_LOCK_REFUSED);
  }
  log(`mutex acquired (token=${token} lockdir=${lockdir})`);
  log(`runner target dir: ${runnerTarget}`);

  const deadlineMs = nowMs() + opts.timeoutSecs * 1000;
  const stallMs = opts.stallSecs * 1000;

  let exitCode = EXIT_PASS;
  try {
    if (opts.mode === "custom") {
      exitCode = await runCustom(opts, { cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs });
    } else if (opts.mode === "prepare") {
      exitCode = await runPrepare({ cargoEnv, repoRealpath, runnerTarget, gateDir, deadlineMs, stallMs });
    } else {
      exitCode = await runGate(opts, { cargoEnv, repoRealpath, runnerTarget, gateDir, deadlineMs, stallMs });
    }
  } catch (e) {
    err(`gate error: ${e && e.stack ? e.stack : e}`);
    exitCode = EXIT_USAGE;
  } finally {
    await teardown();
  }
  process.exit(exitCode);
}

// ----------------------------------------------------------------------------------------------------
// Self-test multi-step seam (INERT unless VERTER_GATE_SELFTEST=1). Drives the REAL whole-gate budget bound
// (the shared `deadlineMs` across every step) with `name|cmd` stand-in steps so the budget semantics can be
// proven cargo-free. This seam can NEVER replace the real cargo gate in production: it is reachable only
// when VERTER_GATE_SELFTEST=1 is set (the harness sets it; production never does). A stray
// VERTER_GATE_SELFTEST_STEPS with the guard unset is IGNORED — the real archive/run path runs unchanged.
// ----------------------------------------------------------------------------------------------------
function selftestStepsActive(opts) {
  return (
    process.env.VERTER_GATE_SELFTEST === "1" &&
    !!process.env.VERTER_GATE_SELFTEST_STEPS &&
    (!opts || opts.customCmd.length === 0)
  );
}

async function runMultiStepSeam(ctx) {
  const { cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs } = ctx;
  const specs = process.env.VERTER_GATE_SELFTEST_STEPS.split("\n").filter((l) => l.trim());
  let overall = EXIT_PASS;
  for (const spec of specs) {
    const bar = spec.indexOf("|");
    const name = bar >= 0 ? spec.slice(0, bar) : spec;
    const cmdStr = bar >= 0 ? spec.slice(bar + 1) : spec;
    const remaining = deadlineMs - nowMs();
    if (remaining <= 0) {
      warn(`whole-gate budget exhausted before step '${name}' => TIMEOUT`);
      overall = EXIT_TIMEOUT;
      break;
    }
    const inv = shellInvocation(cmdStr);
    const res = await runContainedStep({
      cmd: inv.cmd,
      args: inv.args,
      cwd: repoRealpath,
      env: cargoEnv,
      phase: "test",
      deadlineMs,
      stallMs,
      targetDir: runnerTarget,
    });
    log(`step ${name}: exit=${res.code} reason=${res.reason || "-"} secs=${Math.round(res.durationMs / 1000)}`);
    const rc = mapStepReason(res);
    if (rc !== EXIT_PASS) {
      overall = rc;
      break;
    }
  }
  return overall;
}

// ----------------------------------------------------------------------------------------------------
// Custom-command mode: run an arbitrary bounded command under the same containment. Used by the self-test
// (sleep/echo stand-ins). The command is run via `bash -c <string>` (POSIX) / `cmd /c <string>` (Windows)
// so a shell snippet's `&`/`wait` work. A custom step is a TEST-phase step (byte-growth-only liveness):
// the harness's silent-`sleep` stall scenario depends on this.
// ----------------------------------------------------------------------------------------------------
function shellInvocation(cmdString) {
  if (IS_WINDOWS) return { cmd: "cmd.exe", args: ["/d", "/s", "/c", cmdString] };
  return { cmd: "bash", args: ["-c", cmdString] };
}

async function runCustom(opts, ctx) {
  const { cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs } = ctx;

  if (opts.customCmd.length === 0) {
    err("-- given but no command followed");
    return EXIT_USAGE;
  }
  const cmdString = opts.customCmd.join(" ");
  const inv = shellInvocation(cmdString);
  const res = await runContainedStep({
    cmd: inv.cmd,
    args: inv.args,
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "test",
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
  });
  log(`custom: exit=${res.code} reason=${res.reason || "-"} secs=${Math.round(res.durationMs / 1000)}`);
  return mapStepReason(res);
}

function mapStepReason(res) {
  if (res.reason === "TIMEOUT") return EXIT_TIMEOUT;
  if (res.reason === "STALL") return EXIT_STALL;
  if (res.code === 0) return EXIT_PASS;
  return EXIT_FAIL;
}

// ----------------------------------------------------------------------------------------------------
// Archive + list — the shared front half of both the gate and --prepare. Returns the parsed list JSON +
// the extract dir, or throws on setup failure.
// ----------------------------------------------------------------------------------------------------
async function archiveAndList(ctx) {
  const { cargoEnv, repoRealpath, runnerTarget, gateDir, deadlineMs, stallMs } = ctx;
  const archiveFile = join(gateDir, "nextest.tar.zst");
  const extractDir = join(gateDir, "extract");
  mkdirSync(gateDir, { recursive: true });
  // nextest's --extract-to canonicalizes the destination BEFORE extracting, so it must already exist.
  mkdirSync(extractDir, { recursive: true });

  // --- BUILD the whole workspace test universe ONCE (workspace unification => session_metrics ON) ---
  log("archiving workspace test universe (cargo nextest archive --workspace) …");
  const archiveRes = await runContainedStep({
    cmd: "cargo",
    args: [
      "nextest",
      "archive",
      "--workspace",
      "--archive-file",
      archiveFile,
      "--target-dir",
      runnerTarget,
      "--zstd-level",
      "-7",
    ],
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "build",
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
  });
  if (archiveRes.reason) {
    return { error: mapStepReason(archiveRes), where: "archive", res: archiveRes };
  }
  if (archiveRes.code !== 0) {
    err(`cargo nextest archive failed (exit ${archiveRes.code})`);
    return { error: EXIT_USAGE, where: "archive", res: archiveRes };
  }
  log(`archive built in ${Math.round(archiveRes.durationMs / 1000)}s -> ${archiveFile}`);

  // --- LIST the suites from the archive (NO rebuild); JSON to a dedicated stdout capture ---
  log("listing suites from the archive (cargo nextest list --message-format json) …");
  const listRes = await runContainedStep({
    cmd: "cargo",
    args: [
      "nextest",
      "list",
      "--archive-file",
      archiveFile,
      "--extract-to",
      extractDir,
      "--extract-overwrite",
      "--workspace-remap",
      repoRealpath,
      "--message-format",
      "json",
    ],
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "build", // extraction can be silent-ish; allow artifact-growth as progress
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
    captureStdoutSeparately: true, // keep JSON out of the mirrored stderr stream
  });
  if (listRes.reason) {
    return { error: mapStepReason(listRes), where: "list", res: listRes };
  }
  if (listRes.code !== 0) {
    err(`cargo nextest list failed (exit ${listRes.code})`);
    return { error: EXIT_USAGE, where: "list", res: listRes };
  }
  let listJson;
  try {
    listJson = parseNextestListJson(listRes.stdout);
  } catch (e) {
    err(`could not parse nextest list JSON: ${e.message}`);
    return { error: EXIT_USAGE, where: "list-parse", res: listRes };
  }
  return { listJson, extractDir, archiveFile };
}

// ----------------------------------------------------------------------------------------------------
// --prepare: warm-pass. Run the archive build + list (the legitimate assessment pre-warm) and pre-touch
// the built binaries once (a one-shot first-launch that warms the macOS Gatekeeper assessment cache via
// the legitimate first-launch path). This is a PRE-WARM, not a cost removal: it does NOT disable
// Gatekeeper; it only moves the legitimate first-launch assessment earlier, out of a timed gate.
// ----------------------------------------------------------------------------------------------------
async function runPrepare(ctx) {
  const out = await archiveAndList(ctx);
  if (out.error) return out.error;
  const { listJson, extractDir } = out;
  const buildMetaTargetDir = listJson["rust-build-meta"] && listJson["rust-build-meta"]["target-directory"];
  const suites = Object.values(listJson["rust-suites"] || {});
  // One-shot warm: launch each suite binary with --list (no test execution) so the OS first-launch
  // assessment for that binary is performed now via the legitimate path.
  let warmed = 0;
  for (const s of suites) {
    const bin = resolveSuiteBinary(s["binary-path"], buildMetaTargetDir, extractDir);
    if (!bin || !existsSync(bin)) continue;
    const r = spawnSync(bin, ["--list"], { encoding: "utf8", windowsHide: true, timeout: 30000 });
    if (r.status === 0 || r.status === 101 || r.signal == null) warmed++;
  }
  log(`prepare: archived + listed ${suites.length} suites; warmed first-launch assessment for ${warmed} binaries`);
  log("prepare is a PRE-WARM (moves the legitimate first-launch assessment earlier); it does NOT disable Gatekeeper or remove the cost.");
  return EXIT_PASS;
}

// ----------------------------------------------------------------------------------------------------
// runGate: the full canonical gate.
//   1. archive (build ONCE) + list (parse rust-suites).
//   2. SURFACE 1 — nextest run from the archive (process isolation).
//   3. SURFACE 2 — directly exec every verter_session suite (kind ∈ {lib,test}) with cwd = its package
//      manifest dir (the in-process / libtest surface). ZERO recompile (reads the archived artifacts).
//   4. Aggregate failures across both surfaces; tolerated-only => PASS-WITH-TOLERATED.
// ----------------------------------------------------------------------------------------------------
async function runGate(opts, ctx) {
  const { cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs } = ctx;

  // Self-test seam (INERT unless VERTER_GATE_SELFTEST=1): drive the whole-gate budget with stand-in steps
  // WITHOUT issuing the real cargo archive build. Production never sets the guard, so the real path runs.
  if (selftestStepsActive(opts)) {
    return runMultiStepSeam(ctx);
  }

  const out = await archiveAndList(ctx);
  if (out.error) {
    err(`gate setup failed at the ${out.where} step`);
    return out.error;
  }
  const { listJson, extractDir, archiveFile } = out;
  const buildMetaTargetDir = listJson["rust-build-meta"] && listJson["rust-build-meta"]["target-directory"];
  const allSuites = Object.values(listJson["rust-suites"] || {});
  log(`archive lists ${allSuites.length} suites; build-meta target-directory=${buildMetaTargetDir || "?"}`);

  // Aggregate verdict accumulators.
  const failures = []; // { surface, name }
  let toleratedOccurred = false;
  let hardSetupFail = false;

  // ---------- SURFACE 1: nextest run from the archive (process isolation) ----------
  log("SURFACE 1: nextest run from the archive (process isolation) …");
  const runArgs = [
    "nextest",
    "run",
    "--archive-file",
    archiveFile,
    "--extract-to",
    extractDir,
    "--extract-overwrite",
    "--workspace-remap",
    repoRealpath,
  ];
  if (opts.noFailFast) runArgs.push("--no-fail-fast");
  const runRes = await runContainedStep({
    cmd: "cargo",
    args: runArgs,
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "test", // TEST phase: byte-growth-only liveness (a silent test binary is a hang)
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
  });
  if (runRes.reason) {
    err(`nextest run ${runRes.reason} after ${Math.round(runRes.durationMs / 1000)}s`);
    return mapStepReason(runRes);
  }
  const nextestText = runRes.stdout + "\n" + runRes.stderr;
  // SURFACE-1 verdict via the shared analyzer (the same code the `--selftest-classify-nextest-run` hook
  // drives). It consults the run exit code + the summary `failed` total, NOT just the `FAIL [` lines, so a
  // crash (SIGABRT/SIGSEGV/LEAK/TIMEOUT/…) or a setup/harness error in ANY crate fails the gate.
  const s1 = analyzeNextestSurface(nextestText, runRes.code);
  for (const f of s1.failures) failures.push(f);
  if (s1.toleratedCount > 0) toleratedOccurred = true;
  log(
    `SURFACE 1 done in ${Math.round(runRes.durationMs / 1000)}s: ` +
      `${s1.summary.passed} passed, ${s1.summary.failed} failed ` +
      `(${s1.namedCount} named, ${s1.toleratedCount} tolerated), ${s1.summary.skipped} skipped; ` +
      `run exit ${runRes.code}`,
  );

  // ---------- SURFACE 2: direct verter_session libtest execution (in-process surface) ----------
  const sel = selectSessionSuites(allSuites);
  if (sel.error) {
    err(`SURFACE 2 SETUP FAILURE: ${sel.error}`);
    return EXIT_USAGE;
  }
  const sessionSuites = sel.suites;
  log(
    `SURFACE 2: directly executing ${sessionSuites.length} verter_session libtest binaries ` +
      `(lib=${sel.lib}, test=${sel.test}) in-process from the SAME archive …`,
  );
  let s2Passed = 0;
  let s2Failed = 0;
  let s2Tolerated = 0;
  // Package identity derived from the archive list JSON (NOT a separate `cargo metadata` subprocess that
  // would escape the watchdog). All session suites share one package, so derive once from the first.
  const sessionPkgInfo = deriveSuitePkgInfo(sessionSuites[0]);
  for (const s of sessionSuites) {
    const remaining = deadlineMs - nowMs();
    if (remaining <= 0) {
      warn(`whole-gate budget exhausted before verter_session suite '${s["binary-id"]}' => TIMEOUT`);
      return EXIT_TIMEOUT;
    }
    const bin = resolveSuiteBinary(s["binary-path"], buildMetaTargetDir, extractDir);
    if (!bin || !existsSync(bin)) {
      err(`SURFACE 2: suite binary not found for ${s["binary-id"]} (path=${s["binary-path"]}) — setup failure`);
      hardSetupFail = true;
      continue;
    }
    // cwd = the package manifest dir (what Cargo sets). nextest reports it as the suite's `cwd`; defend
    // against a missing/extract-relative value by falling back to <repo>/crates/verter_session.
    const cwd = s.cwd && existsSync(s.cwd) ? s.cwd : join(repoRealpath, "crates", "verter_session");
    // The directly-executed binary needs the runtime Cargo env these tests read — CARGO_MANIFEST_DIR
    // (tests resolve the repo root + read corpus fixtures through it) and CARGO_TARGET_DIR (already on the
    // base cargo env). cwd IS the manifest dir. See buildSuiteEnv for the verified-complete scope.
    const suiteEnv = buildSuiteEnv(cargoEnv, cwd, sessionPkgInfo, s["binary-name"] || "verter_session");
    // Preserve the libtest DEFAULT threading (do NOT force --test-threads=1). Optionally pass an explicit
    // passthrough if the caller asked for it.
    const binArgs = [];
    if (opts.testThreads != null) binArgs.push(`--test-threads=${opts.testThreads}`);
    const res = await runContainedStep({
      cmd: bin,
      args: binArgs,
      cwd,
      env: suiteEnv, // the runtime Cargo env this suite reads (CARGO_MANIFEST_DIR + CARGO_TARGET_DIR)
      phase: "test", // TEST phase: byte-growth-only liveness
      deadlineMs,
      stallMs,
      targetDir: runnerTarget,
      captureStdoutSeparately: true, // keep libtest stdout parseable; still mirror stderr
    });
    if (res.reason) {
      err(`SURFACE 2: suite ${s["binary-id"]} ${res.reason} after ${Math.round(res.durationMs / 1000)}s`);
      return mapStepReason(res);
    }
    const libText = res.stdout + "\n" + res.stderr;
    const libFailNames = extractLibtestFailedNames(libText);
    if (res.code === 0 && libFailNames.length === 0) {
      s2Passed++;
    } else {
      // Qualify each failed test name with the suite binary-id so a bare libtest name maps to the same
      // exact-name space the allowlist uses (suite::name) — but also tolerate the bare form.
      let allTolerated = libFailNames.length > 0;
      for (const nm of libFailNames) {
        const qualified = `${s["binary-id"].replace(/^verter_session::?/, "")}::${nm}`;
        if (TOLERATED_TEST_NAMES.has(nm) || TOLERATED_TEST_NAMES.has(qualified)) {
          s2Tolerated++;
          toleratedOccurred = true;
        } else {
          allTolerated = false;
          failures.push({ surface: `libtest:${s["binary-id"]}`, name: nm });
        }
      }
      if (libFailNames.length === 0 && res.code !== 0) {
        // Non-zero exit with no parseable FAILED line (e.g. a panic/abort) — a real failure.
        failures.push({ surface: `libtest:${s["binary-id"]}`, name: `<exit ${res.code}>` });
        allTolerated = false;
      }
      if (allTolerated) s2Passed++;
      else s2Failed++;
    }
  }
  log(`SURFACE 2 done: ${s2Passed} suites clean, ${s2Failed} suites with non-tolerated failures, ${s2Tolerated} tolerated test failures`);

  // ---------- Aggregate verdict ----------
  if (hardSetupFail) {
    err("VERDICT: FAIL (a verter_session suite binary was missing from the archive — setup integrity failure)");
    return EXIT_FAIL;
  }
  if (failures.length > 0) {
    err(`VERDICT: FAIL — ${failures.length} non-tolerated failure(s):`);
    for (const f of failures.slice(0, 50)) err(`  [${f.surface}] ${f.name}`);
    return EXIT_FAIL;
  }
  if (toleratedOccurred) {
    log("VERDICT: PASS-WITH-TOLERATED (only the env-only typeinfo_proto_ts_freshness pair failed, by exact name)");
    return EXIT_PASS;
  }
  log("VERDICT: PASS (both surfaces green)");
  return EXIT_PASS;
}

// Parse nextest's trailing "Summary [   …s] N tests run: P passed, S skipped" line for counts.
function parseNextestSummary(text) {
  let passed = 0;
  let skipped = 0;
  let failed = 0;
  // nextest emits: "Summary [  63.890s] 15543 tests run: 15541 passed, 547 skipped" and may include
  // "N failed" when there are failures.
  const lines = text.split("\n").filter((l) => /Summary \[/.test(l));
  const line = lines.length ? lines[lines.length - 1] : "";
  let m = /(\d+)\s+passed/.exec(line);
  if (m) passed = parseInt(m[1], 10);
  m = /(\d+)\s+skipped/.exec(line);
  if (m) skipped = parseInt(m[1], 10);
  m = /(\d+)\s+failed/.exec(line);
  if (m) failed = parseInt(m[1], 10);
  return { passed, skipped, failed };
}

main().catch((e) => {
  err(`fatal: ${e && e.stack ? e.stack : e}`);
  process.exit(EXIT_USAGE);
});
