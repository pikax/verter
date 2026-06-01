/**
 * Host-side musl detection must EXACTLY mirror the generated `dist/index.js`
 * loader's `isMusl()` (issue #90 round-3 finding 1). The matrix derivation
 * picks `linux-*-musl` vs `linux-*-gnu` for the CURRENT host from
 * {@link detectHostIsMusl}; if that decision diverges from the loader, the
 * host-tarball smoke and the loader-fallback spec select the WRONG triple on
 * a Linux CI runner whose `/usr/bin/ldd` read fails (e.g. a container without
 * ldd at that path) — exactly the case the round-2 host-smoke fix relied on.
 *
 * The detection algorithm is the SINGLE {@link resolveHostMusl}, a line-for
 * line port of the loader (`dist/index.js` lines 10-61). It is exercised here
 * with FAKED probes (fs / `process.report` / child-process) so each host shape
 * is driven deterministically — never depending on the `.node`-running host —
 * and asserted to match the loader's gnu/musl decision branch-for-branch.
 *
 * The loader's tri-state order (ported verbatim):
 *   1. `isMuslFromFilesystem` — read `/usr/bin/ldd`; musl iff it includes
 *      `"musl"`; `null` (defer) on any read error.
 *   2. `isMuslFromReport` — `process.report.getReport()`: no report ⇒ `null`;
 *      `header.glibcVersionRuntime` present ⇒ `false` (gnu, short-circuits
 *      BEFORE sharedObjects); else an `ld-musl-`/`libc.musl-` shared object ⇒
 *      `true`; else `false`.
 *   3. `isMuslFromChildProcess` — `ldd --version`; musl iff it includes
 *      `"musl"`; `false` on any error (terminal "don't know ⇒ gnu").
 */

import { describe, expect, it } from "vitest";
import { type HostMuslProbes, resolveHostMusl } from "./platforms.ts";

/** A probe set whose every source is inconclusive (throws / returns null). */
function inconclusiveProbes(): HostMuslProbes {
  return {
    readLddBinary: () => {
      throw new Error("no /usr/bin/ldd");
    },
    getReport: () => null,
    lddVersion: () => {
      throw new Error("no ldd in PATH");
    },
  };
}

/**
 * The PRE-FIX `detectHostIsMusl` catch-path, reproduced VERBATIM, so the
 * fail-before/pass-after property is EXECUTABLE rather than a prose claim. The
 * old code, once the ldd read failed, fetched the report and then:
 *   - returned gnu(false) iff `report.header.glibcVersionRuntime` was present;
 *   - otherwise returned musl(true) UNCONDITIONALLY — it never consulted
 *     `sharedObjects` and never ran the child-process fallback.
 * So it mis-classified as musl: (a) any host with NO report facility, and
 * (b) any gnu host whose report lacked `glibcVersionRuntime` but listed only
 * glibc shared objects. We feed it the SAME faked report (or null) the loader
 * sees and assert it diverges on exactly those cases.
 *
 * @param report the value the old `process.report?.getReport()` would yield,
 *        or `null` when there is no report facility (old code: `?.` short
 *        circuits to `undefined`, so the `if` is falsy ⇒ returns musl).
 */
function oldDetectHostIsMuslOnLddFailure(
  report: { header?: { glibcVersionRuntime?: string } } | null,
): boolean {
  if (report?.header?.glibcVersionRuntime) return false;
  return true;
}

