#!/usr/bin/env node
import { GitHubAdapter } from "./adapter.mjs";
import { GitHubDoctor, PROTECTION_CAPABILITIES } from "./doctor.mjs";
import { mutationIdentity, PartialFailureError } from "./errors.mjs";
import { FakeGitHubAdapter } from "./fake.mjs";
import { protectionApply, protectionCheck } from "./protection.mjs";

function printHelp() {
  console.log(`Usage: githubctl <command>

Commands:
  doctor [--fake] [--owner <owner> --repo <repo>]
  protection --check|--apply [--fake] --owner <owner> --repo <repo>

doctor validates GitHub authentication, repository access, issue/PR
mutation capability, and whether Project 3 is readable. It never writes.

protection inspects the expected GitHub ruleset and repository merge
settings. Check reports drift without writing. Apply creates or updates
the named ruleset and patches repository merge settings; it never deletes
other rulesets. Extra unexpected blocking rules are reported and left
alone. Apply is doctor-gated for admin.

Work planning, issue mapping and readiness are not repository concerns:
the project DAG lives in the TAMA controller database, not in this tree.
`);
}

const VALUE_FLAGS = new Set(["--owner", "--repo"]);

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
  if (command === "protection") return runProtection(flags, options);
  throw new Error(`unknown command ${command}; supported commands: doctor, protection`);
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
