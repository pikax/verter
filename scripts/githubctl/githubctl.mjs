#!/usr/bin/env node
import { GitHubAdapter } from "./adapter.mjs";
import { ciResult, finalizeLedger, squashLand, squashLandCapabilities } from "./ci-land.mjs";
import { createPr } from "./create-pr.mjs";
import {
  CREATE_PR_CAPABILITIES,
  GitHubDoctor,
  INSPECT_CAPABILITIES,
  PROJECT_STATUS_CAPABILITIES,
  PROTECTION_CAPABILITIES,
  RELEASE_CUT_CAPABILITIES,
  RELEASE_PLAN_DISPATCH_CAPABILITIES,
  REVIEW_SUMMARY_CAPABILITIES,
  SCHEDULE_CAPABILITIES,
  SYNC_ISSUES_CAPABILITIES,
} from "./doctor.mjs";
import { inspectIssue } from "./inspect.mjs";
import { projectStatus, projectStatusPreflight } from "./project-status.mjs";
import { mutationIdentity, PartialFailureError } from "./errors.mjs";
import { FakeGitHubAdapter } from "./fake.mjs";
import { reviewSummary } from "./review-summary.mjs";
import { releaseCut } from "./release-cut.mjs";
import { releasePlan } from "./release-plan.mjs";
import { schedule, schedulePreflight } from "./schedule.mjs";
import { syncIssues } from "./sync-issues.mjs";
import { workflowInventory } from "./workflow.mjs";
import { protectionApply, protectionCheck } from "./protection.mjs";

