/**
 * Hermetic in-process probe of the REAL NAPI-generated `dist/index.js`
 * loader. Executes the loader source in a fresh `node:vm` context with a
 * faked `process` (platform/arch/musl signal), an empty co-located `dist`
 * (so the local `.node` branch always misses), and an intercepted
 * `require` that records every module id the loader asks for, separately
 * records the ids whose sentinel it actually returned, and returns a
 * sentinel for the optional-dependency package. This lets a test assert,
 * per platform, EXACTLY which `@verter/native-<triple>` package the loader
 * ACCEPTS as the final binding — with no installer, no network, and no real
 * binary.
 *
 * Three strictly-ordered signals are surfaced, weakest to strongest:
 *   - `attemptedDepRequests` — the id was passed to `require` (it may have
 *     thrown MODULE_NOT_FOUND);
 *   - `returnedByRequireDepRequests` — `require` RETURNED the sentinel for
 *     the id (regardless of whether the loader then threw);
 *   - `acceptedPublishedDepRequests` / `accepted` / `acceptedSentinelPackage`
 *     — the loader exported the sentinel AND completed WITHOUT throwing.
 * Only the ACCEPTED signals prove a successful load: a loader that returns
 * the sentinel and then throws (a post-export version check, or any
 * export-then-throw shape) appears in the first two but NOT in ACCEPTED.
 *
 * It is deliberately NOT a paraphrase of the loader: it runs the loader's
 * own bytes (read verbatim from the built `dist/index.js`), so the
 * assertions characterize real loader behavior.
 */

import { readFileSync } from "node:fs";
import { createContext, runInContext } from "node:vm";
import { join } from "node:path";
import { PACKAGE_DIR } from "../platforms.ts";

const SENTINEL = Symbol.for("verter-native-loader-probe-sentinel");

export interface ProbeFakePlatform {
  /** Faked `process.platform`. */
  readonly platform: NodeJS.Platform;
  /** Faked `process.arch`. */
  readonly arch: string;
  /**
   * For linux only: whether the faked host should resolve as musl.
   *
   * The generated `dist/index.js` `isMusl()` consults three sources in
   * order, short-circuiting on the first that returns non-null:
   *   1. `isMuslFromFilesystem()` — `readFileSync('/usr/bin/ldd')` → `null`
   *      on throw;
   *   2. `isMuslFromReport()` — never null once a report exists;
   *   3. `isMuslFromChildProcess()` — `ldd --version`, `false` on throw.
   * We force (1) and (3) to MISS (the fs guard throws for the ldd path;
   * `child_process.execSync` throws), so the musl/gnu decision rests
   * SOLELY on the faked `process.report` we drive in
   * {@link makeFakeProcess} — never on the real host. The `.node` running
   * the suite is therefore irrelevant to the simulated libc.
   */
  readonly musl?: boolean;
}

