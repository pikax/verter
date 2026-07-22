/**
 * The native TypeScript reference lane — orchestration + receipt.
 *
 * THE NORTH-STAR METRIC: overhead over native TypeScript, per operation. The
 * lane samples the corpus's `.vue` files with the corpus gate's OWN sampler
 * (same pure function ⇒ same sample as the gate's baseline, provable via the
 * shared manifest hash), mirrors the corpus into a temp workspace (copied
 * sources + junctioned `node_modules`, so the tsconfig chain, path aliases,
 * ambient shims, sibling modules and installed packages are identical), then
 * derives one plain-TS analogue per sampled SFC (`derive.ts`: script blocks
 * kept, compiler macros lowered to type-preserving plain TS, template dropped
 * — a declared limitation) and drives each configured engine session against
 * the DERIVED files with the SAME provider binaries and spawn shapes Verter
 * uses, with no Verter process in the loop.
 *
 * Privacy: the mirror + derived analogues are generated at run time into a
 * temp directory, never into the repository; receipts carry manifest hashes
 * and counts, never corpus paths (unless VERTER_NATIVE_REF_FILE_DETAIL=1).
 * The GENERATOR is the deliverable; the generated corpus is not.
 *
 * Run:
 *   VERTER_CORPUS_GATE_DIR=<corpus root> \
 *   pnpm --filter @verter/dx-harness test:native-reference
 */
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import {
  profileCorpus,
  sampleManifestHash,
  selectRepresentativeSample,
} from "../corpus-gate/sample.js";
import { deriveSampleIntoMirror, mirrorCorpusWorkspace } from "./derive.js";
import { runTsgoDirectSession } from "./tsgoDirect.js";
import { runTsserverDirectSession } from "./tsserverDirect.js";
import type {
  NativeDerivationReport,
  NativeEngineReport,
  NativeReferenceConfig,
  NativeReferenceEngine,
  NativeReferenceReceipt,
} from "./types.js";

export { resolveNativeReferenceEnv } from "./config.js";
export * from "./types.js";

export interface NativeReferenceOutcome {
  readonly receipt: NativeReferenceReceipt;
  readonly receiptPath: string | null;
  /** The mirror workspace path (temp) — logged for inspection, never committed. */
  readonly mirrorRoot: string;
}

/** Run the whole native reference lane (both engines serially by default). */
export async function runNativeReference(
  config: NativeReferenceConfig,
  log: (message: string) => void = () => {},
): Promise<NativeReferenceOutcome> {
  // 1. Sample the `.vue` set with the corpus gate's own pure sampler — same
  //    corpus content + same sampleSize ⇒ the SAME files the gate benchmarks,
  //    provable by comparing manifest hashes across receipts.
  const vueProfiles = profileCorpus(config.corpusDir);
  const vueSample = selectRepresentativeSample(vueProfiles, config.sampleSize).map(
    (profile) => profile.relativePath,
  );
  const vueHash = sampleManifestHash(vueSample);
  log(
    `[native-ref] corpus has ${vueProfiles.length} SFCs; sampled ${vueSample.length} ` +
      `(vue manifest ${vueHash})`,
  );

  // 2. Mirror the workspace and derive the analogues into it.
  const mirrorRoot = config.mirrorDir;
  mkdirSync(mirrorRoot, { recursive: true });
  const mirrorStats = mirrorCorpusWorkspace(config.corpusDir, mirrorRoot);
  log(
    `[native-ref] mirrored workspace: ${mirrorStats.copiedFiles} files copied, ` +
      `${mirrorStats.junctionedNodeModules} node_modules junctioned`,
  );
  const derived = deriveSampleIntoMirror(config.corpusDir, mirrorRoot, vueSample);
  const derivedHash = sampleManifestHash(derived.derivedRelativePaths);
  log(
    `[native-ref] derived ${derived.derivedRelativePaths.length} analogues ` +
      `(skipped: ${JSON.stringify(derived.skipped)}; derived manifest ${derivedHash})`,
  );

  const derivation: NativeDerivationReport = {
    vueSampleManifestHash: vueHash,
    derivedSampleManifestHash: derivedHash,
    sampledVueCount: vueSample.length,
    derivedCount: derived.derivedRelativePaths.length,
    skipped: derived.skipped,
    tallies: { ...derived.tallies },
    mirror: mirrorStats,
  };

  // 3. Drive each engine against the derived analogues inside the mirror.
  const engines: Partial<Record<NativeReferenceEngine, NativeEngineReport>> = {};
  for (const engine of config.engines) {
    log(`[native-ref] running engine: ${engine}`);
    engines[engine] =
      engine === "tsgo"
        ? await runTsgoDirectSession(config, mirrorRoot, derived.derivedRelativePaths, log)
        : await runTsserverDirectSession(config, mirrorRoot, derived.derivedRelativePaths, log);
  }

  const receipt: NativeReferenceReceipt = {
    schemaVersion: 2,
    generatedAt: new Date().toISOString(),
    corpusLabel: config.corpusLabel,
    platform: `${process.platform}-${process.arch}`,
    nodeVersion: process.version,
    derivation,
    sampleSize: vueSample.length,
    maxProbesPerFile: config.maxProbesPerFile,
    requestTimeoutMs: config.requestTimeoutMs,
    engines,
    ...(config.includeFileDetail ? { files: derived.derivedRelativePaths } : {}),
  };

  let receiptPath: string | null = null;
  if (config.receiptPath !== null) {
    mkdirSync(path.dirname(config.receiptPath), { recursive: true });
    writeFileSync(config.receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
    receiptPath = config.receiptPath;
  }
  return { receipt, receiptPath, mirrorRoot };
}
