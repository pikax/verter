/**
 * Per-carrier mapper construction over the shared CORE strict mapper. The
 * playground registers ONE `CarrierMapper` per generated carrier file (keyed
 * by provider path) in a `CarrierMapperSet`; cross-file provider results map
 * through their OWN file's mapper and unmapped spans DROP — never a
 * closest-segment snap, never a single-active-file mapper.
 */
import { CarrierMapper, CarrierMapperSet } from "@verter/language-shared";

export interface CarrierMapperEntry {
  /** The generated carrier's provider path (`…/Comp.vue.tsx`). */
  providerPath: string;
  /** The generated carrier text the map's offsets index. */
  code: string;
  /** The carrier's V3 source map JSON; absent/invalid ⇒ NO mapper (fail closed). */
  sourceMap?: string | null;
  /**
   * Optional source-text reader consulted before the map's own
   * `sourcesContent` (the mapper falls back to the embedded content).
   */
  readSourceText?: (source: string) => string | undefined;
}

/**
 * Construct the strict CORE mapper for one carrier, or `null` when the
 * carrier ships no usable map — the caller then registers NO mapper and every
 * span for that carrier drops (fail closed), never identity-maps.
 */
export function createCarrierMapper(entry: CarrierMapperEntry): CarrierMapper | null {
  const mapJson = entry.sourceMap;
  if (typeof mapJson !== "string" || mapJson.length <= 2) {
    return null;
  }
  try {
    return new CarrierMapper({
      map: mapJson,
      generatedText: entry.code,
      readSourceText: entry.readSourceText,
    });
  } catch {
    // A torn/unparseable map must never degrade into a heuristic mapper.
    return null;
  }
}

/** Build a `CarrierMapperSet` from per-carrier entries (mapless entries skipped). */
export function buildCarrierMapperSet(entries: readonly CarrierMapperEntry[]): CarrierMapperSet {
  const set = new CarrierMapperSet();
  for (const entry of entries) {
    const mapper = createCarrierMapper(entry);
    if (mapper !== null) {
      set.set(entry.providerPath, mapper);
    }
  }
  return set;
}
