#!/usr/bin/env node
import { GitHubAdapter } from "./adapter.mjs";
import { createPr } from "./create-pr.mjs";
import {
  CREATE_PR_CAPABILITIES,
  GitHubDoctor,
  SCHEDULE_CAPABILITIES,
  SYNC_ISSUES_CAPABILITIES,
} from "./doctor.mjs";
import { mutationIdentity, PartialFailureError } from "./errors.mjs";
import { FakeGitHubAdapter } from "./fake.mjs";
import { schedule } from "./schedule.mjs";
import { syncIssues } from "./sync-issues.mjs";

function printHelp() {
  console.log(`Usage: githubctl <command>

Commands:
  doctor [--fake] [--owner <owner> --repo <repo>]
  check
  sync-issues --check|--apply --train <train> | --nodes <id,id,...>
    [--fake] [--owner <owner> --repo <repo>] [--model <name>]
    [--ledger <path>]
  create-pr --check|--apply --node <id> --title <final conventional commit>
    --head <branch> [--base <base>] [--body <pr prose>] [--model <name>]
    [--ledger <path>] [--write-locator] [--fake]
    [--owner <owner> --repo <repo>]
  schedule --check|--apply --train <train> | --nodes <id,id,...>
    [--fake] [--owner <owner> --repo <repo>] [--ledger <path>]
    [--set-milestone <title>]

doctor validates GitHub authentication, repository access, issue/PR
mutation capability, and whether Project 3 is readable. It never writes.
sync-issues --apply is doctor-gated for issues and does not require
Project 3. create-pr --apply is doctor-gated for issues and pullRequests
and does not require Project 3. schedule --apply requires issues and
Project 3.

Issue create/update and pull-request mutation remain library APIs. Each
requires mode 'check' or 'apply'; apply is doctor-gated.

sync-issues is occasional one-way DAG/charter-to-GitHub issue sync for an
explicit train or node set. It never imports GitHub edits.

create-pr creates one pull request whose title is the planned final
conventional-commit message and whose body contains exactly the mapped
Closes #<n> link. Opt-in mappings refresh the issue description; protected
mappings are not edited. --write-locator sets pull_request on an existing
implemented row only.

schedule overlays READY mapped issues onto GitHub Project 3. Check plans
only. Apply is doctor-gated. --set-milestone is the only milestone write.
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
  "--model",
  "--ledger",
  "--set-milestone",
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
  const model = options.model ?? process.env.GITHUBCTL_MODEL;
  if (!model) throw new Error("sync-issues requires --model or GITHUBCTL_MODEL");
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
    model,
    ledgerPath: options.ledger,
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return 0;
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
    model: options.model ?? process.env.GITHUBCTL_MODEL,
    ledgerPath: options.ledger,
    writeLocator: flags.has("write-locator"),
    clearance,
  });
  console.log(JSON.stringify(report, null, 2));
  return 0;
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
    train: hasTrain ? options.train : undefined,
    nodes: hasNodes ? options.nodes.split(",").map((id) => id.trim()) : undefined,
    ledgerPath: options.ledger,
    clearance,
    setMilestone: options["set-milestone"],
  });
  console.log(JSON.stringify(report, null, 2));
  return 0;
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
    console.log(
      "Mutation APIs exist as library methods: createIssue, updateIssue, createPullRequest.",
    );
    console.log("Each requires mode check or apply; apply is doctor-gated.");
    console.log("sync-issues --check|--apply syncs an explicit train or node set.");
    console.log("create-pr --check|--apply creates a final-title PR that closes the mapped issue.");
    console.log("schedule --check|--apply overlays READY work onto GitHub Project 3.");
    return 0;
  }
  if (command === "sync-issues") return runSyncIssues(flags, options);
  if (command === "create-pr") return runCreatePr(flags, options);
  if (command === "schedule") return runSchedule(flags, options);
  throw new Error(
    `unknown command ${command}; supported commands: doctor, check, sync-issues, create-pr, schedule`,
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
