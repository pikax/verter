/**
 * STP5 mapper protocol proof helpers.
 *
 * Qualifies the TypeScript content-mapper ABI (initialize / openProject /
 * transform / closeProject, encoding, span kinds, diagnostic directives,
 * observation roles). Does not implement a production Vue mapper.
 *
 * The wire model follows the final protocol: microsoft/typescript-go#4712
 * ("Content mappers") as amended by microsoft/TypeScript#63936 ("Content
 * mappers round 2"), which removed `protocolVersion` from `initialize`, made
 * diagnostic `code` required, and lifted the restriction on overlapping
 * original-text spans.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { MAPPER_HOST_OPERATIONS } from "./mapper-host/e2e.mjs";

const STP5_DIR = path.dirname(fileURLToPath(import.meta.url));

/** Version of this harness module. Not a wire field: the protocol has none. */
export const PROTOCOL_VERSION = 1;

export { MAPPER_HOST_OPERATIONS };

export const REQUIRED_HOST_OPERATIONS = Object.freeze([
  "initialize",
  "openProject",
  "transform",
  "closeProject",
]);

/** JSON-RPC methods; TypeScript sends every request, the mapper none. */
export const MAPPER_METHODS = REQUIRED_HOST_OPERATIONS;

/** `MappedOutput.extension`: how TypeScript parses the virtual text. */
export const VIRTUAL_EXTENSIONS = Object.freeze([
  ".js",
  ".jsx",
  ".mjs",
  ".cjs",
  ".ts",
  ".tsx",
  ".mts",
  ".cts",
  ".json",
]);

/**
 * `InitializeResult.diagnosticSource` may not name TypeScript or any file
 * extension TypeScript understands (compared case-insensitively, without the
 * leading dot).
 */
export const RESERVED_DIAGNOSTIC_SOURCES = Object.freeze([
  "typescript",
  "tsc",
  "ts",
  "tsx",
  "d.ts",
  "cts",
  "d.cts",
  "mts",
  "d.mts",
  "js",
  "jsx",
  "cjs",
  "mjs",
  "json",
]);

/** `package.json` `typescript.contentMapper` fields. */
export const MANIFEST_FIELDS = Object.freeze(["exec", "compilerOptions", "dynamicConfig"]);

export const REQUIRED_OBSERVATION_OPERATIONS = Object.freeze([
  "diagnostics",
  "hover",
  "definition",
  "references",
  "edits",
]);

export const POSITION_ENCODINGS = Object.freeze(["utf-8", "utf-16"]);

export const SpanMapKind = Object.freeze({
  Verbatim: 0,
  Atom: 1,
  Alias: 2,
});

export const SpanMapFeature = Object.freeze({
  None: 0,
  Hover: 1 << 0,
  SignatureHelp: 1 << 1,
  Completion: 1 << 2,
  Definition: 1 << 3,
  TypeDefinition: 1 << 4,
  Implementation: 1 << 5,
  References: 1 << 6,
  DocumentHighlights: 1 << 7,
  Rename: 1 << 8,
  CallHierarchy: 1 << 9,
  CodeActions: 1 << 10,
  Formatting: 1 << 11,
  InlayHints: 1 << 12,
  SemanticTokens: 1 << 13,
  FoldingRanges: 1 << 14,
  SelectionRanges: 1 << 15,
  LinkedEditing: 1 << 16,
  AutoInsert: 1 << 17,
  DocumentSymbols: 1 << 18,
  CodeLens: 1 << 19,
  /** Every feature; the default when a mapping omits `features`. */
  All: (1 << 20) - 1,
});

export const DiagnosticDirectivePolicy = Object.freeze({
  Ignore: 0,
  Expect: 1,
});

export const ENCODING_FIXTURE = 'const emoji = "😀";\r\nconst crlfTwin = "a\\r\\nb";\n';

export function err(caseId, code, message) {
  return { caseId, code, message };
}

export function utf8Length(text) {
  return Buffer.byteLength(text, "utf8");
}

export function spanForToken(text, token) {
  const jsStart = text.indexOf(token);
  if (jsStart < 0) throw new Error(`token not found: ${JSON.stringify(token)}`);
  const prefix = text.slice(0, jsStart);
  return {
    token,
    utf16: { start: jsStart, length: token.length },
    utf8: { start: utf8Length(prefix), length: utf8Length(token) },
  };
}

export function decodeSpan(text, span, encoding) {
  if (encoding === "utf-16") {
    return text.slice(span.start, span.start + span.length);
  }
  if (encoding === "utf-8") {
    const buf = Buffer.from(text, "utf8");
    return buf.subarray(span.start, span.start + span.length).toString("utf8");
  }
  throw new Error(`unsupported encoding: ${encoding}`);
}

export function assertEncodingIdentity(text = ENCODING_FIXTURE, tokens = ["😀", "\r\n"]) {
  const errors = [];
  if (!text.includes("😀") || !text.includes("\r\n")) {
    errors.push(
      err(
        "STP5-encoding",
        "missing-encoding-tokens",
        "encoding fixture must contain emoji and CRLF",
      ),
    );
    return errors;
  }
  for (const token of tokens) {
    const mapped = spanForToken(text, token);
    const utf16Text = decodeSpan(text, mapped.utf16, "utf-16");
    const utf8Text = decodeSpan(text, mapped.utf8, "utf-8");
    if (utf16Text !== token || utf8Text !== token || utf16Text !== utf8Text) {
      errors.push(
        err(
          "STP5-encoding",
          "encoding-mismatch",
          `token ${JSON.stringify(token)} utf-16=${JSON.stringify(utf16Text)} utf-8=${JSON.stringify(utf8Text)}`,
        ),
      );
    }
  }
  const emoji = spanForToken(text, "😀");
  if (emoji.utf8.length !== 4 || emoji.utf16.length !== 2) {
    errors.push(
      err(
        "STP5-encoding",
        "encoding-mismatch",
        `emoji must be 4 UTF-8 bytes and 2 UTF-16 units; got utf8=${emoji.utf8.length} utf16=${emoji.utf16.length}`,
      ),
    );
  }
  const crlf = spanForToken(text, "\r\n");
  if (crlf.utf8.length !== 2 || crlf.utf16.length !== 2) {
    errors.push(
      err(
        "STP5-encoding",
        "encoding-mismatch",
        `CRLF must be length 2 in both encodings; got utf8=${crlf.utf8.length} utf16=${crlf.utf16.length}`,
      ),
    );
  }
  return errors;
}

