#!/usr/bin/env node
/** @ai-generated - Isolated Git/CLI negative controls; never mutates the source checkout. */
import cp from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  PACKAGE_ROOT,
  computeAuthorityDigest,
  confinedFile,
  digestPayload,
  loadAuthority,
  parseToml,
  writeGenerated,
} from "./lib.mjs";

const cases = JSON.parse(
  fs.readFileSync(path.join(PACKAGE_ROOT, "fixtures/negative/cases.json"), "utf8"),
).cases;
const sourceRepo = cp
  .execFileSync("git", ["rev-parse", "--show-toplevel"], {
    cwd: PACKAGE_ROOT,
    encoding: "utf8",
    timeout: 30_000,
    killSignal: "SIGKILL",
  })
  .trim();
const commonGit = cp
  .execFileSync("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], {
    cwd: sourceRepo,
    encoding: "utf8",
    timeout: 30_000,
    killSignal: "SIGKILL",
  })
  .trim();
const packageRelative = path.relative(sourceRepo, PACKAGE_ROOT);
const sourceAuthority = loadAuthority();
const reviewProfiles =
  parseToml(fs.readFileSync(path.join(PACKAGE_ROOT, "catalogs/review-profiles.toml"), "utf8"))
    .profile || [];
const profileLenses = new Map(reviewProfiles.map((profile) => [profile.id, profile.lenses]));

function git(args, cwd) {
  return cp
    .execFileSync("git", args, {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 30_000,
      killSignal: "SIGKILL",
    })
    .trim();
}

function nodeRun(script, args, cwd) {
  const result = cp.spawnSync(process.execPath, [script, ...args], {
    cwd,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
    timeout: 300_000,
    killSignal: "SIGKILL",
  });
  return { status: result.status, output: (result.stdout || "") + (result.stderr || "") };
}

function buildBaseline(root) {
  const repo = path.join(root, "baseline repository with spaces");
  fs.mkdirSync(repo, { recursive: true });
  git(["init", "-q", "-b", "negative-fixture"], repo);
  fs.mkdirSync(path.join(repo, ".git/objects/info"), { recursive: true });
  fs.writeFileSync(
    path.join(repo, ".git/objects/info/alternates"),
    path.join(commonGit, "objects") + "\n",
  );
  const packageRoot = path.join(repo, packageRelative);
  fs.mkdirSync(path.dirname(packageRoot), { recursive: true });
  fs.cpSync(PACKAGE_ROOT, packageRoot, { recursive: true });
  const amendmentsDirectory = path.join(packageRoot, "authority/state/amendments");
  fs.rmSync(amendmentsDirectory, { recursive: true, force: true });
  fs.mkdirSync(amendmentsDirectory, { recursive: true });
  const liveRows =
    parseToml(fs.readFileSync(path.join(PACKAGE_ROOT, "provenance/live-source-lock.toml"), "utf8"))
      .source || [];
  const dispositionRows =
    parseToml(
      fs.readFileSync(path.join(PACKAGE_ROOT, "catalogs/legacy-arch-disposition.toml"), "utf8"),
    ).entry || [];
  const externalInputs = new Set([
    ...liveRows.map((row) => row.path),
    ...dispositionRows.flatMap((row) => row.replacement_sources || []),
  ]);
  for (const relative of [...externalInputs].sort()) {
    if (relative.startsWith(packageRelative + "/")) continue;
    const source = confinedFile(sourceRepo, relative, "negative-control external fixture input");
    const target = path.join(repo, relative);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(source, target);
  }
  const fixtureAuthority = loadAuthority(packageRoot);
  writeGenerated(fixtureAuthority, packageRoot);
  const digest = computeAuthorityDigest(packageRoot);
  fs.writeFileSync(
    path.join(packageRoot, "authority/state/authority-lock.toml"),
    [
      "schema = 2",
      'baseline_authority_sha256 = "' + digest + '"',
      'current_authority_sha256 = "' + digest + '"',
      "generation = 0",
      'last_amendment_id = ""',
      'last_amendment_sha256 = ""',
      "",
    ].join("\n"),
  );
  git(["config", "user.name", "Rev11 Negative Controls"], repo);
  git(["config", "user.email", "rev11-negative@example.invalid"], repo);
  git(["add", "--all"], repo);
  git(["commit", "-q", "-m", "test: fixture baseline"], repo);
  git(["branch", "program/architecture-lock", "HEAD"], repo);
  return repo;
}

