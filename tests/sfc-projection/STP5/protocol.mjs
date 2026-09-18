/**
 * STP5 mapper protocol proof helpers.
 *
 * Qualifies the TypeScript content-mapper ABI (initialize / openProject /
 * transform / closeProject, encoding, span kinds, diagnostic directives,
 * observation roles). Does not implement a production Vue mapper.
 */

export const PROTOCOL_VERSION = 1;

export const REQUIRED_HOST_OPERATIONS = Object.freeze([
  "initialize",
  "openProject",
  "transform",
  "closeProject",
]);

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
        features: features ?? (SpanMapFeature.CodeLens << 1) - 1,
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
        features: features ?? (SpanMapFeature.CodeLens << 1) - 1,
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

export function validateDiagnosticDirectives(directives, originalText) {
  const errors = [];
  const seen = new Map();
  for (const directive of directives || []) {
    const [originalStart, originalLength, virtualStart, virtualEnd, policy] = directive;
    if (
      originalLength <= 0 ||
      originalStart < 0 ||
      originalStart + originalLength > originalText.length
    ) {
      errors.push(
        err(
          "STP5-guard-duplicate",
          "fabricated-blame",
          `directive original span [${originalStart}, ${originalLength}) is not in the source`,
        ),
      );
      continue;
    }
    if (virtualEnd <= virtualStart) {
      errors.push(err("STP5-guard-duplicate", "hidden-blame", "directive virtual range is empty"));
    }
    if (
      policy !== DiagnosticDirectivePolicy.Ignore &&
      policy !== DiagnosticDirectivePolicy.Expect
    ) {
      errors.push(
        err("STP5-guard-duplicate", "hidden-blame", `unknown directive policy ${policy}`),
      );
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
    seen.set(key, true);
  }
  const ranges = (directives || []).map((row) => ({
    start: row[2],
    end: row[3],
    originalStart: row[0],
    originalLength: row[1],
  }));
  for (let i = 0; i < ranges.length; i += 1) {
    for (let j = i + 1; j < ranges.length; j += 1) {
      const a = ranges[i];
      const b = ranges[j];
      const overlap = a.start < b.end && b.start < a.end;
      if (overlap) {
        errors.push(
          err(
            "STP5-guard-duplicate",
            "duplicate-guard",
            "overlapping replayed guards hide or fabricate diagnostic blame",
          ),
        );
      }
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

export function createMapperSession() {
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
        result: {
          protocolVersion: PROTOCOL_VERSION,
          positionEncoding: encoding,
          diagnosticSource: "vue-mapper-proof",
        },
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
      projects.set(params.projectHandle, {
        configFileName: params.configFileName,
        options: params.options || {},
        compilerOptions: params.compilerOptions || {},
        watchedFiles: params.dynamicConfig ? params.watchedFiles || [] : [],
      });
      return {
        ok: true,
        result: { configIdentity: params.dynamicConfig ? params.configIdentity : undefined },
      };
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
      return { ok: true, result: { ...CLEAN_TRANSFORM, projectHandle: params.projectHandle } };
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

export const CLEAN_GUARD = Object.freeze({
  original: '<!-- @vue-expect-error -->\n<div :id="first"></div>',
  directives: [[0, 24, 0, 40, DiagnosticDirectivePolicy.Expect]],
});

export const DIRTY_GUARD_DUPLICATE = Object.freeze({
  original: CLEAN_GUARD.original,
  directives: [
    [0, 24, 0, 40, DiagnosticDirectivePolicy.Expect],
    [0, 24, 0, 40, DiagnosticDirectivePolicy.Expect],
  ],
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
  return errors;
}

export function helpTextHasMapperHost(helpText) {
  const text = String(helpText || "");
  return (
    /runExternalCode/i.test(text) || /contentMappers/i.test(text) || /content-mapper/i.test(text)
  );
}

export function capabilityRowsFromProbe({
  mapperHostPresent,
  observation,
  engineId,
  engineVersion,
}) {
  const hostEvidence = mapperHostPresent
    ? { status: "supported", evidence: "executable-selected-build" }
    : {
        status: "blocking-upstream-defect",
        evidence: `${engineId}@${engineVersion} tsc help/API has no content mapper host (--runExternalCode / contentMappers)`,
      };
  const rows = REQUIRED_HOST_OPERATIONS.map((operation) => ({
    operation,
    engineId,
    ...hostEvidence,
  }));
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
  rows.push({
    operation: "encoding-negotiation",
    engineId,
    status: "supported",
    evidence: "executable-selected-build",
  });
  return rows;
}

export function evaluateCapabilityRows(rows) {
  const errors = [];
  const seen = new Set();
  for (const row of rows) {
    seen.add(row.operation);
    errors.push(...validateCapabilityClaim(row));
  }
  for (const operation of [
    ...REQUIRED_HOST_OPERATIONS,
    ...REQUIRED_OBSERVATION_OPERATIONS,
    "encoding-negotiation",
  ]) {
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

export async function evaluateStp5({ mapperHostPresent, observation, engineId, engineVersion }) {
  const errors = [];
  errors.push(...assertEncodingIdentity());
  errors.push(...assertCleanGeometry());
  errors.push(...evaluateRejectTwins());
  const session = createMapperSession();
  const init = session.initialize({ positionEncodings: [...POSITION_ENCODINGS], chosen: "utf-16" });
  if (!init.ok) errors.push(init.error);
  const opened = session.openProject({
    configFileName: "/tmp/tsconfig.json",
    projectHandle: "p1",
    compilerOptions: { strict: true },
  });
  if (!opened.ok) errors.push(opened.error);
  const transformed = session.transform({
    projectHandle: "p1",
    fileName: "App.vue",
    content: CLEAN_TRANSFORM.original,
  });
  if (!transformed.ok) errors.push(transformed.error);
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
  const rows = capabilityRowsFromProbe({ mapperHostPresent, observation, engineId, engineVersion });
  errors.push(...evaluateCapabilityRows(rows));
  return { errors, capabilityRows: rows };
}