export function negotiateEncoding(offered, chosen) {
  if (!Array.isArray(offered) || offered.length === 0) {
    return {
      ok: false,
      error: err("STP5-encoding", "encoding-mismatch", "initialize offered no positionEncodings"),
    };
  }
  for (const encoding of offered) {
    if (!POSITION_ENCODINGS.includes(encoding)) {
      return {
        ok: false,
        error: err(
          "STP5-encoding",
          "encoding-mismatch",
          `initialize offered unsupported encoding ${encoding}`,
        ),
      };
    }
  }
  if (!offered.includes(chosen) || !POSITION_ENCODINGS.includes(chosen)) {
    return {
      ok: false,
      error: err(
        "STP5-encoding",
        "encoding-mismatch",
        `mapper chose ${chosen} which was not offered`,
      ),
    };
  }
  return { ok: true, encoding: chosen };
}

function spanEnd(span) {
  return span[0] + span[1];
}

export function assertNonOverlappingVirtualSpans(mappings) {
  const errors = [];
  const sorted = [...mappings].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  for (let i = 1; i < sorted.length; i += 1) {
    const prev = sorted[i - 1];
    const next = sorted[i];
    if (spanEnd(prev) > next[0]) {
      errors.push(
        err("STP5-capability", "overlapping-virtual-spans", `virtual spans overlap at ${next[0]}`),
      );
    }
  }
  return errors;
}

export function virtualToOriginal(mappings, virtualOffset) {
  const hits = [];
  for (const mapping of mappings) {
    const [vStart, vLen, oStart, oLen, kind, features] = mapping;
    if (virtualOffset >= vStart && virtualOffset < vStart + vLen) {
      hits.push({
        kind,
        features: features ?? SpanMapFeature.All,
        originalStart: oStart,
        originalLength: oLen,
      });
    }
  }
  return hits;
}

export function originalToVirtual(mappings, originalOffset) {
  const hits = [];
  for (const mapping of mappings) {
    const [vStart, vLen, oStart, oLen, kind, features] = mapping;
    if (oLen === 0) continue;
    if (originalOffset >= oStart && originalOffset < oStart + oLen) {
      hits.push({
        kind,
        features: features ?? SpanMapFeature.All,
        virtualStart: vStart,
        virtualLength: vLen,
      });
    }
  }
  return hits;
}

export const CLEAN_TRANSFORM = Object.freeze({
  original: "<script>const n = 1;</script>\n<template>{{ n }}</template>",
  text: "const n = 1;\n__synth();\n_n.value;\n__aliasN;",
  extension: ".ts",
  mappings: [
    [0, 12, 8, 12, SpanMapKind.Verbatim],
    [13, 9, 0, 0, SpanMapKind.Atom, SpanMapFeature.None],
    [23, 8, 43, 1, SpanMapKind.Verbatim],
    [32, 8, 8, 9, SpanMapKind.Alias, SpanMapFeature.Hover | SpanMapFeature.Definition],
  ],
  targetFile: "App.vue",
});

export function assertCleanGeometry(transform = CLEAN_TRANSFORM) {
  const errors = [];
  errors.push(...assertNonOverlappingVirtualSpans(transform.mappings));
  const scriptToken = originalToVirtual(transform.mappings, 8);
  if (scriptToken.length < 1) {
    errors.push(
      err("STP5-capability", "missing-script-map", "verbatim script token has no projection"),
    );
  }
  const templateToken = originalToVirtual(transform.mappings, 43);
  if (templateToken.length < 1) {
    errors.push(err("STP5-capability", "missing-template-map", "template token has no projection"));
  }
  const multi = originalToVirtual(transform.mappings, 8);
  if (multi.length < 2) {
    errors.push(
      err(
        "STP5-capability",
        "missing-multi-observation",
        "one source region must project to every observation, not the first hit",
      ),
    );
  }
  const synthesized = virtualToOriginal(transform.mappings, 14);
  if (synthesized.some((hit) => hit.originalLength > 0 && hit.kind === SpanMapKind.Verbatim)) {
    errors.push(
      err("STP5-capability", "synthesized-leaked", "synthesized scaffolding mapped as verbatim"),
    );
  }
  return errors;
}

function isNonNegativeInteger(value) {
  return Number.isInteger(value) && value >= 0;
}

/**
 * The host's acceptance rules for `MappedOutput.diagnosticDirectives`
 * (`{ unusedExpectDirectiveDiagnostics, directives }`), as the selected build
 * enforces them:
 *
 * - each directive is a 5- or 6-tuple
 *   `[originalStart, originalLength, virtualStart, virtualEnd, policy, unusedExpectDirectiveIndex?]`;
 * - `policy` is `Ignore` (0) or `Expect` (1);
 * - `virtualEnd >= virtualStart` and both lie in the virtual text;
 * - an `Expect` directive needs a valid original range, and names its
 *   unused-Expect diagnostic by index, which may be omitted only when exactly
 *   one `unusedExpectDirectiveDiagnostics` entry exists;
 * - directive virtual ranges do not overlap.
 *
 * Lengths are in the negotiated encoding; `virtualLength` / `originalLength`
 * bound the ranges when supplied.
 */
export function validateWireDiagnosticDirectives(
  value,
  { virtualLength = Infinity, originalLength: originalTextLength = Infinity } = {},
) {
  const errors = [];
  const fail = (code, message) => errors.push(err("STP5-guard-duplicate", code, message));
  if (value === undefined || value === null) return errors;
  if (Array.isArray(value) || typeof value !== "object") {
    fail(
      "directive-shape",
      "diagnosticDirectives is an object { unusedExpectDirectiveDiagnostics, directives }, not a bare tuple array",
    );
    return errors;
  }
  const unused = value.unusedExpectDirectiveDiagnostics;
  if (!Array.isArray(unused)) {
    fail(
      "directive-shape",
      "diagnosticDirectives.unusedExpectDirectiveDiagnostics must be an array",
    );
    return errors;
  }
  for (const [index, row] of unused.entries()) {
    if (!Number.isInteger(row?.code) || typeof row?.messageText !== "string") {
      fail(
        "directive-shape",
        `unusedExpectDirectiveDiagnostics[${index}] needs an integer code and messageText`,
      );
    }
  }
  if (!Array.isArray(value.directives)) {
    fail("directive-shape", "diagnosticDirectives.directives must be an array");
    return errors;
  }
  const ranges = [];
  for (const [index, directive] of value.directives.entries()) {
    if (!Array.isArray(directive) || (directive.length !== 5 && directive.length !== 6)) {
      fail("directive-shape", `directive ${index} must be a 5- or 6-element tuple`);
      continue;
    }
    const [originalStart, originalLength, virtualStart, virtualEnd, policy, unusedIndex] =
      directive;
    if (
      policy !== DiagnosticDirectivePolicy.Ignore &&
      policy !== DiagnosticDirectivePolicy.Expect
    ) {
      fail("hidden-blame", `directive ${index} has unknown policy ${policy}`);
      continue;
    }
    if (
      !isNonNegativeInteger(virtualStart) ||
      !Number.isInteger(virtualEnd) ||
      virtualEnd < virtualStart ||
      virtualEnd > virtualLength
    ) {
      fail(
        "hidden-blame",
        `directive ${index} virtual range [${virtualStart}, ${virtualEnd}) is invalid`,
      );
      continue;
    }
    if (policy === DiagnosticDirectivePolicy.Expect) {
      if (
        !isNonNegativeInteger(originalStart) ||
        !isNonNegativeInteger(originalLength) ||
        originalStart + originalLength > originalTextLength
      ) {
        fail("fabricated-blame", `Expect directive ${index} original range is not in the source`);
      }
      if (unusedIndex === undefined) {
        if (unused.length !== 1) {
          fail(
            "unused-expect-index",
            `Expect directive ${index} must name unusedExpectDirectiveIndex when there is not exactly one unused diagnostic`,
          );
        }
      } else if (!isNonNegativeInteger(unusedIndex) || unusedIndex >= unused.length) {
        fail(
          "unused-expect-index",
          `Expect directive ${index} unusedExpectDirectiveIndex ${unusedIndex} is out of range`,
        );
      }
    }
    ranges.push({ index, start: virtualStart, end: virtualEnd });
  }
  ranges.sort((a, b) => a.start - b.start);
  for (let i = 1; i < ranges.length; i += 1) {
    if (ranges[i].start < ranges[i - 1].end) {
      fail(
        "duplicate-guard",
        `directive ${ranges[i].index} virtual range overlaps directive ${ranges[i - 1].index}`,
      );
    }
  }
  return errors;
}

