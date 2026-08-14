// Candidate-vs-golden acceptance check — the reusable primitive a consumer
// (a Rust conformance suite, CI) drives to compare one produced artifact
// against one committed golden across every independent axis: parse,
// structural, diagnostics, mapping, link, and runtime execution.
//
// SKIP SEMANTICS. Default behavior preserves the harness's ordinary
// skip-with-reason discipline: an axis whose environment prerequisite is
// absent (the isolated oracle install not realizable on this machine)
// reports `skipped` with its reason and does not fail the check. The
// AUTHORITATIVE mode (`authoritative: true`, or the CLI's --authoritative /
// BF2_AUTHORITATIVE=1) is the fail-closed contract: every applicable axis
// must genuinely RUN — a skipped axis is a hard failure, so a consumer can
// prove its acceptance evidence actually executed instead of silently
// narrowing. `not-applicable` (a runtime axis on a VDOM client artifact —
// ssr artifacts execute through the pinned server renderer and vapor
// artifacts mount through the pinned with-vapor interop runtime) is a
// structural fact, not a skip, and never fails authoritative mode.

import path from "node:path";

import { GOLDENS_ROOT, HARNESS_ROOT } from "./paths.mjs";
import { readGoldenByName } from "./golden-store.mjs";
import { FIXTURE_ANCHORS, MAPPING_PROFILES } from "./mapping-oracle.mjs";
import { compareArtifacts, cleanupLinkScratch } from "./compare.mjs";
import { ensureOracleDomain } from "./oracle-install.mjs";
import { executeVueSsr } from "./execute-vue-runtime.mjs";
import {
  executeVueVaporInterop,
  ensureVaporRuntimePreloaded,
  vaporRuntimeHref,
} from "./execute-vue-vapor.mjs";
import { executeSvelteSsr } from "./execute-svelte-runtime.mjs";

function frameworkOfGolden(goldenName) {
  if (goldenName.startsWith("vue/")) return "vue";
  if (goldenName.startsWith("svelte/")) return "svelte";
  throw new Error(`golden name carries no framework prefix: ${goldenName}`);
}

function isVaporGolden(framework, record) {
  return framework === "vue" && record.options?.backend === "vapor";
}

/**
 * Whether the golden's recorded options make it an executable artifact:
 * server-rendered artifacts execute through the pinned SSR renderer;
 * vapor-backend client artifacts mount through the pinned with-vapor
 * runtime under vaporInteropPlugin (the behavioral check that observes the
 * `__vapor` interop marker). VDOM client artifacts stay `not-applicable`.
 */
function runtimeApplicability(framework, record) {
  if (record.code === null) return false;
  if (framework === "vue")
    return record.options?.backend === "ssr" || isVaporGolden(framework, record);
  return record.options?.generate === "server";
}

/** Executes one arm through the runtime the golden's recorded profile targets. */
async function executeArm(framework, record, code) {
  if (framework === "svelte") return { ...(await executeSvelteSsr(code)), warnings: [] };
  if (isVaporGolden(framework, record)) return executeVueVaporInterop(code);
  return { ...(await executeVueSsr(code)), warnings: [] };
}

/**
 * A vapor-backend artifact's `vue` imports must link against the export
 * surface its deployment target actually resolves — the with-vapor runtime
 * build (the Node CJS `vue` entry publishes no vapor exports, so linking a
 * correct vapor artifact against it false-fails every runtime helper
 * import). Same pinned install; only the entry differs.
 */
function linkOverridesFor(framework, record, installDir) {
  if (installDir === null || !isVaporGolden(framework, record)) return undefined;
  return new Map([["vue", vaporRuntimeHref()]]);
}

/**
 * The authored-source context the mapping axis validates the CANDIDATE's own
 * map against. Everything comes from the golden's PROVENANCE (which fixture
 * was compiled, and under which profile) plus the fixture on disk — never
 * from the golden's own map, which is not an input to this axis at all.
 *
 * Generated-only ranges are NOT part of this context: `validateAuthoredMapping`
 * derives them from the CANDIDATE's own generated code, so the
 * no-authored-provenance-over-scaffolding requirement runs on every
 * candidate and cannot be switched off from a call site. Deriving them here
 * from golden geometry would be wrong as well as unnecessary — a candidate's
 * layout is legitimately its own.
 */
function mappingContextFor(framework, record) {
  const fixturePath = record.fixture?.path;
  if (typeof fixturePath !== "string") return null;
  const profileKey =
    framework === "vue" ? `vue:${record.options?.backend}` : `svelte:${record.options?.generate}`;
  const profile = MAPPING_PROFILES[profileKey];
  if (profile === undefined) return null;
  return {
    sourceMapRequested: framework === "vue" ? record.options?.sourceMap === true : true,
    fixture: { path: fixturePath, absolutePath: path.join(HARNESS_ROOT, fixturePath) },
    sourceResolveBases: [HARNESS_ROOT, path.dirname(path.join(HARNESS_ROOT, fixturePath))],
    profile,
    anchors: FIXTURE_ANCHORS[fixturePath] ?? [],
  };
}