describe("issue #90 — host musl detection mirrors the generated loader's isMusl()", () => {
  // ---- Probe 1: filesystem ldd wins when readable --------------------------
  it("glibc-with-ldd: a readable ldd WITHOUT 'musl' ⇒ gnu (false), report never consulted", () => {
    let reportConsulted = false;
    const musl = resolveHostMusl({
      readLddBinary: () => "/lib64/ld-linux-x86-64.so.2 => GNU C Library",
      getReport: () => {
        reportConsulted = true;
        return { header: {}, sharedObjects: ["ld-musl-x86_64.so.1"] };
      },
      lddVersion: () => {
        throw new Error("should not be reached");
      },
    });
    expect(musl).toBe(false);
    // fs probe returned a definite boolean ⇒ the loader short-circuits and
    // never touches the report. Prove that ordering.
    expect(reportConsulted).toBe(false);
  });

  it("musl-with-ldd: a readable ldd containing 'musl' ⇒ musl (true), report never consulted", () => {
    let reportConsulted = false;
    const musl = resolveHostMusl({
      readLddBinary: () => "musl libc (x86_64)\nVersion 1.2.4",
      getReport: () => {
        reportConsulted = true;
        return { header: { glibcVersionRuntime: "2.35" }, sharedObjects: [] };
      },
      lddVersion: () => {
        throw new Error("should not be reached");
      },
    });
    expect(musl).toBe(true);
    // Even though the (faked) report would say gnu, the fs probe already
    // decided musl — the loader never reaches the report. Proves fs precedence.
    expect(reportConsulted).toBe(false);
  });

  // ---- Probe 2: process.report decides when ldd read fails -----------------
  // glibcVersionRuntime present ⇒ gnu. This sub-case the OLD code already got
  // right (its only correct report branch), so we do NOT claim a divergence
  // here — we PIN that the new detector still returns gnu, and document that
  // the old code agreed, to bound exactly which report sub-cases the fix flips.
  it("glibc-via-report: ldd read fails but report has glibcVersionRuntime ⇒ gnu (false)", () => {
    const report = { header: { glibcVersionRuntime: "2.35" }, sharedObjects: [] };
    let childProcessConsulted = false;
    const musl = resolveHostMusl({
      readLddBinary: () => {
        throw new Error("ldd read fails (e.g. container without /usr/bin/ldd)");
      },
      getReport: () => report,
      lddVersion: () => {
        childProcessConsulted = true;
        throw new Error("should not be reached");
      },
    });
    // The loader returns gnu here (header.glibcVersionRuntime present).
    expect(musl).toBe(false);
    // A present report is a definite answer ⇒ the child-process fallback is
    // never reached.
    expect(childProcessConsulted).toBe(false);
    // The OLD code ALSO returned gnu for this sub-case (glibcVersionRuntime
    // present) — so no divergence here. The genuine fail-before cases (the
    // ones the fix flips) are `gnu-via-report` (no glibc marker, glibc-only
    // shared objects) and `childprocess-gnu` (no report facility), asserted
    // below with explicit divergence guards.
    expect(oldDetectHostIsMuslOnLddFailure(report)).toBe(false);
  });

  it("musl-via-report: ldd read fails, no glibcVersionRuntime, an ld-musl shared object ⇒ musl (true)", () => {
    const musl = resolveHostMusl({
      readLddBinary: () => {
        throw new Error("ldd read fails");
      },
      getReport: () => ({ header: {}, sharedObjects: ["ld-musl-x86_64.so.1"] }),
      lddVersion: () => {
        throw new Error("should not be reached");
      },
    });
    expect(musl).toBe(true);
  });

  it("musl-via-report (libc.musl- variant): the OTHER musl marker the loader's isFileMusl matches", () => {
    const musl = resolveHostMusl({
      readLddBinary: () => {
        throw new Error("ldd read fails");
      },
      getReport: () => ({
        header: {},
        sharedObjects: ["/usr/lib/libc.musl-x86_64.so.1"],
      }),
      lddVersion: () => {
        throw new Error("should not be reached");
      },
    });
    expect(musl).toBe(true);
  });

  // The case the pre-fix detector got SILENTLY wrong the other way: a report
  // exists, has NO glibcVersionRuntime, and has NO musl shared object (only
  // glibc ones). The loader returns gnu(false); the old code returned musl
  // (its "absent glibcVersionRuntime ⇒ musl" shortcut). Fail-before/pass-after.
  it("gnu-via-report (no glibc marker, no musl shared object) ⇒ gnu (false) [was musl before]", () => {
    const report = {
      header: {},
      sharedObjects: ["/lib/x86_64-linux-gnu/libc.so.6", "/lib64/ld-linux-x86-64.so.2"],
    };
    const musl = resolveHostMusl({
      readLddBinary: () => {
        throw new Error("ldd read fails");
      },
      getReport: () => report,
      lddVersion: () => {
        throw new Error("should not be reached");
      },
    });
    // Loader: sharedObjects has no ld-musl/libc.musl entry ⇒ false.
    expect(musl).toBe(false);
    // Fail-before/pass-after: the OLD rule, given this very report (no
    // glibcVersionRuntime), returned musl(true) — it never inspected
    // sharedObjects. The loader (and new detector) returns gnu(false).
    expect(oldDetectHostIsMuslOnLddFailure(report)).toBe(true);
    expect(musl).not.toBe(oldDetectHostIsMuslOnLddFailure(report));
  });

  // ---- Probe 3: child-process ldd --version is the last resort -------------
  // Reached ONLY when fs AND report are BOTH unavailable (no report facility).
  it("childprocess-musl: ldd read fails, NO report facility, `ldd --version` says musl ⇒ musl (true)", () => {
    let lddVersionCalled = false;
    const musl = resolveHostMusl({
      readLddBinary: () => {
        throw new Error("ldd read fails");
      },
      getReport: () => null,
      lddVersion: () => {
        lddVersionCalled = true;
        return "musl libc (x86_64)\nVersion 1.2.4";
      },
    });
    expect(musl).toBe(true);
    // The child-process fallback genuinely ran (fs + report both inconclusive).
    expect(lddVersionCalled).toBe(true);
  });

  it("childprocess-gnu: ldd read fails, NO report facility, `ldd --version` says glibc ⇒ gnu (false) [was musl before]", () => {
    const musl = resolveHostMusl({
      readLddBinary: () => {
        throw new Error("ldd read fails");
      },
      getReport: () => null,
      lddVersion: () => "ldd (Ubuntu GLIBC 2.35-0ubuntu3) 2.35",
    });
    expect(musl).toBe(false);
    // Fail-before/pass-after: with NO report facility the OLD code returned
    // musl(true) immediately (it never reached the child-process probe). The
    // loader (and new detector) consult `ldd --version`, which here says glibc
    // ⇒ gnu(false). This is the second case the fix flips.
    expect(oldDetectHostIsMuslOnLddFailure(null)).toBe(true);
    expect(musl).not.toBe(oldDetectHostIsMuslOnLddFailure(null));
  });

  it("all-inconclusive: fs read fails, no report, `ldd --version` throws ⇒ gnu (false) [loader terminal default; was musl before]", () => {
    expect(resolveHostMusl(inconclusiveProbes())).toBe(false);
    // Fail-before/pass-after: the OLD code, on a host with no report facility,
    // returned musl(true) regardless of the child-process probe. The loader's
    // terminal default (every probe inconclusive) is gnu(false).
    expect(oldDetectHostIsMuslOnLddFailure(null)).toBe(true);
    expect(resolveHostMusl(inconclusiveProbes())).not.toBe(oldDetectHostIsMuslOnLddFailure(null));
  });

  // ---- Ordering self-proof: report is NOT consulted before fs --------------
  // A focused proof that the tri-state short-circuit order matches the loader:
  // when fs gives a definite answer, neither report nor child-process runs;
  // when fs defers and report gives a definite answer, child-process does not
  // run. Without correct ordering a glibc host with a musl-looking report (or
  // vice versa) would be misclassified.
  it("ordering: a definite fs answer wins over a contradicting report (no cross-leak)", () => {
    // fs says gnu, report says musl ⇒ fs wins ⇒ gnu.
    expect(
      resolveHostMusl({
        readLddBinary: () => "GNU C Library",
        getReport: () => ({ header: {}, sharedObjects: ["ld-musl-x86_64.so.1"] }),
        lddVersion: () => {
          throw new Error("unreached");
        },
      }),
    ).toBe(false);
    // fs says musl, report says gnu ⇒ fs wins ⇒ musl.
    expect(
      resolveHostMusl({
        readLddBinary: () => "musl",
        getReport: () => ({ header: { glibcVersionRuntime: "2.35" }, sharedObjects: [] }),
        lddVersion: () => {
          throw new Error("unreached");
        },
      }),
    ).toBe(true);
  });
});
