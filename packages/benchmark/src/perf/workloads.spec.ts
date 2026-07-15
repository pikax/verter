import { describe, it, expect } from "vitest";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { EventEmitter } from "node:events";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import {
  spawnWithRssSampling,
  exitCodeOrThrow,
  typecheckExitCodeOrThrow,
  parseDiagnosticSet,
  sideCorpus,
  sideCorpusView,
  incrementalRetypecheck,
  type SampledRun,
  type SampledChild,
  type SpawnChild,
  type WorkloadContext,
} from "./workloads.js";
import type { EnsuredCorpus } from "./corpus.js";

const HERE = dirname(fileURLToPath(import.meta.url));

/** A scriptable in-process child for the spawn seam (no real subprocess). */
class FakeSpawnChild extends EventEmitter {
  pid = 4242;
  killed = 0;
  // The driver only reads `.stdout?.on`/`.stderr?.on` for data; a no-op stream
  // is enough — the spec scripts exit/error via the EventEmitter base.
  stdout = { on: (): void => {} } as unknown as NodeJS.ReadableStream;
  stderr = { on: (): void => {} } as unknown as NodeJS.ReadableStream;
  kill(): void {
    this.killed++;
  }
}

/** A `SpawnChild` whose child emits the scripted events on the next macrotask. */
function fakeSpawn(emit: (child: FakeSpawnChild) => void): SpawnChild {
  return () => {
    const child = new FakeSpawnChild();
    setImmediate(() => emit(child));
    return child as unknown as SampledChild;
  };
}

/**
 * A `SpawnChild` whose child has NO pid, so RSS sampling is unavailable on EVERY
 * platform (`sampleProcessRss(undefined)` cannot read any real process) — used to
 * prove unavailable RSS is reported as `null`, never coerced to `0`.
 */
function fakeSpawnNoPid(emit: (child: FakeSpawnChild) => void): SpawnChild {
  return () => {
    const child = new FakeSpawnChild();
    (child as { pid?: number }).pid = undefined;
    setImmediate(() => emit(child));
    return child as unknown as SampledChild;
  };
}

function stubCorpus(dir: string): EnsuredCorpus {
  return {
    dir,
    rootTsconfig: join(dir, "tsconfig.json"),
    appTsconfig: join(dir, "app", "tsconfig.json"),
    kernelTsconfig: join(dir, "kernel", "tsconfig.json"),
    manifest: {} as EnsuredCorpus["manifest"],
    contentHash: "test",
    isGateCorpus: false,
  };
}

describe("subprocess crash/timeout/spawn-error is a HARD failure (never a matching exit code)", () => {
  it("exitCodeOrThrow: a clean nonzero exit is a real exit code, a crash is a throw", () => {
    const ok = (over: Partial<SampledRun>): SampledRun => ({
      status: 0,
      signal: null,
      spawnError: null,
      timedOut: false,
      stdout: "",
      stderr: "",
      peakRssBytes: 0,
      ...over,
    });
    // A diagnostic exit (tsc exits nonzero on type errors) is a real code.
    expect(exitCodeOrThrow(ok({ status: 2 }), "x")).toBe(2);
    expect(exitCodeOrThrow(ok({ status: 0 }), "x")).toBe(0);
    // A timeout / signal / spawn-error / abnormal exit is NOT an exit code.
    expect(() => exitCodeOrThrow(ok({ status: null, timedOut: true }), "x")).toThrow(/timed out/i);
    expect(() => exitCodeOrThrow(ok({ status: null, signal: "SIGKILL" }), "x")).toThrow(/signal/i);
    expect(() =>
      exitCodeOrThrow(ok({ status: null, spawnError: new Error("ENOENT") }), "x"),
    ).toThrow(/spawn/i);
    expect(() => exitCodeOrThrow(ok({ status: null }), "x")).toThrow(/abnormal|no exit code/i);
  });

  it("spawnWithRssSampling reports a real timeout as timedOut (not a collapsed status:-1)", async () => {
    // A child that would run far longer than the timeout must surface timedOut,
    // so exitCodeOrThrow turns it into a hard failure (two timing-out sides must
    // NOT both report a matching exit code and pass).
    const run = await spawnWithRssSampling(
      process.execPath,
      ["-e", "setTimeout(() => {}, 60000)"],
      HERE,
      300,
    );
    expect(run.timedOut).toBe(true);
    expect(() => exitCodeOrThrow(run, "timeout-child")).toThrow(/timed out/i);
  });

  it("spawnWithRssSampling reports a real clean exit code (a nonzero diagnostic exit is NOT a crash)", async () => {
    const run = await spawnWithRssSampling(
      process.execPath,
      ["-e", "process.exit(3)"],
      HERE,
      30_000,
    );
    expect(run.timedOut).toBe(false);
    expect(run.signal).toBeNull();
    expect(run.spawnError).toBeNull();
    expect(exitCodeOrThrow(run, "exit-child")).toBe(3);
  });

  it("spawnWithRssSampling reports a spawn error for a missing binary", async () => {
    const run = await spawnWithRssSampling(
      "verter-this-binary-does-not-exist-xyz",
      [],
      HERE,
      5_000,
    );
    expect(run.spawnError).not.toBeNull();
    expect(() => exitCodeOrThrow(run, "missing-bin")).toThrow(/spawn/i);
  });
});

