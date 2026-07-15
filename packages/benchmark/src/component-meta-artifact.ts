import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

import {
  normalizeForBenchmark,
  type NormalizedDiagnostic,
  type NormalizedMetaArtifact,
} from "./meta-ui-core.js";
import { propsToJsonSchema, refineMetaForBenchmark } from "./meta-ui-meta.js";

export function normalizeComponentMetaArtifact(
  componentPath: string,
  raw: any,
): NormalizedMetaArtifact {
  const refined = refineMetaForBenchmark(raw);
  const propsJsonSchema = propsToJsonSchema(refined.props);
  const diagnostics = collectComponentMetaDiagnostics(raw, refined);
  return normalizeForBenchmark(componentPath, refined, propsJsonSchema, diagnostics);
}

export function collectComponentMetaDiagnostics(raw: any, refined: any): NormalizedDiagnostic[] {
  const diagnostics: NormalizedDiagnostic[] = [];
  if (!raw) {
    diagnostics.push({
      level: "error",
      code: "meta_ui_empty_meta",
      message: "Backend returned no metadata.",
    });
  }
  if (
    !Array.isArray(refined?.props) ||
    !Array.isArray(refined?.events) ||
    !Array.isArray(refined?.slots)
  ) {
    diagnostics.push({
      level: "warning",
      code: "meta_ui_incomplete_surface",
      message: "Backend returned an incomplete metadata surface.",
    });
  }
  return diagnostics;
}

export function writeNormalizedComponentMetaArtifact(
  outputPath: string,
  artifact: NormalizedMetaArtifact,
): void {
  mkdirSync(dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, JSON.stringify(artifact, null, 2), "utf8");
}
