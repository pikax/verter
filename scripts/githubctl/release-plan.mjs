import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  assertApplyClearance,
  assertIssueNumber,
  assertMutationMode,
  assertRequiredText,
} from "./adapter.mjs";
import { AmbiguousWaiverError, GitHubAdapterError } from "./errors.mjs";
import { loadLedgerFile } from "./ledger-write.mjs";
import {
  deriveState,
  loadAuthority,
  validateFindingCarryForward,
} from "../../roadmap/0.1.0-tama/tools/lib.mjs";

export const RELEASE_REHEARSAL = Object.freeze({
  workflow: "release-check.yml",
  uses: "release.yml",
  dry_run: true,
});

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const BLOCKING_SEVERITY = new Set(["P0", "P1"]);

const RELEASE_YML_USES = "./.github/workflows/release.yml";

function stripInlineYamlComment(line) {
  let quote = null;
  for (let i = 0; i < line.length; i += 1) {
    const ch = line[i];
    if (quote === "'") {
      if (ch === "'") quote = null;
      continue;
    }
    if (quote === '"') {
      if (ch === "\\" && i + 1 < line.length) {
        i += 1;
        continue;
      }
      if (ch === '"') quote = null;
      continue;
    }
    if (ch === "'" || ch === '"') {
      quote = ch;
      continue;
    }
    if (ch === "#") return line.slice(0, i);
  }
  return line;
}

function parseYamlScalar(raw) {
  const value = raw.trim();
  if (value === "" || value === "~" || value === "null" || value === "Null" || value === "NULL") {
    return null;
  }
  if (value === "true" || value === "True" || value === "TRUE") return true;
  if (value === "false" || value === "False" || value === "FALSE") return false;
  if (value.length >= 2) {
    const start = value[0];
    const end = value[value.length - 1];
    if ((start === '"' && end === '"') || (start === "'" && end === "'")) {
      return value.slice(1, -1);
    }
  }
  return value;
}

function splitYamlKeyValue(text) {
  const colon = text.indexOf(":");
  if (colon <= 0) return null;
  return { key: parseYamlScalar(text.slice(0, colon)), rest: text.slice(colon + 1).trim() };
}

function parseYamlMapping(rows, start, parentIndent) {
  const object = {};
  if (start >= rows.length) return { value: object, next: start };
  const level = rows[start].indent;
  if (level <= parentIndent) return { value: object, next: start };
  let i = start;
  while (i < rows.length) {
    const row = rows[i];
    if (row.indent < level) break;
    if (row.indent > level) {
      throw new GitHubAdapterError("release-check.yml must invoke release.yml");
    }
    const split = splitYamlKeyValue(row.text);
    if (!split || typeof split.key !== "string" || split.key.length === 0) {
      throw new GitHubAdapterError("release-check.yml must invoke release.yml");
    }
    if (split.rest !== "") {
      object[split.key] = parseYamlScalar(split.rest);
      i += 1;
      continue;
    }
    if (i + 1 < rows.length && rows[i + 1].indent > level) {
      const nested = parseYamlMapping(rows, i + 1, level);
      object[split.key] = nested.value;
      i = nested.next;
      continue;
    }
    object[split.key] = null;
    i += 1;
  }
  return { value: object, next: i };
}

function workflowJobs(text) {
  const rows = [];
  for (const raw of String(text).split(/\r?\n/u)) {
    const stripped = stripInlineYamlComment(raw);
    if (/^\s*$/u.test(stripped)) continue;
    if (stripped.includes("\t")) {
      throw new GitHubAdapterError("release-check.yml must invoke release.yml");
    }
    const indent = stripped.match(/^ */u)[0].length;
    rows.push({ indent, text: stripped.trim() });
  }
  const jobsIndex = rows.findIndex((row) => {
    if (row.indent !== 0) return false;
    const split = splitYamlKeyValue(row.text);
    return split?.key === "jobs" && split.rest === "";
  });
  if (jobsIndex === -1 || jobsIndex + 1 >= rows.length) return {};
  return parseYamlMapping(rows, jobsIndex + 1, 0).value;
}

