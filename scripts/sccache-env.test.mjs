// Self-tests for scripts/sccache-env.mjs — a Vitest suite collected by the
// root vitest config, e.g.:
//   pnpm vitest --run scripts/sccache-env.test.mjs
//
// Every test forces sccache presence/absence deterministically via
// VERTER_SCCACHE_BIN (+ PATH), so it never depends on the host having (or
// lacking) sccache. The helper is exercised as a real subprocess (spawnSync),
// exactly as callers run it.

import { describe, test, expect } from "vitest";
import os from "node:os";
import path from "node:path";
import fs from "node:fs";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { isWindowsExecutableName } from "./sccache-env.mjs";

const HELPER = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "sccache-env.mjs");

const SPAWN_TIMEOUT_MS = 30_000;

function mkTmp(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

/** Copy of the host env with all sccache-relevant vars removed. */
function baseEnv(overrides = {}) {
  const env = { ...process.env };
  delete env.RUSTC_WRAPPER;
  delete env.CARGO_INCREMENTAL;
  delete env.VERTER_SCCACHE_BIN;
  delete env.SCCACHE_DIR;
  delete env.SCCACHE_CACHE_SIZE;
  delete env.SCCACHE_BASEDIRS;
  return { ...env, ...overrides };
}

/** Env in which sccache is deterministically ABSENT. */
function absentEnv(tmp) {
  return baseEnv({
    PATH: "",
    VERTER_SCCACHE_BIN: path.join(tmp, "definitely-missing-sccache"),
  });
}

/** Create an executable stub file and return its absolute path (forced PRESENT). */
function makeStub(tmp) {
  const stub = path.join(tmp, process.platform === "win32" ? "sccache-stub.exe" : "sccache-stub");
  fs.writeFileSync(stub, "#!/bin/sh\nexit 0\n");
  fs.chmodSync(stub, 0o755);
  return stub;
}

/**
 * Create a NON-executable stub and return its absolute path: a regular file
 * without the execute bit (POSIX) and without an executable extension (win32).
 */
function makeNonExecutableStub(tmp) {
  const stub = path.join(tmp, "sccache-noexec");
  fs.writeFileSync(stub, "not an executable\n");
  fs.chmodSync(stub, 0o644);
  return stub;
}

function runHelper(args, { env, cwd } = {}) {
  return spawnSync(process.execPath, [HELPER, ...args], {
    env,
    cwd,
    encoding: "utf8",
  });
}

function parseEnvOutput(stdout) {
  const map = {};
  for (const line of stdout.split(/\r?\n/)) {
    if (line === "" || line.startsWith("#")) continue;
    const idx = line.indexOf("=");
    expect(idx, `expected KEY=VALUE line, got: ${line}`).toBeGreaterThan(0);
    map[line.slice(0, idx)] = line.slice(idx + 1);
  }
  return map;
}

/** Child argv that writes `content` to `markerPath` (absolute-node child, PATH-free). */
function markerChild(markerPath, content) {
  return [
    process.execPath,
    "-e",
    `require('node:fs').writeFileSync(${JSON.stringify(markerPath)}, ${JSON.stringify(content)})`,
  ];
}

describe("isWindowsExecutableName (pure win32 executable-extension decision)", () => {
  test(".exe is accepted with an empty (or unset) PATHEXT", () => {
    expect(isWindowsExecutableName("sccache.exe", "")).toBe(true);
    expect(isWindowsExecutableName("sccache.exe", undefined)).toBe(true);
  });

  test(".exe is accepted when PATHEXT omits .EXE", () => {
    expect(isWindowsExecutableName("sccache.exe", ".COM;.BAT;.CMD")).toBe(true);
  });

  test("a non-executable extension (.txt) is rejected", () => {
    expect(isWindowsExecutableName("sccache.txt", ".COM;.EXE;.BAT;.CMD")).toBe(false);
    expect(isWindowsExecutableName("sccache.txt", "")).toBe(false);
  });

  test("a custom PATHEXT entry (.PS1) is accepted — and rejected without it", () => {
    expect(isWindowsExecutableName("deploy.ps1", ".COM;.EXE;.BAT;.CMD;.PS1")).toBe(true);
    expect(isWindowsExecutableName("deploy.ps1", ".COM;.EXE;.BAT;.CMD")).toBe(false);
  });

  test("extension matching is case-insensitive (.EXE vs .exe)", () => {
    expect(isWindowsExecutableName("SCCACHE.EXE", "")).toBe(true);
    expect(isWindowsExecutableName("sccache.exe", ".EXE")).toBe(true);
    expect(isWindowsExecutableName("run.CmD", "")).toBe(true);
  });
});

describe("sccache-env helper", () => {
  test(
    "--required with sccache absent fails non-zero and does NOT run the child",
    () => {
      const tmp = mkTmp("verter-sccache-t1-");
      try {
        const marker = path.join(tmp, "child-ran.marker");
        const res = runHelper(["--exec", "--required", "--", ...markerChild(marker, "ran")], {
          env: absentEnv(tmp),
        });
        expect(res.status, "expected a non-zero exit").not.toBe(0);
        expect(res.stderr).toMatch(/sccache/i);
        expect(res.stderr).toMatch(/not found/i);
        expect(fs.existsSync(marker), "child must NOT have run").toBe(false);
      } finally {
        fs.rmSync(tmp, { recursive: true, force: true });
      }
    },
    SPAWN_TIMEOUT_MS,
  );

  test(
    "optional --exec with sccache absent warns loudly and still runs the child",
    () => {
      const tmp = mkTmp("verter-sccache-t2-");
      try {
        const marker = path.join(tmp, "child-ran.marker");
        const res = runHelper(["--exec", "--", ...markerChild(marker, "ran")], {
          env: absentEnv(tmp),
        });
        expect(res.status, `expected exit 0, stderr: ${res.stderr}`).toBe(0);
        expect(res.stderr).toMatch(/WARNING/);
        expect(res.stderr).toMatch(/without compiler cache/i);
        expect(
          fs.readFileSync(marker, "utf8"),
          "child must have run (loud no-op, not a blocked run)",
        ).toBe("ran");
      } finally {
        fs.rmSync(tmp, { recursive: true, force: true });
      }
    },
    SPAWN_TIMEOUT_MS,
  );

  test(
    "--print-env with sccache present prints wrapper, incremental=0, absolute basedirs",
    () => {
      const tmp = mkTmp("verter-sccache-t3-");
      try {
        const stub = makeStub(tmp);
        const res = runHelper(["--print-env"], {
          // Real PATH kept so `git worktree list` derivation works; presence is
          // forced (and proven) via the authoritative VERTER_SCCACHE_BIN override.
          env: baseEnv({
            VERTER_SCCACHE_BIN: stub,
            SCCACHE_DIR: path.join(tmp, "cache-dir"),
          }),
        });
        expect(res.status, `stderr: ${res.stderr}`).toBe(0);
        const envMap = parseEnvOutput(res.stdout);
        expect(envMap.RUSTC_WRAPPER).toBe(stub);
        expect(path.isAbsolute(envMap.RUSTC_WRAPPER)).toBe(true);
        expect(envMap.CARGO_INCREMENTAL).toBe("0");
        expect(path.isAbsolute(envMap.SCCACHE_DIR)).toBe(true);
        expect(envMap.SCCACHE_CACHE_SIZE).toBe("10G");
        const entries = envMap.SCCACHE_BASEDIRS.split(path.delimiter);
        expect(entries.length, "expected at least one basedir").toBeGreaterThanOrEqual(1);
        for (const entry of entries) {
          expect(path.isAbsolute(entry), `basedir not absolute: ${entry}`).toBe(true);
        }
      } finally {
        fs.rmSync(tmp, { recursive: true, force: true });
      }
    },
    SPAWN_TIMEOUT_MS,
  );

  test(
    "SCCACHE_BASEDIRS joins every worktree root with the platform path.delimiter",
    () => {
      const tmp = mkTmp("verter-sccache-t4-");
      try {
        const stub = makeStub(tmp);
        // Hermetic fixture: a fresh repo with TWO worktrees, so the derived value
        // must contain exactly two absolute entries split by path.delimiter — a
        // wrong join delimiter yields a wrong entry count and fails.
        const repo = path.join(tmp, "repo");
        fs.mkdirSync(repo);
        const git = (args, cwd) => {
          const r = spawnSync("git", args, { cwd, encoding: "utf8" });
          expect(r.status, `git ${args.join(" ")} failed: ${r.stderr}`).toBe(0);
        };
        git(["init", "-q", "-b", "main"], repo);
        git(
          [
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            "init",
          ],
          repo,
        );
        const wt2 = path.join(tmp, "wt2");
        git(["worktree", "add", "-q", "-b", "wt2", wt2], repo);

        const res = runHelper(["--print-env"], {
          env: baseEnv({ VERTER_SCCACHE_BIN: stub }),
          cwd: repo,
        });
        expect(res.status, `stderr: ${res.stderr}`).toBe(0);
        const entries = parseEnvOutput(res.stdout).SCCACHE_BASEDIRS.split(path.delimiter);
        expect(entries.length, `expected 2 basedirs, got: ${entries}`).toBe(2);
        for (const entry of entries) {
          expect(path.isAbsolute(entry), `basedir not absolute: ${entry}`).toBe(true);
        }
        const real = (p) => fs.realpathSync(p);
        const got = new Set(entries.map(real));
        expect(got.has(real(repo)), "repo root missing from basedirs").toBe(true);
        expect(got.has(real(wt2)), "second worktree missing from basedirs").toBe(true);
      } finally {
        fs.rmSync(tmp, { recursive: true, force: true });
      }
    },
    SPAWN_TIMEOUT_MS,
  );

  test(
    "no rustc-direct fallback: wrapper is never the helper or rustc; absent exec adds no RUSTC_WRAPPER",
    () => {
      const tmp = mkTmp("verter-sccache-t5-");
      try {
        // Present: RUSTC_WRAPPER must be the discovered sccache, never the helper
        // itself and never literally rustc.
        const stub = makeStub(tmp);
        const res = runHelper(["--print-env"], {
          env: baseEnv({
            VERTER_SCCACHE_BIN: stub,
            SCCACHE_DIR: path.join(tmp, "cache-dir"),
          }),
        });
        expect(res.status, `stderr: ${res.stderr}`).toBe(0);
        const wrapper = parseEnvOutput(res.stdout).RUSTC_WRAPPER;
        expect(wrapper).not.toBe("rustc");
        expect(path.resolve(wrapper)).not.toBe(HELPER);
        expect(wrapper.endsWith("sccache-env.mjs")).toBe(false);

        // Absent + optional --exec: the spawned child env has NO RUSTC_WRAPPER key
        // at all (unmodified environment, not a shim).
        const marker = path.join(tmp, "env-dump.json");
        const child = [
          process.execPath,
          "-e",
          `require('node:fs').writeFileSync(${JSON.stringify(marker)}, JSON.stringify({ hasWrapper: 'RUSTC_WRAPPER' in process.env }))`,
        ];
        const res2 = runHelper(["--exec", "--", ...child], { env: absentEnv(tmp) });
        expect(res2.status, `stderr: ${res2.stderr}`).toBe(0);
        const dump = JSON.parse(fs.readFileSync(marker, "utf8"));
        expect(dump.hasWrapper, "child env must not contain RUSTC_WRAPPER").toBe(false);
      } finally {
        fs.rmSync(tmp, { recursive: true, force: true });
      }
    },
    SPAWN_TIMEOUT_MS,
  );

  test(
    "optional --print-env with sccache absent exits 0, WARNS on stderr, and emits no env assignments",
    () => {
      const tmp = mkTmp("verter-sccache-t6-");
      try {
        const res = runHelper(["--print-env"], { env: absentEnv(tmp) });
        expect(res.status, `stderr: ${res.stderr}`).toBe(0);
        expect(res.stderr).toMatch(/WARNING/);
        expect(res.stderr).toMatch(/sccache not found/i);
        // STDOUT must contain NO env assignments — only blank/comment lines.
        for (const line of res.stdout.split(/\r?\n/)) {
          if (line === "" || line.startsWith("#")) continue;
          expect.fail(`unexpected non-comment stdout line: ${line}`);
        }
        expect(res.stdout).not.toMatch(/^RUSTC_WRAPPER=/m);
      } finally {
        fs.rmSync(tmp, { recursive: true, force: true });
      }
    },
    SPAWN_TIMEOUT_MS,
  );

  test(
    "a NON-executable VERTER_SCCACHE_BIN counts as absent: optional no-ops without RUSTC_WRAPPER, --required hard-fails",
    () => {
      const tmp = mkTmp("verter-sccache-t7-");
      try {
        const nonExec = makeNonExecutableStub(tmp);
        const env = baseEnv({ PATH: "", VERTER_SCCACHE_BIN: nonExec });

        // Optional --exec: LOUD no-op, child runs with NO RUSTC_WRAPPER — the
        // non-executable file is never promoted to a wrapper.
        const marker = path.join(tmp, "env-dump.json");
        const child = [
          process.execPath,
          "-e",
          `require('node:fs').writeFileSync(${JSON.stringify(marker)}, JSON.stringify({ hasWrapper: 'RUSTC_WRAPPER' in process.env }))`,
        ];
        const res = runHelper(["--exec", "--", ...child], { env });
        expect(res.status, `stderr: ${res.stderr}`).toBe(0);
        expect(res.stderr).toMatch(/WARNING/);
        const dump = JSON.parse(fs.readFileSync(marker, "utf8"));
        expect(dump.hasWrapper, "non-executable candidate must never become RUSTC_WRAPPER").toBe(
          false,
        );

        // --required: hard failure, child not run.
        const marker2 = path.join(tmp, "child-ran.marker");
        const res2 = runHelper(["--exec", "--required", "--", ...markerChild(marker2, "ran")], {
          env,
        });
        expect(res2.status, "expected a non-zero exit").not.toBe(0);
        expect(res2.stderr).toMatch(/not found/i);
        expect(fs.existsSync(marker2), "child must NOT have run").toBe(false);
      } finally {
        fs.rmSync(tmp, { recursive: true, force: true });
      }
    },
    SPAWN_TIMEOUT_MS,
  );

  test(
    "a relative SCCACHE_BASEDIRS override entry fails non-zero with a clear diagnostic (fail-closed)",
    () => {
      const tmp = mkTmp("verter-sccache-t8-");
      try {
        const stub = makeStub(tmp);
        const res = runHelper(["--print-env"], {
          env: baseEnv({
            VERTER_SCCACHE_BIN: stub,
            SCCACHE_DIR: path.join(tmp, "cache-dir"),
            SCCACHE_BASEDIRS: ["relative/dir", tmp].join(path.delimiter),
          }),
        });
        expect(res.status, "expected a non-zero exit").not.toBe(0);
        expect(res.stderr).toMatch(/SCCACHE_BASEDIRS/);
        expect(res.stderr).toMatch(/absolute/i);
        // Fail-closed: the invalid value must never be emitted.
        expect(res.stdout).not.toMatch(/^SCCACHE_BASEDIRS=/m);
      } finally {
        fs.rmSync(tmp, { recursive: true, force: true });
      }
    },
    SPAWN_TIMEOUT_MS,
  );

  test(
    "--exec with sccache present merges the computed env into the child and preserves caller PATH",
    () => {
      const tmp = mkTmp("verter-sccache-t9-");
      try {
        const stub = makeStub(tmp);
        const cacheDir = path.join(tmp, "cache-dir");
        const env = baseEnv({ VERTER_SCCACHE_BIN: stub, SCCACHE_DIR: cacheDir });
        const marker = path.join(tmp, "env-dump.json");
        const child = [
          process.execPath,
          "-e",
          `require('node:fs').writeFileSync(${JSON.stringify(marker)}, JSON.stringify({ wrapper: process.env.RUSTC_WRAPPER ?? null, incremental: process.env.CARGO_INCREMENTAL ?? null, dir: process.env.SCCACHE_DIR ?? null, path: process.env.PATH ?? null }))`,
        ];
        const res = runHelper(["--exec", "--", ...child], { env });
        expect(res.status, `stderr: ${res.stderr}`).toBe(0);
        const dump = JSON.parse(fs.readFileSync(marker, "utf8"));
        expect(dump.wrapper, "child must receive the absolute sccache path").toBe(stub);
        expect(dump.incremental).toBe("0");
        expect(dump.dir).toBe(path.resolve(cacheDir));
        // Caller env survives the merge: PATH arrives unchanged in the child.
        expect(dump.path, "caller PATH must survive the env merge").toBe(env.PATH);
      } finally {
        fs.rmSync(tmp, { recursive: true, force: true });
      }
    },
    SPAWN_TIMEOUT_MS,
  );
});
