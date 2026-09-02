import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { assertApplyClearance, assertRepository } from "./adapter.mjs";
import {
  DoctorRequiredError,
  GitHubAdapterError,
  UnstructuredGitHubOutputError,
} from "./errors.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_PROTECTION_FILE = path.join(HERE, "protection-expected.json");
const REQUIRED_CHECK_CONTEXT = "CI Required";
const FINDING_KINDS = new Set([
  "missing-ruleset",
  "wrong-enforcement",
  "missing-rule",
  "wrong-parameter",
  "extra-blocking-rule",
  "repo-setting",
]);

function resolveProtectionFile(file) {
  if (file == null || file === "") return DEFAULT_PROTECTION_FILE;
  return path.isAbsolute(file) ? file : path.join(HERE, file);
}

function requiredObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new GitHubAdapterError(`${label} must be an object`);
  }
  return value;
}

function requiredText(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    throw new GitHubAdapterError(`${label} is required`);
  }
  return value;
}

function statusCheckContext(row) {
  if (typeof row === "string") return row;
  if (row !== null && typeof row === "object" && typeof row.context === "string") {
    return row.context;
  }
  return null;
}

function hasRequiredCheckContext(rules) {
  for (const rule of rules) {
    if (rule?.type !== "required_status_checks") continue;
    const checks = rule.parameters?.required_status_checks;
    if (!Array.isArray(checks)) continue;
    if (checks.some((row) => statusCheckContext(row) === REQUIRED_CHECK_CONTEXT)) return true;
  }
  return false;
}

function validateExpectedProtection(parsed) {
  const expected = requiredObject(parsed, "expected protection");
  const ruleset = requiredObject(expected.ruleset, "expected ruleset");
  requiredText(ruleset.name, "expected ruleset name");
  if (ruleset.target !== "branch") {
    throw new GitHubAdapterError("expected ruleset target must be branch");
  }
  if (!Array.isArray(ruleset.rules) || ruleset.rules.length === 0) {
    throw new GitHubAdapterError("expected ruleset rules must be a non-empty array");
  }
  if (!hasRequiredCheckContext(ruleset.rules)) {
    throw new GitHubAdapterError(
      `expected ruleset must require status check ${REQUIRED_CHECK_CONTEXT}`,
    );
  }
  requiredObject(expected.repositorySettings, "expected repository settings");
  return expected;
}

export function loadExpectedProtection(file) {
  const resolved = resolveProtectionFile(file);
  let text;
  try {
    text = fs.readFileSync(resolved, "utf8");
  } catch (error) {
    throw new GitHubAdapterError(`expected protection file is missing: ${resolved}`, {
      cause: error,
    });
  }
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new UnstructuredGitHubOutputError("expected protection file is not JSON", {
      cause: error,
    });
  }
  return validateExpectedProtection(parsed);
}

function finding(kind, path, expected, actual, action) {
  if (!FINDING_KINDS.has(kind)) {
    throw new GitHubAdapterError(`unknown protection finding kind ${kind}`);
  }
  return { kind, path, expected, actual, action };
}

function sameDeclared(expected, actual) {
  if (Object.is(expected, actual)) return true;
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual) || expected.length !== actual.length) return false;
    return expected.every((row, index) => sameDeclared(row, actual[index]));
  }
  if (expected !== null && typeof expected === "object") {
    if (actual === null || typeof actual !== "object" || Array.isArray(actual)) return false;
    return Object.keys(expected).every((key) => sameDeclared(expected[key], actual[key]));
  }
  return false;
}

function statusCheckContexts(checks) {
  const contexts = new Set();
  for (const row of Array.isArray(checks) ? checks : []) {
    const context = statusCheckContext(row);
    if (typeof context === "string" && context.length > 0) contexts.add(context);
  }
  return contexts;
}

