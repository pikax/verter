// Self-test: the authoritative (fail-closed) acceptance mode.
//
// Default behavior is UNCHANGED: an axis whose environment prerequisite is
// absent reports `skipped` with a reason and does not fail — ordinary local
// development keeps its skip-with-reason discipline. The authoritative mode
// is the opt-in fail-closed contract: a skipped axis is a hard failure, so
// a consumer that must PROVE every acceptance axis genuinely executed
// (rather than silently narrowing) can demand it. Both halves are asserted
// against the same inputs, so the mode itself is the only variable.

import { describe, expect, it, afterAll } from "vitest";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { compareArtifacts, cleanupLinkScratch } from "../src/compare.mjs";
import { checkCandidate } from "../src/check-candidate.mjs";
import { cleanupScratch as cleanupVueScratch } from "../src/execute-vue-runtime.mjs";
import { cleanupScratch as cleanupVaporScratch } from "../src/execute-vue-vapor.mjs";
import { oracleLinkBaseDir } from "../src/oracle-install.mjs";
import { readGoldenManifest, readGoldenByName } from "../src/golden-store.mjs";
import { MAPPING_PROFILES } from "../src/mapping-oracle.mjs";
import { GOLDENS_ROOT, HARNESS_ROOT } from "../src/paths.mjs";

const TRIVIAL = { code: 'import { ref } from "vue";\nexport default ref;', diagnostics: [] };
// The mapping axis is self-referential: it needs the AUTHORED source it is
// meant to be a map of. TRIVIAL is hand-written and requests no map, so the
// axis genuinely runs here and asserts the compliant absent/not-requested
// pair — it is not standing in for a real mapped compilation (those are the
// checkCandidate cases below and test/mapping-oracle*.spec.mjs).
const TRIVIAL_MAPPING_CONTEXT = {
  sourceMapRequested: false,
  fixture: {
    path: "fixtures/vue/basic-interpolation.vue",
    absolutePath: path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
  },
  sourceResolveBases: [HARNESS_ROOT],
  profile: MAPPING_PROFILES["vue:vdom"],
  anchors: [],
};
const SCRATCH = mkdtempSync(path.join(tmpdir(), "bf2-authoritative-"));

afterAll(() => {
  cleanupVueScratch();
  cleanupVaporScratch();
  cleanupLinkScratch(oracleLinkBaseDir("vue"));
  rmSync(SCRATCH, { recursive: true, force: true });
});

function goldenNameWhere(predicate) {
  const manifest = readGoldenManifest(GOLDENS_ROOT);
  const name = Object.keys(manifest.entries).find(predicate);
  expect(name).toBeDefined();
  return name;
}

describe("compareArtifacts axis statuses + authoritative option", () => {
  it("default: a skipped link axis is informational, not a failure", async () => {
    const report = await compareArtifacts(TRIVIAL, TRIVIAL, {});
    expect(report.verdict).toBe("pass");
    expect(report.axes.link.status).toBe("skipped");
    expect(report.axes.link.reason).toContain("no linkBaseDir");
    // Same skip-with-reason discipline for the mapping axis when no
    // authored-source context is supplied.
    expect(report.axes.mapping.status).toBe("skipped");
    expect(report.axes.mapping.reason).toContain("authored-source mapping context");
    for (const axis of ["parse", "structural", "diagnostics"]) {
      expect(report.axes[axis].status).toBe("ran");
    }
  });

  it("authoritative: the SAME skipped link axis is a hard failure naming the axis", async () => {
    const report = await compareArtifacts(TRIVIAL, TRIVIAL, {
      authoritative: true,
      mappingContext: TRIVIAL_MAPPING_CONTEXT,
    });
    expect(report.verdict).toBe("fail");
    expect(report.reasons.some((r) => r.startsWith("authoritative mode: link axis skipped"))).toBe(
      true,
    );
  });

  it("authoritative passes when every axis genuinely runs", async () => {
    const report = await compareArtifacts(TRIVIAL, TRIVIAL, {
      linkBaseDir: oracleLinkBaseDir("vue"),
      authoritative: true,
      mappingContext: TRIVIAL_MAPPING_CONTEXT,
    });
    expect(report.verdict).toBe("pass");
    expect(report.axes.link.status).toBe("ran");
    expect(report.link.ok).toBe(true);
    expect(report.axes.mapping.status).toBe("ran");
    expect(report.mapping.ok).toBe(true);
  });
});