describe("parseDiagnosticSet captures no-file (global/options/config) diagnostics", () => {
  const root = "/corpus";

  it("includes a global diagnostic even when file diagnostics are present (regression cannot hide)", () => {
    const withGlobal =
      "src/a.ts(1,1): error TS2304: Cannot find name 'x'.\n" +
      "error TS5023: Unknown compiler option 'foo'.";
    const withoutGlobal = "src/a.ts(1,1): error TS2304: Cannot find name 'x'.";

    const base = parseDiagnosticSet(withGlobal, root);
    const cand = parseDiagnosticSet(withoutGlobal, root);

    // A candidate that DROPS the global diagnostic must produce a DIFFERENT set
    // (the correctness gate must catch it) — if both collapsed to the file
    // diagnostic only, the regression would hide.
    expect(base).not.toEqual(cand);
    expect(base.some((d) => /TS5023/.test(d))).toBe(true);
    expect(cand.some((d) => /TS5023/.test(d))).toBe(false);
    // The file diagnostic is still captured on both sides.
    expect(base.some((d) => /TS2304/.test(d))).toBe(true);
    expect(cand.some((d) => /TS2304/.test(d))).toBe(true);
  });

  it("captures a standalone global diagnostic with stable code+message keying", () => {
    const set = parseDiagnosticSet("error TS18003: No inputs were found in config file.", root);
    expect(set.length).toBe(1);
    expect(set[0]).toMatch(/TS18003/);
  });

  it("normalizes to a SIDE-INDEPENDENT set: per-side carrier hashes + temp dirs collapse", () => {
    // The per-side isolated trees produce DIFFERENT path-dependent carrier hashes
    // (and different per-run .tmp dirs); the diagnostic SET correctness compares
    // LOGICAL diagnostics, so both sides must normalize to the same set — without
    // the carrier-hash collapse they were disjoint (a false correctness regression).
    const cand = parseDiagnosticSet(
      "/work/candidate/.tmp9a1/Comp0000_000_6ddcf2fde2948b33.vue.ts(18,22): error TS2307: x",
      "/work/candidate",
    );
    const base = parseDiagnosticSet(
      "/work/baseline/.tmpZZ7/Comp0000_000_3c666ede0afcd39b.vue.ts(18,22): error TS2307: x",
      "/work/baseline",
    );
    expect(cand).toEqual(base);
    expect(cand[0]).toBe(".tmp/Comp0000_000.vue.ts:18:22:TS2307:error:x");
  });

  it("includes the MESSAGE in the file-diagnostic key: same path/range/code, DIFFERENT message ⇒ NOT equal", () => {
    // The file key must include the message, not just path:line:col:code — with a
    // code-only key a candidate that changes the diagnostic TEXT/type detail at the
    // same location+code would pass "diagnostic SET equality".
    const a = parseDiagnosticSet(
      "src/a.ts(3,5): error TS2322: Type 'string' is not assignable to type 'number'.",
      root,
    );
    const b = parseDiagnosticSet(
      "src/a.ts(3,5): error TS2322: Type 'boolean' is not assignable to type 'number'.",
      root,
    );
    expect(a).not.toEqual(b);
    expect(a[0]).toBe(
      "src/a.ts:3:5:TS2322:error:Type 'string' is not assignable to type 'number'.",
    );
    expect(b[0]).toBe(
      "src/a.ts:3:5:TS2322:error:Type 'boolean' is not assignable to type 'number'.",
    );
  });

  it("includes the SEVERITY in the key: same path/range/code/message but error vs warning ⇒ NOT equal", () => {
    const asError = parseDiagnosticSet(
      "src/a.ts(1,1): error TS6133: 'x' is declared but its value is never read.",
      root,
    );
    const asWarning = parseDiagnosticSet(
      "src/a.ts(1,1): warning TS6133: 'x' is declared but its value is never read.",
      root,
    );
    expect(asError).not.toEqual(asWarning);
    expect(asError[0]).toMatch(/:TS6133:error:/);
    expect(asWarning[0]).toMatch(/:TS6133:warning:/);
  });

  it("same path/range/code AND identical message ⇒ EQUAL (no false regression on a real match)", () => {
    const a = parseDiagnosticSet(
      "src/a.ts(3,5): error TS2322: Type 'string' is not assignable to type 'number'.",
      root,
    );
    const b = parseDiagnosticSet(
      "src/a.ts(3,5): error TS2322: Type 'string' is not assignable to type 'number'.",
      root,
    );
    expect(a).toEqual(b);
  });
});

