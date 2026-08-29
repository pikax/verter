import path from "node:path";

import { loadAuthority, readToml } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { IssueSyncError } from "./errors.mjs";

const CATALOG_FILE = "github-issue-labels.toml";

function requiredArray(value, name) {
  if (!Array.isArray(value)) throw new IssueSyncError(`${name} must be an array`);
  return value;
}

function requiredText(value, name) {
  if (typeof value !== "string" || value.length === 0) {
    throw new IssueSyncError(`${name} must be a non-empty string`);
  }
  return value;
}

function normalized(name) {
  return name.toLocaleLowerCase("en-US");
}

function validateRuleRows(catalog, key, selector, prefix, labelByName, options = {}) {
  const seen = new Set();
  for (const [index, row] of requiredArray(catalog[key], key).entries()) {
    const label = requiredText(row?.label, `${key}[${index}].label`);
    if (!label.startsWith(prefix) || !labelByName.has(normalized(label))) {
      throw new IssueSyncError(`${key}[${index}] references unknown ${prefix} label ${label}`);
    }
    for (const value of requiredArray(row?.[selector], `${key}[${index}].${selector}`)) {
      requiredText(value, `${key}[${index}].${selector}`);
      const identity = options.multiple === true ? `${normalized(label)}\0${value}` : value;
      if (seen.has(identity)) {
        throw new IssueSyncError(`${key} classifies ${value} more than once`);
      }
      seen.add(identity);
    }
  }
}

function freezeRules(rows, selector) {
  return Object.freeze(
    rows.map((row) =>
      Object.freeze({ label: row.label, [selector]: Object.freeze([...row[selector]]) }),
    ),
  );
}

function validateCatalog(catalog, file) {
  if (catalog?.schema !== 1) throw new IssueSyncError(`${file}: expected schema 1`);
  const managedPrefixes = requiredArray(catalog.managed_prefixes, "managed_prefixes");
  const managedExact = requiredArray(catalog.managed_exact, "managed_exact");
  for (const [index, prefix] of managedPrefixes.entries()) {
    requiredText(prefix, `managed_prefixes[${index}]`);
  }
  for (const [index, name] of managedExact.entries()) {
    requiredText(name, `managed_exact[${index}]`);
  }

  const labels = requiredArray(catalog.label, "label");
  const labelByName = new Map();
  for (const [index, label] of labels.entries()) {
    const name = requiredText(label?.name, `label[${index}].name`);
    const key = normalized(name);
    if (labelByName.has(key)) throw new IssueSyncError(`duplicate label ${name}`);
    const color = requiredText(label?.color, `label[${index}].color`).toLowerCase();
    if (!/^[0-9a-f]{6}$/u.test(color)) {
      throw new IssueSyncError(`label ${name} color must be six hexadecimal characters`);
    }
    const description = requiredText(label?.description, `label[${index}].description`);
    if (description.length > 100) {
      throw new IssueSyncError(`label ${name} description exceeds 100 characters`);
    }
    labelByName.set(key, Object.freeze({ name, color, description }));
  }

  validateRuleRows(catalog, "area_rule", "trains", "area:", labelByName);
  validateRuleRows(catalog, "problem_train_rule", "trains", "problem:", labelByName);
  validateRuleRows(catalog, "problem_kind_rule", "kinds", "problem:", labelByName);
  validateRuleRows(catalog, "framework_rule", "trains", "framework:", labelByName, {
    multiple: true,
  });
  if (!labelByName.has("origin:ai")) {
    throw new IssueSyncError(`${file}: origin:ai is required`);
  }

  return Object.freeze({
    schema: 1,
    file,
    managedPrefixes: Object.freeze([...managedPrefixes]),
    managedExact: Object.freeze([...managedExact]),
    labels: Object.freeze([...labelByName.values()]),
    areaRules: freezeRules(catalog.area_rule, "trains"),
    problemTrainRules: freezeRules(catalog.problem_train_rule, "trains"),
    problemKindRules: freezeRules(catalog.problem_kind_rule, "kinds"),
    frameworkRules: freezeRules(catalog.framework_rule, "trains"),
  });
}

export function loadIssueLabelCatalog(packageRoot = loadAuthority().packageRoot) {
  const file = path.join(packageRoot, "catalogs", CATALOG_FILE);
  return validateCatalog(readToml(file), file);
}

function matchingRuleLabels(rules, selector, value) {
  return rules.filter((row) => row[selector].includes(value)).map((row) => row.label);
}

function exactlyOne(labels, description) {
  if (labels.length !== 1) {
    throw new IssueSyncError(`${description} must resolve to exactly one label`);
  }
  return labels[0];
}

export function labelsForNode(node, catalog) {
  const train = requiredText(node?.train, "work item train");
  const kind = requiredText(node?.kind, "work item kind");
  const area = exactlyOne(
    matchingRuleLabels(catalog.areaRules, "trains", train),
    `area classification for ${train}`,
  );
  const trainProblems = matchingRuleLabels(catalog.problemTrainRules, "trains", train);
  const problem = trainProblems.length
    ? exactlyOne(trainProblems, `problem classification for ${train}`)
    : exactlyOne(
        matchingRuleLabels(catalog.problemKindRules, "kinds", kind),
        `problem classification for kind ${kind}`,
      );
  const frameworks = matchingRuleLabels(catalog.frameworkRules, "trains", train);
  return [area, problem, ...frameworks, "origin:ai"];
}

export function isManagedIssueLabel(name, catalog) {
  const candidate = normalized(requiredText(name, "issue label"));
  return (
    catalog.managedExact.some((value) => normalized(value) === candidate) ||
    catalog.managedPrefixes.some((prefix) => candidate.startsWith(normalized(prefix)))
  );
}

export function planIssueLabels(current, desired, catalog) {
  const currentByName = new Map(current.map((name) => [normalized(name), name]));
  const desiredByName = new Map(desired.map((name) => [normalized(name), name]));
  const add = desired.filter((name) => !currentByName.has(normalized(name)));
  const remove = current
    .filter((name) => isManagedIssueLabel(name, catalog) && !desiredByName.has(normalized(name)))
    .sort((left, right) => left.localeCompare(right));
  return { add, remove, changed: add.length > 0 || remove.length > 0 };
}

export function planRepositoryLabels(current, catalog) {
  const currentByName = new Map(current.map((label) => [normalized(label.name), label]));
  const missing = [];
  const drift = [];
  const currentNames = [];
  for (const desired of catalog.labels) {
    const existing = currentByName.get(normalized(desired.name));
    if (!existing) {
      missing.push(desired);
    } else if (
      existing.name !== desired.name ||
      existing.color.toLowerCase() !== desired.color ||
      (existing.description ?? "") !== desired.description
    ) {
      drift.push({ existing: existing.name, desired });
    } else {
      currentNames.push(desired.name);
    }
  }
  return { missing, drift, current: currentNames };
}
