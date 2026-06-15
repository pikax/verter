/**
 * Definition comparator.
 *
 * Compares a definition result by SYMBOL IDENTITY (file + range), never by
 * `line === 0` — a precise line-0 declaration is a valid target. Generated
 * targets are projected back to authored Vue space through the source map before
 * matching; a target that resolves ONLY into generated artifacts with no way back
 * to source is the distinct `unmappedGenerated` divergence, not a line-0 failure
 * and not a crash. The shared normalizer predicates (`definitionMatchesExpected`,
 * `isGeneratedUri`) are reused rather than re-implemented.
 *
 * When the scenario declares an expected Vue identity, that identity governs;
 * otherwise verter and the baseline are compared as authored-space location sets.
 * Comparing the baseline requires the file texts behind its byte offsets, so they
 * are a bundled, required input. A baseline location that cannot enter the authored
 * comparison is surfaced, never silently dropped into a false agreement: one whose
 * text is unavailable becomes a conservative `baselineOnly`; a generated-artifact
 * location that converts but has no mapping back to authored source becomes an
 * `unmappedGenerated` divergence.
 */

import type { NormalizedLocation } from "../baseline/bridgeClient.js";
import type { CanonicalDefinitionTarget, ExpectedDefinition, Range } from "../normalize/index.js";
import {
  definitionMatchesExpected,
  isDefinitionGeneratedOnly,
  isGeneratedUri,
  rangesEqual,
} from "../normalize/index.js";
import type { Divergence } from "./outcome.js";
import { GeneratedDocument, projectGeneratedRange, type ParsedSourceMap } from "./projection.js";

/** The baseline provider's resolved locations plus the file texts behind their byte offsets. */
export interface BaselineLocations {
  readonly locations: readonly NormalizedLocation[];
  /** Path → text for every location's path; converts byte offsets into positions. */
  readonly texts: Readonly<Record<string, string>>;
}

/** Options driving the definition comparison. */
export interface DefinitionCompareOptions {
  /** The expected authored-Vue symbol identity; when present it governs the comparison. */
  readonly expected?: ExpectedDefinition;
  /** The source map for projecting generated targets back to authored Vue space. */
  readonly map?: ParsedSourceMap;
  /** The baseline provider's resolved locations plus the texts needed to convert them. */
  readonly baseline?: BaselineLocations;
}

/** An authored-space identity: source file plus range, both comparators reduce to this. */
interface AuthoredIdentity {
  readonly uri: string;
  readonly range: Range;
}

/** The baseline reduced to authored identities, plus the locations that could not enter it. */
interface BaselineAuthored {
  readonly authored: AuthoredIdentity[];
  /** Locations whose byte offsets could not be converted (no text for their path). */
  readonly unconvertible: NormalizedLocation[];
  /** Generated-artifact locations that converted but had no mapping back to authored source. */
  readonly unprojectableGenerated: NormalizedLocation[];
}

/** Reduce verter targets to authored identities, projecting generated targets via the map. */
function verterAuthored(
  verter: readonly CanonicalDefinitionTarget[],
  map: ParsedSourceMap | undefined,
): AuthoredIdentity[] {
  const authored: AuthoredIdentity[] = [];
  for (const target of verter) {
    if (target.fromGenerated !== true) {
      authored.push({ uri: target.uri, range: target.range });
      continue;
    }
    const projected = map ? projectGeneratedRange(map, target.range) : null;
    if (projected !== null) authored.push({ uri: projected.source, range: projected.range });
    // else: a generated target with no mapping back — omitted (see unmappedGenerated below)
  }
  return authored;
}

/** Reduce baseline locations to authored identities, projecting generated paths via the map. */
function baselineAuthored(
  baseline: BaselineLocations,
  map: ParsedSourceMap | undefined,
): BaselineAuthored {
  const authored: AuthoredIdentity[] = [];
  const unconvertible: NormalizedLocation[] = [];
  const unprojectableGenerated: NormalizedLocation[] = [];
  // One converter per unique path: each file's line index is scanned once, not per location.
  const documents = new Map<string, GeneratedDocument>();
  const documentFor = (path: string): GeneratedDocument | undefined => {
    const existing = documents.get(path);
    if (existing !== undefined) return existing;
    const text = baseline.texts[path];
    if (text === undefined) return undefined;
    const doc = new GeneratedDocument(text);
    documents.set(path, doc);
    return doc;
  };

  for (const loc of baseline.locations) {
    const doc = documentFor(loc.path);
    if (doc === undefined) {
      // No text for this path: the offsets cannot be converted. Surface it rather than
      // drop it — a dropped baseline location would read as a false agreement.
      unconvertible.push(loc);
      continue;
    }
    const generatedRange = doc.byteRangeToPosition(loc.start, loc.end);
    if (isGeneratedUri(loc.path)) {
      // The baseline's byte offsets converted to a generated position through THIS path's document
      // (`doc`) above; the source map projects that position back to authored Vue space.
      const projected = map ? projectGeneratedRange(map, generatedRange) : null;
      if (projected !== null) authored.push({ uri: projected.source, range: projected.range });
      // A generated baseline target that converted but has no mapping back to authored
      // source: surface it (see unmappedGenerated below), never drop it into agreement.
      else unprojectableGenerated.push(loc);
    } else {
      authored.push({ uri: loc.path, range: generatedRange });
    }
  }
  return { authored, unconvertible, unprojectableGenerated };
}

