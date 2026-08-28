#!/usr/bin/env node
import { GitHubAdapter } from "./adapter.mjs";
import { GitHubDoctor } from "./doctor.mjs";
import { FakeGitHubAdapter } from "./fake.mjs";

function printHelp() {
  console.log(`Usage: githubctl <command>

Commands:
  doctor [--fake] [--owner <owner> --repo <repo>]
  check

doctor validates GitHub authentication, repository access, and issue/PR
mutation capability. It never writes.

createIssue, updateIssue, and createPullRequest are library APIs. Each
requires mode 'check' or 'apply'; apply is doctor-gated.

sync-issues is not part of this command set.
`);
}

function parseArgs(argv) {
  const flags = new Set();
  const options = {};
  const positionals = [];
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help" || arg === "-h") flags.add("help");
    else if (arg === "--fake") flags.add("fake");
    else if (arg === "--owner" || arg === "--repo") {
      const value = argv[i + 1];
      if (!value || value.startsWith("--")) throw new Error(`${arg} requires a value`);
      options[arg.slice(2)] = value;
      i += 1;
    } else if (arg.startsWith("--")) throw new Error(`unknown flag ${arg}`);
    else positionals.push(arg);
  }
  return { flags, options, positionals };
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
    const owner = options.owner ?? (flags.has("fake") ? "example" : null);
    const repo = options.repo ?? (flags.has("fake") ? "repo" : null);
    if (!owner || !repo) throw new Error("doctor requires --owner and --repo");
    const adapter = flags.has("fake")
      ? new FakeGitHubAdapter({ owner, repo })
      : new GitHubAdapter({ owner, repo });
    const report = new GitHubDoctor(adapter).check();
    console.log(JSON.stringify(report, null, 2));
    return report.ok ? 0 : 1;
  }
  if (command === "check") {
    console.log(
      "Mutation APIs exist as library methods: createIssue, updateIssue, createPullRequest.",
    );
    console.log("Each requires mode check or apply; apply is doctor-gated.");
    console.log("sync-issues is not part of this command set.");
    return 0;
  }
  throw new Error(`unknown command ${command}; supported commands: doctor, check`);
}

try {
  process.exitCode = main(process.argv.slice(2));
} catch (error) {
  console.error(`ERROR: ${error.message}`);
  process.exitCode = 1;
}