/**
 * Verter's emission policy for the directives it sends: the host's wire
 * rules, plus no directive without an authored directive behind it (empty
 * original span) and no directive that can cover nothing (empty virtual
 * range). The host accepts both of those; Verter does not emit them, because
 * an empty Expect is always reported unused and an empty-origin directive
 * suppresses with no blame the author can see.
 */
export function validateDiagnosticDirectives(value, originalText) {
  const errors = validateWireDiagnosticDirectives(value, { originalLength: originalText.length });
  if (errors.some((error) => error.code === "directive-shape")) return errors;
  const directives = value?.directives || [];
  const seen = new Set();
  for (const directive of directives) {
    const [originalStart, originalLength, virtualStart, virtualEnd, policy] = directive;
    if (
      !(originalLength > 0) ||
      !(originalStart >= 0) ||
      originalStart + originalLength > originalText.length
    ) {
      errors.push(
        err(
          "STP5-guard-duplicate",
          "fabricated-blame",
          `directive original span [${originalStart}, ${originalLength}) is not an authored directive in the source`,
        ),
      );
    }
    if (virtualEnd <= virtualStart) {
      errors.push(err("STP5-guard-duplicate", "hidden-blame", "directive virtual range is empty"));
    }
    const key = `${originalStart}:${originalLength}:${virtualStart}:${virtualEnd}:${policy}`;
    if (seen.has(key)) {
      errors.push(
        err(
          "STP5-guard-duplicate",
          "duplicate-guard",
          "invalid repeated guards hide or fabricate diagnostic blame",
        ),
      );
    }
    seen.add(key);
  }
  return errors;
}

/**
 * An unused `Expect` directive is reported in the ORIGINAL file, over the
 * directive's original span, with the code and text of the
 * `unusedExpectDirectiveDiagnostics` entry it names (index 0 when omitted)
 * and the mapper's `diagnosticSource`.
 */
export function validateUnusedExpectReportedInOriginal({
  directives,
  virtualDiagnostics,
  originalReports,
  diagnosticSource,
} = {}) {
  const errors = [];
  const unusedTable = directives?.unusedExpectDirectiveDiagnostics || [];
  for (const directive of directives?.directives || []) {
    const [originalStart, originalLength, virtualStart, virtualEnd, policy, unusedIndex] =
      directive;
    if (policy !== DiagnosticDirectivePolicy.Expect) continue;
    const used = (virtualDiagnostics || []).some((diag) => {
      const diagStart = diag.start;
      const diagEnd = diag.end ?? diagStart + (diag.length || 0);
      return diagStart < virtualEnd && virtualStart < diagEnd;
    });
    if (used) continue;
    const expected = unusedTable[unusedIndex ?? 0];
    const reported = (originalReports || []).some((report) => {
      if (report.file && report.file !== "original") return false;
      if (expected && report.code !== expected.code) return false;
      if (diagnosticSource && report.source !== diagnosticSource) return false;
      const reportEnd = report.start + (report.length || 0);
      const originalEnd = originalStart + originalLength;
      return report.start < originalEnd && originalStart < reportEnd;
    });
    if (!reported) {
      errors.push(
        err(
          "STP5-guard-duplicate",
          "unused-expect-not-in-original",
          "unused Expect directive must be reported in the original file with its unusedExpectDirectiveDiagnostics code",
        ),
      );
    }
  }
  return errors;
}

export function assertFeatureMaskDoesNotSuppressDiagnostics(claim) {
  if (
    claim?.diagnosticsDroppedBecause === "feature-mask" ||
    claim?.featureMaskSuppressesDiagnostics === true
  ) {
    return [
      err(
        "STP5-guard-duplicate",
        "feature-mask-suppression",
        "feature mask must not suppress diagnostics; use diagnosticDirectives",
      ),
    ];
  }
  return [];
}

export function validateEdit(mapping, edit) {
  const [vStart, vLen, oStart, oLen, kind] = mapping;
  const isKebabPascal =
    Boolean(edit?.kebabToPascal) ||
    Boolean(edit?.pascalToKebab) ||
    Boolean(edit?.aliasAsRenameCodec);
  if (kind === SpanMapKind.Alias && (edit?.feature === "rename" || isKebabPascal)) {
    return [
      err(
        "STP5-alias-edit",
        "alias-rename-codec",
        "Alias spans are diagnostic-display substitutions, not kebab/Pascal rename codecs",
      ),
    ];
  }
  if (edit && kind !== SpanMapKind.Verbatim) {
    return [
      err(
        "STP5-alias-edit",
        "non-verbatim-edit",
        "authored edits may be written back only through length-preserving Verbatim spans",
      ),
    ];
  }
  if (edit && kind === SpanMapKind.Verbatim && vLen !== oLen) {
    return [
      err(
        "STP5-alias-edit",
        "non-verbatim-edit",
        `Verbatim edit span is not length-preserving (virtual ${vLen} original ${oLen} at ${vStart}/${oStart})`,
      ),
    ];
  }
  return [];
}

export function validateDefinitionTarget(query, target) {
  if (!target || !target.file) {
    return [err("STP5-stale-target", "missing-target", "definition target file is missing")];
  }
  if (query.file !== target.file && query.snapshotId === target.mappedWithSnapshotId) {
    return [
      err(
        "STP5-stale-target",
        "query-snapshot-reuse",
        "a definition in another file must not be mapped using the querying file's snapshot",
      ),
    ];
  }
  if (
    target.ownSnapshotId &&
    target.mappedWithSnapshotId &&
    target.ownSnapshotId !== target.mappedWithSnapshotId
  ) {
    return [
      err(
        "STP5-stale-target",
        "stale-target-snapshot",
        "foreign definition mapped with a snapshot that is not the target file's own",
      ),
    ];
  }
  return [];
}