function printHelp() {
  console.log(`Usage: githubctl <command>

Commands:
  doctor [--fake] [--owner <owner> --repo <repo>]
  check
  sync-issues --check|--apply --train <train> | --nodes <id,id,...>
    [--fake] [--owner <owner> --repo <repo>]
    [--ledger <path>] [--refresh-content]
    [--create-blockers | --ignore-blockers]
  project-status --check|--apply --node <id> --status in-progress|done
    [--ledger <path>] [--fake] [--owner <owner> --repo <repo>]
  create-pr --check|--apply --node <id> --title <final conventional commit>
    --head <branch> [--base <base>] [--body <pr prose>]
    [--ledger <path>] [--write-locator] [--fake]
    [--owner <owner> --repo <repo>]
  review-summary --check|--apply --pr <n> --node <id> --verdict PASS|FAIL
    --body <human prose> [--findings <json>]
    [--ledger <path>] [--fake] [--owner <owner> --repo <repo>]
  ci-result --check|--apply --pr <n> [--required <name,name>] [--tama-changed]
    [--fake] [--owner <owner> --repo <repo>]
  finalize-ledger --node <id> --message <title> --date <ISO> --pr <n>
    [--ledger <path>]
  squash-land --check|--apply --pr <n> --node <id> [--required <name,name>]
    [--tama-changed] [--ledger <path>] [--fake]
    [--owner <owner> --repo <repo>]
  inspect --check|--apply --issue <n> --verdict <AiIssueVerdict>
    [--fake] [--owner <owner> --repo <repo>] [--ledger <path>]
    [--report-dir <dir>]
  schedule --check|--apply --train <train> | --nodes <id,id,...>
    [--fake] [--owner <owner> --repo <repo>] [--ledger <path>]
  release-plan --check|--apply --milestone <title>
    [--fake] [--owner <owner> --repo <repo>] [--ledger <path>]
    [--findings <json>] [--waive-item <n>] [--dispatch]
  release-cut --check|--apply --version <semver> [--authorize]
    [--head <branch>] [--base main] [--body <pr prose>] [--findings <json>]
    [--land] [--pr <n>] [--close-milestone] [--fake]
    [--owner <owner> --repo <repo>]
  protection --check|--apply [--fake] --owner <owner> --repo <repo>

check prints the frozen composed-workflow inventory as JSON. It is
local and never contacts GitHub. Issue-sync stays available as an
explicit command after cutover.

doctor validates GitHub authentication, repository access, issue/PR
mutation capability, and whether Project 3 is readable. It never writes.
sync-issues --apply is doctor-gated for issues and Project 3.
project-status --apply is doctor-gated for Project 3.
create-pr --apply and review-summary --apply are doctor-gated for issues
and pullRequests and do not require Project 3. squash-land --apply is
doctor-gated for pullRequests and Project 3. release-cut --apply is
doctor-gated for pullRequests and does not require Project 3.
schedule --apply requires issues and Project 3.
release-plan --apply does not write GitHub. --dispatch is the only
workflow_dispatch path, is never the default, and is doctor-gated for
actions. HTTP 204 means the dispatch was accepted; rehearsal.dispatched
does not imply the rehearsal passed. Live job poll is not default.
protection --apply is doctor-gated for admin.

Issue create/update and pull-request mutation remain library APIs. Each
requires mode 'check' or 'apply'; apply is doctor-gated.

sync-issues is occasional one-way DAG/charter-to-GitHub issue sync for an
explicit train or node set. Normal runs reconcile the versioned label
catalog and managed issue labels without rewriting issue prose. Creating a
missing issue or using --refresh-content writes the stable AI-Generated
footer. It also reconciles catalog-backed milestones, direct blocked-by
edges, stable train parents, native sub-issues, and Project membership. Every
new Project item is initialized to Todo without resetting an existing status.
A selection with predecessor blocks outside its boundary fails before
mutation. --create-blockers recursively includes those predecessors and
creates missing issues; --ignore-blockers leaves their relationships
untouched. The flags are mutually exclusive. Sync never imports GitHub edits.

project-status projects local work lifecycle onto Project 3. In Progress
marks the selected issue and its native parent. Done rolls the parent to
Done only after every locally mapped child in the train is Done.

create-pr creates one pull request whose title is the planned final
conventional-commit message and whose body contains exactly the mapped
Closes #<n> link. Protected mappings are not edited. --write-locator sets
pull_request on an existing implemented row only.

review-summary posts one ordinary ReviewCycleSummary PR comment. Opt-in
mappings keep exactly one AI-Generated footer on the issue; protected
mappings are not edited. P0/P1 findings cannot accept. Apply is doctor-gated for issues
and pullRequests and does not require Project 3.

ci-result presents the live pull-request check-runs list as CiResult
evidence without SHA identity. Missing required jobs and unexpected skips
fail. finalize-ledger updates an existing implemented row only.
squash-land squash-merges after a successful CiResult and then marks the
mapped issue Done in Project 3. Check plans only. Apply is doctor-gated for
pullRequests and Project 3.
There is no post-merge ledger write.

schedule overlays READY mapped issues onto GitHub Project 3 and initializes
unset Project status to Todo. Check plans only. Milestone assignment belongs
exclusively to sync-issues through each block's gh_milestone field.

inspect retrieves a non-DAG issue, writes a local FeedbackReport, and
replaces exactly one AI-owned result label when the mapping allows it.
Check plans only. Apply is doctor-gated for issues and does not require
Project 3. Protected mappings and ai:ignore never write GitHub labels.

release-plan inspects milestone issues and derives readiness only from
local [[implemented]] rows. GitHub closure, labels, and Project status
never complete a node. Unmapped items are blockers unless --waive-item
names them. Apply records rehearsal identity without dispatching unless
--dispatch is explicit. --dispatch is doctor-gated for actions and records
terminal_result pending; plan.ok stays ledger-blocker emptiness.

release-cut opens a release pull request whose title is exactly
release: v<version> and whose squash subject is that same title with no
PR suffix. Check may plan without --authorize; apply requires --authorize
and is doctor-gated for pullRequests. Release PRs must not contain a
GitHub closing reference. Apply records rehearsal identity and does not
dispatch. --land squash-merges after a successful CiResult with
commit_title preserved.
Do not auto-close the milestone; --close-milestone is the only close path.
P0/P1 findings block; GitHub issue state cannot erase them.

protection inspects the expected GitHub ruleset and repository merge
settings. Check reports drift without writing. Apply creates or updates
the named ruleset and patches repository merge settings; it never deletes
other rulesets. Extra unexpected blocking rules are reported and left
alone. Apply is doctor-gated for admin.
`);
}

const VALUE_FLAGS = new Set([
  "--owner",
  "--repo",
  "--train",
  "--nodes",
  "--node",
  "--title",
  "--head",
  "--base",
  "--body",
  "--ledger",
  "--status",
  "--milestone",
  "--pr",
  "--verdict",
  "--findings",
  "--message",
  "--date",
  "--required",
  "--issue",
  "--report-dir",
  "--classification",
  "--reproduction",
  "--code-paths",
  "--commands",
  "--confidence",
  "--owner-hint",
  "--recommendation",
  "--inspected-at",
  "--version",
]);