/** Classify the symmetric difference of two authored-identity sets. */
function pushLocationSetDiff(
  divergences: Divergence[],
  verter: readonly AuthoredIdentity[],
  baseline: readonly AuthoredIdentity[],
): void {
  const exact = (a: AuthoredIdentity, b: AuthoredIdentity): boolean =>
    a.uri === b.uri && rangesEqual(a.range, b.range);

  for (const v of verter) {
    if (baseline.some((b) => exact(v, b))) continue;
    const sameFile = baseline.find((b) => b.uri === v.uri);
    if (sameFile !== undefined) {
      divergences.push({
        class: "rangeMismatch",
        detail: `definition for ${v.uri} differs in range`,
        verterValue: v.range,
        baselineValue: sameFile.range,
      });
    } else {
      divergences.push({
        class: "verterOnly",
        detail: `verter resolved a definition in ${v.uri} the baseline did not`,
        verterValue: v,
      });
    }
  }
  for (const b of baseline) {
    if (verter.some((v) => exact(v, b))) continue;
    if (verter.some((v) => v.uri === b.uri)) continue; // counted as a rangeMismatch above
    divergences.push({
      class: "baselineOnly",
      detail: `the baseline resolved a definition in ${b.uri} verter did not`,
      baselineValue: b,
    });
  }
}

/**
 * Compare a definition result. Returns a flat divergence list (empty =
 * agreement). Total over an empty result and over generated-only targets — the
 * generated-only-unmapped case yields a divergence, never a throw.
 */
export function compareDefinition(
  verter: readonly CanonicalDefinitionTarget[],
  options: DefinitionCompareOptions = {},
): Divergence[] {
  const divergences: Divergence[] = [];
  const verterIds = verterAuthored(verter, options.map);
  // Generated-only (the shared normalizer predicate) AND none mapped back = unmapped-generated.
  const verterAllUnmapped = isDefinitionGeneratedOnly(verter) && verterIds.length === 0;

  if (verterAllUnmapped) {
    divergences.push({
      class: "unmappedGenerated",
      detail: "verter resolved only generated targets with no mapping back to authored source",
      verterValue: verter,
    });
  }

  // When the scenario declares an expected authored identity, it governs.
  if (options.expected !== undefined) {
    const matched = definitionMatchesExpected(verterIds, options.expected);
    if (!matched && verterIds.length > 0) {
      divergences.push({
        class: "wrongTarget",
        detail: "no resolved definition matches the expected authored identity",
        verterValue: verterIds,
        baselineValue: options.expected,
      });
    } else if (!matched && verterIds.length === 0 && !verterAllUnmapped) {
      divergences.push({
        class: "wrongTarget",
        detail: "verter produced no definition for the expected symbol",
        baselineValue: options.expected,
      });
    }
    return divergences;
  }

  // Otherwise compare verter and the baseline as authored-space location sets.
  if (options.baseline !== undefined) {
    const {
      authored: baselineIds,
      unconvertible,
      unprojectableGenerated,
    } = baselineAuthored(options.baseline, options.map);
    pushLocationSetDiff(divergences, verterIds, baselineIds);
    for (const loc of unconvertible) {
      divergences.push({
        class: "baselineOnly",
        detail: `the baseline resolved a definition in ${loc.path} whose offsets could not be converted (no text supplied)`,
        baselineValue: loc,
      });
    }
    for (const loc of unprojectableGenerated) {
      divergences.push({
        class: "unmappedGenerated",
        detail: `the baseline resolved a generated definition target in ${loc.path} with no mapping back to authored source`,
        baselineValue: loc,
      });
    }
  }
  return divergences;
}
