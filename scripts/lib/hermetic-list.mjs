// The versioned hermetic candidate list (`scripts/manifests/ci-hermetic-candidates.json`).
//
// The perturbation workflow proposes the candidates: tests that passed, unchanged,
// in every perturbation rerun and that no exclusion names. The list stays
// unapproved until the operator approves it, and only an approved list makes a
// test hermetic. Exclusions name, with a reason, tests that stay non-hermetic
// whatever the reruns show: tests reading the clock for timing budgets and tests
// depending on build-time values.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const HERMETIC_LIST_SCHEMA_VERSION = 2;

export const HERMETIC_LIST_PATH = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "manifests",
  "ci-hermetic-candidates.json",
);

const nonEmpty = (v) => typeof v === "string" && v.trim() !== "";
const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;

function compile(pattern) {
  try {
    return new RegExp(pattern, "u");
  } catch {
    return null;
  }
}

/** Every problem with the list document; empty when it is well formed. */
export function validateHermeticList(doc) {
  const problems = [];
  if (doc?.schemaVersion !== HERMETIC_LIST_SCHEMA_VERSION) {
    problems.push(`schemaVersion must be ${HERMETIC_LIST_SCHEMA_VERSION}`);
  }
  if (!Number.isInteger(doc?.version) || doc.version < 1) {
    problems.push("version must be a positive integer");
  }
  if (typeof doc?.approved !== "boolean") {
    problems.push("approved must be a boolean");
  } else if (doc.approved) {
    if (!nonEmpty(doc.approvedBy)) problems.push("an approved list names approvedBy");
    if (!ISO_DATE.test(doc.approvedOn ?? "")) {
      problems.push("an approved list records approvedOn as YYYY-MM-DD");
    }
  } else if (doc.approvedBy !== null || doc.approvedOn !== null) {
    problems.push("an unapproved list has approvedBy and approvedOn null");
  }

  const exclusions = Array.isArray(doc?.exclusions) ? doc.exclusions : null;
  if (!exclusions) problems.push("exclusions must be an array");
  const matchers = [];
  const patterns = [];
  for (const [i, entry] of (exclusions ?? []).entries()) {
    if (!nonEmpty(entry?.reason)) problems.push(`exclusions[${i}] states no reason`);
    if (!Array.isArray(entry?.patterns) || entry.patterns.length === 0) {
      problems.push(`exclusions[${i}] names no pattern`);
      continue;
    }
    for (const [j, pattern] of entry.patterns.entries()) {
      const regex = nonEmpty(pattern) ? compile(pattern) : null;
      if (!regex)
        problems.push(`exclusions[${i}].patterns[${j}] must be a valid regular expression`);
      else matchers.push(regex);
      patterns.push(pattern);
    }
  }
  if (new Set(patterns).size !== patterns.length) problems.push("exclusion patterns repeat");

  const candidates = Array.isArray(doc?.candidates) ? doc.candidates : null;
  if (!candidates || !candidates.every((id) => nonEmpty(id) && id.includes(" "))) {
    problems.push("candidates must be an array of `<binary id> <test name>` ids");
  } else {
    for (let i = 1; i < candidates.length; i += 1) {
      if (candidates[i - 1] >= candidates[i]) {
        problems.push("candidates must be sorted and unique");
        break;
      }
    }
    for (const id of candidates) {
      if (matchers.some((m) => m.test(id))) problems.push(`candidate ${id} is excluded`);
    }
    if (candidates.length > 0) {
      const from = doc.derivedFrom;
      if (!nonEmpty(from?.run) || !Number.isInteger(from?.reruns) || from.reruns < 2) {
        problems.push("candidates need derivedFrom {run, reruns >= 2} naming the perturbation run");
      }
    }
  }
  return problems;
}

/** Every exclusion pattern of the list. */
export function exclusionPatterns(doc) {
  return doc.exclusions.flatMap((entry) => entry.patterns);
}

/** The reason an exclusion gives for `id`, or null when none names it. */
export function exclusionReason(doc, id) {
  for (const entry of doc.exclusions) {
    if (entry.patterns.some((p) => new RegExp(p, "u").test(id))) return entry.reason;
  }
  return null;
}

/** Reads and validates the list; throws on a malformed document. */
export function readHermeticListDocument(path = HERMETIC_LIST_PATH) {
  const doc = JSON.parse(readFileSync(path, "utf8"));
  const problems = validateHermeticList(doc);
  if (problems.length > 0) throw new Error(`${path}: ${problems.join("; ")}`);
  return doc;
}

/** Test ids the list makes hermetic: none unless the operator approved it. */
export function hermeticTestIds(doc) {
  if (!doc.approved) return new Set();
  return new Set(doc.candidates.filter((id) => exclusionReason(doc, id) === null));
}
