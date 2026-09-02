#!/usr/bin/env node

/**
 * check-ci-aggregate.mjs — static completeness check for the CI aggregator job.
 *
 * Parses `.github/workflows/ci.yml` (no YAML library) and asserts:
 *   - every workflow job except the aggregator itself is listed in its `needs`
 *   - every `needs` entry names a real job
 *   - the aggregator runs with `if: always()`
 *   - the aggregator display name is exactly `CI Required`
 *
 * Usage:
 *   node scripts/check-ci-aggregate.mjs
 *
 * The workflow path is resolved from this script's location, not cwd.
 */

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const CI_YML = resolve(REPO_ROOT, ".github", "workflows", "ci.yml");

const JOB_ID_RE = /^  ([A-Za-z0-9_-]+):\s*(#.*)?$/;
const TOP_KEY_RE = /^[A-Za-z0-9_-]+:/;
const JOBS_HEADER_RE = /^jobs:\s*(#.*)?$/;
const ALWAYS_RE = /^(?:always\(\)|\$\{\{\s*always\(\)\s*\}\})$/;

function stripComment(raw) {
  let inSingle = false;
  let inDouble = false;
  for (let i = 0; i < raw.length; i += 1) {
    const ch = raw[i];
    if (ch === "'" && !inDouble) inSingle = !inSingle;
    else if (ch === '"' && !inSingle) inDouble = !inDouble;
    else if (ch === "#" && !inSingle && !inDouble) return raw.slice(0, i);
  }
  return raw;
}

function unquote(value) {
  const v = value.trim();
  if (v.length >= 2) {
    const start = v[0];
    const end = v[v.length - 1];
    if ((start === '"' && end === '"') || (start === "'" && end === "'")) {
      return v.slice(1, -1);
    }
  }
  return v;
}

function leadingSpaces(line) {
  let n = 0;
  while (n < line.length && line[n] === " ") n += 1;
  return n;
}

function parseInlineNeeds(raw) {
  const trimmed = raw.trim();
  if (!trimmed.startsWith("[") || !trimmed.endsWith("]")) return [];
  const inner = trimmed.slice(1, -1).trim();
  if (inner === "") return [];
  return inner
    .split(",")
    .map((part) => unquote(part))
    .filter((part) => part !== "");
}

function parseBlockNeeds(lines, startIndex) {
  const items = [];
  let i = startIndex;
  for (; i < lines.length; i += 1) {
    const raw = lines[i];
    const stripped = stripComment(raw);
    if (stripped.trim() === "") continue;
    const indent = leadingSpaces(stripped);
    if (indent <= 4) break;
    const item = stripped.trim();
    if (!item.startsWith("-")) break;
    const value = unquote(item.slice(1));
    if (value !== "") items.push(value);
  }
  return { items, nextIndex: i };
}

/**
 * Hand-parse a GitHub Actions workflow enough to check aggregator completeness.
 *
 * @param {string} yamlText
 * @param {{ aggregateJobId?: string, optionalJobs?: string[] }} [options]
 * @returns {{
 *   jobIds: string[],
 *   aggregateName: string | null,
 *   hasAlways: boolean,
 *   needs: string[],
 *   missing: string[],
 *   unknownNeeds: string[],
 * }}
 */
export function analyzeCiAggregate(
  yamlText,
  { aggregateJobId = "ci-success", optionalJobs = [] } = {},
) {
  const lines = yamlText.split(/\r?\n/);
  const jobIds = [];
  let inJobs = false;
  let aggregateStart = -1;
  let aggregateEnd = -1;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (!inJobs) {
      if (JOBS_HEADER_RE.test(line)) inJobs = true;
      continue;
    }
    if (TOP_KEY_RE.test(line)) {
      if (aggregateStart !== -1 && aggregateEnd === -1) aggregateEnd = i;
      break;
    }
    const match = line.match(JOB_ID_RE);
    if (!match) continue;
    if (aggregateStart !== -1 && aggregateEnd === -1) aggregateEnd = i;
    jobIds.push(match[1]);
    if (match[1] === aggregateJobId) aggregateStart = i + 1;
  }
  if (aggregateStart !== -1 && aggregateEnd === -1) aggregateEnd = lines.length;

  let aggregateName = null;
  let hasAlways = false;
  const needs = [];

  if (aggregateStart !== -1) {
    const body = lines.slice(aggregateStart, aggregateEnd);
    for (let i = 0; i < body.length; i += 1) {
      const stripped = stripComment(body[i]);
      const nameMatch = stripped.match(/^    name:\s*(.*)$/);
      if (nameMatch && aggregateName === null) {
        const value = unquote(nameMatch[1]);
        aggregateName = value === "" ? null : value;
        continue;
      }
      const ifMatch = stripped.match(/^    if:\s*(.*)$/);
      if (ifMatch) {
        const value = unquote(ifMatch[1]);
        if (ALWAYS_RE.test(value)) hasAlways = true;
        continue;
      }
      const needsMatch = stripped.match(/^    needs:\s*(.*)$/);
      if (!needsMatch) continue;
      const rest = needsMatch[1].trim();
      if (rest === "") {
        const parsed = parseBlockNeeds(body, i + 1);
        needs.push(...parsed.items);
        i = parsed.nextIndex - 1;
      } else if (rest.startsWith("[")) {
        needs.push(...parseInlineNeeds(rest));
      } else {
        const scalar = unquote(rest);
        if (scalar !== "") needs.push(scalar);
      }
    }
  }

  const optional = new Set(optionalJobs);
  const jobSet = new Set(jobIds);
  const needsSet = new Set(needs);
  const missing = jobIds.filter(
    (id) => id !== aggregateJobId && !optional.has(id) && !needsSet.has(id),
  );
  const unknownNeeds = needs.filter((id) => !jobSet.has(id));

  return { jobIds, aggregateName, hasAlways, needs, missing, unknownNeeds };
}

function problemsOf(result) {
  const problems = [];
  for (const id of result.missing) problems.push(`missing from needs: ${id}`);
  for (const id of result.unknownNeeds) problems.push(`unknown needs entry: ${id}`);
  if (!result.hasAlways) problems.push("aggregator missing if: always()");
  if (result.aggregateName !== "CI Required") {
    problems.push(
      `aggregator name must be "CI Required" (got ${JSON.stringify(result.aggregateName)})`,
    );
  }
  return problems;
}

function main() {
  let yamlText;
  try {
    yamlText = readFileSync(CI_YML, "utf8");
  } catch (error) {
    process.stderr.write(`check-ci-aggregate: cannot read ${CI_YML}: ${error.message}\n`);
    return 1;
  }

  const result = analyzeCiAggregate(yamlText);
  const problems = problemsOf(result);
  if (problems.length > 0) {
    for (const problem of problems) process.stdout.write(`${problem}\n`);
    return 1;
  }

  process.stdout.write(
    `check-ci-aggregate: PASS jobs=${result.jobIds.length} aggregated=${result.needs.length}\n`,
  );
  return 0;
}

const isMain =
  Boolean(process.argv[1]) && pathToFileURL(resolve(process.argv[1])).href === import.meta.url;

if (isMain) {
  process.exit(main());
}