export function validateCliAttribution(record) {
  if (!record) {
    return [err("STP5-raw-cli", "missing-presentation", "CLI diagnostic attribution is missing")];
  }
  const stock = record.presentation === "stock-cli";
  const verterOnly = record.source === "verter-postprocessor" || record.source === "verter-only";
  if (stock && verterOnly) {
    return [
      err(
        "STP5-raw-cli",
        "verter-claimed-as-stock",
        "stock mapper diagnostic behavior must not be claimed from a Verter-only postprocessor",
      ),
    ];
  }
  return [];
}

export function validateCapabilityClaim(row) {
  const errors = [];
  if (!row || !row.operation) {
    return [err("STP5-capability", "missing-operation", "capability row missing operation")];
  }
  if (row.evidence === "7.1.0-dev-label" || row.evidence === "version-label") {
    errors.push(
      err(
        "STP5-capability",
        "version-label-is-not-proof",
        `${row.operation}: descriptive 7.1.0-dev / version label is not a capability proof`,
      ),
    );
  }
  if (row.evidence === "dormant-tcm2" || row.completeFromDormantProduct === true) {
    errors.push(
      err(
        "STP5-capability",
        "dormant-product-claim",
        `${row.operation}: complete capability claim backed only by dormant TCM2 is forbidden`,
      ),
    );
  }
  const supported = row.status === "supported";
  const blocked = row.status === "blocking-upstream-defect";
  if (supported && row.evidence !== "executable-selected-build") {
    errors.push(
      err(
        "STP5-capability",
        "unsupported-evidence",
        `${row.operation}: supported claims require executable selected-build evidence`,
      ),
    );
  }
  if (blocked && (!row.evidence || String(row.evidence).length === 0)) {
    errors.push(
      err(
        "STP5-capability",
        "missing-defect-evidence",
        `${row.operation}: blocking upstream defect must name the selected-build gap`,
      ),
    );
  }
  if (!supported && !blocked) {
    errors.push(
      err(
        "STP5-capability",
        "silent-degrade",
        `${row.operation}: absent native capability must block qualification, not silently degrade`,
      ),
    );
  }
  return errors;
}

/**
 * `InitializeResult`: `{ positionEncoding, diagnosticSource }` and nothing
 * else the host reads. The encoding must be one TypeScript offered; the
 * source must be non-empty and not reserved.
 */
export function validateInitializeResult(result, offered = POSITION_ENCODINGS) {
  const errors = [];
  if (!result || typeof result !== "object") {
    return [err("STP5-capability", "initialize-shape", "initialize returned no result object")];
  }
  if (!offered.includes(result.positionEncoding)) {
    errors.push(
      err(
        "STP5-encoding",
        "encoding-mismatch",
        `positionEncoding ${result.positionEncoding} was not offered`,
      ),
    );
  }
  const source = typeof result.diagnosticSource === "string" ? result.diagnosticSource.trim() : "";
  if (source.length === 0) {
    errors.push(
      err("STP5-raw-cli", "missing-diagnostic-source", "initialize must return a diagnosticSource"),
    );
  } else if (RESERVED_DIAGNOSTIC_SOURCES.includes(source.toLowerCase())) {
    errors.push(
      err(
        "STP5-raw-cli",
        "reserved-diagnostic-source",
        `diagnosticSource ${JSON.stringify(result.diagnosticSource)} names TypeScript or a TypeScript extension`,
      ),
    );
  }
  if ("protocolVersion" in result) {
    errors.push(
      err(
        "STP5-capability",
        "protocol-version",
        "protocolVersion was removed from initialize (TypeScript#63936)",
      ),
    );
  }
  return errors;
}

/**
 * `OpenProjectResult` against the mapper manifest's `dynamicConfig`:
 * `configIdentity` is required with it and forbidden without it,
 * `watchedFiles` (absolute paths) are allowed only with it, and each
 * `optionDiagnostics` row is
 * `{ path: (string | nonnegative integer)[], messageText, code }`.
 */
export function validateOpenProjectResult(result, { dynamicConfig = false } = {}) {
  const errors = [];
  const fail = (code, message) => errors.push(err("STP5-capability", code, message));
  if (!result || typeof result !== "object") {
    fail("open-project-shape", "openProject returned no result object");
    return errors;
  }
  const hasIdentity = typeof result.configIdentity === "string" && result.configIdentity.length > 0;
  if (dynamicConfig && !hasIdentity) {
    fail("missing-config-identity", "dynamicConfig mapper must return configIdentity");
  }
  if (!dynamicConfig && result.configIdentity) {
    fail(
      "unexpected-config-identity",
      "configIdentity is only allowed when the manifest declares dynamicConfig",
    );
  }
  if (result.watchedFiles !== undefined) {
    if (!dynamicConfig && result.watchedFiles.length > 0) {
      fail(
        "unexpected-watched-files",
        "watchedFiles are only allowed when the manifest declares dynamicConfig",
      );
    }
    for (const file of result.watchedFiles) {
      if (typeof file !== "string" || !path.isAbsolute(file)) {
        fail("relative-watched-file", `watched file ${JSON.stringify(file)} is not absolute`);
      }
    }
  }
  for (const [index, row] of (result.optionDiagnostics || []).entries()) {
    const pathOk =
      Array.isArray(row?.path) &&
      row.path.every((segment) => typeof segment === "string" || isNonNegativeInteger(segment));
    if (!pathOk || typeof row.messageText !== "string" || !Number.isInteger(row.code)) {
      fail(
        "option-diagnostic-shape",
        `optionDiagnostics[${index}] needs path, messageText and an integer code`,
      );
    }
  }
  return errors;
}

function validateMappedOutput(output, { originalLength, label }) {
  const errors = [];
  const fail = (code, message) => errors.push(err("STP5-capability", code, `${label}: ${message}`));
  if (typeof output?.text !== "string") {
    fail("mapped-output-shape", "text must be a string");
    return errors;
  }
  if (!VIRTUAL_EXTENSIONS.includes(output.extension)) {
    fail(
      "virtual-extension",
      `extension ${JSON.stringify(output.extension)} is not a supported virtual extension`,
    );
  }
  const mappings = output.mappings || [];
  const kinds = [SpanMapKind.Verbatim, SpanMapKind.Atom, SpanMapKind.Alias];
  for (const [index, mapping] of mappings.entries()) {
    const [vStart, vLen, oStart, oLen, kind, features] = Array.isArray(mapping) ? mapping : [];
    if (
      !Array.isArray(mapping) ||
      (mapping.length !== 5 && mapping.length !== 6) ||
      ![vStart, vLen, oStart, oLen].every(isNonNegativeInteger) ||
      !kinds.includes(kind) ||
      (features !== undefined &&
        !(isNonNegativeInteger(features) && features <= SpanMapFeature.All))
    ) {
      fail(
        "mapping-shape",
        `mapping ${index} is not [virtualStart, virtualLength, originalStart, originalLength, kind, features?]`,
      );
      continue;
    }
    if (oStart + oLen > originalLength) {
      fail("mapping-range", `mapping ${index} original span exceeds the source`);
    }
  }
  errors.push(...assertNonOverlappingVirtualSpans(mappings.filter(Array.isArray)));
  errors.push(...validateWireDiagnosticDirectives(output.diagnosticDirectives, { originalLength }));
  return errors;
}

