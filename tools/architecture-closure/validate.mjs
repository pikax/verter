// The closure decision over a supplied plan export and candidate report. It
// reads no repository plan state, Git history or live service, and changes
// nothing: the answer is a pure function of its two inputs.

import { readFileSync } from "node:fs";

import { readPlanExport } from "./plan-export.mjs";
import { checkClosureInventories, checkStructuralInventories } from "./rules-inventories.mjs";
import { checkClosureOwners, checkOwnerLockOwners } from "./rules-owners.mjs";
import {
  checkCapabilities,
  checkEvidence,
  checkExceptions,
  checkMetrics,
  checkTrainReviews,
} from "./rules-populations.mjs";
import { checkSchema } from "./schema-check.mjs";

const readJson = (url) => JSON.parse(readFileSync(url, "utf8"));

export const REPORT_SCHEMA = readJson(
  new URL("../../schemas/architecture-closure.schema.json", import.meta.url),
);
export const POLICY = readJson(new URL("./closure-policy.json", import.meta.url));
export const MODES = Object.freeze(["owner-lock", "closure"]);

function finish(mode, target, errors, pending, advisories) {
  const order = (a, b) =>
    a.code.localeCompare(b.code) || String(a.subject).localeCompare(String(b.subject));
  return {
    ok: errors.length === 0,
    mode,
    target,
    errors: [...errors].sort(order),
    pending: [...pending].sort((a, b) => a.slug.localeCompare(b.slug)),
    advisories: [...advisories].sort(order),
  };
}

/** Decides closure. `errors[].code` is the stable reason for each rejection. */
export function validateClosure({ mode, plan, report, policy = POLICY, schema = REPORT_SCHEMA }) {
  if (!MODES.includes(mode)) throw new TypeError(`unknown mode ${mode}`);
  const target = mode === "closure" ? policy.closureTarget : policy.ownerLockTarget;
  const errors = [];
  const pending = [];
  const advisories = [];
  const done = () => finish(mode, target, errors, pending, advisories);

  const { problems, graph } = readPlanExport(plan, mode);
  errors.push(...problems);
  for (const violation of checkSchema(schema, report)) {
    errors.push({
      code: "schema-invalid",
      subject: violation.path || "/",
      message: `${violation.keyword}: ${violation.message}`,
    });
  }
  if (errors.length > 0) return done();

  if (report.mode !== mode) {
    errors.push({
      code: "report-mode-mismatch",
      subject: report.mode,
      message: `expected ${mode}`,
    });
  }
  if (report.target !== target) {
    errors.push({
      code: "report-target-mismatch",
      subject: report.target,
      message: `expected ${target}`,
    });
  }
  if (!graph.has(target)) {
    errors.push({ code: "plan-target-missing", subject: target, message: "not in the export" });
    return done();
  }
  const roots = mode === "closure" ? [target] : [target, policy.closureTarget];
  for (const { slug, predecessor } of roots.flatMap((root) => graph.danglingFrom(root))) {
    errors.push({
      code: "plan-dangling-predecessor",
      subject: slug,
      message: `predecessor ${predecessor} is not in the export`,
    });
  }
  if (errors.length > 0) return done();

  const evidenceById = new Map();
  for (const row of report.evidence) {
    if (evidenceById.has(row.id)) {
      errors.push({ code: "evidence-row-duplicate", subject: row.id, message: "two rows, one id" });
    }
    evidenceById.set(row.id, row);
  }
  const ctx = {
    mode,
    target,
    report,
    graph,
    policy,
    evidenceById,
    pending,
    advisories,
    add: (code, subject, message) => errors.push({ code, subject, message }),
  };

  checkStructuralInventories(ctx);
  if (mode === "owner-lock") {
    checkOwnerLockOwners(ctx);
    return done();
  }
  checkClosureOwners(ctx);
  checkTrainReviews(ctx);
  checkCapabilities(ctx);
  checkMetrics(ctx);
  checkEvidence(ctx);
  checkExceptions(ctx);
  checkClosureInventories(ctx);
  return done();
}