describe("spawnWithRssSampling settles EXACTLY once (a spawn error is authoritative)", () => {
  it("a spawn error then a later close settles once, as a hard failure", async () => {
    const run = await spawnWithRssSampling(
      "bin",
      [],
      HERE,
      5_000,
      fakeSpawn((c) => {
        c.emit("error", Object.assign(new Error("ENOENT"), { code: "ENOENT" }));
        c.emit("close", 0, null); // a later close must NOT mask the spawn failure
      }),
    );
    expect(run.spawnError).not.toBeNull();
    expect(run.status).toBeNull();
    expect(() => exitCodeOrThrow(run, "x")).toThrow(/spawn/i);
  });

  it("a close(0) FOLLOWED BY a spawn error still settles as a hard failure (ordering-independent)", async () => {
    // The dangerous ordering: a stray success close fires first, then the spawn
    // error. A first-event-wins settle would report status 0 (a pass) and LOSE the
    // spawn failure. The fix makes the spawn error authoritative regardless of
    // arrival order.
    const run = await spawnWithRssSampling(
      "bin",
      [],
      HERE,
      5_000,
      fakeSpawn((c) => {
        c.emit("close", 0, null);
        c.emit("error", Object.assign(new Error("EACCES"), { code: "EACCES" }));
      }),
    );
    expect(run.spawnError).not.toBeNull();
    expect(() => exitCodeOrThrow(run, "x")).toThrow(/spawn/i);
  });

  it("a normal exit (no error) settles once with the real exit code", async () => {
    const run = await spawnWithRssSampling(
      "bin",
      [],
      HERE,
      5_000,
      fakeSpawn((c) => c.emit("close", 3, null)),
    );
    expect(run.spawnError).toBeNull();
    expect(run.timedOut).toBe(false);
    expect(exitCodeOrThrow(run, "x")).toBe(3);
  });

  it("reports peakRssBytes as NULL when RSS is unavailable — never coerced to 0", async () => {
    // A child with no pid: RSS cannot be sampled on any platform. The producer
    // MUST surface `null` (unavailable), not `0` — a `0` would slip past the
    // gate's presence rail as a "present" datum and be averaged into a nonzero
    // vector. (A coerced `peakRssBytes` of `0` would be that false present datum.)
    const run = await spawnWithRssSampling(
      "bin",
      [],
      HERE,
      5_000,
      fakeSpawnNoPid((c) => c.emit("close", 0, null)),
    );
    expect(run.peakRssBytes).toBeNull();
    // A clean exit-0 with unavailable RSS is still a real exit code, not a crash.
    expect(exitCodeOrThrow(run, "x")).toBe(0);
  });
});

