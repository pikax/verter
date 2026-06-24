/**
 * Discriminating validator for the replacement-validation deviation ledger.
 *
 * The ledger's machine source of truth is `docs/arch/followups/
 * replacement-deviations.json` (schema `verter.replacement-deviations.v1`). This
 * test loads the committed schema + sidecar and validates them with a small
 * hand-written conditional validator (no `ajv` dependency — adding one would drift
 * the lockfile). It is DISCRIMINATING: it accepts the empty sidecar, a good
 * `VERTER_BUG`, and a good `REFERENCE_WRONG` carrying all four anti-misclassification
 * fields; and it REJECTS a `REFERENCE_WRONG` missing one of those fields, a
 * `HARNESS_OR_ORACLE_GAP` missing its `subtype`, and an `UNDECIDED` marked final.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { canonicalizePath, joinCanonical } from "../src/paths.js";

/** Workspace root, derived from this test file's location. */
const REPO_ROOT = canonicalizePath(fileURLToPath(new URL("../../..", import.meta.url)));
const LEDGER_DIR = joinCanonical(REPO_ROOT, "docs", "arch", "followups");

const SCHEMA_CONST = "verter.replacement-deviations.v1";
const CLASSES = [
  "VERTER_BUG",
  "REFERENCE_WRONG",
  "HARNESS_OR_ORACLE_GAP",
  "INTENTIONAL_DEVIATION",
  "UNDECIDED",
] as const;
const WORKSTREAMS = ["ide", "tsc", "build"] as const;
const SUBTYPES = ["HARNESS_BUG", "ORACLE_GAP"] as const;
const DISPOSITIONS = ["open", "fixed", "documented", "deferred", "blocked"] as const;
const STATUSES = ["draft", "final"] as const;
/** The four anti-misclassification fields required for any non-VERTER_BUG ruling. */
const ANTI_FIELDS = [
  "independentRepro",
  "sourceOfTruth",
  "reviewerApproval",
  "lockingAssertion",
] as const;
const NON_VERTER_BUG_AFFIRMATIVE = [
  "REFERENCE_WRONG",
  "INTENTIONAL_DEVIATION",
  "HARNESS_OR_ORACLE_GAP",
];

type Json = Record<string, unknown>;

/** A string field is present iff it is a non-empty string. */
function hasString(o: Json, key: string): boolean {
  return typeof o[key] === "string" && (o[key] as string).length > 0;
}

/**
 * Validate one ledger entry against `verter.replacement-deviations.v1`, returning
 * the list of violation codes (empty = valid). Encodes the schema's required
 * fields, enums, and the three conditional rules (HARNESS_OR_ORACLE_GAP→subtype;
 * non-VERTER_BUG→four anti-fields; UNDECIDED→reason+nextAction and not final).
 */
function validateEntry(entry: Json): string[] {
  const errors: string[] = [];

  // Base required fields.
  for (const key of ["id", "workstream", "class", "genericReproFixture", "status", "disposition"]) {
    if (!hasString(entry, key)) errors.push(`missing:${key}`);
  }
  // Enums.
  if (entry.class !== undefined && !CLASSES.includes(entry.class as never))
    errors.push("enum:class");
  if (entry.workstream !== undefined && !WORKSTREAMS.includes(entry.workstream as never))
    errors.push("enum:workstream");
  if (entry.disposition !== undefined && !DISPOSITIONS.includes(entry.disposition as never))
    errors.push("enum:disposition");
  if (entry.status !== undefined && !STATUSES.includes(entry.status as never))
    errors.push("enum:status");
  if (entry.subtype !== undefined && !SUBTYPES.includes(entry.subtype as never))
    errors.push("enum:subtype");

  const cls = entry.class as string;

  // Conditional 1: HARNESS_OR_ORACLE_GAP requires a subtype.
  if (cls === "HARNESS_OR_ORACLE_GAP" && !hasString(entry, "subtype")) {
    errors.push("missing:subtype");
  }
  // Conditional 2: any affirmative non-VERTER_BUG ruling requires the four anti-fields.
  if (NON_VERTER_BUG_AFFIRMATIVE.includes(cls)) {
    for (const f of ANTI_FIELDS) {
      if (!hasString(entry, f)) errors.push(`missing:${f}`);
    }
  }
  // Conditional 3: UNDECIDED requires reason + nextAction and must NOT be final/approved.
  if (cls === "UNDECIDED") {
    if (!hasString(entry, "undecidedReason")) errors.push("missing:undecidedReason");
    if (!hasString(entry, "nextAction")) errors.push("missing:nextAction");
    if (entry.status === "final") errors.push("undecided:final-forbidden");
    if (entry.disposition === "fixed" || entry.disposition === "documented")
      errors.push("undecided:disposition-forbidden");
  }

  return errors;
}