/**
 * A `transform` result (`TransformResult extends MappedOutput`): canonical
 * output, optional `supplemental` outputs, and `diagnostics` whose `code` is
 * required (TypeScript#63936). Absent `mappings` mean fully synthesized
 * output. Original lengths are measured in the negotiated `encoding`.
 */
export function validateTransformResult(result, originalText, encoding = "utf-16") {
  const originalLength = encoding === "utf-8" ? utf8Length(originalText) : originalText.length;
  const errors = validateMappedOutput(result, { originalLength, label: "transform" });
  for (const [index, supplemental] of (result?.supplemental || []).entries()) {
    errors.push(
      ...validateMappedOutput(supplemental, { originalLength, label: `supplemental[${index}]` }),
    );
  }
  for (const [index, diagnostic] of (result?.diagnostics || []).entries()) {
    if (
      typeof diagnostic?.messageText !== "string" ||
      !isNonNegativeInteger(diagnostic.start) ||
      !isNonNegativeInteger(diagnostic.length) ||
      !Number.isInteger(diagnostic.code) ||
      diagnostic.start + diagnostic.length > originalLength
    ) {
      errors.push(
        err(
          "STP5-capability",
          "mapper-diagnostic-shape",
          `diagnostics[${index}] needs messageText, start, length and code`,
        ),
      );
    }
  }
  return errors;
}

/**
 * A harness mapper session over the final wire shapes. `manifest` stands in
 * for the package's `typescript.contentMapper`; only `dynamicConfig` matters
 * to the session. `dynamicConfig` is a manifest field, never an openProject
 * parameter, and `configIdentity` / `watchedFiles` are openProject RESULTS.
 */
export function createMapperSession({ manifest = { dynamicConfig: false } } = {}) {
  const projects = new Map();
  let initialized = false;
  let encoding = null;
  return {
    initialize(params) {
      const negotiated = negotiateEncoding(params.positionEncodings, params.chosen || "utf-16");
      if (!negotiated.ok) return negotiated;
      initialized = true;
      encoding = negotiated.encoding;
      return {
        ok: true,
        result: { positionEncoding: encoding, diagnosticSource: "vue-mapper-proof" },
        locale: params.locale,
      };
    },
    openProject(params) {
      if (!initialized) {
        return {
          ok: false,
          error: err("STP5-capability", "missing-initialize", "openProject before initialize"),
        };
      }
      if (!params.projectHandle) {
        return {
          ok: false,
          error: err(
            "STP5-capability",
            "missing-project-handle",
            "openProject requires projectHandle",
          ),
        };
      }
      // `options` and `compilerOptions` arrive only here; transform state is
      // keyed by the handle and released on closeProject.
      projects.set(params.projectHandle, {
        configFileName: params.configFileName,
        options: params.options,
        compilerOptions: params.compilerOptions || {},
      });
      const result = { optionDiagnostics: [] };
      if (manifest.dynamicConfig) {
        result.configIdentity = `identity:${params.configFileName}`;
        result.watchedFiles = [];
      }
      return { ok: true, result };
    },
    transform(params) {
      const project = projects.get(params.projectHandle);
      if (!project) {
        return {
          ok: false,
          error: err(
            "STP5-stale-target",
            "unknown-project-handle",
            "transform used a closed or foreign projectHandle",
          ),
        };
      }
      if (typeof params.content !== "string") {
        return {
          ok: false,
          error: err(
            "STP5-capability",
            "missing-content",
            "transform requires full original content",
          ),
        };
      }
      const { original: _original, targetFile: _targetFile, ...output } = CLEAN_TRANSFORM;
      return { ok: true, result: output };
    },
    closeProject(params) {
      if (!projects.has(params.projectHandle)) {
        return {
          ok: false,
          error: err("STP5-capability", "double-close", "closeProject for unknown projectHandle"),
        };
      }
      projects.delete(params.projectHandle);
      return { ok: true };
    },
    isOpen(handle) {
      return projects.has(handle);
    },
  };
}

const UNUSED_EXPECT_TABLE = Object.freeze([
  Object.freeze({ code: 9002, messageText: "Unused '@vue-expect-error' directive." }),
]);

export const CLEAN_GUARD = Object.freeze({
  original: '<!-- @vue-expect-error -->\n<div :id="first"></div>',
  directives: {
    unusedExpectDirectiveDiagnostics: UNUSED_EXPECT_TABLE,
    directives: [[0, 24, 0, 40, DiagnosticDirectivePolicy.Expect]],
  },
});

export const DIRTY_GUARD_DUPLICATE = Object.freeze({
  original: CLEAN_GUARD.original,
  directives: {
    unusedExpectDirectiveDiagnostics: UNUSED_EXPECT_TABLE,
    directives: [
      [0, 24, 0, 40, DiagnosticDirectivePolicy.Expect],
      [0, 24, 0, 40, DiagnosticDirectivePolicy.Expect],
    ],
  },
});

/** The bare tuple array Verter modelled before: the host refuses it. */
export const DIRTY_GUARD_BARE_ARRAY = Object.freeze({
  original: CLEAN_GUARD.original,
  directives: [[0, 24, 0, 40, DiagnosticDirectivePolicy.Expect]],
});

/** Two unused diagnostics and an Expect that names neither. */
export const DIRTY_GUARD_AMBIGUOUS_INDEX = Object.freeze({
  original: CLEAN_GUARD.original,
  directives: {
    unusedExpectDirectiveDiagnostics: [
      ...UNUSED_EXPECT_TABLE,
      { code: 9003, messageText: "Unused '@vue-skip' directive." },
    ],
    directives: [[0, 24, 0, 40, DiagnosticDirectivePolicy.Expect]],
  },
});

export const CLEAN_ALIAS = Object.freeze({
  mapping: [0, 6, 0, 7, SpanMapKind.Alias, SpanMapFeature.Hover],
  edit: null,
});

export const DIRTY_ALIAS_EDIT = Object.freeze({
  mapping: [0, 6, 0, 7, SpanMapKind.Alias, SpanMapFeature.Rename],
  edit: {
    feature: "rename",
    kebabToPascal: true,
    from: "my-comp",
    to: "MyComp",
    aliasAsRenameCodec: true,
  },
});