describe("per-side isolation: each side gets its OWN working tree", () => {
  it("returns the SHARED corpus when no workDir is set", () => {
    const corpus = stubCorpus("/corpus");
    const ctx: WorkloadContext = { corpus, threads: 1 };
    expect(sideCorpus(ctx).dir).toBe("/corpus");
    expect(sideCorpusView(ctx)).toBe(corpus);
  });

  it("materializes a DISTINCT isolated copy per side (candidate cannot perturb baseline)", () => {
    const src = mkdtempSync(join(tmpdir(), "verter-iso-src-"));
    const work = mkdtempSync(join(tmpdir(), "verter-iso-work-"));
    try {
      mkdirSync(join(src, "app", "m0040"), { recursive: true });
      writeFileSync(join(src, "tsconfig.json"), "{}");
      writeFileSync(
        join(src, "app", "m0040", "types.ts"),
        "export interface Props0040 {\n  id: number;\n}\n",
      );
      const corpus = stubCorpus(src);
      const candCtx: WorkloadContext = { corpus, threads: 1, workDir: join(work, "candidate") };
      const baseCtx: WorkloadContext = { corpus, threads: 1, workDir: join(work, "baseline") };
      const c = sideCorpus(candCtx);
      const b = sideCorpus(baseCtx);

      // Distinct side dirs, and neither is the shared source tree.
      expect(c.dir).not.toBe(b.dir);
      expect(c.dir).not.toBe(src);
      expect(b.dir).not.toBe(src);
      // sideCorpusView agrees on the side dir + its tsconfig roots.
      expect(sideCorpusView(candCtx).dir).toBe(c.dir);
      expect(sideCorpusView(candCtx).rootTsconfig).toBe(join(c.dir, "tsconfig.json"));

      // The copy actually materialized the corpus content in EACH side's tree.
      expect(existsSync(join(c.dir, "app", "m0040", "types.ts"))).toBe(true);
      expect(existsSync(join(b.dir, "app", "m0040", "types.ts"))).toBe(true);

      // A candidate-side edit does NOT touch the baseline tree or the source.
      writeFileSync(join(c.dir, "app", "m0040", "types.ts"), "// candidate-only edit\n");
      expect(readFileSync(join(b.dir, "app", "m0040", "types.ts"), "utf-8")).toContain("Props0040");
      expect(readFileSync(join(src, "app", "m0040", "types.ts"), "utf-8")).toContain("Props0040");
    } finally {
      rmSync(src, { recursive: true, force: true });
      rmSync(work, { recursive: true, force: true });
    }
  });
});