export interface ProbeResult {
  /** Every module id the loader passed to `require`, in call order. */
  readonly requireCalls: string[];
  /**
   * The `@verter/native-*` optional-dependency package ids the loader
   * ACCEPTED as the final binding: the probe's intercepted `require`
   * RETURNED a sentinel for them AND the loader completed WITHOUT throwing
   * ({@link threwMessage} is `null`). This is the ACCEPTED set — it gates on
   * no-throw, so a loader that returns the sentinel from `require` but then
   * throws (e.g. a post-resolve version-check, or any export-then-throw
   * loader) contributes NOTHING here (the set is empty). For a single
   * platform a clean generated loader accepts exactly one published package;
   * earlier attempts at unpublished aliases (e.g.
   * `@verter/native-darwin-universal`) throw `MODULE_NOT_FOUND` in a real
   * install and fall through, so they never enter this set.
   *
   * Use this to prove "the right package was ACCEPTED". For the weaker
   * "require returned a sentinel, regardless of a later throw" signal (a
   * diagnostic), see {@link returnedByRequireDepRequests}; for "the id was
   * merely requested", see {@link attemptedDepRequests}.
   */
  readonly acceptedPublishedDepRequests: string[];
  /**
   * The `@verter/native-*` package ids whose sentinel the intercepted
   * `require` RETURNED, REGARDLESS of whether the loader subsequently threw.
   * This is a DIAGNOSTIC signal (it answers "did require hand back the
   * sentinel for this id"), strictly weaker than
   * {@link acceptedPublishedDepRequests}: a loader that returns the sentinel
   * and THEN throws still lists the id here but is NOT accepted. Do NOT use
   * this to prove acceptance — assert {@link accepted} / membership in
   * {@link acceptedPublishedDepRequests} instead.
   */
  readonly returnedByRequireDepRequests: string[];
  /**
   * Every `@verter/native-*` package id the loader TRIED to require
   * (published or not), regardless of whether the require returned a
   * sentinel or threw `MODULE_NOT_FOUND`. Diagnostics / discrimination
   * only: do NOT use this to prove resolution — an unpublished-alias
   * attempt (`darwin-universal`) appears here yet never resolves.
   */
  readonly attemptedDepRequests: string[];
  /**
   * Whether the loader ACCEPTED our sentinel optional-dependency as the final
   * binding: it `module.exports`'d the sentinel AND completed WITHOUT throwing
   * ({@link threwMessage} is `null`). This is the authoritative "the loader
   * accepted a published package" signal. The no-throw gate is essential: a
   * loader that exports the sentinel and THEN throws (a post-export version
   * check, or any export-then-throw shape) is NOT accepted — `accepted` is
   * `false` and {@link threwMessage} is set, so a caller cannot mistake a
   * thrown load for a success.
   */
  readonly accepted: boolean;
  /**
   * The single `@verter/native-*` package id the loader ACCEPTED as the final
   * binding (read back from the exported sentinel's `requestedId`), or `null`
   * when the loader did not cleanly accept one — i.e. it never exported the
   * sentinel OR it threw ({@link threwMessage} non-null). For a single
   * platform a clean generated loader accepts exactly one published package;
   * this names it. Callers assert this equals the platform's expected package
   * to prove the CORRECT package was accepted (not merely attempted, and not
   * exported-then-thrown). It is `null` for both the version-check-throw
   * (sentinel never exported) AND the export-then-throw (sentinel exported but
   * the load threw) shapes.
   */
  readonly acceptedSentinelPackage: string | null;
  /** The thrown top-level load error message, if the loader threw instead. */
  readonly threwMessage: string | null;
  /**
   * The thrown error's `cause` chain (each layer's message, outermost
   * first). The napi loader puts the precise root cause here — the raw
   * module-not-found, or the `Unsupported OS/architecture` detail — while
   * keeping the actionable guidance at the top level.
   */
  readonly threwCauseChain: string[];
}

/** Read the real built loader source. Throws if the build has not run. */
export function readGeneratedLoaderSource(): string {
  const loaderPath = join(PACKAGE_DIR, "dist", "index.js");
  try {
    return readFileSync(loaderPath, "utf8");
  } catch {
    throw new Error(
      `Generated napi loader not found at ${loaderPath}. ` +
        `Run \`pnpm --filter @verter/native build:debug\` (or build) first.`,
    );
  }
}

/**
 * Build a faked `process` object that presents the requested platform and
 * (for linux) musl signal to the generated loader. Everything the loader
 * actually reads off `process` is provided; nothing else.
 */
function makeFakeProcess(fake: ProbeFakePlatform): Record<string, unknown> {
  const isMusl = fake.platform === "linux" && fake.musl === true;
  return {
    platform: fake.platform,
    arch: fake.arch,
    // The loader gates win32-x64 msvc-vs-gnu on these; default (msvc) is
    // the absence of the shared-library signals, so leave `config` empty.
    config: { variables: {} },
    env: {},
    // Drive the loader's `isMuslFromReport()` EXACTLY — and only — through
    // the two report fields it reads (dist/index.js, see `isMuslFromReport`):
    //   1. `report.header.glibcVersionRuntime` — if present, the loader
    //      returns `false` (gnu) immediately and never inspects
    //      `sharedObjects`. So the gnu case is decided SOLELY by this field;
    //      we still ship `sharedObjects: []` because the loader does
    //      `Array.isArray(report.sharedObjects)` and an absent/non-array
    //      value is not what a real glibc report yields.
    //   2. `report.sharedObjects.some(isFileMusl)` — reached only when
    //      `glibcVersionRuntime` is ABSENT; the loader's `isFileMusl`
    //      matches `ld-musl-` / `libc.musl-`. The musl case is decided
    //      SOLELY by an entry matching that predicate, with the header
    //      carrying NO `glibcVersionRuntime` so check (1) does not
    //      short-circuit. We provide exactly ONE musl shared object and no
    //      other musl/gnu signal (the fs `/usr/bin/ldd` and child_process
    //      `ldd --version` sources are forced to MISS in `fakeRequire`).
    // `excludeNetwork` is a settable property because the loader assigns
    // `process.report.excludeNetwork = true` before calling `getReport()`.
    report: {
      excludeNetwork: false,
      getReport() {
        return isMusl
          ? { header: {}, sharedObjects: ["ld-musl-x86_64.so.1"] }
          : { header: { glibcVersionRuntime: "2.35" }, sharedObjects: [] };
      },
    },
  };
}

/**
 * Read the published optional-dependency package names from the real
 * `package.json#optionalDependencies`. The probe satisfies `require()` for
 * THESE packages only — everything else (including napi's `*-universal`
 * alias attempts that we do not publish) misses, exactly as in a real
 * install where only these packages exist on disk.
 */