export const CLEAN_DEFINITION = Object.freeze({
  query: { file: "App.vue", snapshotId: "snap-A" },
  target: { file: "Button.vue", ownSnapshotId: "snap-B", mappedWithSnapshotId: "snap-B" },
});

export const DIRTY_STALE_TARGET = Object.freeze({
  query: { file: "App.vue", snapshotId: "snap-A" },
  target: { file: "Button.vue", ownSnapshotId: "snap-B", mappedWithSnapshotId: "snap-A" },
});

export const CLEAN_STOCK_CLI = Object.freeze({
  presentation: "stock-cli",
  source: "typescript-tsc",
});

export const DIRTY_RAW_CLI = Object.freeze({
  presentation: "stock-cli",
  source: "verter-postprocessor",
});

export const DIRTY_VERSION_LABEL_CLAIM = Object.freeze({
  operation: "initialize",
  status: "supported",
  evidence: "7.1.0-dev-label",
});

export const DIRTY_DORMANT_CLAIM = Object.freeze({
  operation: "transform",
  status: "supported",
  evidence: "dormant-tcm2",
  completeFromDormantProduct: true,
});

export const DIRTY_SILENT_DEGRADE_CLAIM = Object.freeze({
  operation: "transform",
  status: "unavailable",
  evidence: "not-built",
});

export const DIRTY_BLOCKED_EMPTY_EVIDENCE = Object.freeze({
  operation: "initialize",
  status: "blocking-upstream-defect",
  evidence: "",
});

export const CLEAN_UNUSED_EXPECT = Object.freeze({
  original: '<!-- @vue-expect-error -->\n<div :id="ok"></div>',
  directives: CLEAN_GUARD.directives,
  virtualDiagnostics: [],
  diagnosticSource: "vue-mapper-proof",
  originalReports: [
    { file: "original", start: 0, length: 24, code: 9002, source: "vue-mapper-proof" },
  ],
});

export const DIRTY_UNUSED_EXPECT = Object.freeze({
  original: CLEAN_UNUSED_EXPECT.original,
  directives: CLEAN_UNUSED_EXPECT.directives,
  virtualDiagnostics: [],
  originalReports: [],
});

/** Reported at the directive, but not with the code the mapper declared. */
export const DIRTY_UNUSED_EXPECT_WRONG_CODE = Object.freeze({
  ...CLEAN_UNUSED_EXPECT,
  originalReports: [
    { file: "original", start: 0, length: 24, code: 2578, source: "vue-mapper-proof" },
  ],
});

export const USED_EXPECT_NO_UNUSED_REPORT = Object.freeze({
  original: CLEAN_GUARD.original,
  directives: CLEAN_GUARD.directives,
  virtualDiagnostics: [{ start: 0, end: 40 }],
  originalReports: [],
});

export function evaluateRejectTwins() {
  const errors = [];
  const cleanGuard = validateDiagnosticDirectives(CLEAN_GUARD.directives, CLEAN_GUARD.original);
  if (cleanGuard.length) errors.push(...cleanGuard);
  const dirtyGuard = validateDiagnosticDirectives(
    DIRTY_GUARD_DUPLICATE.directives,
    DIRTY_GUARD_DUPLICATE.original,
  );
  if (dirtyGuard.length === 0) {
    errors.push(
      err(
        "STP5-guard-duplicate",
        "missed-duplicate",
        "duplicate guard dirty twin was not rejected",
      ),
    );
  }
  const cleanAlias = validateEdit(CLEAN_ALIAS.mapping, CLEAN_ALIAS.edit);
  if (cleanAlias.length) errors.push(...cleanAlias);
  const dirtyAlias = validateEdit(DIRTY_ALIAS_EDIT.mapping, DIRTY_ALIAS_EDIT.edit);
  if (dirtyAlias.length === 0) {
    errors.push(
      err("STP5-alias-edit", "missed-alias-edit", "Alias rename-codec dirty twin was not rejected"),
    );
  }
  const cleanDef = validateDefinitionTarget(CLEAN_DEFINITION.query, CLEAN_DEFINITION.target);
  if (cleanDef.length) errors.push(...cleanDef);
  const dirtyDef = validateDefinitionTarget(DIRTY_STALE_TARGET.query, DIRTY_STALE_TARGET.target);
  if (dirtyDef.length === 0) {
    errors.push(
      err(
        "STP5-stale-target",
        "missed-stale-target",
        "query-snapshot reuse dirty twin was not rejected",
      ),
    );
  }
  const cleanCli = validateCliAttribution(CLEAN_STOCK_CLI);
  if (cleanCli.length) errors.push(...cleanCli);
  const dirtyCli = validateCliAttribution(DIRTY_RAW_CLI);
  if (dirtyCli.length === 0) {
    errors.push(
      err("STP5-raw-cli", "missed-raw-cli", "Verter-as-stock-CLI dirty twin was not rejected"),
    );
  }
  const dirtyLabel = validateCapabilityClaim(DIRTY_VERSION_LABEL_CLAIM);
  if (dirtyLabel.length === 0) {
    errors.push(
      err("STP5-capability", "missed-version-label", "7.1.0-dev label claim was not rejected"),
    );
  }
  const dirtyDormant = validateCapabilityClaim(DIRTY_DORMANT_CLAIM);
  if (dirtyDormant.length === 0) {
    errors.push(
      err(
        "STP5-capability",
        "missed-dormant-claim",
        "dormant TCM2 complete-capability claim was not rejected",
      ),
    );
  }
  const dirtySilent = validateCapabilityClaim(DIRTY_SILENT_DEGRADE_CLAIM);
  if (!dirtySilent.some((error) => error.code === "silent-degrade")) {
    errors.push(
      err(
        "STP5-capability",
        "missed-silent-degrade",
        "neither-status capability row was not rejected as silent-degrade",
      ),
    );
  }
  const dirtyBlockedEmpty = validateCapabilityClaim(DIRTY_BLOCKED_EMPTY_EVIDENCE);
  if (!dirtyBlockedEmpty.some((error) => error.code === "missing-defect-evidence")) {
    errors.push(
      err(
        "STP5-capability",
        "missed-defect-evidence",
        "blocking row with empty evidence was not rejected",
      ),
    );
  }
  const cleanUnused = validateUnusedExpectReportedInOriginal(CLEAN_UNUSED_EXPECT);
  if (cleanUnused.length) errors.push(...cleanUnused);
  const usedExpect = validateUnusedExpectReportedInOriginal(USED_EXPECT_NO_UNUSED_REPORT);
  if (usedExpect.length) errors.push(...usedExpect);
  const dirtyUnused = validateUnusedExpectReportedInOriginal(DIRTY_UNUSED_EXPECT);
  if (!dirtyUnused.some((error) => error.code === "unused-expect-not-in-original")) {
    errors.push(
      err(
        "STP5-guard-duplicate",
        "missed-unused-expect",
        "unused Expect without an original-file report was not rejected",
      ),
    );
  }
  const dirtyMask = assertFeatureMaskDoesNotSuppressDiagnostics({
    featureMaskSuppressesDiagnostics: true,
    diagnosticsDroppedBecause: "feature-mask",
  });
  if (dirtyMask.length === 0) {
    errors.push(
      err(
        "STP5-guard-duplicate",
        "missed-feature-mask",
        "feature-mask diagnostic suppression was not rejected",
      ),
    );
  }
  const cleanMask = assertFeatureMaskDoesNotSuppressDiagnostics({
    featureMaskSuppressesDiagnostics: false,
    diagnosticsDroppedBecause: null,
  });
  if (cleanMask.length) errors.push(...cleanMask);
  for (const twin of WIRE_REJECT_TWINS) {
    const found = twin.validate();
    if (!found.some((error) => error.code === twin.code)) {
      errors.push(
        err(twin.caseId, "missed-wire-twin", `${twin.name} was not rejected with ${twin.code}`),
      );
    }
  }
  return errors;
}