function assertReleaseCheckDryRun(text) {
  const jobs = workflowJobs(text);
  const callers = Object.values(jobs).filter(
    (job) =>
      job !== null &&
      typeof job === "object" &&
      !Array.isArray(job) &&
      job.uses === RELEASE_YML_USES,
  );
  if (callers.length === 0) {
    throw new GitHubAdapterError("release-check.yml must invoke release.yml");
  }
  for (const job of callers) {
    const withInputs = job.with;
    if (withInputs === null || typeof withInputs !== "object" || Array.isArray(withInputs)) {
      throw new GitHubAdapterError("release-check.yml must pass dry_run: true");
    }
    if (withInputs.dry_run !== true) {
      throw new GitHubAdapterError("release-check.yml must pass dry_run: true");
    }
  }
}

export function rehearsalIdentity(repoRoot = REPO_ROOT) {
  const checkPath = path.join(repoRoot, ".github", "workflows", RELEASE_REHEARSAL.workflow);
  const releasePath = path.join(repoRoot, ".github", "workflows", RELEASE_REHEARSAL.uses);
  if (!fs.existsSync(checkPath)) {
    throw new GitHubAdapterError(`missing required workflow ${RELEASE_REHEARSAL.workflow}`);
  }
  if (!fs.existsSync(releasePath)) {
    throw new GitHubAdapterError(`missing required workflow ${RELEASE_REHEARSAL.uses}`);
  }
  assertReleaseCheckDryRun(fs.readFileSync(checkPath, "utf8"));
  return { workflow: RELEASE_REHEARSAL.workflow, uses: RELEASE_REHEARSAL.uses, dry_run: true };
}

function parseFindings(raw) {
  if (raw == null || raw === "") return [];
  let parsed = raw;
  if (typeof raw === "string") {
    try {
      parsed = JSON.parse(raw);
    } catch {
      throw new GitHubAdapterError("findings must be a JSON array");
    }
  }
  if (!Array.isArray(parsed)) throw new GitHubAdapterError("findings must be a JSON array");
  return parsed.map((row, index) => {
    const errors = validateFindingCarryForward(row, `findings[${index}]`);
    if (errors.length > 0) throw new GitHubAdapterError(errors.join("; "));
    return { issue: row.issue, severity: row.severity, owner: row.owner };
  });
}

function parseWaiveItems(raw) {
  if (raw == null || raw === "") return [];
  const values = Array.isArray(raw) ? raw : [raw];
  const numbers = [];
  const seen = new Set();
  for (const value of values) {
    const parts = typeof value === "number" ? [value] : String(value).split(",");
    for (const part of parts) {
      if (typeof part === "string" && part.trim() === "") continue;
      const parsed = typeof part === "number" ? part : Number(part.trim());
      const number = assertIssueNumber(parsed, "waive-item");
      if (seen.has(number)) continue;
      seen.add(number);
      numbers.push(number);
    }
  }
  return numbers.sort((left, right) => left - right);
}

function compareBlockers(left, right) {
  const byReason = left.reason.localeCompare(right.reason);
  if (byReason !== 0) return byReason;
  const byNode = (left.node_id ?? "").localeCompare(right.node_id ?? "");
  if (byNode !== 0) return byNode;
  return String(left.gh_issue ?? left.issue ?? "").localeCompare(
    String(right.gh_issue ?? right.issue ?? ""),
    undefined,
    { numeric: true },
  );
}

