// Validation: total order and the `UncomposableInputMap` taxonomy.
//
// Implements layer 1 §4.1 (contract), §4.2 (fail-closed outcome kinds),
// §4.3 (total order over every check), §4.4 (exhaustive taxonomy), §4.5
// (interoperable JSON domain) of `spec/assembled-map-composition-layer1.md`.
//
// Validation runs to completion before any composition work begins: no
// segment is translated, no table row appended, no artifact field computed
// until §4.3's steps have all passed (§4.1).
//
// Every rejection reports exactly one outcome. §4.3 is a total order over
// every check (including element order within each scanned array and field
// order within one segment). First failing check is the outcome.

import { TextGeometry } from "./assembled-map-coordinates.mjs";
import {
  hasUnpairedSurrogate,
  isJsonArray,
  isJsonObject,
  JsonSyntaxError,
  readJsonDocument,
} from "./assembled-map-json.mjs";
import { decodeMappingsStrict } from "./assembled-map-wire.mjs";

/** `U3.4` → family `U3`. Every sub-code belongs to exactly one family (§4.4). */
export function familyOf(code) {
  return code.slice(0, code.indexOf("."));
}

function uncomposable(code) {
  return { ok: false, family: familyOf(code), code };
}

/**
 * §4.3 stage 1 — every per-map check, in order, for ONE contributing map.
 *
 * @param {string} rawMap the fragment's raw, unparsed `sourceMap` string (§3.3)
 * @param {string} fragmentCode the fragment's own `code`, in its own pre-rewrite
 *   coordinate space — what `U7` is checked against (§4.4)
 */