/**
 * Non-conforming wire replies, each paired with the code the validators must
 * refuse it with. These are the shapes the selected build refuses (see the
 * executed end-to-end reject twins), modelled without a host.
 */
export const WIRE_REJECT_TWINS = Object.freeze([
  {
    name: "bare directive tuple array",
    caseId: "STP5-guard-duplicate",
    code: "directive-shape",
    validate: () => validateWireDiagnosticDirectives(DIRTY_GUARD_BARE_ARRAY.directives),
  },
  {
    name: "Expect without unusedExpectDirectiveIndex among two unused diagnostics",
    caseId: "STP5-guard-duplicate",
    code: "unused-expect-index",
    validate: () => validateWireDiagnosticDirectives(DIRTY_GUARD_AMBIGUOUS_INDEX.directives),
  },
  {
    name: "unused Expect reported with a code the mapper did not declare",
    caseId: "STP5-guard-duplicate",
    code: "unused-expect-not-in-original",
    validate: () => validateUnusedExpectReportedInOriginal(DIRTY_UNUSED_EXPECT_WRONG_CODE),
  },
  {
    name: "initialize returning protocolVersion",
    caseId: "STP5-capability",
    code: "protocol-version",
    validate: () =>
      validateInitializeResult({
        protocolVersion: 1,
        positionEncoding: "utf-16",
        diagnosticSource: "vue",
      }),
  },
  {
    name: "diagnosticSource naming TypeScript",
    caseId: "STP5-raw-cli",
    code: "reserved-diagnostic-source",
    validate: () =>
      validateInitializeResult({ positionEncoding: "utf-8", diagnosticSource: "TSC" }),
  },
  {
    name: "dynamicConfig openProject without configIdentity",
    caseId: "STP5-capability",
    code: "missing-config-identity",
    validate: () => validateOpenProjectResult({ watchedFiles: [] }, { dynamicConfig: true }),
  },
  {
    name: "static openProject returning watchedFiles",
    caseId: "STP5-capability",
    code: "unexpected-watched-files",
    validate: () =>
      validateOpenProjectResult(
        { watchedFiles: [path.resolve("/tmp/vue.config.json")] },
        { dynamicConfig: false },
      ),
  },
  {
    name: "transform result without extension",
    caseId: "STP5-capability",
    code: "virtual-extension",
    validate: () => {
      const {
        original,
        targetFile: _targetFile,
        extension: _extension,
        ...output
      } = CLEAN_TRANSFORM;
      return validateTransformResult(output, original);
    },
  },
  {
    name: "mapper diagnostic without code",
    caseId: "STP5-capability",
    code: "mapper-diagnostic-shape",
    validate: () => {
      const { original, targetFile: _targetFile, ...output } = CLEAN_TRANSFORM;
      return validateTransformResult(
        { ...output, diagnostics: [{ messageText: "bad", start: 0, length: 1 }] },
        original,
      );
    },
  },
]);

export function helpTextHasMapperHost(helpText) {
  const text = String(helpText || "");
  return (
    /runExternalCode/i.test(text) || /contentMappers/i.test(text) || /content-mapper/i.test(text)
  );
}

/**
 * Every host-side operation a selected build is asked to prove: the four
 * methods plus each wire rule the executed end-to-end run exercises.
 */
export const HOST_CAPABILITY_OPERATIONS = Object.freeze([
  ...new Set([...REQUIRED_HOST_OPERATIONS, ...MAPPER_HOST_OPERATIONS]),
]);

/** The phrase a build without a content-mapper host is recorded with. */
export function noMapperHostEvidence(engineId, engineVersion, detail) {
  const base = `no mapper host on ${engineId}@${engineVersion}`;
  return detail ? `${base}: ${detail}` : base;
}

/**
 * Selected-build capability rows for one engine.
 *
 * Host operations are `supported` only from the executed end-to-end run
 * (`hostRun`, from `mapper-host/e2e.mjs`) on that build. A build without a
 * host, or a host whose run was not executed, records every host operation as
 * a blocking upstream defect naming the gap; it never passes silently.
 * `encoding-negotiation` additionally requires the harness encoding identity.
 */
export function capabilityRowsFromProbe({
  mapperHostPresent,
  mapperHostEvidence,
  hostRun,
  observation,
  engineId,
  engineVersion,
  encodingIdentityOk,
}) {
  const rows = [];
  const executed = new Map((hostRun?.operations || []).map((row) => [row.operation, row]));
  for (const operation of HOST_CAPABILITY_OPERATIONS) {
    let blocked = null;
    if (!mapperHostPresent) {
      blocked =
        mapperHostEvidence && /no mapper host on/.test(mapperHostEvidence)
          ? mapperHostEvidence
          : noMapperHostEvidence(
              engineId,
              engineVersion,
              mapperHostEvidence || "mapper-host probe not performed",
            );
    } else if (!hostRun?.performed) {
      blocked = `${engineId}@${engineVersion} has a mapper host but the end-to-end mapper run was not executed`;
    } else if (!executed.get(operation)?.ok) {
      blocked = `${engineId}@${engineVersion} ${operation}: ${executed.get(operation)?.detail ?? "not exercised"}`;
    } else if (operation === "encoding-negotiation" && !encodingIdentityOk) {
      blocked = `${engineId} encoding identity (emoji/CRLF UTF-8/UTF-16) failed`;
    }
    rows.push(
      blocked
        ? { operation, engineId, status: "blocking-upstream-defect", evidence: blocked }
        : {
            operation,
            engineId,
            status: "supported",
            evidence: "executable-selected-build",
            detail: executed.get(operation).detail,
          },
    );
  }
  for (const operation of REQUIRED_OBSERVATION_OPERATIONS) {
    const ok = Boolean(observation?.[operation]);
    rows.push(
      ok
        ? { operation, engineId, status: "supported", evidence: "executable-selected-build" }
        : {
            operation,
            engineId,
            status: "blocking-upstream-defect",
            evidence: `${engineId} missing ${operation} observation`,
          },
    );
  }
  return rows;
}