function blockFor(text, id) {
  const blocks = [...text.matchAll(/^\[\[node\]\]\n[\s\S]*?(?=^\[\[node\]\]|$(?![\s\S]))/gm)];
  const found = blocks.find((match) =>
    new RegExp("^id = " + JSON.stringify(id) + "$", "m").test(match[0]),
  );
  if (!found) throw new Error("fixture cannot find node " + id);
  return found[0];
}

function moduleFor(packageRoot, id) {
  const directory = path.join(packageRoot, "authority/dag");
  for (const name of fs.readdirSync(directory).sort()) {
    const file = path.join(directory, name);
    if (new RegExp("^id = " + JSON.stringify(id) + "$", "m").test(fs.readFileSync(file, "utf8")))
      return file;
  }
  throw new Error("fixture cannot find module for " + id);
}

function field(block, name, value) {
  const expression = new RegExp("^" + name + " = .*?$", "m");
  if (!expression.test(block)) throw new Error("fixture cannot find field " + name);
  return block.replace(expression, name + " = " + value);
}

function mutateNode(packageRoot, id, mutation) {
  const file = moduleFor(packageRoot, id);
  const text = fs.readFileSync(file, "utf8");
  const block = blockFor(text, id);
  const changed = mutation(block);
  if (changed === block) throw new Error("fixture mutation made no change for " + id);
  fs.writeFileSync(file, text.replace(block, changed));
}

function artifact(body) {
  const prefix = body.endsWith("\n") ? body : body + "\n";
  return prefix + 'payload_sha256 = "' + digestPayload(prefix) + '"\n';
}

function runtimeFile(root, directory, name, content) {
  const file = path.join(root, directory, name);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, content);
}

function identity(repo) {
  const sha = git(["rev-parse", "HEAD"], repo);
  return {
    ref: "refs/heads/" + git(["branch", "--show-current"], repo),
    sha,
    tree: git(["show", "-s", "--format=%T", sha], repo),
    worktree: fs.realpathSync(repo),
  };
}

function admitArgs(id, runtimeRoot, candidateRef) {
  const node = sourceAuthority.nodes.find((row) => row.id === id);
  const reviewers = profileLenses
    .get(node.review_profile)
    .flatMap((lens, index) => ["--reviewer", lens + "=negative-reviewer-" + index]);
  return [
    "admit",
    id,
    "--holder",
    "negative-holder",
    "--candidate-ref",
    candidateRef,
    "--gate-runner",
    "negative-gate-runner",
    ...reviewers,
    "--runtime-root",
    runtimeRoot,
  ];
}