function sameStatusCheckContexts(expectedChecks, actualChecks) {
  const expected = statusCheckContexts(expectedChecks);
  const actual = statusCheckContexts(actualChecks);
  if (expected.size !== actual.size) return false;
  for (const context of expected) {
    if (!actual.has(context)) return false;
  }
  return true;
}

function sameRuleParameters(expectedRule, actualRule) {
  const expectedParams = expectedRule.parameters;
  if (expectedParams == null) return true;
  const actualParams = actualRule.parameters;
  if (actualParams === null || typeof actualParams !== "object" || Array.isArray(actualParams)) {
    return false;
  }
  for (const [key, value] of Object.entries(expectedParams)) {
    if (key === "required_status_checks") {
      if (!sameStatusCheckContexts(value, actualParams[key])) return false;
      continue;
    }
    if (!sameDeclared(value, actualParams[key])) return false;
  }
  return true;
}

function rulesOf(ruleset) {
  return Array.isArray(ruleset?.rules) ? ruleset.rules : [];
}

function namedRuleset(rulesets, name) {
  if (!Array.isArray(rulesets)) return null;
  return (
    rulesets.find((row) => row !== null && typeof row === "object" && row.name === name) ?? null
  );
}

export function diffProtection(expected, actual) {
  const ruleset = requiredObject(expected?.ruleset, "expected ruleset");
  const repositorySettings = requiredObject(
    expected?.repositorySettings,
    "expected repository settings",
  );
  const actualState =
    actual === null || typeof actual !== "object" || Array.isArray(actual) ? {} : actual;
  const findings = [];
  const matched = namedRuleset(actualState.rulesets, ruleset.name);
  if (matched == null) {
    findings.push(finding("missing-ruleset", "ruleset.name", ruleset.name, null, "create"));
  } else {
    if (matched.enforcement !== ruleset.enforcement) {
      findings.push(
        finding(
          "wrong-enforcement",
          "ruleset.enforcement",
          ruleset.enforcement,
          matched.enforcement ?? null,
          "update",
        ),
      );
    }
    if (ruleset.target != null && matched.target !== ruleset.target) {
      findings.push(
        finding(
          "wrong-parameter",
          "ruleset.target",
          ruleset.target,
          matched.target ?? null,
          "update",
        ),
      );
    }
    if (ruleset.conditions != null && !sameDeclared(ruleset.conditions, matched.conditions)) {
      findings.push(
        finding(
          "wrong-parameter",
          "ruleset.conditions",
          ruleset.conditions,
          matched.conditions ?? null,
          "update",
        ),
      );
    }
    const actualRules = rulesOf(matched);
    const actualByType = new Map();
    for (const rule of actualRules) {
      if (rule === null || typeof rule !== "object" || typeof rule.type !== "string") continue;
      if (!actualByType.has(rule.type)) actualByType.set(rule.type, rule);
    }
    for (const expectedRule of ruleset.rules) {
      const type = expectedRule?.type;
      if (typeof type !== "string" || type.length === 0) continue;
      const actualRule = actualByType.get(type);
      if (actualRule == null) {
        findings.push(
          finding("missing-rule", `ruleset.rules.${type}`, expectedRule, null, "update"),
        );
        continue;
      }
      if (!sameRuleParameters(expectedRule, actualRule)) {
        findings.push(
          finding(
            "wrong-parameter",
            `ruleset.rules.${type}.parameters`,
            expectedRule.parameters ?? null,
            actualRule.parameters ?? null,
            "update",
          ),
        );
      }
    }
    for (const actualRule of actualRules) {
      const type = actualRule?.type;
      if (typeof type !== "string" || type.length === 0) continue;
      if (ruleset.rules.some((row) => row?.type === type)) continue;
      findings.push(
        finding("extra-blocking-rule", `ruleset.rules.${type}`, null, actualRule, "report"),
      );
    }
  }

  const repository =
    actualState.repository === null || typeof actualState.repository !== "object"
      ? {}
      : actualState.repository;
  for (const [key, value] of Object.entries(repositorySettings)) {
    if (repository[key] !== value) {
      findings.push(
        finding("repo-setting", `repository.${key}`, value, repository[key] ?? null, "patch"),
      );
    }
  }
  return { ok: findings.length === 0, findings };
}