function parseArgs(argv) {
  const flags = new Set();
  const options = {};
  const positionals = [];
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help" || arg === "-h") flags.add("help");
    else if (arg === "--fake") flags.add("fake");
    else if (arg === "--check") flags.add("check");
    else if (arg === "--apply") flags.add("apply");
    else if (arg === "--write-locator") flags.add("write-locator");
    else if (arg === "--refresh-content") flags.add("refresh-content");
    else if (arg === "--create-blockers") flags.add("create-blockers");
    else if (arg === "--ignore-blockers") flags.add("ignore-blockers");
    else if (arg === "--dispatch") flags.add("dispatch");
    else if (arg === "--authorize") flags.add("authorize");
    else if (arg === "--close-milestone") flags.add("close-milestone");
    else if (arg === "--land") flags.add("land");
    else if (arg === "--waive-item") {
      const value = argv[i + 1];
      if (!value || value.startsWith("--")) throw new Error(`${arg} requires a value`);
      if (!Array.isArray(options["waive-item"])) options["waive-item"] = [];
      options["waive-item"].push(value);
      i += 1;
    } else if (arg === "--tama-changed") flags.add("tama-changed");
    else if (VALUE_FLAGS.has(arg)) {
      const value = argv[i + 1];
      if (!value || value.startsWith("--")) throw new Error(`${arg} requires a value`);
      options[arg.slice(2)] = value;
      i += 1;
    } else if (arg.startsWith("--")) throw new Error(`unknown flag ${arg}`);
    else positionals.push(arg);
  }
  return { flags, options, positionals };
}

function boundAdapter(flags, options, label) {
  const owner = options.owner ?? (flags.has("fake") ? "example" : null);
  const repo = options.repo ?? (flags.has("fake") ? "repo" : null);
  if (!owner || !repo) throw new Error(`${label} requires --owner and --repo`);
  return flags.has("fake")
    ? new FakeGitHubAdapter({ owner, repo })
    : new GitHubAdapter({ owner, repo });
}