export function releasePlan(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  assertRequiredText(options.milestone, "milestone title");
  if (options.dispatch === true && mode !== "apply") {
    throw new GitHubAdapterError("--dispatch requires apply");
  }
  if (typeof options.adapter.listMilestoneIssues !== "function") {
    throw new GitHubAdapterError("adapter.listMilestoneIssues is required");
  }
  if (options.dispatch === true) {
    assertApplyClearance("apply", options.clearance, "actions", options.adapter);
  }
  const authority = options.authority ?? loadAuthority();
  const ledgerPath = options.ledgerPath ?? authority.ledgerFile;
  const ledger = loadLedgerFile(ledgerPath);
  const implemented = new Set(ledger.implemented.map((row) => row.node_id));
  const mappings = new Map(ledger.github_issue.map((row) => [row.gh_issue, row]));
  const state = deriveState(authority, { implemented: ledger.implemented });
  const items = options.adapter
    .listMilestoneIssues(options.milestone)
    .map((row) => {
      const mapping = mappings.get(row.number) ?? null;
      const nodeId = mapping?.node_id ?? null;
      return {
        number: row.number,
        title: row.title,
        state: row.state,
        node_id: nodeId,
        mapped: nodeId != null,
        implemented: nodeId != null && implemented.has(nodeId),
      };
    })
    .sort((left, right) => left.number - right.number);

  const waiveItems = parseWaiveItems(options.waiveItems);
  const itemNumbers = new Set(items.map((row) => row.number));
  for (const number of waiveItems) {
    if (!itemNumbers.has(number)) {
      throw new AmbiguousWaiverError(`--waive-item ${number} is not a milestone item`);
    }
    const item = items.find((row) => row.number === number);
    if (item.mapped) {
      throw new AmbiguousWaiverError(`--waive-item ${number} names a mapped DAG item`);
    }
  }
  const waived = new Set(waiveItems);

  const ready = [];
  const blockers = [];
  for (const item of items) {
    if (!item.mapped) {
      if (!waived.has(item.number)) {
        blockers.push({ kind: "ReleaseBlocker", reason: "unmapped", gh_issue: item.number });
      }
      continue;
    }
    if (item.implemented) {
      ready.push({ kind: "ReleaseReadiness", node_id: item.node_id, gh_issue: item.number });
    } else {
      blockers.push({
        kind: "ReleaseBlocker",
        reason: "unimplemented",
        node_id: item.node_id,
        gh_issue: item.number,
      });
    }
    for (const predecessor of state.states.get(item.node_id)?.missing_ancestors ?? []) {
      blockers.push({
        kind: "ReleaseBlocker",
        reason: "missing-predecessor",
        node_id: item.node_id,
        predecessor,
        gh_issue: item.number,
      });
    }
  }
  ready.sort((left, right) => left.node_id.localeCompare(right.node_id));

  const findings = parseFindings(options.findings);
  for (const finding of findings) {
    if (BLOCKING_SEVERITY.has(finding.severity)) {
      blockers.push({
        kind: "ReleaseBlocker",
        reason: "finding",
        issue: finding.issue,
        severity: finding.severity,
        owner: finding.owner,
      });
    }
  }
  blockers.sort(compareBlockers);

  const rehearsal = {
    ...rehearsalIdentity(options.repoRoot),
    recorded: mode === "apply",
    dispatched: false,
    terminal_result: "not-run",
  };
  const report = {
    kind: "release-plan",
    mode,
    ok: blockers.length === 0,
    milestone: { title: options.milestone },
    items,
    ready,
    blockers,
    waived: waiveItems.map((gh_issue) => ({ gh_issue })),
    findings,
    rehearsal,
  };
  if (mode === "check") return report;
  if (options.dispatch === true && report.ok) {
    if (typeof options.adapter.dispatchReleaseRehearsal !== "function") {
      throw new GitHubAdapterError("adapter.dispatchReleaseRehearsal is required");
    }
    options.adapter.dispatchReleaseRehearsal({
      mode: "apply",
      clearance: options.clearance,
    });
    report.rehearsal.dispatched = true;
    report.rehearsal.terminal_result = "pending";
  }
  return report;
}