/** Validate a whole ledger document. */
function validateLedger(doc: Json): string[] {
  const errors: string[] = [];
  if (doc.schema !== SCHEMA_CONST) errors.push("schema:const");
  if (!Array.isArray(doc.entries)) {
    errors.push("entries:type");
    return errors;
  }
  (doc.entries as Json[]).forEach((e, i) => {
    for (const code of validateEntry(e)) errors.push(`entries[${i}]:${code}`);
  });
  return errors;
}

/** A complete, valid VERTER_BUG row. */
function goodVerterBug(): Json {
  return {
    id: "dev-0001",
    workstream: "tsc",
    class: "VERTER_BUG",
    genericReproFixture: "fixtures/generic/options-api-remap.vue",
    reference: "vue-tsc 3.2.x",
    ownerCrate: "verter_compiler",
    regressionTest: "options_api_diagnostic_remaps_to_vue",
    disposition: "open",
    status: "draft",
  };
}

/** A complete, valid REFERENCE_WRONG row with all four anti-fields. */
function goodReferenceWrong(): Json {
  return {
    id: "dev-0002",
    workstream: "ide",
    class: "REFERENCE_WRONG",
    genericReproFixture: "fixtures/generic/inherit-attrs-fallthrough.vue",
    reference: "Volar",
    oracleRuling: "Verter exposes the recursive native-root attrs; the reference does not.",
    independentRepro: "fixtures/generic/inherit-attrs-fallthrough.spec.ts",
    sourceOfTruth: "Vue runtime fallthrough semantics (inheritAttrs:false)",
    reviewerApproval: "approved by reviewer in dual review",
    lockingAssertion: "fallthrough_exposes_recursive_native_root_attrs",
    disposition: "documented",
    status: "final",
  };
}

describe("deviation ledger schema", () => {
  it("the committed schema declares verter.replacement-deviations.v1", () => {
    const schema = JSON.parse(
      readFileSync(joinCanonical(LEDGER_DIR, "replacement-deviations.schema.json"), "utf-8"),
    ) as Json;
    const props = schema.properties as Json;
    expect((props.schema as Json).const).toBe(SCHEMA_CONST);
  });

  it("accepts the committed empty sidecar", () => {
    const sidecar = JSON.parse(
      readFileSync(joinCanonical(LEDGER_DIR, "replacement-deviations.json"), "utf-8"),
    ) as Json;
    expect(sidecar.schema).toBe(SCHEMA_CONST);
    expect(sidecar.entries).toEqual([]);
    expect(validateLedger(sidecar)).toEqual([]);
  });

  it("accepts a good VERTER_BUG row", () => {
    expect(validateEntry(goodVerterBug())).toEqual([]);
  });

  it("accepts a good REFERENCE_WRONG row carrying all four anti-fields", () => {
    expect(validateEntry(goodReferenceWrong())).toEqual([]);
  });

  it("rejects a REFERENCE_WRONG row missing an anti-misclassification field", () => {
    const bad = goodReferenceWrong();
    delete bad.sourceOfTruth;
    const errors = validateEntry(bad);
    expect(errors).toContain("missing:sourceOfTruth");
    // The other three anti-fields still validate (only the deleted one fails).
    expect(errors).not.toContain("missing:independentRepro");
  });

  it("rejects a HARNESS_OR_ORACLE_GAP row missing its subtype", () => {
    const bad: Json = {
      id: "dev-0003",
      workstream: "build",
      class: "HARNESS_OR_ORACLE_GAP",
      genericReproFixture: "fixtures/generic/stale-golden.vue",
      // four anti-fields present, but subtype is absent
      independentRepro: "x",
      sourceOfTruth: "y",
      reviewerApproval: "z",
      lockingAssertion: "w",
      disposition: "open",
      status: "draft",
    };
    expect(validateEntry(bad)).toContain("missing:subtype");
    // A valid subtype clears it.
    expect(validateEntry({ ...bad, subtype: "ORACLE_GAP" })).toEqual([]);
  });

  it("rejects an UNDECIDED row marked final/approved", () => {
    const base: Json = {
      id: "dev-0004",
      workstream: "ide",
      class: "UNDECIDED",
      genericReproFixture: "fixtures/generic/ambiguous-generic.vue",
      undecidedReason: "the spec is ambiguous on generic prop instantiation",
      nextAction: "escalate to the architect with the TS spec reference",
      disposition: "open",
      status: "final",
    };
    expect(validateEntry(base)).toContain("undecided:final-forbidden");
    // Draft status with open disposition validates.
    expect(validateEntry({ ...base, status: "draft" })).toEqual([]);
  });

  it("rejects an UNDECIDED row missing its reason or next action", () => {
    const bad: Json = {
      id: "dev-0005",
      workstream: "tsc",
      class: "UNDECIDED",
      genericReproFixture: "fixtures/generic/x.vue",
      disposition: "open",
      status: "draft",
    };
    const errors = validateEntry(bad);
    expect(errors).toContain("missing:undecidedReason");
    expect(errors).toContain("missing:nextAction");
  });
});
