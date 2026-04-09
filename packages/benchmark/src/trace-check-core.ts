import { existsSync, readFileSync } from "node:fs";
import { join, normalize, resolve } from "node:path";

import {
  compareNormalizedArtifacts,
  type ArtifactComparison,
  type NormalizedDiagnostic,
  type NormalizedMetaArtifact,
} from "./meta-ui-core.js";

export interface ExpectedArtifactComparisonInput {
  componentPath: string;
  traceDir: string;
  expectedDir: string;
}

export interface TraceLogLookupInput {
  componentName: string;
  componentPath: string;
  traceDir: string;
}

export interface ExpectedArtifactComparisonResult {
  passed: boolean;
  actualPath: string;
  expectedPath: string;
  comparison: ArtifactComparison | null;
  message: string;
}

export function resolveTraceResultArtifactPath(traceDir: string, componentPath: string): string {
  return join(
    resolve(traceDir),
    "results",
    `${normalizeArtifactComponentPath(componentPath)}.json`,
  );
}

export function resolveExpectedArtifactPath(expectedDir: string, componentPath: string): string {
  return join(resolve(expectedDir), `${normalizeArtifactComponentPath(componentPath)}.json`);
}

export function resolveTraceLogCandidatePaths(input: TraceLogLookupInput): string[] {
  const baseDir = resolve(input.traceDir);
  const fileNames = [
    `${input.componentName}.trace.log`,
    `src__runtime__components__${input.componentName}.vue.trace.log`,
    `${sanitizeTraceStem(input.componentPath)}.trace.log`,
  ];

  return fileNames.flatMap((fileName) => [
    join(baseDir, fileName),
    join(baseDir, "traces", fileName),
  ]);
}

export function findTraceLogPath(input: TraceLogLookupInput): string | null {
  for (const candidatePath of resolveTraceLogCandidatePaths(input)) {
    if (existsSync(candidatePath)) {
      return candidatePath;
    }
  }
  return null;
}

/**
 * Validate that the expected artifact directory has manifest provenance
 * covering the requested component. Returns null if valid, or a failure
 * message if the manifest is missing or doesn't list the component.
 */
export function validateExpectedManifestProvenance(
  expectedDir: string,
  componentPath: string,
): string | null {
  const manifestPath = join(resolve(expectedDir), "meta-ui-expected-manifest.json");
  if (!existsSync(manifestPath)) {
    return `expected manifest not found: ${manifestPath}`;
  }
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as {
    resolvedTargetSha?: string;
    componentPaths?: string[];
  };
  if (!manifest.resolvedTargetSha) {
    return "expected manifest has no resolvedTargetSha — provenance unknown";
  }
  const normalized = componentPath.replace(/\\/g, "/");
  if (!manifest.componentPaths?.includes(normalized)) {
    return (
      `component ${normalized} is not listed in the expected manifest's componentPaths ` +
      `(manifest covers: ${manifest.componentPaths?.join(", ") ?? "none"})`
    );
  }
  return null;
}

export function compareResultArtifactToExpected(
  input: ExpectedArtifactComparisonInput,
): ExpectedArtifactComparisonResult {
  const actualPath = resolveTraceResultArtifactPath(input.traceDir, input.componentPath);
  const expectedPath = resolveExpectedArtifactPath(input.expectedDir, input.componentPath);

  if (!existsSync(actualPath)) {
    return {
      passed: false,
      actualPath,
      expectedPath,
      comparison: null,
      message: `missing normalized result artifact: ${actualPath}`,
    };
  }

  if (!existsSync(expectedPath)) {
    return {
      passed: false,
      actualPath,
      expectedPath,
      comparison: null,
      message: `missing expected artifact: ${expectedPath}`,
    };
  }

  const actualArtifact = readNormalizedArtifact(actualPath);
  const expectedArtifact = readNormalizedArtifact(expectedPath);
  const comparison = compareNormalizedArtifacts(actualArtifact, expectedArtifact);
  const artifactMismatches = compareArtifactEnvelope(actualArtifact, expectedArtifact);

  if (comparison.exact && artifactMismatches.length === 0) {
    return {
      passed: true,
      actualPath,
      expectedPath,
      comparison,
      message: "normalized result matches expected artifact",
    };
  }

  const details = [...artifactMismatches, ...formatCollectionDifferences(comparison)];

  return {
    passed: false,
    actualPath,
    expectedPath,
    comparison,
    message: `normalized result diverges from expected artifact: ${details.join("; ")}`,
  };
}

function normalizeArtifactComponentPath(componentPath: string): string {
  return normalize(componentPath.replace(/\\/g, "/"));
}

function sanitizeTraceStem(componentPath: string): string {
  return componentPath.replace(/[/\\]/g, "__").replace(/\.vue$/, "__vue");
}

function readNormalizedArtifact(filePath: string): NormalizedMetaArtifact {
  return JSON.parse(readFileSync(filePath, "utf8")) as NormalizedMetaArtifact;
}

function compareArtifactEnvelope(
  actual: NormalizedMetaArtifact,
  expected: NormalizedMetaArtifact,
): string[] {
  const mismatches: string[] = [];

  if (actual.componentPath !== expected.componentPath) {
    mismatches.push(
      `componentPath expected ${JSON.stringify(expected.componentPath)} got ${JSON.stringify(actual.componentPath)}`,
    );
  }

  if (actual.componentName !== expected.componentName) {
    mismatches.push(
      `componentName expected ${JSON.stringify(expected.componentName)} got ${JSON.stringify(actual.componentName)}`,
    );
  }

  if (
    JSON.stringify(normalizeDiagnostics(actual.diagnostics)) !==
    JSON.stringify(normalizeDiagnostics(expected.diagnostics))
  ) {
    mismatches.push(
      `diagnostics expected ${JSON.stringify(normalizeDiagnostics(expected.diagnostics))} got ${JSON.stringify(normalizeDiagnostics(actual.diagnostics))}`,
    );
  }

  return mismatches;
}

function normalizeDiagnostics(diagnostics: NormalizedDiagnostic[]): NormalizedDiagnostic[] {
  return [...diagnostics].sort((left, right) =>
    left.code === right.code
      ? left.message.localeCompare(right.message)
      : left.code.localeCompare(right.code),
  );
}

function formatCollectionDifferences(comparison: ArtifactComparison): string[] {
  const details: string[] = [];

  for (const [collectionName, result] of Object.entries(comparison.collections)) {
    if (result.missing.length > 0) {
      details.push(`missing ${collectionName}: ${result.missing.join(", ")}`);
    }
    if (result.extra.length > 0) {
      details.push(`extra ${collectionName}: ${result.extra.join(", ")}`);
    }
    for (const mismatch of result.fieldMismatches.slice(0, 5)) {
      details.push(
        `${collectionName}.${mismatch.name}.${mismatch.field} expected ${mismatch.expected} got ${mismatch.actual}`,
      );
    }
    if (result.fieldMismatches.length > 5) {
      details.push(
        `${collectionName} has ${result.fieldMismatches.length - 5} additional field mismatches`,
      );
    }
  }

  return details;
}