describe("warm-incremental edit forces a dependent diagnostic transition (a non-forcing/ignored edit fails)", () => {
  function makeCorpus(): { corpus: EnsuredCorpus; binDir: string; cleanup: () => void } {
    const dir = mkdtempSync(join(tmpdir(), "verter-pop-spec-"));
    const binDir = mkdtempSync(join(tmpdir(), "verter-pop-bin-"));
    mkdirSync(join(dir, "app", "m0040"), { recursive: true });
    writeFileSync(join(dir, "tsconfig.json"), "{}");
    // The shared `id: number;` field is the type-changing edit target the workload
    // retypes (`id: string;`) so dependents MUST re-diagnose.
    writeFileSync(
      join(dir, "app", "m0040", "types.ts"),
      "export interface Props0040 {\n  id: number;\n}\n",
    );
    // A verter-tsc binary the workload's findBinary() can resolve (the spawn seam
    // replaces the actual invocation, so the file content is irrelevant).
    const exe = process.platform === "win32" ? ".exe" : "";
    writeFileSync(join(binDir, `verter-tsc${exe}`), "stub");
    return {
      corpus: stubCorpus(dir),
      binDir,
      cleanup: () => {
        rmSync(dir, { recursive: true, force: true });
        rmSync(binDir, { recursive: true, force: true });
      },
    };
  }

  /** A child whose stdout emits scripted bytes (so parseDiagnosticSet sees diagnostics). */
  class FakeSpawnChildIO extends EventEmitter {
    pid = 4243;
    killed = 0;
    readonly outStream = new EventEmitter();
    readonly stdout = this.outStream as unknown as NodeJS.ReadableStream;
    readonly stderr = { on: (): void => {} } as unknown as NodeJS.ReadableStream;
    constructor(private readonly out: string) {
      super();
    }
    kill(): void {
      this.killed++;
    }
    run(code: number): void {
      if (this.out) this.outStream.emit("data", Buffer.from(this.out, "utf-8"));
      this.emit("close", code, null);
    }
  }

  /** A spawn seam scripting each child's {exit code, stdout} in call order. */
  function scriptedSpawnIO(runs: { code: number; stdout?: string }[]): SpawnChild {
    let n = 0;
    return () => {
      const run = runs[Math.min(n, runs.length - 1)];
      n++;
      const child = new FakeSpawnChildIO(run.stdout ?? "");
      setImmediate(() => child.run(run.code));
      return child as unknown as SampledChild;
    };
  }

  const TS2322 =
    "app/m0040/Comp0040_000.vue(3,5): error TS2322: Type 'string' is not assignable to type 'number'.";

  it("FAILS when the post-edit diagnostic set does NOT differ from the pre-edit set (an ignored/no-op edit)", async () => {
    const { corpus, binDir, cleanup } = makeCorpus();
    try {
      // population = clean; measured = SAME (empty) set ⇒ the edit produced no
      // dependent transition (the recheck ignored the edit). A non-forcing edit
      // must NOT read as a passing "warm recheck".
      const ctx: WorkloadContext = {
        corpus,
        binDir,
        threads: 1,
        spawnChild: scriptedSpawnIO([
          { code: 0, stdout: "" },
          { code: 0, stdout: "" },
        ]),
      };
      await expect(incrementalRetypecheck.runOnce(ctx)).rejects.toThrow(
        /did not alter|not rechecked|transition/i,
      );
    } finally {
      cleanup();
    }
  });

  it("RESOLVES and records the post-edit set when the type-changing edit DOES alter the dependent diagnostics", async () => {
    const { corpus, binDir, cleanup } = makeCorpus();
    try {
      // population = clean; measured = a real TS2322 the dependent re-diagnoses ⇒
      // a genuine transition. The correctness payload carries the post-edit set.
      const ctx: WorkloadContext = {
        corpus,
        binDir,
        threads: 1,
        spawnChild: scriptedSpawnIO([
          { code: 0, stdout: "" },
          { code: 2, stdout: TS2322 },
        ]),
      };
      const s = await incrementalRetypecheck.runOnce(ctx);
      expect(s.correctness?.exitCode).toBe(2);
      expect(s.correctness?.diagnostics.some((d) => /TS2322/.test(d))).toBe(true);
    } finally {
      cleanup();
    }
  });

  it("hard-fails when the warm-cache POPULATION run crashes (never silently measures a cold cache)", async () => {
    const { corpus, binDir, cleanup } = makeCorpus();
    try {
      // 1st spawn = population emits a spawn error (a crash) ⇒ exitCodeOrThrow on
      // the population run hard-fails before any measured recheck.
      const errSpawn: SpawnChild = () => {
        const child = new FakeSpawnChild();
        setImmediate(() =>
          child.emit("error", Object.assign(new Error("ENOENT"), { code: "ENOENT" })),
        );
        return child as unknown as SampledChild;
      };
      const ctx: WorkloadContext = { corpus, binDir, threads: 1, spawnChild: errSpawn };
      await expect(incrementalRetypecheck.runOnce(ctx)).rejects.toThrow(/population|spawn/i);
    } finally {
      cleanup();
    }
  });
});

describe("spawnWithRssSampling captures complete stdout (settles on close, after streams drain)", () => {
  // A stream-backed fake whose stdout can emit a chunk AFTER 'exit' but BEFORE 'close'.
  class FakeStreamChild extends EventEmitter {
    pid = 7777;
    readonly stdout = new EventEmitter() as unknown as NodeJS.ReadableStream;
    readonly stderr = new EventEmitter() as unknown as NodeJS.ReadableStream;
    kill(): void {
      /* no-op */
    }
  }

  it("includes a stdout chunk emitted AFTER 'exit' but BEFORE 'close' (not truncated)", async () => {
    // 'exit' can fire before stdout/stderr finish flushing; settling on 'exit'
    // truncated the captured diagnostic output. Settling on 'close' (after stdio
    // drains) captures the post-exit chunk.
    const spawnChild: SpawnChild = () => {
      const child = new FakeStreamChild();
      setImmediate(() => {
        (child.stdout as unknown as EventEmitter).emit("data", Buffer.from("PRE-"));
        child.emit("exit", 0, null);
        setImmediate(() => {
          (child.stdout as unknown as EventEmitter).emit("data", Buffer.from("POST"));
          child.emit("close", 0, null);
        });
      });
      return child as unknown as SampledChild;
    };
    const run = await spawnWithRssSampling("bin", [], HERE, 5_000, spawnChild);
    expect(run.stdout).toBe("PRE-POST"); // settling on 'exit' (not 'close') would capture "PRE-" only
    expect(run.status).toBe(0);
    expect(run.spawnError).toBeNull();
  });

  it("a spawn error still settles authoritatively even if a later 'close' arrives (no mask)", async () => {
    const spawnChild: SpawnChild = () => {
      const child = new FakeStreamChild();
      setImmediate(() => {
        child.emit("error", Object.assign(new Error("ENOENT"), { code: "ENOENT" }));
        child.emit("close", 0, null); // a later close must not mask the spawn failure
      });
      return child as unknown as SampledChild;
    };
    const run = await spawnWithRssSampling("bin", [], HERE, 5_000, spawnChild);
    expect(run.spawnError).not.toBeNull();
    expect(() => exitCodeOrThrow(run, "x")).toThrow(/spawn/i);
  });

  it("captures a stderr chunk emitted AFTER 'error' but BEFORE 'close' (not truncated)", async () => {
    // The normal path settles on 'close' (after stdio drains); the error path
    // settled immediately, truncating stderr flushed around a late close. The spawn
    // error stays AUTHORITATIVE (status null; exitCodeOrThrow throws), but capture
    // must wait for 'close' so the failure artifact's stderr is COMPLETE.
    const spawnChild: SpawnChild = () => {
      const child = new FakeStreamChild();
      setImmediate(() => {
        child.emit("error", Object.assign(new Error("boom"), { code: "EPIPE" }));
        (child.stderr as unknown as EventEmitter).emit(
          "data",
          Buffer.from("late-stderr-after-error"),
        );
        child.emit("close", 1, null);
      });
      return child as unknown as SampledChild;
    };
    const run = await spawnWithRssSampling("bin", [], HERE, 5_000, spawnChild);
    expect(run.spawnError).not.toBeNull();
    expect(run.status).toBeNull();
    expect(() => exitCodeOrThrow(run, "x")).toThrow(/spawn/i);
    // The late stderr chunk is captured COMPLETE — an immediate error-settle would
    // resolve before this chunk landed, truncating it.
    expect(run.stderr).toContain("late-stderr-after-error");
  });
});