function runSyncIssues(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) throw new Error("sync-issues requires exactly one of --check or --apply");
  const hasTrain = typeof options.train === "string" && options.train.length > 0;
  const hasNodes = typeof options.nodes === "string" && options.nodes.length > 0;
  if (hasTrain === hasNodes) {
    throw new Error("sync-issues requires exactly one of --train or --nodes");
  }
  const adapter = boundAdapter(flags, options, "sync-issues");
  let clearance;
  if (apply) {
    const doctor = new GitHubDoctor(adapter).check({ require: SYNC_ISSUES_CAPABILITIES });
    if (!doctor.ok) {
      console.log(JSON.stringify(doctor, null, 2));
      return 1;
    }
    clearance = doctor.clearance;
  }
  const report = syncIssues({
    adapter,
    mode: apply ? "apply" : "check",
    train: hasTrain ? options.train : undefined,
    nodes: hasNodes ? options.nodes.split(",").map((id) => id.trim()) : undefined,
    refreshContent: flags.has("refresh-content"),
    createBlockers: flags.has("create-blockers"),
    ignoreBlockers: flags.has("ignore-blockers"),
    ledgerPath: options.ledger,
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return report.ok ? 0 : 1;
}

function runCreatePr(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) throw new Error("create-pr requires exactly one of --check or --apply");
  if (typeof options.train === "string" || typeof options.nodes === "string") {
    throw new Error("create-pr accepts exactly one --node; batch selection is forbidden");
  }
  if (typeof options.node !== "string" || options.node.length === 0) {
    throw new Error("create-pr requires --node");
  }
  if (typeof options.title !== "string" || options.title.length === 0) {
    throw new Error("create-pr requires --title");
  }
  if (typeof options.head !== "string" || options.head.length === 0) {
    throw new Error("create-pr requires --head");
  }
  const adapter = boundAdapter(flags, options, "create-pr");
  let clearance;
  if (apply) {
    const doctor = new GitHubDoctor(adapter).check({ require: CREATE_PR_CAPABILITIES });
    if (!doctor.ok) {
      console.log(JSON.stringify(doctor, null, 2));
      return 1;
    }
    clearance = doctor.clearance;
  }
  const report = createPr({
    adapter,
    mode: apply ? "apply" : "check",
    node: options.node,
    title: options.title,
    head: options.head,
    base: options.base,
    body: options.body,
    ledgerPath: options.ledger,
    writeLocator: flags.has("write-locator"),
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return 0;
}

function runReviewSummary(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) {
    throw new Error("review-summary requires exactly one of --check or --apply");
  }
  if (typeof options.train === "string" || typeof options.nodes === "string") {
    throw new Error("review-summary accepts exactly one --node; batch selection is forbidden");
  }
  if (typeof options.node !== "string" || options.node.length === 0) {
    throw new Error("review-summary requires --node");
  }
  if (typeof options.pr !== "string" || options.pr.length === 0) {
    throw new Error("review-summary requires --pr");
  }
  if (typeof options.verdict !== "string" || options.verdict.length === 0) {
    throw new Error("review-summary requires --verdict");
  }
  if (typeof options.body !== "string" || options.body.length === 0) {
    throw new Error("review-summary requires --body");
  }
  const adapter = boundAdapter(flags, options, "review-summary");
  let clearance;
  if (apply) {
    const doctor = new GitHubDoctor(adapter).check({ require: REVIEW_SUMMARY_CAPABILITIES });
    if (!doctor.ok) {
      console.log(JSON.stringify(doctor, null, 2));
      return 1;
    }
    clearance = doctor.clearance;
  }
  const report = reviewSummary({
    adapter,
    mode: apply ? "apply" : "check",
    node: options.node,
    pr: Number(options.pr),
    verdict: options.verdict,
    body: options.body,
    findings: options.findings,
    ledgerPath: options.ledger,
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return 0;
}

function parseRequiredJobs(options) {
  if (typeof options.required !== "string" || options.required.length === 0) return undefined;
  return options.required
    .split(",")
    .map((name) => name.trim())
    .filter(Boolean);
}

function runCiResult(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) throw new Error("ci-result requires exactly one of --check or --apply");
  if (typeof options.pr !== "string" || options.pr.length === 0) {
    throw new Error("ci-result requires --pr");
  }
  const adapter = boundAdapter(flags, options, "ci-result");
  const report = ciResult({
    adapter,
    mode: apply ? "apply" : "check",
    pr: Number(options.pr),
    requiredJobs: parseRequiredJobs(options),
    tamaChanged: flags.has("tama-changed"),
    owner: options.owner,
    repo: options.repo,
  });
  console.log(JSON.stringify(report, null, 2));
  return report.ok ? 0 : 1;
}

function runFinalizeLedger(flags, options) {
  if (typeof options.node !== "string" || options.node.length === 0) {
    throw new Error("finalize-ledger requires --node");
  }
  if (typeof options.message !== "string" || options.message.length === 0) {
    throw new Error("finalize-ledger requires --message");
  }
  if (typeof options.date !== "string" || options.date.length === 0) {
    throw new Error("finalize-ledger requires --date");
  }
  if (typeof options.pr !== "string" || options.pr.length === 0) {
    throw new Error("finalize-ledger requires --pr");
  }
  const report = finalizeLedger({
    node: options.node,
    message: options.message,
    date: options.date,
    pr: Number(options.pr),
    ledgerPath: options.ledger,
  });
  console.log(JSON.stringify(report, null, 2));
  return 0;
}

function runSquashLand(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) throw new Error("squash-land requires exactly one of --check or --apply");
  if (typeof options.pr !== "string" || options.pr.length === 0) {
    throw new Error("squash-land requires --pr");
  }
  if (typeof options.node !== "string" || options.node.length === 0) {
    throw new Error("squash-land requires --node");
  }
  const adapter = boundAdapter(flags, options, "squash-land");
  let clearance;
  if (apply) {
    const doctor = new GitHubDoctor(adapter).check({
      require: squashLandCapabilities({ node: options.node, ledgerPath: options.ledger }),
    });
    if (!doctor.ok) {
      console.log(JSON.stringify(doctor, null, 2));
      return 1;
    }
    clearance = doctor.clearance;
  }
  const report = squashLand({
    adapter,
    mode: apply ? "apply" : "check",
    pr: Number(options.pr),
    node: options.node,
    requiredJobs: parseRequiredJobs(options),
    tamaChanged: flags.has("tama-changed"),
    ledgerPath: options.ledger,
    clearance,
    owner: options.owner,
    repo: options.repo,
  });
  console.log(JSON.stringify(report, null, 2));
  return 0;
}

function runInspect(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) throw new Error("inspect requires exactly one of --check or --apply");
  if (typeof options.issue !== "string" || options.issue.length === 0) {
    throw new Error("inspect requires --issue");
  }
  if (typeof options.verdict !== "string" || options.verdict.length === 0) {
    throw new Error("inspect requires --verdict");
  }
  const adapter = boundAdapter(flags, options, "inspect");
  let clearance;
  if (apply) {
    const doctor = new GitHubDoctor(adapter).check({ require: INSPECT_CAPABILITIES });
    if (!doctor.ok) {
      console.log(JSON.stringify(doctor, null, 2));
      return 1;
    }
    clearance = doctor.clearance;
  }
  const report = inspectIssue({
    adapter,
    mode: apply ? "apply" : "check",
    issue: Number(options.issue),
    verdict: options.verdict,
    ledgerPath: options.ledger,
    reportDir: options["report-dir"],
    classification: options.classification,
    reproduction: options.reproduction,
    codePaths: options["code-paths"],
    commands: options.commands,
    confidence: options.confidence,
    ownerHint: options["owner-hint"],
    recommendation: options.recommendation,
    inspectedAt: options["inspected-at"],
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return report.ok ? 0 : 1;
}

function runSchedule(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) throw new Error("schedule requires exactly one of --check or --apply");
  const hasTrain = typeof options.train === "string" && options.train.length > 0;
  const hasNodes = typeof options.nodes === "string" && options.nodes.length > 0;
  if (hasTrain === hasNodes) {
    throw new Error("schedule requires exactly one of --train or --nodes");
  }
  const selection = {
    train: hasTrain ? options.train : undefined,
    nodes: hasNodes ? options.nodes.split(",").map((id) => id.trim()) : undefined,
    ledgerPath: options.ledger,
  };
  const preflight = schedulePreflight(selection);
  const adapter = boundAdapter(flags, options, "schedule");
  let clearance;
  if (apply) {
    const doctor = new GitHubDoctor(adapter).check({ require: SCHEDULE_CAPABILITIES });
    if (!doctor.ok) {
      console.log(JSON.stringify(doctor, null, 2));
      return 1;
    }
    clearance = doctor.clearance;
  }
  const report = schedule({
    adapter,
    mode: apply ? "apply" : "check",
    ...selection,
    preflight,
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return 0;
}

function runProjectStatus(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) {
    throw new Error("project-status requires exactly one of --check or --apply");
  }
  if (typeof options.node !== "string" || options.node.length === 0) {
    throw new Error("project-status requires --node");
  }
  if (options.status !== "in-progress" && options.status !== "done") {
    throw new Error("project-status requires --status in-progress|done");
  }
  const selection = {
    node: options.node,
    status: options.status,
    ledgerPath: options.ledger,
  };
  const preflight = projectStatusPreflight(selection);
  const adapter = boundAdapter(flags, options, "project-status");
  let clearance;
  if (apply) {
    const doctor = new GitHubDoctor(adapter).check({ require: PROJECT_STATUS_CAPABILITIES });
    if (!doctor.ok) {
      console.log(JSON.stringify(doctor, null, 2));
      return 1;
    }
    clearance = doctor.clearance;
  }
  const report = projectStatus({
    adapter,
    mode: apply ? "apply" : "check",
    ...selection,
    preflight,
    owner: options.owner,
    repo: options.repo,
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return 0;
}

function runReleasePlan(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) throw new Error("release-plan requires exactly one of --check or --apply");
  if (typeof options.milestone !== "string" || options.milestone.length === 0) {
    throw new Error("release-plan requires --milestone");
  }
  const dispatch = flags.has("dispatch");
  if (dispatch && !apply) throw new Error("--dispatch requires apply");
  const adapter = boundAdapter(flags, options, "release-plan");
  let clearance;
  if (dispatch) {
    const doctor = new GitHubDoctor(adapter).check({
      require: RELEASE_PLAN_DISPATCH_CAPABILITIES,
    });
    if (!doctor.ok) {
      console.log(JSON.stringify(doctor, null, 2));
      return 1;
    }
    clearance = doctor.clearance;
  }
  const report = releasePlan({
    adapter,
    mode: apply ? "apply" : "check",
    milestone: options.milestone,
    ledgerPath: options.ledger,
    findings: options.findings,
    waiveItems: options["waive-item"],
    dispatch,
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return report.ok ? 0 : 1;
}

function runReleaseCut(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) throw new Error("release-cut requires exactly one of --check or --apply");
  if (typeof options.version !== "string" || options.version.length === 0) {
    throw new Error("release-cut requires --version");
  }
  const land = flags.has("land");
  if (land && apply && (typeof options.pr !== "string" || options.pr.length === 0)) {
    throw new Error("release-cut --land requires --pr");
  }
  const adapter = boundAdapter(flags, options, "release-cut");
  let clearance;
  if (apply) {
    const doctor = new GitHubDoctor(adapter).check({ require: RELEASE_CUT_CAPABILITIES });
    if (!doctor.ok) {
      console.log(JSON.stringify(doctor, null, 2));
      return 1;
    }
    clearance = doctor.clearance;
  }
  const report = releaseCut({
    adapter,
    mode: apply ? "apply" : "check",
    version: options.version,
    head: options.head,
    base: options.base,
    body: options.body,
    authorize: flags.has("authorize"),
    land,
    pr: typeof options.pr === "string" ? Number(options.pr) : undefined,
    closeMilestone: flags.has("close-milestone"),
    findings: options.findings,
    requiredJobs: parseRequiredJobs(options),
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return report.ok ? 0 : 1;
}

function runProtection(flags, options) {
  const check = flags.has("check");
  const apply = flags.has("apply");
  if (check === apply) throw new Error("protection requires exactly one of --check or --apply");
  const adapter = boundAdapter(flags, options, "protection");
  if (check) {
    const report = protectionCheck({
      adapter,
      owner: options.owner,
      repo: options.repo,
    });
    console.log(JSON.stringify(report, null, 2));
    return report.ok ? 0 : 1;
  }
  const doctor = new GitHubDoctor(adapter).check({ require: PROTECTION_CAPABILITIES });
  if (!doctor.ok) {
    console.log(JSON.stringify(doctor, null, 2));
    return 1;
  }
  const report = protectionApply({
    adapter,
    owner: options.owner,
    repo: options.repo,
    clearance: doctor.clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return report.ok ? 0 : 1;
}

function main(argv) {
  const { flags, options, positionals } = parseArgs(argv);
  if (flags.has("help") && positionals.length === 0) {
    printHelp();
    return 0;
  }
  const command = positionals[0];
  if (!command) {
    printHelp();
    return 1;
  }
  if (positionals.length > 1) throw new Error(`${command} takes no positional arguments`);
  if (command === "doctor") {
    const adapter = boundAdapter(flags, options, "doctor");
    const report = new GitHubDoctor(adapter).check();
    console.log(JSON.stringify(report, null, 2));
    return report.ok ? 0 : 1;
  }
  if (command === "check") {
    console.log(JSON.stringify(workflowInventory(), null, 2));
    return 0;
  }
  if (command === "inspect") return runInspect(flags, options);
  if (command === "sync-issues") return runSyncIssues(flags, options);
  if (command === "project-status") return runProjectStatus(flags, options);
  if (command === "create-pr") return runCreatePr(flags, options);
  if (command === "review-summary") return runReviewSummary(flags, options);
  if (command === "ci-result") return runCiResult(flags, options);
  if (command === "finalize-ledger") return runFinalizeLedger(flags, options);
  if (command === "squash-land") return runSquashLand(flags, options);
  if (command === "schedule") return runSchedule(flags, options);
  if (command === "release-plan") return runReleasePlan(flags, options);
  if (command === "release-cut") return runReleaseCut(flags, options);
  if (command === "protection") return runProtection(flags, options);
  throw new Error(
    `unknown command ${command}; supported commands: doctor, check, inspect, sync-issues, project-status, create-pr, review-summary, ci-result, finalize-ledger, squash-land, schedule, release-plan, release-cut, protection`,
  );
}

try {
  process.exitCode = main(process.argv.slice(2));
} catch (error) {
  console.error(`ERROR: ${error.message}`);
  if (error instanceof PartialFailureError) {
    for (const row of error.succeeded) {
      const identity = mutationIdentity(row);
      if (identity) console.error(JSON.stringify(identity));
    }
  }
  process.exitCode = 1;
}
