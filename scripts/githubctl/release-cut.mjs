import {
  assertApplyClearance,
  assertIssueNumber,
  assertMutationMode,
  assertNoClosingLink,
  assertRepository,
  planCreateReleasePullRequest,
  releasePullRequestTitle,
} from "./adapter.mjs";
import { ciResult } from "./ci-land.mjs";
import {
  BlockingFindingError,
  CiFailedError,
  DoctorRequiredError,
  DuplicateError,
  GitHubAdapterError,
  NotFoundError,
  UnauthorizedReleaseError,
} from "./errors.mjs";
import { rehearsalIdentity } from "./release-plan.mjs";
import { validateFindingCarryForward } from "../../roadmap/0.1.0-tama/tools/lib.mjs";

const BLOCKING_SEVERITY = new Set(["P0", "P1"]);

export function createReleasePullRequest(adapter, request) {
  if (!adapter || typeof adapter.createReleasePullRequest !== "function") {
    throw new GitHubAdapterError("adapter.createReleasePullRequest is required");
  }
  return adapter.createReleasePullRequest(request);
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

function compareBlockers(left, right) {
  const byReason = left.reason.localeCompare(right.reason);
  if (byReason !== 0) return byReason;
  return String(left.issue ?? "").localeCompare(String(right.issue ?? ""), undefined, {
    numeric: true,
  });
}

function pullsForHead(adapter, head) {
  if (typeof adapter.pullsForHead !== "function") {
    throw new GitHubAdapterError("adapter.pullsForHead is required");
  }
  return adapter.pullsForHead(head);
}

function findingBlockers(findings) {
  const blockers = [];
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
  return blockers;
}

function plannedPull(title, body, head, base) {
  return planCreateReleasePullRequest({ title, body, head, base });
}

function closeMilestoneIfRequested(options, mode, title, clearance) {
  if (options.closeMilestone !== true) {
    return { title, close: false, applied: false };
  }
  if (mode === "check") {
    return { title, close: true, applied: false };
  }
  if (typeof options.adapter.closeMilestone !== "function") {
    throw new GitHubAdapterError("adapter.closeMilestone is required");
  }
  const closed = options.adapter.closeMilestone({
    title,
    mode: "apply",
    clearance,
    owner: options.owner,
    repo: options.repo,
  });
  return { title, close: true, applied: closed.applied === true };
}

export function releaseCut(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  const versionTitle = releasePullRequestTitle(options.version);
  const head =
    typeof options.head === "string" && options.head.length > 0
      ? options.head
      : `release/v${options.version}`;
  const base = typeof options.base === "string" && options.base.length > 0 ? options.base : "main";
  const body = typeof options.body === "string" ? options.body : "";
  assertNoClosingLink(body);
  const authorize = options.authorize === true;
  const land = options.land === true;
  assertRepository(options.adapter, options);

  const findings = parseFindings(options.findings);
  const blockers = findingBlockers(findings);
  const rehearsal = {
    ...rehearsalIdentity(options.repoRoot),
    recorded: mode === "apply",
    dispatched: false,
    terminal_result: "not-run",
  };
  const authorization = { kind: "ReleaseCutAuthorization", authorized: authorize };
  const landing = {
    kind: "ReleaseLanding",
    commit_title: versionTitle,
    merge_method: "squash",
    applied: false,
  };
  const milestoneTitle = `v${options.version}`;

  if (mode === "apply" && !authorize) {
    throw new UnauthorizedReleaseError("apply requires --authorize");
  }
  if (mode === "apply" && blockers.length > 0) {
    throw new BlockingFindingError("P0/P1 findings block release");
  }

  const report = {
    kind: "release-cut",
    mode,
    ok: blockers.length === 0,
    version: options.version,
    title: versionTitle,
    authorization,
    findings,
    blockers,
    rehearsal,
    landing,
    milestone: { title: milestoneTitle, close: options.closeMilestone === true, applied: false },
  };

  if (mode === "check") {
    if (land && options.pr != null) {
      report.pull_request = {
        ...plannedPull(versionTitle, body, head, base),
        number: assertIssueNumber(options.pr, "pull request number"),
      };
    } else {
      report.pull_request = plannedPull(versionTitle, body, head, base);
    }
    return report;
  }

  if (!options.clearance) {
    throw new DoctorRequiredError("apply requires GitHubDoctor pullRequests clearance");
  }
  assertApplyClearance(mode, options.clearance, "pullRequests", options.adapter);

  if (land) {
    const pr = assertIssueNumber(options.pr, "pull request number");
    if (typeof options.adapter.getPullRequest !== "function") {
      throw new GitHubAdapterError("adapter.getPullRequest is required");
    }
    const pull = options.adapter.getPullRequest(pr);
    if (pull == null) throw new NotFoundError(`pull request #${pr} is missing`);
    if (pull.title !== versionTitle) {
      throw new GitHubAdapterError(`pull request title must be ${versionTitle}`);
    }
    assertNoClosingLink(pull.body ?? "");
    const ci = ciResult({
      adapter: options.adapter,
      pr,
      requiredJobs: options.requiredJobs,
      tamaChanged: options.tamaChanged,
      owner: options.owner,
      repo: options.repo,
      mode: "check",
    });
    if (!ci.ok) throw new CiFailedError("release landing requires a successful CiResult");
    if (typeof options.adapter.mergePullRequest !== "function") {
      throw new GitHubAdapterError("adapter.mergePullRequest is required");
    }
    const merged = options.adapter.mergePullRequest({
      number: pr,
      mergeMethod: "squash",
      commitTitle: versionTitle,
      mode: "apply",
      clearance: options.clearance,
      owner: options.owner,
      repo: options.repo,
    });
    report.pull_request = {
      ...plannedPull(versionTitle, pull.body ?? body, pull.head ?? head, pull.base ?? base),
      number: pr,
    };
    report.landing = {
      kind: "ReleaseLanding",
      commit_title: versionTitle,
      merge_method: merged.merge_method,
      applied: merged.applied === true,
      number: merged.number,
    };
    report.milestone = closeMilestoneIfRequested(options, mode, milestoneTitle, options.clearance);
    return report;
  }

  const existing = pullsForHead(options.adapter, head);
  if (existing.length === 1) {
    throw new DuplicateError(`pull request already exists for head ${head}`);
  }
  if (existing.length > 1) {
    throw new DuplicateError(`ambiguous existing pull requests for head ${head}`);
  }
  const created = createReleasePullRequest(options.adapter, {
    title: versionTitle,
    body,
    head,
    base,
    mode: "apply",
    clearance: options.clearance,
    owner: options.owner,
    repo: options.repo,
  });
  report.pull_request = created;
  report.milestone = closeMilestoneIfRequested(options, mode, milestoneTitle, options.clearance);
  return report;
}