describe("typecheckExitCodeOrThrow: a nonzero exit with no TS diagnostics is a crash, not a diagnostic exit", () => {
  const run = (over: Partial<SampledRun>): SampledRun => ({
    status: 0,
    signal: null,
    spawnError: null,
    timedOut: false,
    stdout: "",
    stderr: "",
    peakRssBytes: null,
    ...over,
  });

  it("nonzero status + NO parsed TS diagnostic ⇒ throws (a startup/crash exit)", () => {
    // A fatal verter-tsc/tsgo startup failure exits 1/101 with no `error TS####:`
    // diagnostics; a both-sides-identical crash must NOT compare as a matching empty
    // diagnostic set and pass the correctness gate.
    expect(() => typecheckExitCodeOrThrow(run({ status: 1, stdout: "" }), "", "x")).toThrow(
      /crash|startup|no parsed|diagnostic/i,
    );
    const panic = "thread 'main' panicked at src/main.rs:1:1\n";
    expect(() => typecheckExitCodeOrThrow(run({ status: 101, stderr: panic }), panic, "x")).toThrow(
      /crash|startup|no parsed|diagnostic/i,
    );
  });

  it("nonzero status + a real `error TS####:` diagnostic ⇒ returns the code (a valid diagnostic exit)", () => {
    const out =
      "app/Comp.vue.ts(3,5): error TS2322: Type 'string' is not assignable to type 'number'.";
    expect(typecheckExitCodeOrThrow(run({ status: 2, stdout: out }), out, "x")).toBe(2);
    // a no-file global config diagnostic (also `error TS####:`) stays valid.
    const cfg = "error TS5023: Unknown compiler option 'foo'.";
    expect(typecheckExitCodeOrThrow(run({ status: 1, stdout: cfg }), cfg, "x")).toBe(1);
  });

  it("status 0 + empty output ⇒ a valid clean pass (returns 0)", () => {
    expect(typecheckExitCodeOrThrow(run({ status: 0, stdout: "" }), "", "x")).toBe(0);
  });

  it("delegates spawn-error / timeout / signal / abnormal to exitCodeOrThrow (hard failure)", () => {
    expect(() => typecheckExitCodeOrThrow(run({ status: null, timedOut: true }), "", "x")).toThrow(
      /timed out/i,
    );
    expect(() =>
      typecheckExitCodeOrThrow(run({ status: null, signal: "SIGKILL" }), "", "x"),
    ).toThrow(/signal/i);
    expect(() =>
      typecheckExitCodeOrThrow(run({ status: null, spawnError: new Error("ENOENT") }), "", "x"),
    ).toThrow(/spawn/i);
  });
});