function applyCase(row, packageRoot, repo, runtimeRoot, scratchRoot) {
  const validator = path.join(packageRoot, "tools/validate-program-dag.mjs");
  const cli = path.join(packageRoot, "tools/programctl.mjs");
  const strict = () => nodeRun(validator, ["--strict"], repo);
  const phase = () => nodeRun(cli, ["phase", "--runtime-root", runtimeRoot], repo);
  const candidate = identity(repo);
  switch (row.mutation) {
    case "duplicate_node": {
      const file = moduleFor(packageRoot, "ORC0");
      fs.appendFileSync(file, "\n" + blockFor(fs.readFileSync(file, "utf8"), "ORC0"));
      return strict();
    }
    case "duplicate_domain":
      mutateNode(packageRoot, "ORC0", (block) =>
        field(block, "conflict_domains", '["program_authority", "program_authority"]'),
      );
      return strict();
    case "missing_predecessor":
      mutateNode(packageRoot, "ORC0", (block) =>
        field(block, "predecessors", '["C1", "J1", "MISSING"]'),
      );
      return strict();
    case "cycle":
      mutateNode(packageRoot, "A0", (block) => field(block, "predecessors", '["A1"]'));
      return strict();
    case "missing_charter":
      fs.rmSync(
        path.join(packageRoot, sourceAuthority.nodes.find((node) => node.id === "ORC0").charter),
      );
      return strict();
    case "charter_predecessor": {
      const file = path.join(
        packageRoot,
        sourceAuthority.nodes.find((node) => node.id === "ORC0").charter,
      );
      fs.writeFileSync(
        file,
        fs.readFileSync(file, "utf8").replace("predecessors=C1,J1", "predecessors=C1"),
      );
      return strict();
    }
    case "stale_projection":
      fs.appendFileSync(
        path.join(packageRoot, "program-dag.toml"),
        "# negative stale projection\n",
      );
      return strict();
    case "module_traversal": {
      const file = path.join(packageRoot, "authority/root.toml");
      fs.writeFileSync(
        file,
        fs
          .readFileSync(file, "utf8")
          .replace('"dag/compiler-compiler-bridge.toml"', '"../outside.toml"'),
      );
      return strict();
    }
    case "module_symlink": {
      const file = path.join(packageRoot, "authority/dag/compiler-compiler-bridge.toml");
      const outside = path.join(scratchRoot, "outside-module.toml");
      fs.writeFileSync(outside, fs.readFileSync(file));
      fs.rmSync(file);
      fs.symlinkSync(outside, file);
      return strict();
    }
    case "generated_authority": {
      const generated = path.join(packageRoot, "authority/dag/generated-cycle.toml");
      fs.copyFileSync(path.join(packageRoot, "program-dag.toml"), generated);
      const root = path.join(packageRoot, "authority/root.toml");
      fs.writeFileSync(
        root,
        fs
          .readFileSync(root, "utf8")
          .replace(/^modules = \[/m, 'modules = ["dag/generated-cycle.toml",'),
      );
      return strict();
    }
    case "prototype_table":
      fs.appendFileSync(
        path.join(packageRoot, "authority/state/activation.toml"),
        '[__proto__]\nnegative_pollution = "yes"\n',
      );
      return strict();
    case "windows_charter_path":
      mutateNode(packageRoot, "ORC0", (block) => field(block, "charter", '"C:\\\\escape.md"'));
      return strict();
    case "semantic_convergence":
      mutateNode(packageRoot, "CCA2", (block) => field(block, "max_production_loc", "301"));
      return strict();
    case "synthetic_split_class":
      mutateNode(packageRoot, "ORC0", (block) => field(block, "class", '"proposal-subblock"'));
      return strict();
    case "product_before_br0":
      mutateNode(packageRoot, "SCP7", (block) => field(block, "predecessors", "[]"));
      return strict();
    case "malformed_receipt":
      runtimeFile(runtimeRoot, "receipts", "X.toml", "this is not = [toml\n");
      return phase();
    case "forged_receipt":
      runtimeFile(
        runtimeRoot,
        "receipts",
        "ORC0.toml",
        fs
          .readFileSync(
            path.join(packageRoot, "templates/acceptance-receipt.template.toml"),
            "utf8",
          )
          .replace('node_id = "REQUIRED"', 'node_id = "ORC0"'),
      );
      return phase();
    case "forged_authorization": {
      const body = [
        "schema = 2",
        'type = "external-authorization"',
        'authorization = "maintainer_unified_v2_activation"',
        'node_id = "ORC0"',
        'candidate_sha = "' + candidate.sha + '"',
        'candidate_tree = "' + candidate.tree + '"',
        'authority_sha256 = "' + computeAuthorityDigest(packageRoot) + '"',
        'granted_by = "attacker"',
        'ratification_path = "authority/state/ratification-receipts/attacker.txt"',
        'ratification_receipt_sha256 = "' + "0".repeat(64) + '"',
        'expires_at = "never"',
        'grant_mode = "MAINTAINER_DIRECTIVE_FINALIZED_CANDIDATE"',
        'directive_scope = "unified-v2-orc0-activation-only"',
        "",
      ].join("\n");
      runtimeFile(
        runtimeRoot,
        "external",
        "ORC0--maintainer_unified_v2_activation.toml",
        artifact(body),
      );
      return phase();
    }
    case "forged_gate":
      runtimeFile(
        runtimeRoot,
        "gates",
        "NEG-GATE.toml",
        artifact('schema = 2\ntype = "gate-evidence"\nevidence_id = "NEG-GATE"\n'),
      );
      return phase();
    case "forged_review":
      runtimeFile(
        runtimeRoot,
        "reviews",
        "NEG-REVIEW.toml",
        artifact('schema = 2\ntype = "review-evidence"\nevidence_id = "NEG-REVIEW"\n'),
      );
      return phase();
    case "forged_lease": {
      const body = [
        "schema = 2",
        'type = "admission-lease"',
        'lease_id = "NEG-LEASE"',
        'node_id = "ORC0"',
        'holder = "attacker"',
        'base_sha = "' + candidate.sha + '"',
        'base_tree = "' + candidate.tree + '"',
        'candidate_ref = "' + candidate.ref + '"',
        'candidate_sha = "' + candidate.sha + '"',
        'candidate_tree = "' + candidate.tree + '"',
        'candidate_worktree = "' + candidate.worktree + '"',
        'authority_sha256 = "' + computeAuthorityDigest(packageRoot) + '"',
        'conflict_domains = ["forged_domain"]',
        'scope_path_roots = ["docs/arch/refactor/rev11"]',
        'scope_symbols = ["forged"]',
        'resource_class = "docs-light"',
        'gate_runner = "negative-gate-runner"',
        'gate_result_path = "gate-results/NEG-LEASE.txt"',
        'integration_gate_result_path = "gate-results/NEG-LEASE--integration.txt"',
        'reviewer_assignments = ["wire-public=negative-reviewer-0", "compatibility=negative-reviewer-1", "adversarial=negative-reviewer-2"]',
        'review_report_paths = ["wire-public=review-reports/NEG-LEASE--wire-public.md", "compatibility=review-reports/NEG-LEASE--compatibility.md", "adversarial=review-reports/NEG-LEASE--adversarial.md"]',
        'renewed_from = ""',
        'acquired_at = "' + new Date(Date.now() - 1000).toISOString() + '"',
        'expires_at = "' + new Date(Date.now() + 60000).toISOString() + '"',
        "",
      ].join("\n");
      runtimeFile(runtimeRoot, "leases", "NEG-LEASE.toml", artifact(body));
      return phase();
    }
    case "runtime_symlink": {
      fs.mkdirSync(runtimeRoot, { recursive: true });
      const outside = path.join(scratchRoot, "outside-receipts");
      fs.mkdirSync(outside, { recursive: true });
      fs.symlinkSync(outside, path.join(runtimeRoot, "receipts"));
      return phase();
    }
    case "forged_amendment": {
      const file = path.join(packageRoot, "authority/state/amendments/AMD-FORGED.toml");
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(
        file,
        artifact('schema = 2\ntype = "authority-amendment"\namendment_id = "AMD-FORGED"\n'),
      );
      return strict();
    }
    case "partial_activation": {
      const file = path.join(packageRoot, "authority/root.toml");
      fs.writeFileSync(
        file,
        fs.readFileSync(file, "utf8").replace('state = "DORMANT"', 'state = "ACTIVE"'),
      );
      return strict();
    }
    case "missing_external":
      fs.writeFileSync(
        path.join(packageRoot, "authority/state/external-authorizations.toml"),
        "schema = 2\nauthorization = []\n",
      );
      return strict();
    case "non_ready_admit":
      return nodeRun(cli, admitArgs("J2", runtimeRoot, candidate.ref), repo);
    default:
      throw new Error("unknown negative mutation " + row.mutation);
  }
}

if (new Set(cases.map((row) => row.expect)).size !== cases.length)
  throw new Error("each negative mutation must have a unique expected diagnostic");
const scratchRoot = fs.mkdtempSync(path.join(os.tmpdir(), "rev11 unified negative controls "));
try {
  const baseline = buildBaseline(scratchRoot);
  let passed = 0;
  for (const row of cases) {
    const repo = path.join(scratchRoot, "case " + row.id);
    git(["worktree", "add", "--quiet", "-b", "negative-" + row.id, repo, "HEAD"], baseline);
    let result;
    try {
      result = applyCase(
        row,
        path.join(repo, packageRelative),
        repo,
        path.join(scratchRoot, "runtime " + row.id),
        scratchRoot,
      );
    } finally {
      git(["worktree", "remove", "--force", repo], baseline);
    }
    if (result.status === 0 || !result.output.includes(row.expect)) {
      console.error(
        "FAIL " +
          row.id +
          ": status=" +
          result.status +
          "; expected " +
          JSON.stringify(row.expect) +
          " in " +
          JSON.stringify(result.output.slice(0, 1200)),
      );
      process.exit(1);
    }
    console.log("PASS " + row.id + ": " + row.expect);
    passed += 1;
  }
  console.log("validate-negative-controls: PASS cases=" + passed + " mode=isolated-git-cli");
} finally {
  fs.rmSync(scratchRoot, { recursive: true, force: true });
}