export function evaluateCapabilityRows(rows) {
  const errors = [];
  const seen = new Set();
  for (const row of rows) {
    seen.add(row.operation);
    errors.push(...validateCapabilityClaim(row));
  }
  for (const operation of [...HOST_CAPABILITY_OPERATIONS, ...REQUIRED_OBSERVATION_OPERATIONS]) {
    if (!seen.has(operation)) {
      errors.push(
        err(
          "STP5-capability",
          "missing-operation",
          `required operation ${operation} has no selected-build row`,
        ),
      );
    }
  }
  return errors;
}

/**
 * The committed MapperCapabilityEvidence must describe the protocol this
 * module validates: same methods, same required operations, the final
 * field set, and a host probe that executes rather than reads a label.
 */
export function validateMapperCapabilityEvidence(evidence) {
  const errors = [];
  const fail = (message) => errors.push(err("STP5-capability", "evidence-protocol-drift", message));
  const protocol = evidence?.selectedMapperProtocol || {};
  if (JSON.stringify(protocol.methods) !== JSON.stringify(MAPPER_METHODS)) {
    fail(
      `selectedMapperProtocol.methods ${JSON.stringify(protocol.methods)} != ${JSON.stringify(MAPPER_METHODS)}`,
    );
  }
  if (
    protocol.protocolVersion !== undefined ||
    protocol.initialize?.result?.includes?.("protocolVersion")
  ) {
    fail("selectedMapperProtocol still models protocolVersion");
  }
  const required = new Set(evidence?.requiredOperations || []);
  for (const operation of [...HOST_CAPABILITY_OPERATIONS, ...REQUIRED_OBSERVATION_OPERATIONS]) {
    if (!required.has(operation)) fail(`requiredOperations is missing ${operation}`);
  }
  if (JSON.stringify(protocol.virtualExtensions) !== JSON.stringify(VIRTUAL_EXTENSIONS)) {
    fail("selectedMapperProtocol.virtualExtensions differs from VIRTUAL_EXTENSIONS");
  }
  if (JSON.stringify(protocol.manifestFields) !== JSON.stringify(MANIFEST_FIELDS)) {
    fail("selectedMapperProtocol.manifestFields differs from MANIFEST_FIELDS");
  }
  if (protocol.diagnosticDirectives?.shape !== "object") {
    fail("selectedMapperProtocol.diagnosticDirectives.shape must be object");
  }
  const probe = evidence?.hostProbe || {};
  if (!/--help --all/.test(probe.method || "") || !/end-to-end/.test(probe.execution || "")) {
    fail("hostProbe must run tsc --help --all and execute the end-to-end mapper run");
  }
  return errors;
}

export function loadStp5Product(name) {
  return JSON.parse(fs.readFileSync(path.join(STP5_DIR, "products", name), "utf8"));
}

export function validateStp5Products({
  originPolicy = loadStp5Product("diagnostic-origin-policy.json"),
  rolePolicy = loadStp5Product("observation-role-policy.json"),
  capabilityEvidence = loadStp5Product("mapper-capability-evidence.json"),
  evidenceText = fs.readFileSync(path.join(STP5_DIR, "../evidence/STP5/cases.md"), "utf8"),
} = {}) {
  const errors = [...validateMapperCapabilityEvidence(capabilityEvidence)];
  if (originPolicy?.diagnosticDirectives?.unusedExpectReportedInOriginal !== true) {
    errors.push(
      err(
        "STP5-guard-duplicate",
        "unused-expect-policy",
        "DiagnosticOriginPolicy must declare unusedExpectReportedInOriginal",
      ),
    );
  }
  if (
    !rolePolicy?.ac2EditApplicationRationale ||
    String(rolePolicy.ac2EditApplicationRationale).trim().length === 0
  ) {
    errors.push(
      err(
        "STP5-capability",
        "missing-ac2-rationale",
        "ObservationRolePolicy must record AC2 edit-application untouched-owner rationale",
      ),
    );
  }
  if (!/[0-9a-f]{40}/.test(evidenceText)) {
    errors.push(
      err(
        "STP5-capability",
        "missing-source-revision",
        "STP5 evidence must record a 40-character source revision",
      ),
    );
  }
  if (!/6\.0\.3/.test(evidenceText) || !/7\.0\.2/.test(evidenceText)) {
    errors.push(
      err(
        "STP5-capability",
        "missing-engine-pins",
        "STP5 evidence must record ts-js 6.0.3 and ts-native 7.0.2 engine pins",
      ),
    );
  }
  return errors;
}

function engineInputsFrom(input = {}) {
  if (Array.isArray(input.engines) && input.engines.length > 0) return input.engines;
  return [
    {
      mapperHostPresent: input.mapperHostPresent,
      mapperHostEvidence: input.mapperHostEvidence,
      hostRun: input.hostRun,
      observation: input.observation,
      engineId: input.engineId,
      engineVersion: input.engineVersion,
    },
  ];
}

export async function evaluateStp5(input = {}) {
  const errors = [];
  errors.push(...validateStp5Products());
  const encodingErrors = assertEncodingIdentity();
  errors.push(...encodingErrors);
  errors.push(...assertCleanGeometry());
  errors.push(...evaluateRejectTwins());
  const session = createMapperSession({ manifest: { dynamicConfig: true } });
  const init = session.initialize({ positionEncodings: [...POSITION_ENCODINGS], chosen: "utf-16" });
  if (!init.ok) errors.push(init.error);
  else errors.push(...validateInitializeResult(init.result));
  const opened = session.openProject({
    configFileName: path.resolve("/tmp/tsconfig.json"),
    projectHandle: "p1",
    compilerOptions: { strict: true },
  });
  if (!opened.ok) errors.push(opened.error);
  else errors.push(...validateOpenProjectResult(opened.result, { dynamicConfig: true }));
  const transformed = session.transform({
    projectHandle: "p1",
    fileName: "App.vue",
    content: CLEAN_TRANSFORM.original,
  });
  if (!transformed.ok) errors.push(transformed.error);
  else errors.push(...validateTransformResult(transformed.result, CLEAN_TRANSFORM.original));
  const closed = session.closeProject({ projectHandle: "p1" });
  if (!closed.ok) errors.push(closed.error);
  const stale = session.transform({
    projectHandle: "p1",
    fileName: "App.vue",
    content: CLEAN_TRANSFORM.original,
  });
  if (stale.ok) {
    errors.push(
      err(
        "STP5-stale-target",
        "closed-handle-reuse",
        "transform after closeProject must fail closed",
      ),
    );
  }
  const encodingIdentityOk = encodingErrors.length === 0;
  const rows = engineInputsFrom(input).flatMap((engine) =>
    capabilityRowsFromProbe({ ...engine, encodingIdentityOk }),
  );
  errors.push(...evaluateCapabilityRows(rows));
  return { errors, capabilityRows: rows };
}