describe("checkCandidate — full acceptance over a committed golden", () => {
  it("a candidate identical to the golden passes with link ran and runtime not-applicable (vdom)", async () => {
    const name = goldenNameWhere((n) => n.includes("__vdom__map1__prod0"));
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const result = await checkCandidate({
      goldenName: name,
      candidate: { code: golden.code, map: golden.map, diagnostics: golden.diagnostics },
      authoritative: true,
    });
    expect(result.reasons).toEqual([]);
    expect(result.verdict).toBe("pass");
    expect(result.axes.link.status).toBe("ran");
    expect(result.axes.runtime.status).toBe("not-applicable");
  });

  it("an SSR golden's runtime axis RUNS and passes for an identical candidate", async () => {
    const name = goldenNameWhere((n) => n.includes("__ssr__map0__prod0"));
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const result = await checkCandidate({
      goldenName: name,
      candidate: { code: golden.code, map: golden.map, diagnostics: golden.diagnostics },
      authoritative: true,
    });
    expect(result.reasons).toEqual([]);
    expect(result.verdict).toBe("pass");
    expect(result.axes.runtime.status).toBe("ran");
  });

  it("a candidate identical to a VAPOR golden passes end-to-end under authoritative — link axis included", async () => {
    // Regression lock: the link axis must resolve a vapor artifact's `vue`
    // imports against the with-vapor runtime entry (the Node CJS `vue`
    // entry publishes no vapor exports), so a byte-identical-to-official
    // vapor candidate passes the FULL authoritative acceptance — no axis
    // skipped, no false link failure, runtime genuinely mounted.
    const name = goldenNameWhere((n) => n.includes("__vapor__map1__prod0"));
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const result = await checkCandidate({
      goldenName: name,
      candidate: { code: golden.code, map: golden.map, diagnostics: golden.diagnostics },
      authoritative: true,
    });
    expect(result.reasons).toEqual([]);
    expect(result.verdict).toBe("pass");
    expect(result.axes.link.status).toBe("ran");
    expect(result.report.link.ok).toBe(true);
    expect(result.axes.runtime.status).toBe("ran");
  });

  it("EVERY committed vapor golden passes end-to-end as its own candidate — slot-geometry fixtures included", async () => {
    // Regression lock for the link-axis/runtime-axis module-cache
    // interference class: the link axis imports the with-vapor runtime to
    // inspect its exports, and that module captures `document` once at
    // evaluation — an import before the shared document exists pins the
    // capture to `null` and every mount that reaches the runtime's
    // VDOM-fragment path (the slots fixture's fallback/multi-root
    // geometry) throws. basic-interpolation and props-emit never reach
    // that path, so this MUST iterate the full vapor set rather than the
    // manifest's first match — a single-fixture pick is exactly the gap
    // that hid the defect.
    const manifest = readGoldenManifest(GOLDENS_ROOT);
    const vaporNames = Object.keys(manifest.entries).filter((n) => n.includes("__vapor__"));
    expect(vaporNames.some((n) => n.startsWith("vue/slots__vapor__"))).toBe(true);
    expect(vaporNames.length).toBeGreaterThanOrEqual(12);
    for (const name of vaporNames) {
      const golden = readGoldenByName(GOLDENS_ROOT, name);
      const result = await checkCandidate({
        goldenName: name,
        candidate: { code: golden.code, map: golden.map, diagnostics: golden.diagnostics },
        authoritative: true,
      });
      expect(result.reasons, name).toEqual([]);
      expect(result.verdict, name).toBe("pass");
      expect(result.axes.link.status, name).toBe("ran");
      expect(result.axes.runtime.status, name).toBe("ran");
    }
    // 12 full acceptance runs (link + two mounts each) exceed the default
    // per-test budget on a cold oracle scratch; the loop is inherently
    // multi-candidate, not slow-by-defect.
  }, 60_000);

  it("a vapor candidate stripped of its `__vapor` marker fails the RUNTIME axis behaviorally", async () => {
    const name = goldenNameWhere(
      (n) => n.startsWith("vue/basic-interpolation__vapor") && n.includes("map0__prod0"),
    );
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const mutated = golden.code.replace("\n  __vapor: true,", "");
    expect(mutated).not.toBe(golden.code); // the plant applied
    const result = await checkCandidate({
      goldenName: name,
      candidate: { code: mutated, map: golden.map, diagnostics: golden.diagnostics },
      authoritative: true,
    });
    expect(result.verdict).toBe("fail");
    // The failure must come from a runtime axis that genuinely EXECUTED —
    // not from an authoritative skip or any other axis standing in for it.
    expect(result.axes.runtime.status).toBe("ran");
    // The unmarked component takes the VDOM interop path: the acceptance
    // primitive itself observes the behavioral divergence (wrong DOM and/or
    // runtime warnings), not only the structural one.
    expect(result.reasons.some((r) => r.startsWith("runtime divergence"))).toBe(true);
  });

  it("a candidate rendering DIFFERENT html fails the runtime axis specifically", async () => {
    const name = goldenNameWhere(
      (n) => n.startsWith("vue/basic-interpolation__ssr") && n.includes("map0__prod0"),
    );
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    // The v-else branch's static text rides through SSR string buffers;
    // mutating it changes rendered output while remaining valid JS.
    const mutated = golden.code.replaceAll("zero", "MUTATED-zero");
    expect(mutated).not.toBe(golden.code); // the plant applied
    const result = await checkCandidate({
      goldenName: name,
      candidate: { code: mutated, map: golden.map, diagnostics: golden.diagnostics },
    });
    expect(result.verdict).toBe("fail");
    expect(
      result.reasons.some((r) => r.startsWith("runtime divergence: rendered HTML differs")),
    ).toBe(true);
  });
});