export function readPublishedOptionalDeps(
  packageJsonPath = join(PACKAGE_DIR, "package.json"),
): Set<string> {
  const pkg = JSON.parse(readFileSync(packageJsonPath, "utf8")) as {
    optionalDependencies?: Record<string, string>;
  };
  return new Set(
    Object.keys(pkg.optionalDependencies ?? {}).filter((name) =>
      name.startsWith("@verter/native-"),
    ),
  );
}

/**
 * Execute the real generated loader for a single faked platform and report
 * which optional-dependency package it resolved to.
 *
 * @param loaderSource verbatim `dist/index.js` source (from
 *        {@link readGeneratedLoaderSource}).
 * @param fake the platform/arch/musl signal to present.
 * @param published the set of package names that exist in a real install
 *        (defaults to `package.json#optionalDependencies`). The probe
 *        returns the sentinel ONLY for these; any other `@verter/native-*`
 *        attempt misses and falls through.
 */
export function probeLoaderForPlatform(
  loaderSource: string,
  fake: ProbeFakePlatform,
  published: Set<string> = readPublishedOptionalDeps(),
): ProbeResult {
  const requireCalls: string[] = [];
  // The `@verter/native-*` package ids for which the intercepted require
  // actually RETURNED a sentinel (the package was in the published set, so
  // no MODULE_NOT_FOUND). This is "require returned a sentinel", distinct
  // from "attempted" — a loader that requests the right package but whose
  // require throws never records the id here. It is ALSO distinct from
  // "accepted": a require can return the sentinel and the loader can THEN
  // throw, so the no-throw gate is applied later (see the ACCEPTED fields)
  // and this raw list is surfaced as `returnedByRequireDepRequests`.
  const returnedByRequireRequests: string[] = [];

  const moduleNotFound = (id: string): never => {
    const err = new Error(`Cannot find module '${id}'`) as NodeJS.ErrnoException;
    err.code = "MODULE_NOT_FOUND";
    throw err;
  };

  // Intercept require: the loader tries `require('./verter-native.*.node')`
  // (local — we force a miss with an empty dist) then
  // `require('@verter/native-<triple>')` (the optional dep). We satisfy
  // ONLY the published optional deps (returning a sentinel) plus their
  // `/package.json` version check; everything else misses.
  const fakeRequire = (id: string): unknown => {
    requireCalls.push(id);

    // Local co-located binary: always missing (empty dist).
    if (id.startsWith("./") && id.endsWith(".node")) {
      return moduleNotFound(id);
    }

    // node core the loader uses. The loader's filesystem-based musl probe
    // reads `/usr/bin/ldd`; we MUST NOT let it read the real host fs, or
    // the musl/gnu decision would depend on the machine running the test
    // (a glibc CI runner would force every linux probe to gnu). Hand it a
    // readFileSync that throws for the ldd path so the loader falls through
    // to the report-based probe — which we drive via the faked
    // `process.report` (the only musl signal source in the sandbox).
    //
    // INTENTIONALLY ldd-only: the generated `dist/index.js` reads exactly
    // ONE host file via fs — `/usr/bin/ldd` (its `isMuslFromFilesystem`).
    // We also defensively guard `ld-musl` paths in case a future napi
    // template adds a second libc-file probe. If a future loader starts
    // reading OTHER host files through fs (e.g. `/etc/os-release`), extend
    // the guard here to throw ENOENT for those paths too so the simulation
    // stays hermetic; do NOT let any new path fall through to the real host
    // fs, or the test outcome would again depend on the runner.
    if (id === "node:fs") {
      const guardedReadFileSync = ((path: string, ...rest: unknown[]): unknown => {
        if (typeof path === "string" && (path.includes("/ldd") || path.includes("ld-musl"))) {
          const err = new Error(`probe: refusing host fs read of ${path}`) as NodeJS.ErrnoException;
          err.code = "ENOENT";
          throw err;
        }
        return (readFileSync as (...a: unknown[]) => unknown)(path, ...rest);
      }) as typeof readFileSync;
      return { readFileSync: guardedReadFileSync };
    }
    if (id === "child_process") {
      // The loader's last-resort musl probe shells out to `ldd --version`.
      // Force it to miss so, again, only the faked report decides musl.
      return {
        execSync() {
          throw new Error("probe: no ldd in sandbox");
        },
      };
    }

    // The optional-dependency package's package.json (version check). Only
    // published packages have one; match the loader's baked version so the
    // version guard passes.
    if (id.startsWith("@verter/native-") && id.endsWith("/package.json")) {
      const base = id.slice(0, -"/package.json".length);
      if (!published.has(base)) return moduleNotFound(id);
      const pkgPath = join(PACKAGE_DIR, "package.json");
      const { version } = JSON.parse(readFileSync(pkgPath, "utf8")) as { version: string };
      return { version };
    }

    // The optional-dependency package itself: sentinel for published only.
    // Record the id here — at the point the require ACTUALLY returns the
    // sentinel — so the result reports resolved (returned), not merely
    // attempted (requested above in `requireCalls`).
    if (id.startsWith("@verter/native-")) {
      if (!published.has(id)) return moduleNotFound(id);
      returnedByRequireRequests.push(id);
      return { __PROBE_SENTINEL__: SENTINEL, requestedId: id };
    }

    // Anything else the loader might touch (wasi fallbacks etc.): miss.
    return moduleNotFound(id);
  };

  const moduleObj = { exports: {} as Record<string, unknown> };
  const sandbox: Record<string, unknown> = {
    process: makeFakeProcess(fake),
    require: fakeRequire,
    module: moduleObj,
    exports: moduleObj.exports,
    __dirname: join(PACKAGE_DIR, "dist"),
    __filename: join(PACKAGE_DIR, "dist", "index.js"),
    console,
    Buffer,
  };
  createContext(sandbox);

  let threwMessage: string | null = null;
  let threwCauseChain: string[] = [];
  try {
    runInContext(loaderSource, sandbox, { filename: "dist/index.js" });
  } catch (err) {
    // The loader throws inside the vm realm; read `.message` by duck-typing
    // (cross-realm `instanceof Error` is false).
    threwMessage =
      typeof err === "object" && err !== null && "message" in err
        ? String((err as { message: unknown }).message)
        : String(err);
    threwCauseChain = collectCauseChain(err);
  }

  const exported = moduleObj.exports as { __PROBE_SENTINEL__?: symbol; requestedId?: unknown };
  // The sentinel was exported to `module.exports`. NOTE: this alone is NOT
  // acceptance — a loader can assign `module.exports` and THEN throw (a
  // post-export version check, or any export-then-throw shape), in which case
  // the export is present but the load FAILED. Acceptance additionally
  // requires no-throw (gated below).
  const sentinelExported = exported?.__PROBE_SENTINEL__ === SENTINEL;
  const noThrow = threwMessage === null;

  // ACCEPTED = the loader exported our sentinel AND completed without throwing.
  // The no-throw gate is the round-3 fix: without it, an export-then-throw
  // loader would falsely read as accepted. With it, a thrown load is never
  // counted as acceptance regardless of what reached `module.exports`.
  const accepted = sentinelExported && noThrow;
  // The package the loader ACCEPTED: read back the exported sentinel's own
  // `requestedId`, but ONLY when accepted (so both the version-check-throw —
  // sentinel never exported — AND the export-then-throw — sentinel exported
  // but the load threw — yield `null`).
  const acceptedSentinelPackage =
    accepted && typeof exported.requestedId === "string" ? exported.requestedId : null;

  const attemptedDepRequests = requireCalls.filter(
    (id) => id.startsWith("@verter/native-") && !id.endsWith("/package.json"),
  );
  // DIAGNOSTIC (no no-throw gate): every id whose require RETURNED a sentinel,
  // recorded at the return site — REGARDLESS of a later throw. An attempted
  // but-throwing unpublished-alias request never enters this list (its require
  // threw MODULE_NOT_FOUND), but an export-then-throw id DOES appear here while
  // being absent from the ACCEPTED set below.
  const returnedByRequireDepRequests = [...returnedByRequireRequests];
  // ACCEPTED set: the require-returned ids, but EMPTY when the loader threw —
  // so this proves "the right package was accepted", not merely returned.
  const acceptedPublishedDepRequests = noThrow ? [...returnedByRequireRequests] : [];

  return {
    requireCalls,
    acceptedPublishedDepRequests,
    returnedByRequireDepRequests,
    attemptedDepRequests,
    accepted,
    acceptedSentinelPackage,
    threwMessage,
    threwCauseChain,
  };
}

/**
 * Walk an error's `.cause` chain, collecting each layer's message. Uses
 * duck-typing rather than `instanceof Error`: the loader throws inside a
 * `node:vm` realm, so its errors are NOT instances of this realm's `Error`
 * and `instanceof` would silently see an empty chain.
 */
function collectCauseChain(err: unknown): string[] {
  const chain: string[] = [];
  const isErrorLike = (v: unknown): v is { message: unknown; cause?: unknown } =>
    typeof v === "object" && v !== null && "message" in v;
  let cur: unknown = isErrorLike(err) ? err.cause : undefined;
  const seen = new Set<unknown>();
  while (isErrorLike(cur) && !seen.has(cur)) {
    seen.add(cur);
    chain.push(String(cur.message));
    cur = cur.cause;
  }
  return chain;
}
