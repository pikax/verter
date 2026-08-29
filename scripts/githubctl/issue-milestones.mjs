import path from "node:path";

import { loadAuthority, readToml } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { IssueSyncError } from "./errors.mjs";

const CATALOG_FILE = "github-milestones.toml";

function requiredText(value, name) {
  if (typeof value !== "string" || value.length === 0) {
    throw new IssueSyncError(`${name} must be a non-empty string`);
  }
  return value;
}

function validateCatalog(catalog, file) {
  if (catalog?.schema !== 1) throw new IssueSyncError(`${file}: expected schema 1`);
  if (!Array.isArray(catalog.milestone) || catalog.milestone.length === 0) {
    throw new IssueSyncError(`${file}: milestone must be a non-empty array`);
  }
  const byTitle = new Map();
  for (const [index, row] of catalog.milestone.entries()) {
    const title = requiredText(row?.title, `milestone[${index}].title`);
    const description = requiredText(row?.description, `milestone[${index}].description`);
    if (byTitle.has(title)) throw new IssueSyncError(`${file}: duplicate milestone ${title}`);
    byTitle.set(title, Object.freeze({ title, description }));
  }
  return Object.freeze({
    schema: 1,
    file,
    milestones: Object.freeze([...byTitle.values()]),
    byTitle,
  });
}

export function loadIssueMilestoneCatalog(packageRoot = loadAuthority().packageRoot) {
  const file = path.join(packageRoot, "catalogs", CATALOG_FILE);
  return validateCatalog(readToml(file), file);
}

export function milestoneForNode(node, catalog) {
  if (node.gh_milestone == null) return null;
  const milestone =
    catalog.byTitle?.get(node.gh_milestone) ??
    catalog.milestones.find((row) => row.title === node.gh_milestone);
  if (!milestone) {
    throw new IssueSyncError(`${node.id}: unknown gh_milestone ${node.gh_milestone}`);
  }
  return milestone;
}

export function planRepositoryMilestones(current, catalog) {
  const currentByTitle = new Map(current.map((row) => [row.title, row]));
  const missing = [];
  const drift = [];
  const currentTitles = [];
  for (const desired of catalog.milestones) {
    const existing = currentByTitle.get(desired.title);
    if (!existing) missing.push(desired);
    else if ((existing.description ?? "") !== desired.description) {
      drift.push({ existing, desired });
    } else currentTitles.push(desired.title);
  }
  return { missing, drift, current: currentTitles };
}