/**
 * @param {{
 *   goldenName: string,
 *   candidate: { code: string|null, map?: object|null, diagnostics?: Array<object> },
 *   authoritative?: boolean,
 * }} input
 * @returns {Promise<{ verdict: "pass"|"fail", reasons: string[], axes: object, report: object|null }>}
 */
export async function checkCandidate({ goldenName, candidate, authoritative = false }) {
  const framework = frameworkOfGolden(goldenName);
  const golden = readGoldenByName(GOLDENS_ROOT, goldenName);

  // ONE environment probe decides availability for every oracle-backed
  // axis, so an infrastructure absence is always a SKIP (or an
  // authoritative failure) and never masquerades as a candidate execution
  // failure downstream.
  let installDir = null;
  let environmentReason = null;
  try {
    installDir = ensureOracleDomain(framework).installDir;
  } catch (error) {
    environmentReason = `oracle install unavailable: ${String(error?.message ?? error)}`;
  }

  // The link axis below imports a vapor artifact's redirected `vue` entry
  // to inspect its export surface — for a vapor golden that is the pinned
  // with-vapor runtime module itself, which captures `document` ONCE at
  // module evaluation. That first evaluation must happen with the shared
  // process document already installed, or the capture pins to `null` for
  // the life of the ESM cache and every later mount that reaches the
  // runtime's VDOM-fragment path (slot fallback geometry, multi-root
  // fragments) throws — reported as a candidate/golden execution failure,
  // which the SKIP-semantics contract above forbids for an environment
  // artifact. Preload the runtime under the shared document FIRST so the
  // link axis's export inspection is a cache hit on a correctly-captured
  // module.
  const linkSpecifierOverrides = linkOverridesFor(framework, golden, installDir);
  if (linkSpecifierOverrides !== undefined) await ensureVaporRuntimePreloaded();

  const report = await compareArtifacts(
    { code: golden.code, map: golden.map ?? null, diagnostics: golden.diagnostics ?? [] },
    {
      code: candidate.code,
      map: candidate.map ?? null,
      diagnostics: candidate.diagnostics ?? [],
    },
    {
      linkBaseDir: installDir ?? undefined,
      authoritative,
      linkSpecifierOverrides,
      mappingContext: mappingContextFor(framework, golden),
    },
  );
  const reasons = [...report.reasons];
  const axes = { ...report.axes };
  if (installDir === null) {
    axes.link = { status: "skipped", reason: environmentReason };
    // compareArtifacts already recorded its own "no linkBaseDir" skip
    // reason under authoritative mode; replace it with the real cause.
    const index = reasons.findIndex((r) => r.startsWith("authoritative mode: link axis skipped"));
    if (index !== -1)
      reasons[index] = `authoritative mode: link axis skipped (${environmentReason})`;
  }

  if (!runtimeApplicability(framework, golden)) {
    axes.runtime = { status: "not-applicable", reason: "not a runtime-executable artifact" };
  } else if (installDir === null) {
    axes.runtime = { status: "skipped", reason: environmentReason };
    if (authoritative) {
      reasons.push(`authoritative mode: runtime axis skipped (${environmentReason})`);
    }
  } else if (candidate.code === null || candidate.code === undefined) {
    axes.runtime = { status: "ran", reason: null };
    reasons.push("runtime divergence: candidate produced no executable code");
  } else {
    axes.runtime = { status: "ran", reason: null };
    const goldenRun = await executeArm(framework, golden, golden.code);
    const candidateRun = await executeArm(framework, golden, candidate.code);
    if (!goldenRun.ok) {
      reasons.push(`runtime divergence: golden failed to execute: ${goldenRun.error}`);
    }
    if (!candidateRun.ok) {
      reasons.push(`runtime divergence: candidate failed to execute: ${candidateRun.error}`);
    }
    if (goldenRun.ok && candidateRun.ok && goldenRun.html !== candidateRun.html) {
      reasons.push(
        `runtime divergence: rendered HTML differs (golden ${JSON.stringify(goldenRun.html)} vs candidate ${JSON.stringify(candidateRun.html)})`,
      );
    }
    // Runtime warnings are a behavioral signal in their own right: an
    // unmarked vapor component mis-routed through the VDOM interop path
    // warns even when its DOM happens to coincide.
    if (goldenRun.ok && candidateRun.ok) {
      const goldenWarned = (goldenRun.warnings ?? []).length > 0;
      const candidateWarned = (candidateRun.warnings ?? []).length > 0;
      if (candidateWarned && !goldenWarned) {
        reasons.push(
          `runtime divergence: candidate mount produced runtime warnings the golden does not: ${JSON.stringify(candidateRun.warnings)}`,
        );
      }
    }
  }

  if (installDir !== null) cleanupLinkScratch(installDir);
  return { verdict: reasons.length === 0 ? "pass" : "fail", reasons, axes, report };
}