describe("check-candidate CLI — exit codes", () => {
  function runCli(name, candidate, { authoritative = false, env = {} } = {}) {
    const candidatePath = path.join(
      SCRATCH,
      `candidate-${Math.random().toString(36).slice(2)}.json`,
    );
    writeFileSync(candidatePath, JSON.stringify(candidate));
    const args = ["bin/check-candidate.mjs", "--golden", name, "--candidate", candidatePath];
    if (authoritative) args.push("--authoritative");
    return spawnSync(process.execPath, args, {
      cwd: HARNESS_ROOT,
      encoding: "utf8",
      env: { ...process.env, ...env },
    });
  }

  it("exit 0 on a passing candidate", () => {
    const name = goldenNameWhere((n) => n.includes("__vdom__map0__prod0"));
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const result = runCli(name, {
      code: golden.code,
      map: golden.map,
      diagnostics: golden.diagnostics,
    });
    expect(result.status).toBe(0);
    expect(JSON.parse(result.stdout).verdict).toBe("pass");
  });

  it("exit 0 for a self-candidate VAPOR golden under --authoritative (CLI regression: the link axis must not false-fail vapor)", () => {
    const name = goldenNameWhere((n) => n.includes("__vapor__map0__prod0"));
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const result = runCli(
      name,
      { code: golden.code, map: golden.map, diagnostics: golden.diagnostics },
      { authoritative: true },
    );
    expect(result.status).toBe(0);
    const report = JSON.parse(result.stdout);
    expect(report.verdict).toBe("pass");
    expect(report.axes.link.status).toBe("ran");
    expect(report.axes.runtime.status).toBe("ran");
  });

  it("exit 1 on a structurally divergent candidate", () => {
    const name = goldenNameWhere((n) => n.includes("__vdom__map0__prod0"));
    const result = runCli(name, { code: "export default 42;", diagnostics: [] });
    expect(result.status).toBe(1);
    expect(JSON.parse(result.stdout).verdict).toBe("fail");
  });

  it("oracle-unavailable environment: default run exits 0 with skips reported; --authoritative exits 2", () => {
    const name = goldenNameWhere((n) => n.includes("__ssr__map0__prod0"));
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const candidate = { code: golden.code, map: golden.map, diagnostics: golden.diagnostics };
    // Point the isolated-install roots at empty scratch so realization is
    // impossible WITHOUT touching the real installs: link + runtime must
    // SKIP, not fail, by default — and hard-fail under --authoritative.
    const env = {
      BF2_ORACLE_INSTALLS: path.join(SCRATCH, "no-installs"),
      BF2_ORACLE_NPM_CACHE: path.join(SCRATCH, "no-cache"),
    };
    const informational = runCli(name, candidate, { env });
    expect(informational.status).toBe(0);
    const report = JSON.parse(informational.stdout);
    expect(report.axes.link.status).toBe("skipped");
    expect(report.axes.runtime.status).toBe("skipped");

    const authoritative = runCli(name, candidate, { authoritative: true, env });
    expect(authoritative.status).toBe(2);
    const strict = JSON.parse(authoritative.stdout);
    expect(strict.verdict).toBe("fail");
    expect(
      strict.reasons.some((r) => r.startsWith("authoritative mode: runtime axis skipped")),
    ).toBe(true);
    expect(strict.reasons.some((r) => r.startsWith("authoritative mode: link axis skipped"))).toBe(
      true,
    );
  });

  it("BF2_AUTHORITATIVE=1 arms the same fail-closed behavior as the flag", () => {
    const name = goldenNameWhere((n) => n.includes("__ssr__map0__prod0"));
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const result = runCli(
      name,
      { code: golden.code, map: golden.map, diagnostics: golden.diagnostics },
      {
        env: {
          BF2_AUTHORITATIVE: "1",
          BF2_ORACLE_INSTALLS: path.join(SCRATCH, "no-installs"),
          BF2_ORACLE_NPM_CACHE: path.join(SCRATCH, "no-cache"),
        },
      },
    );
    expect(result.status).toBe(2);
  });
});