function assertAdapterMethod(adapter, name) {
  if (typeof adapter[name] !== "function") {
    throw new GitHubAdapterError(`adapter.${name} is required`);
  }
}

function fetchActual(adapter) {
  assertAdapterMethod(adapter, "listRulesets");
  assertAdapterMethod(adapter, "getRuleset");
  assertAdapterMethod(adapter, "getRepositorySettings");
  const listed = adapter.listRulesets();
  if (!Array.isArray(listed)) {
    throw new UnstructuredGitHubOutputError("GitHub ruleset list is not a JSON array");
  }
  const rulesets = listed.map((row) => adapter.getRuleset(row.id));
  const repository = adapter.getRepositorySettings();
  return { rulesets, repository };
}

function extraActualRules(expectedRuleset, actualRuleset) {
  return rulesOf(actualRuleset).filter(
    (rule) =>
      typeof rule?.type === "string" &&
      !expectedRuleset.rules.some((row) => row?.type === rule.type),
  );
}

function rulesetWritePayload(expectedRuleset, actualRuleset) {
  const extra = extraActualRules(expectedRuleset, actualRuleset);
  return {
    name: expectedRuleset.name,
    target: expectedRuleset.target,
    enforcement: expectedRuleset.enforcement,
    conditions: expectedRuleset.conditions,
    rules: extra.length === 0 ? [...expectedRuleset.rules] : [...expectedRuleset.rules, ...extra],
  };
}

export function protectionCheck(options = {}) {
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  assertRepository(options.adapter, options);
  const expected = options.expected ?? loadExpectedProtection(options.file);
  const { ok, findings } = diffProtection(expected, fetchActual(options.adapter));
  return { mode: "check", ok, findings };
}

export function protectionApply(options = {}) {
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  assertRepository(options.adapter, options);
  if (!options.clearance) {
    throw new DoctorRequiredError("apply requires GitHubDoctor admin clearance");
  }
  assertApplyClearance("apply", options.clearance, "admin", options.adapter);
  assertAdapterMethod(options.adapter, "createRuleset");
  assertAdapterMethod(options.adapter, "updateRuleset");
  assertAdapterMethod(options.adapter, "updateRepositorySettings");
  const expected = options.expected ?? loadExpectedProtection(options.file);
  const actual = fetchActual(options.adapter);
  const { findings } = diffProtection(expected, actual);
  const applied = [];
  const matched = namedRuleset(actual.rulesets, expected.ruleset.name);
  const missing = findings.some((row) => row.kind === "missing-ruleset");
  const rulesetDrift = findings.some(
    (row) =>
      row.kind === "wrong-enforcement" ||
      row.kind === "missing-rule" ||
      row.kind === "wrong-parameter",
  );
  if (missing) {
    const created = options.adapter.createRuleset(rulesetWritePayload(expected.ruleset, null));
    applied.push({
      kind: "create-ruleset",
      id: created.id,
      name: expected.ruleset.name,
    });
  } else if (rulesetDrift) {
    const updated = options.adapter.updateRuleset(
      matched.id,
      rulesetWritePayload(expected.ruleset, matched),
    );
    applied.push({
      kind: "update-ruleset",
      id: updated.id ?? matched.id,
      name: expected.ruleset.name,
    });
  }

  const patch = {};
  for (const row of findings) {
    if (row.kind !== "repo-setting") continue;
    const key = row.path.slice("repository.".length);
    patch[key] = row.expected;
  }
  if (Object.keys(patch).length > 0) {
    options.adapter.updateRepositorySettings(patch);
    applied.push({ kind: "update-repository-settings", patch });
  }

  const remaining = findings.filter((row) => row.kind === "extra-blocking-rule");
  return {
    mode: "apply",
    ok: remaining.length === 0,
    findings,
    applied,
  };
}