export function validateContributingMap(rawMap, fragmentCode) {
  // 1.1 interoperable JSON domain (§4.5)
  // Three ordered clauses, first failure wins, checked before any member of
  // the document is read.
  let document;
  try {
    document = readJsonDocument(rawMap);
  } catch (error) {
    if (error instanceof JsonSyntaxError) return uncomposable("U1.1"); // (a)
    throw error;
  }
  for (const number of document.numbers) {
    // (b) every JSON number denotes a finite IEEE-754 double, in source
    // order. Predicate is over the converted binary64 value; conversion is
    // round-ties-to-even over the exact lexeme.
    if (!Number.isFinite(number.value)) return uncomposable("U1.9");
  }
  for (const text of document.strings) {
    // (c) every JSON string, after unescaping, is well-formed Unicode.
    if (hasUnpairedSurrogate(text)) return uncomposable("U1.10");
  }

  // 1.2 duplicate object member
  // Precedes every member read, so no later check can silently read whichever
  // duplicate the parser happened to keep.
  if (document.hasDuplicateMember) return uncomposable("U1.8");

  // 1.3 root is an object
  const root = document.value;
  if (!isJsonObject(root)) return uncomposable("U1.2");

  const member = (name) => root.get(name);
  const has = (name) => root.has(name);

  // 1.4 – 1.6 version
  if (!has("version")) return uncomposable("U2.1");
  const version = member("version");
  if (typeof version !== "number" || !Number.isInteger(version)) return uncomposable("U2.2");
  if (version !== 3) return uncomposable("U2.3");

  // 1.7 indexed map
  // Version beats indexed-map; indexed-map beats missing `mappings`.
  if (has("sections")) return uncomposable("U5.1");

  // 1.8 – 1.9 mappings
  if (!has("mappings")) return uncomposable("U1.3"); // never read as an empty map
  const mappings = member("mappings");
  if (typeof mappings !== "string") return uncomposable("U1.4");

  // 1.10 – 1.11 table containers
  const sources = member("sources");
  if (!has("sources") || !isJsonArray(sources)) return uncomposable("U1.5");
  const names = member("names");
  if (!has("names") || !isJsonArray(names)) return uncomposable("U1.6");

  // 1.12 – 1.16 metadata member types
  const sourcesContent = has("sourcesContent") ? member("sourcesContent") : undefined;
  if (sourcesContent !== undefined && !isJsonArray(sourcesContent)) return uncomposable("U1.7");

  if (has("sourceRoot")) {
    const sourceRoot = member("sourceRoot");
    if (typeof sourceRoot !== "string" && sourceRoot !== null) return uncomposable("U1.7");
  }

  if (has("file")) {
    const file = member("file");
    if (typeof file !== "string" && file !== null) return uncomposable("U1.7");
  }

  const ignoreListSpellings = [];
  for (const spelling of ["ignoreList", "x_google_ignoreList"]) {
    if (!has(spelling)) continue;
    const list = member(spelling);
    if (!isJsonArray(list)) return uncomposable("U1.7");
    for (const entry of list) {
      if (typeof entry !== "number" || !Number.isInteger(entry) || entry < 0) {
        return uncomposable("U1.7");
      }
    }
    ignoreListSpellings.push(list);
  }
  if (ignoreListSpellings.length === 2) {
    const [first, second] = ignoreListSpellings;
    if (first.length !== second.length) return uncomposable("U1.7");
    for (let i = 0; i < first.length; i += 1) {
      if (first[i] !== second[i]) return uncomposable("U1.7");
    }
  }

  if (has("debugId") && typeof member("debugId") !== "string") return uncomposable("U1.7");

  // 1.17 – 1.19 table rows, ascending index order
  // `sources` rows beat `names` rows beat `sourcesContent` rows.
  for (const row of sources) {
    if (typeof row !== "string") return uncomposable("U4.1");
  }
  for (const row of names) {
    if (typeof row !== "string") return uncomposable("U4.2");
  }
  if (sourcesContent !== undefined) {
    for (const row of sourcesContent) {
      if (typeof row !== "string" && row !== null) return uncomposable("U4.3");
    }
    // 1.20
    if (sourcesContent.length !== sources.length) return uncomposable("U4.4");
  }

  // 1.21 the wire decode (phases A → B → C)
  const decoded = decodeMappingsStrict(mappings);
  if (!decoded.ok) return uncomposable(decoded.code);
  const segments = decoded.segments;

  // 1.22 table indices, wire order, `srcIdx` before `nameIdx`
  // Both checks are guarded on the field being NON-NULL: a 1-field segment is
  // sourceless by definition (§2.2) and `null` is in no index range; an
  // unguarded check would reject every sourceless segment and take the whole
  // sourceless-barrier algebra with it.
  for (const segment of segments) {
    if (segment.srcIdx !== null && (segment.srcIdx < 0 || segment.srcIdx >= sources.length)) {
      return uncomposable("U6.1");
    }
    if (segment.nameIdx !== null && (segment.nameIdx < 0 || segment.nameIdx >= names.length)) {
      return uncomposable("U6.2");
    }
  }

  // 1.23 ignore-list indices, ascending index order
  const ignoreList = ignoreListSpellings.length > 0 ? ignoreListSpellings[0] : null;
  if (ignoreList !== null) {
    for (const entry of ignoreList) {
      if (entry < 0 || entry >= sources.length) return uncomposable("U6.3");
    }
  }

  // 1.24 generated coordinates, wire order
  const geometry = new TextGeometry(fragmentCode);
  for (const segment of segments) {
    if (segment.genLine < 0 || segment.genLine >= geometry.lines.length) {
      return uncomposable("U7.1");
    }
    const lineText = geometry.lines[segment.genLine];
    if (segment.genCol < 0 || segment.genCol > lineText.length) {
      return uncomposable("U7.2");
    }
    if (segment.genCol >= 1) {
      const before = lineText.charCodeAt(segment.genCol - 1);
      const at = lineText.charCodeAt(segment.genCol);
      if (before >= 0xd800 && before <= 0xdbff && at >= 0xdc00 && at <= 0xdfff) {
        return uncomposable("U7.3");
      }
    }
  }

  return {
    ok: true,
    map: {
      sources: [...sources],
      names: [...names],
      // `null` distinguishes "member absent" from "member present and empty".
      sourcesContent: sourcesContent === undefined ? null : [...sourcesContent],
      // §7.5 normalisation: absent when the member is absent or JSON `null`.
      sourceRoot:
        has("sourceRoot") && member("sourceRoot") !== null
          ? { present: true, value: member("sourceRoot") }
          : { present: false, value: null },
      ignoreList: ignoreList === null ? null : [...ignoreList],
      segments,
      geometry,
    },
  };
}

/**
 * §4.3 stage 2.1 — all contributing maps agree on the normalised `sourceRoot`
 * (§7.5). Runs over the contributing set AT ANY CARDINALITY, including exactly
 * one, where it is vacuously satisfied and that map's value carries through.
 */
export function checkSourceRootAgreement(contributingMaps) {
  let agreed = null;
  for (const { map } of contributingMaps) {
    if (agreed === null) {
      agreed = map.sourceRoot;
      continue;
    }
    if (agreed.present !== map.sourceRoot.present) return uncomposable("U8.1");
    if (agreed.present && agreed.value !== map.sourceRoot.value) return uncomposable("U8.1");
  }
  // With ZERO contributing maps there is no value to agree on and the composed
  // `sourceRoot` is ABSENT (§7.5).
  return { ok: true, sourceRoot: agreed ?? { present: false, value: null } };
}
