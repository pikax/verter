#!/usr/bin/env node
// Program-state validator. Each check cites its source in the program tree.
// Must pass after every transition and before a block starts, enters review,
// is recommended for acceptance, or is accepted.
//
//   node scripts/validate-program-state.mjs \
//     --dag <program-dag.toml> --state <program-state.toml> --mode template|live
//
// Exit: 0 pass, 1 validation failure (one violation per line), 2 usage /
// unreadable input. No deps beyond node:fs / path / process. Unknown TOML
// is a loud failure, never a silent skip.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { basename, dirname, isAbsolute, join, resolve as resolvePath } from "node:path";
import process from "node:process";

// Minimal strict TOML reader.
//
// Supported shapes (the full set used by program-dag.toml and
// templates/program-state.template.toml): full-line comments, `[table]`,
// `[[array-of-tables]]`, and `key = value` where value is a basic
// double-quoted string (no escapes), a single-line array of basic strings,
// an integer, or a boolean. A trailing `# comment` after a value is allowed.
// Everything else fails loudly with the file/line.

class TomlError extends Error {}

function parseToml(text, label) {
  const root = Object.create(null);
  let current = root; // table currently receiving keys
  const lines = text.split(/\r?\n/);
  const fail = (lineNo, msg) => {
    throw new TomlError(`${label}:${lineNo}: unparseable TOML — ${msg}`);
  };

  const parseValue = (raw, lineNo) => {
    const s = raw.trim();
    if (s.startsWith('"')) {
      // Basic string, no escape support: fail loudly if a backslash appears
      // before the closing quote rather than mis-reading it.
      const end = s.indexOf('"', 1);
      if (end === -1) fail(lineNo, "unterminated string");
      const body = s.slice(1, end);
      if (body.includes("\\")) fail(lineNo, "escape sequences are not supported");
      const rest = s.slice(end + 1).trim();
      if (rest !== "") {
        if (!rest.startsWith("#")) {
          fail(lineNo, `trailing content after string: ${JSON.stringify(rest)}`);
        }
        // A double-quote inside the trailing "comment" is indistinguishable from
        // an unbalanced/ambiguous string (`"ACT"#IVE"`, `""#REQUIRED_X"`): the
        // reader closed at the FIRST inner quote and would otherwise silently
        // mis-read the value (and bypass the live-mode REQUIRED_ scan). Loud
        // failure, per this file's header promise.
        if (rest.includes('"')) {
          fail(
            lineNo,
            `trailing comment after string contains a double-quote — ambiguous/unbalanced quoting: ${JSON.stringify(rest)}`,
          );
        }
      }
      return body;
    }
    if (s.startsWith("[")) {
      // Single-line array of basic strings (e.g. predecessors = ["A0", "A1"]).
      const end = s.lastIndexOf("]");
      if (end === -1) fail(lineNo, "unterminated array (multi-line arrays unsupported)");
      const rest = s.slice(end + 1).trim();
      if (rest !== "") {
        if (!rest.startsWith("#")) {
          fail(lineNo, `trailing content after array: ${JSON.stringify(rest)}`);
        }
        if (rest.includes('"')) {
          fail(
            lineNo,
            `trailing comment after array contains a double-quote — ambiguous/unbalanced quoting: ${JSON.stringify(rest)}`,
          );
        }
      }
      const inner = s.slice(1, end).trim();
      if (inner === "") return [];
      return inner.split(",").map((piece) => {
        const p = piece.trim();
        if (p === "") fail(lineNo, "empty array element");
        if (!(p.startsWith('"') && p.endsWith('"') && p.length >= 2)) {
          fail(lineNo, `non-string array element: ${JSON.stringify(p)}`);
        }
        const body = p.slice(1, -1);
        if (body.includes('"') || body.includes("\\")) {
          fail(lineNo, `unsupported array element: ${JSON.stringify(p)}`);
        }
        return body;
      });
    }
    // Bare scalar: strip a trailing comment, then integer or boolean only.
    const bare = s.split("#")[0].trim();
    if (bare === "true") return true;
    if (bare === "false") return false;
    if (/^[+-]?\d+$/.test(bare)) {
      // TOML forbids leading zeros on integers (`007` is invalid TOML, not 7).
      // Silently reading it as 7 would contradict this file's loud-failure
      // promise, so reject it here.
      if (/^[+-]?0\d/.test(bare)) {
        fail(lineNo, `integer with leading zero(s) is not valid TOML: ${JSON.stringify(bare)}`);
      }
      return Number.parseInt(bare, 10);
    }
    fail(lineNo, `unsupported value: ${JSON.stringify(s)}`);
  };

  for (let i = 0; i < lines.length; i++) {
    const lineNo = i + 1;
    const line = lines[i].trim();
    if (line === "" || line.startsWith("#")) continue;

    let m;
    if ((m = /^\[\[([A-Za-z0-9_-]+)\]\]$/.exec(line))) {
      const name = m[1];
      if (!Array.isArray(root[name])) {
        if (name in root) fail(lineNo, `[[${name}]] conflicts with existing key`);
        root[name] = [];
      }
      current = Object.create(null);
      root[name].push(current);
      continue;
    }
    if ((m = /^\[([A-Za-z0-9_-]+)\]$/.exec(line))) {
      const name = m[1];
      if (name in root) fail(lineNo, `duplicate table [${name}]`);
      current = Object.create(null);
      root[name] = current;
      continue;
    }
    if ((m = /^([A-Za-z0-9_-]+)\s*=\s*(.+)$/.exec(line))) {
      const key = m[1];
      if (key in current) fail(lineNo, `duplicate key ${JSON.stringify(key)}`);
      current[key] = parseValue(m[2], lineNo);
      continue;
    }
    fail(lineNo, `unrecognized line: ${JSON.stringify(line)}`);
  }
  return root;
}

function resolveExistingDir(raw, statePath) {
  const candidates = [];
  if (isAbsolute(raw)) {
    candidates.push(raw);
  } else {
    candidates.push(resolvePath(raw));
    candidates.push(resolvePath(dirname(statePath), raw));
  }
  for (const candidate of candidates) {
    try {
      if (statSync(candidate).isDirectory()) return candidate;
    } catch {
      // missing or not a directory
    }
  }
  return null;
}

// Named candidates, searched in this order, plus one nested-level fallback
// (`<root>/<id>/*/landing-record.md`) for artifacts one directory deeper
// than the block dir (e.g. a reopen subfolder). Only WHICH file gets hashed
// is decided here — the digest comparison at the call site still has to
// match, so a wrong pick fails exactly like a missing one. Returns:
//   { path }            — a single artifact resolved
//   { ambiguous: [...] } — more than one nested match; caller must fail
//                          closed rather than silently pick one
//   null                 — nothing resolved under this root
function resolveEvidenceArtifact(root, id) {
  const named = [
    join(root, id, "landing-record.md"),
    join(root, id, `${id}-exact-candidate-record.md`),
    join(root, id, `${id}-summary.md`),
    join(root, id, "landing-equivalence.md"),
    // Root-level sibling to the block dir, not nested inside it — some
    // blocks' summary lives at <root>/<id>-summary.md rather than
    // <root>/<id>/<id>-summary.md.
    join(root, `${id}-summary.md`),
  ];
  for (const candidate of named) {
    if (!existsSync(candidate)) continue;
    try {
      if (statSync(candidate).isFile()) return { path: candidate };
    } catch {
      // skip
    }
  }
  const blockDir = join(root, id);
  let entries;
  try {
    entries = readdirSync(blockDir, { withFileTypes: true });
  } catch {
    entries = [];
  }
  const nested = [];
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const candidate = join(blockDir, entry.name, "landing-record.md");
    try {
      if (statSync(candidate).isFile()) nested.push(candidate);
    } catch {
      // skip
    }
  }
  nested.sort();
  if (nested.length > 1) return { ambiguous: nested };
  if (nested.length === 1) return { path: nested[0] };
  return null;
}

// Rule constants, each derived from the program tree.

// templates/program-state.template.toml:44-45 — the declared block-status enum.
const BLOCK_STATUS_ENUM = new Set([
  "LOCKED",
  "READY",
  "IN_PROGRESS",
  "REVIEW",
  "ACCEPTANCE_RECOMMENDED",
  "ACCEPTED",
  "BLOCKED",
  "RESCOPE_REQUIRED",
  "ABORTED",
  "SUPERSEDED",
  "PRIVATE_CHECKPOINT",
]);

// templates/program-state.template.toml:46 — the declared review-result enum.
const REVIEW_ENUM = new Set([
  "NOT_REQUIRED",
  "PENDING",
  "PASS",
  "BLOCKING",
  "NOT_PROVEN",
  "INVALIDATED",
]);
const REVIEW_FIELDS = ["conformance_review", "architecture_review", "adversarial_review"];

// Begun statuses require every direct predecessor ACCEPTED. READY is begun:
// a stackless READY with an unaccepted predecessor has begun illegally.
// The stacked exception covers READY/IN_PROGRESS/REVIEW only when the
// ledger can establish the stack (shared snapshot digest, same stack_id,
// predecessor begun, predecessor layer strictly below). It does not cover
// acceptance-recommendation or acceptance.
//
// PRIVATE_CHECKPOINT is begun (reviewed work) but not in the stacked
// exception: a checkpoint is legal only over ACCEPTED predecessors. This
// validator does not model a stack-window relaxation, so it fails closed.
//
// Not begun (intentional):
//   - ABORTED / SUPERSEDED — terminal; nothing left to sequence
//   - BLOCKED / RESCOPE_REQUIRED — paused from begun work. Treating them
//     as begun would reject a legal pause. Re-entering a begun status
//     re-runs the full sequencing gate. A block minted directly into
//     these states is a recorded limit this check does not catch.
const BEGUN_STATUSES = new Set([
  "READY",
  "IN_PROGRESS",
  "REVIEW",
  "ACCEPTANCE_RECOMMENDED",
  "ACCEPTED",
  "PRIVATE_CHECKPOINT",
]);
const STACK_EXCEPTION_STATUSES = new Set(["READY", "IN_PROGRESS", "REVIEW"]);

const SHA_RE = /^[0-9a-f]{40}$/; // full lowercase git object id
const DIGEST_RE = /^[0-9a-f]{64}$/; // lowercase SHA-256

// Live-mode git identity verification.
//
// base_sha/candidate_sha/accepted_sha/candidate_tree/accepted_tree are shape-
// checked above (SHA_RE) but that never asked git whether a commit exists or
// carries the stated tree — a well-formed-looking, entirely fabricated or
// dangling identity passed. This section is that check: it consults the real
// repository so a self-declared identity is never treated as evidence
// (CLAUDE.md "Verification Must Prove Execution"). Template mode never
// reaches this — a template ledger's identity fields are placeholders and
// carry no promise of naming real objects.
//
// One batched `git cat-file --batch-check` pass covers existence (every
// well-formed sha) and tree-pairing (every well-formed tree field, resolved
// via its paired sha's `^{tree}`) in a single shell-out, never one per field.
// Reachability-from-tip is one `git rev-list <tip>` pass, checked in-memory
// for every ACCEPTED block's accepted_sha. Only the base_sha-ancestor-of-
// accepted_sha check (different target commit per block) shells out once per
// ACCEPTED block — bounded by the count of accepted blocks, not by field
// count across the whole ledger.

function runGit(args, cwd, input) {
  try {
    return spawnSync("git", args, { cwd, input, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  } catch (err) {
    return { error: err, status: null, stdout: "", stderr: "" };
  }
}

function gitFailureReason(res) {
  if (res.error) return res.error.message;
  const stderr = (res.stderr ?? "").trim();
  return stderr !== "" ? stderr : `git exited with status ${res.status}`;
}

// One batched pass over every distinct object spec (a bare 40-char sha, or
// `<sha>^{tree}`) collected across every block/field. Returns a Map spec ->
// {oid, type} for a resolved object, spec -> null for "missing", or `null`
// (not a Map) if the batch-check invocation itself failed.
function batchCatFileCheck(cwd, specs) {
  const map = new Map();
  if (specs.length === 0) return map;
  const res = runGit(
    ["cat-file", "--batch-check=%(objectname) %(objecttype)"],
    cwd,
    specs.map((s) => `${s}\n`).join(""),
  );
  if (res.status !== 0) return null;
  const lines = (res.stdout ?? "").split("\n");
  for (let i = 0; i < specs.length; i++) {
    const m = /^([0-9a-f]{40}) (\S+)$/.exec(lines[i] ?? "");
    // A missing/ambiguous object echoes the literal input token followed by
    // "missing"/"ambiguous" rather than a resolved oid+type — a 40-hex input
    // spec (a bare sha, as opposed to a `<sha>^{tree}` derivation) would
    // otherwise false-match the regex above as if it were a resolved commit.
    map.set(specs[i], m && m[2] !== "missing" && m[2] !== "ambiguous" ? { oid: m[1], type: m[2] } : null);
  }
  return map;
}

function verifyLiveGitIdentities(stateById, v) {
  const cwd = process.cwd();
  const probe = runGit(["rev-parse", "--is-inside-work-tree"], cwd);
  if (probe.status !== 0 || (probe.stdout ?? "").trim() !== "true") {
    v(
      `live mode requires a git repository to verify base_sha/candidate_sha/accepted_sha/candidate_tree/accepted_tree against real git objects, but git is unavailable at ${cwd}: ${gitFailureReason(probe)} — this is a loud setup failure, never a silent skip of identity verification`,
    );
    return;
  }

  const SHA_FIELDS = ["base_sha", "candidate_sha", "accepted_sha"];
  const TREE_PAIRS = [
    { treeField: "candidate_tree", shaField: "candidate_sha" },
    { treeField: "accepted_tree", shaField: "accepted_sha" },
  ];
  const wellFormed = (b, f) => typeof b[f] === "string" && SHA_RE.test(b[f]);

  const specs = new Set();
  for (const [, b] of stateById) {
    for (const f of SHA_FIELDS) if (wellFormed(b, f)) specs.add(b[f]);
    for (const { treeField, shaField } of TREE_PAIRS) {
      if (wellFormed(b, treeField)) specs.add(b[treeField]);
      if (wellFormed(b, treeField) && wellFormed(b, shaField)) specs.add(`${b[shaField]}^{tree}`);
    }
  }

  const resolved = batchCatFileCheck(cwd, [...specs]);
  if (resolved === null) {
    v(
      `live mode git cat-file --batch-check failed while verifying identity fields against ${specs.size} object spec(s) — cannot verify base_sha/candidate_sha/accepted_sha/candidate_tree/accepted_tree`,
    );
    return;
  }

  // Check 1: every well-formed base_sha/candidate_sha/accepted_sha must
  // resolve to an existing commit object.
  for (const [id, b] of stateById) {
    for (const f of SHA_FIELDS) {
      if (!wellFormed(b, f)) continue;
      const info = resolved.get(b[f]);
      if (!info || info.type !== "commit") {
        v(
          `state block ${id} field ${f} = ${b[f]} does not resolve to an existing git commit object (git reports: ${info ? `object exists but is type ${info.type}` : "missing"})`,
        );
      }
    }
  }

  // Check 2: candidate_tree/accepted_tree must be EXACTLY the tree of the
  // commit recorded beside it (its paired candidate_sha/accepted_sha).
  for (const [id, b] of stateById) {
    for (const { treeField, shaField } of TREE_PAIRS) {
      if (!wellFormed(b, treeField)) continue;
      if (!wellFormed(b, shaField)) {
        v(
          `state block ${id} field ${treeField} = ${b[treeField]} cannot be verified — its paired ${shaField} is not a well-formed git object id`,
        );
        continue;
      }
      const commitInfo = resolved.get(b[shaField]);
      if (!commitInfo || commitInfo.type !== "commit") continue; // already reported by check 1
      const derived = resolved.get(`${b[shaField]}^{tree}`);
      if (!derived) {
        v(
          `state block ${id} field ${treeField} = ${b[treeField]} could not be checked — git could not resolve ${b[shaField]}^{tree}`,
        );
        continue;
      }
      if (derived.oid !== b[treeField]) {
        v(
          `state block ${id} field ${treeField} = ${b[treeField]} is not the tree of ${shaField} ${b[shaField]} — git reports that commit's tree as ${derived.oid}`,
        );
      }
    }
  }

  // Checks 3 & 4 are ACCEPTED-only: accepted_sha must be genuinely landed
  // (reachable from the repository tip — never a dangling or since-rewritten
  // commit), and base_sha must be its ancestor.
  const tipRes = runGit(["rev-parse", "HEAD"], cwd);
  if (tipRes.status !== 0) {
    v(
      `live mode could not resolve the repository tip (git rev-parse HEAD) to verify accepted_sha landing: ${gitFailureReason(tipRes)}`,
    );
    return;
  }
  const tip = tipRes.stdout.trim();
  const revListRes = runGit(["rev-list", tip], cwd);
  if (revListRes.status !== 0) {
    v(
      `live mode could not enumerate commits reachable from the repository tip ${tip} (git rev-list) to verify accepted_sha landing: ${gitFailureReason(revListRes)}`,
    );
    return;
  }
  const reachableFromTip = new Set(revListRes.stdout.split("\n").filter((l) => l !== ""));
  reachableFromTip.add(tip);

  for (const [id, b] of stateById) {
    if (b.status !== "ACCEPTED") continue;
    if (wellFormed(b, "accepted_sha")) {
      const info = resolved.get(b.accepted_sha);
      if (info && info.type === "commit" && !reachableFromTip.has(b.accepted_sha)) {
        v(
          `state block ${id} is ACCEPTED with accepted_sha ${b.accepted_sha} but that commit is not reachable from the repository tip ${tip} — it is not genuinely landed (a dangling or since-rewritten commit is not sufficient evidence of acceptance)`,
        );
      }
    }
    if (wellFormed(b, "base_sha") && wellFormed(b, "accepted_sha")) {
      const baseInfo = resolved.get(b.base_sha);
      const accInfo = resolved.get(b.accepted_sha);
      if (baseInfo?.type === "commit" && accInfo?.type === "commit") {
        const anc = runGit(["merge-base", "--is-ancestor", b.base_sha, b.accepted_sha], cwd);
        if (anc.status !== 0 && anc.status !== 1) {
          v(
            `state block ${id} base_sha ${b.base_sha} ancestry against accepted_sha ${b.accepted_sha} could not be checked: ${gitFailureReason(anc)}`,
          );
        } else if (anc.status === 1) {
          v(
            `state block ${id} is ACCEPTED but base_sha ${b.base_sha} is not an ancestor of accepted_sha ${b.accepted_sha} (git merge-base --is-ancestor)`,
          );
        }
      }
    }
  }
}

// Validation

function usageFail(msg) {
  process.stderr.write(
    `${msg}\nusage: node scripts/validate-program-state.mjs --dag <program-dag.toml> --state <program-state.toml> --mode template|live\n`,
  );
  process.exit(2);
}

function parseArgs(argv) {
  const opts = Object.create(null);
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (!["--dag", "--state", "--mode"].includes(flag)) usageFail(`unknown argument: ${flag}`);
    if (value === undefined) usageFail(`missing value for ${flag}`);
    opts[flag.slice(2)] = value;
  }
  if (!opts.dag || !opts.state || !opts.mode)
    usageFail("--dag, --state, and --mode are all required");
  if (opts.mode !== "template" && opts.mode !== "live") {
    usageFail(`--mode must be "template" or "live", got ${JSON.stringify(opts.mode)}`);
  }
  return opts;
}

function loadFile(path, what) {
  try {
    return readFileSync(path, "utf8");
  } catch (err) {
    usageFail(`cannot read ${what} file ${path}: ${err.message}`);
  }
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const violations = [];
  const v = (msg) => violations.push(msg);

  let dag;
  let state;
  try {
    dag = parseToml(loadFile(opts.dag, "DAG"), opts.dag);
    state = parseToml(loadFile(opts.state, "state"), opts.state);
  } catch (err) {
    if (err instanceof TomlError) {
      process.stderr.write(`VIOLATION: ${err.message}\n`);
      process.stderr.write("FAIL: 0 checks completed — input could not be parsed\n");
      process.exit(1);
    }
    throw err;
  }

  // -- State header
  // templates/program-state.template.toml:5-7 — the state file carries
  // top-level `schema`, `revision`, `status`.
  for (const key of ["schema", "revision", "status"]) {
    if (!(key in state)) v(`state is missing required top-level key ${JSON.stringify(key)}`);
  }
  // program-dag.toml:1-2 — the DAG declares schema/revision; the state must
  // describe the same program.
  for (const key of ["schema", "revision"]) {
    if (key in state && key in dag && state[key] !== dag[key]) {
      v(`state ${key} (${state[key]}) does not match DAG ${key} (${dag[key]})`);
    }
  }
  if (opts.mode === "live" && state.status !== "ACTIVE") {
    // ORCHESTRATOR.md:83 — the live ledger is the template copied with
    // `status = "ACTIVE"` and every A0-required field resolved. Any other
    // top-level status (TEMPLATE included) is not a live ledger.
    v(
      `live state top-level status is ${state.status === undefined ? "missing" : JSON.stringify(state.status)} (ORCHESTRATOR.md:83 requires the live ledger to carry status = "ACTIVE")`,
    );
  }
  // program_dag_digest binds the ledger to the exact DAG file it claims to track.
  // In live mode the binding is REQUIRED: an empty or malformed value would
  // silently disable both this comparison and the placeholder scan, so it is a
  // violation, not a skip.
  if (
    opts.mode === "live" &&
    !(typeof state.program_dag_digest === "string" && DIGEST_RE.test(state.program_dag_digest))
  ) {
    v(
      `live state program_dag_digest ${JSON.stringify(state.program_dag_digest ?? "")} is not a resolved 64-char lowercase SHA-256 — an empty/malformed value silently disables the ledger-to-DAG binding`,
    );
  }
  // When the field carries a resolved digest (not empty, not a template
  // placeholder), it must equal the SHA-256 of the DAG file actually validated
  // against — otherwise the ledger and the DAG have silently diverged.
  if (typeof state.program_dag_digest === "string" && DIGEST_RE.test(state.program_dag_digest)) {
    const actual = createHash("sha256").update(readFileSync(opts.dag)).digest("hex");
    if (actual !== state.program_dag_digest) {
      v(
        `state program_dag_digest ${state.program_dag_digest} does not match the SHA-256 of the DAG file ${opts.dag} (${actual})`,
      );
    }
  }

  // -- DAG structure
  const dagBlocks = Array.isArray(dag.block) ? dag.block : [];
  const dagIds = [];
  const dagById = new Map();
  for (const b of dagBlocks) {
    if (typeof b.id !== "string" || b.id === "") {
      v("DAG contains a [[block]] without a string id");
      continue;
    }
    if (dagById.has(b.id)) v(`DAG declares duplicate block id ${JSON.stringify(b.id)}`);
    dagIds.push(b.id);
    dagById.set(b.id, b);
  }

  // program-dag.toml:6 — "`predecessors` are acceptance dependencies"; every
  // entry must name a real block.
  for (const b of dagById.values()) {
    const preds = Array.isArray(b.predecessors) ? b.predecessors : null;
    if (preds === null) {
      v(`DAG block ${b.id} has no predecessors array`);
      continue;
    }
    for (const p of preds) {
      if (!dagById.has(p)) v(`DAG block ${b.id} names unknown predecessor ${JSON.stringify(p)}`);
    }
    // program-dag.toml:309 — conditional predecessors (L4/L3) must also be real blocks.
    for (const p of b.conditional_predecessor_if_opened ?? []) {
      if (!dagById.has(p)) {
        v(`DAG block ${b.id} names unknown conditional predecessor ${JSON.stringify(p)}`);
      }
    }
  }

  // Cycle detection over predecessor edges (a cyclic acceptance dependency can
  // never satisfy governance.md:6, so it is rejected structurally).
  {
    const color = new Map(); // 0 = visiting, 1 = done
    const visit = (id, stack) => {
      if (color.get(id) === 1) return;
      if (color.get(id) === 0) {
        v(`DAG contains a predecessor cycle through ${[...stack, id].join(" -> ")}`);
        return;
      }
      color.set(id, 0);
      for (const p of dagById.get(id)?.predecessors ?? []) {
        if (dagById.has(p)) visit(p, [...stack, id]);
      }
      color.set(id, 1);
    };
    for (const id of dagIds) visit(id, []);
  }

  // Single root + reachability: the program has one entry block (the only
  // block with `predecessors = []` — see program-dag.toml:9-13), and every
  // block must be reachable from it via predecessor edges — an unreachable
  // block could never legally begin under governance.md:6. `dagRoots` is also
  // consumed by the entry-lock gate below: the root is derived STRUCTURALLY
  // from the DAG, never keyed on a block name.
  const dagRoots = dagIds.filter((id) => (dagById.get(id).predecessors ?? []).length === 0);
  {
    const roots = dagRoots;
    if (roots.length !== 1) {
      v(
        `DAG must have exactly one root block (predecessors = []); found ${roots.length}: [${roots.join(", ")}]`,
      );
    } else {
      const successors = new Map(dagIds.map((id) => [id, []]));
      for (const b of dagById.values()) {
        for (const p of b.predecessors ?? []) successors.get(p)?.push(b.id);
      }
      const seen = new Set([roots[0]]);
      const queue = [roots[0]];
      while (queue.length) {
        for (const s of successors.get(queue.shift()) ?? []) {
          if (!seen.has(s)) {
            seen.add(s);
            queue.push(s);
          }
        }
      }
      for (const id of dagIds) {
        if (!seen.has(id)) v(`DAG block ${id} is not reachable from root ${roots[0]}`);
      }
    }
  }

  // -- State blocks vs DAG
  const stateBlocks = Array.isArray(state.block) ? state.block : [];
  const stateById = new Map();
  for (const b of stateBlocks) {
    if (typeof b.id !== "string" || b.id === "") {
      v("state contains a [[block]] without a string id");
      continue;
    }
    if (stateById.has(b.id)) v(`state declares duplicate block id ${JSON.stringify(b.id)}`);
    stateById.set(b.id, b);
  }

  // The state's block id set must EXACTLY equal the DAG's — the ledger tracks
  // the whole program, no more, no less (governance.md:181: `program-state.toml`
  // is "the durable execution ledger" for the program the DAG defines).
  {
    const missing = dagIds.filter((id) => !stateById.has(id));
    const extra = [...stateById.keys()].filter((id) => !dagById.has(id));
    if (missing.length || extra.length) {
      v(
        `state block set does not equal DAG block set — missing from state: [${missing.join(", ")}]; in state but not in DAG: [${extra.join(", ")}]`,
      );
    }
  }

  // -- Per-block status
  for (const [id, b] of stateById) {
    if (typeof b.status !== "string") {
      v(`state block ${id} has no status`);
      continue;
    }
    // templates/program-state.template.toml:44-45 — closed status enum.
    if (!BLOCK_STATUS_ENUM.has(b.status)) {
      v(`state block ${id} has status ${JSON.stringify(b.status)} outside the declared enum`);
    }
    // templates/program-state.template.toml:46 — closed review enum.
    for (const field of REVIEW_FIELDS) {
      if (field in b && !REVIEW_ENUM.has(b[field])) {
        v(
          `state block ${id} has ${field} ${JSON.stringify(b[field])} outside the declared review enum`,
        );
      }
    }
  }

  // -- Sequencing invariant (governance.md:6, the core rule)
  // "no block may begin before every direct predecessor in program-dag.toml is
  // accepted, except contingent ... work ... in the same validated immutable
  // stack snapshot. Such work cannot be acceptance-recommended or accepted
  // until the predecessor lands."
  for (const [id, b] of stateById) {
    if (!BEGUN_STATUSES.has(b.status)) continue;
    const dagBlock = dagById.get(id);
    if (!dagBlock) continue; // already reported as extra

    // Fail closed on what this validator does not fully model:
    // (a) a PRIVATE_CHECKPOINT predecessor. contracts/stacked-prs.md:39,53 let a
    //     PRIVATE_CHECKPOINT predecessor satisfy sequencing only inside a
    //     validated stack window and only for the final acceptance block —
    //     conditions this validator cannot check (they live in the stack-window
    //     validator). Reject rather than silently pass or silently never-satisfy.
    for (const p of dagBlock.predecessors ?? []) {
      if (stateById.get(p)?.status === "PRIVATE_CHECKPOINT") {
        v(
          `block ${id} is ${b.status} with predecessor ${p} in PRIVATE_CHECKPOINT — a PRIVATE_CHECKPOINT predecessor satisfies sequencing only inside a validated stack window for the final acceptance block (contracts/stacked-prs.md), which this validator does not model — fail closed`,
        );
      }
    }
    // (b) an OPENED conditional predecessor (program-dag.toml:
    //     conditional_predecessor_if_opened — "If opened, it becomes an
    //     additional predecessor"). LOCKED = never opened (no dependency);
    //     ACCEPTED = opened and satisfied; anything else is an opened-but-not-
    //     accepted additional acceptance dependency, and no stacked path is
    //     modelled for conditional edges — fail closed.
    for (const cp of dagBlock.conditional_predecessor_if_opened ?? []) {
      const cpStatus = stateById.get(cp)?.status;
      if (cpStatus === undefined || cpStatus === "LOCKED" || cpStatus === "ACCEPTED") continue;
      v(
        `block ${id} is ${b.status} but conditional predecessor ${cp} is ${JSON.stringify(cpStatus)} — an opened conditional predecessor is an additional acceptance dependency (program-dag.toml) and this path is not modelled beyond LOCKED/ACCEPTED — fail closed`,
      );
    }

    const unaccepted = (dagBlock.predecessors ?? []).filter(
      (p) => stateById.get(p)?.status !== "ACCEPTED",
    );
    if (unaccepted.length === 0) continue;
    // A whitespace-only stack_id is EMPTY (it identifies nothing), never a
    // stack claim.
    const nonEmptyStackId = (s) => typeof s === "string" && s.trim() !== "";
    const stacked = nonEmptyStackId(b.stack_id);
    if (stacked && STACK_EXCEPTION_STATUSES.has(b.status)) {
      // The contingent stacked-work exception is GRANTED only when the stack it
      // claims can actually be established from the ledger (governance.md:6 —
      // "in the same validated immutable stack snapshot"):
      //   1. a bound snapshot: stack_snapshot_digest is a real SHA-256;
      //   2. every unaccepted predecessor is in the SAME stack (same non-empty
      //      stack_id);
      //   3. every unaccepted predecessor cites the SAME validated immutable
      //      snapshot (identical well-formed stack_snapshot_digest) — the
      //      snapshot the exception text is ABOUT;
      //   4. every unaccepted predecessor has itself BEGUN (a LOCKED,
      //      never-begun, ABORTED, or otherwise non-begun predecessor cannot be
      //      a lower layer of a live stack);
      //   5. every unaccepted predecessor is BELOW this block in that stack
      //      (predecessor.stack_layer < block.stack_layer).
      // Anything that cannot be established REJECTS the exception.
      const problems = [];
      if (
        !(typeof b.stack_snapshot_digest === "string" && DIGEST_RE.test(b.stack_snapshot_digest))
      ) {
        problems.push(
          `stack_snapshot_digest ${JSON.stringify(b.stack_snapshot_digest ?? "")} is not a 64-char lowercase SHA-256, so no validated immutable stack snapshot is bound`,
        );
      }
      if (!Number.isInteger(b.stack_layer)) {
        problems.push(`stack_layer ${JSON.stringify(b.stack_layer ?? "")} is not an integer`);
      }
      for (const p of unaccepted) {
        const ps = stateById.get(p);
        if (!ps || !nonEmptyStackId(ps.stack_id) || ps.stack_id !== b.stack_id) {
          problems.push(
            `unaccepted predecessor ${p} does not carry the same non-empty stack_id ${JSON.stringify(b.stack_id)}`,
          );
          continue;
        }
        if (
          !(
            typeof ps.stack_snapshot_digest === "string" &&
            DIGEST_RE.test(ps.stack_snapshot_digest) &&
            ps.stack_snapshot_digest === b.stack_snapshot_digest
          )
        ) {
          problems.push(
            `unaccepted predecessor ${p} stack_snapshot_digest ${JSON.stringify(ps.stack_snapshot_digest ?? "")} is not the same well-formed snapshot digest as block ${id} — not "the same validated immutable stack snapshot" (governance.md:6)`,
          );
        }
        if (!BEGUN_STATUSES.has(ps.status)) {
          problems.push(
            `unaccepted predecessor ${p} is ${JSON.stringify(ps.status ?? "")} — a predecessor that has not begun (or has terminated) cannot be a lower layer of the same validated stack snapshot`,
          );
        }
        if (
          !(
            Number.isInteger(ps.stack_layer) &&
            Number.isInteger(b.stack_layer) &&
            ps.stack_layer < b.stack_layer
          )
        ) {
          problems.push(
            `unaccepted predecessor ${p} stack_layer ${JSON.stringify(ps.stack_layer ?? "")} is not below block ${id} stack_layer ${JSON.stringify(b.stack_layer ?? "")}`,
          );
        }
      }
      if (problems.length === 0) continue; // exception properly established
      v(
        `sequencing violation (governance.md sequencing authority): block ${id} is ${b.status} with unaccepted direct predecessor(s) [${unaccepted.join(", ")}] and the contingent stacked-work exception is REJECTED — ${problems.join("; ")}`,
      );
      continue;
    }
    // Diagnostic precision: the acceptance-specific wording applies only to
    // the two acceptance statuses governance.md:6 names ("Such work cannot be
    // acceptance-recommended or accepted until the predecessor lands"); any
    // other status carrying a stack_id (e.g. PRIVATE_CHECKPOINT) is simply not
    // eligible for the contingent stacked-work exception.
    const stackedNote = !stacked
      ? " (no stack_id, so the contingent stacked-work exception does not apply)"
      : b.status === "ACCEPTANCE_RECOMMENDED" || b.status === "ACCEPTED"
        ? " (stacked work cannot be acceptance-recommended or accepted until the predecessor lands)"
        : ` (status ${b.status} is not eligible for the contingent stacked-work exception)`;
    v(
      `sequencing violation (governance.md sequencing authority): block ${id} is ${b.status} but direct predecessor(s) not ACCEPTED: [${unaccepted.join(", ")}]${stackedNote}`,
    );
  }

  // -- Status-dependent identity/review invariants
  // governance.md:181 mandates this validator pass "before a block ... enters
  // review, is recommended for acceptance, or is accepted"; governance.md §9
  // attaches approval to one exact candidate SHA and tree plus the evidence
  // digest. So the gated transitions carry status-dependent obligations:
  //   REVIEW or later, and
  //   PRIVATE_CHECKPOINT         — exact base/candidate identity and the
  //                                charter/context/evidence digests exist and
  //                                are well-formed;
  //   PRIVATE_CHECKPOINT         — additionally: the status is permitted ONLY
  //                                for a DAG block whose class is exactly
  //                                "foundational-private-checkpoint" (program-
  //                                dag.toml declares exactly one — D1), and
  //                                every review mandate is PASS (program.md §7:
  //                                a checkpoint "may receive checkpoint review
  //                                approval" — approval, not pending review).
  //                                accepted_sha/accepted_tree and maintainer
  //                                acceptance are NOT required: a checkpoint is
  //                                never merged or released independently
  //                                (program.md §7), so there is no accepted
  //                                landing identity to record;
  //   ACCEPTANCE_RECOMMENDED     — additionally, every mandatory review mandate
  //                                is PASS; a PENDING/BLOCKING/NOT_PROVEN/
  //                                INVALIDATED, missing, or empty mandate
  //                                rejects. NOT_REQUIRED is permitted ONLY for
  //                                architecture_review on a `subsystem`-class
  //                                block (governance.md §2.2 — "architecture
  //                                review added when authority/lifetime risk
  //                                warrants it"); every `foundational*` class
  //                                "Requires ... all three review mandates on
  //                                one exact candidate SHA/tree"
  //                                (governance.md:106; §9 "all three PASS",
  //                                governance.md:277);
  //   ACCEPTED                   — additionally, maintainer acceptance is
  //                                recorded, accepted_sha/accepted_tree are
  //                                non-empty and well-formed, and — when the
  //                                accepted identity DIVERGES from the reviewed
  //                                candidate identity — a repository-validated
  //                                landing-equivalence artifact is bound
  //                                (well-formed landing_equivalence_digest;
  //                                governance.md:283,
  //                                contracts/stacked-prs.md:140).
  {
    const EVIDENCE_BOUND = new Set([
      "REVIEW",
      "ACCEPTANCE_RECOMMENDED",
      "ACCEPTED",
      "PRIVATE_CHECKPOINT",
    ]);
    for (const [id, b] of stateById) {
      if (typeof b.status !== "string" || !EVIDENCE_BOUND.has(b.status)) continue;
      const requireSha = (field) => {
        if (!(typeof b[field] === "string" && SHA_RE.test(b[field]))) {
          v(
            `state block ${id} is ${b.status} but ${field} is not a non-empty 40-char lowercase git object id: ${JSON.stringify(b[field] ?? "")}`,
          );
        }
      };
      const requireDigest = (field) => {
        if (!(typeof b[field] === "string" && DIGEST_RE.test(b[field]))) {
          v(
            `state block ${id} is ${b.status} but ${field} is not a non-empty 64-char lowercase SHA-256: ${JSON.stringify(b[field] ?? "")}`,
          );
        }
      };
      requireSha("base_sha");
      requireSha("candidate_sha");
      requireSha("candidate_tree");
      requireDigest("charter_digest");
      requireDigest("context_packet_digest");
      requireDigest("evidence_digest");
      // Entry-lock binding for the program's ENTRY block. The DAG's single
      // root (the one block with `predecessors = []`) owns the entry lock —
      // "Completed entry lock" is the first required-evidence item of the
      // entry charter (charters/A0.md) and the contracts/baseline-lock.md §2
      // record — recorded on its ledger row as entry_lock_digest. The root is
      // derived STRUCTURALLY from the DAG (never a hardcoded block name), and
      // the digest is REQUIRED at every gated transition of that block
      // (REVIEW, ACCEPTANCE_RECOMMENDED, ACCEPTED): without this gate the
      // ledger could carry the entry block through review to acceptance with
      // the field absent or emptied, never binding the charter-named central
      // artifact. A zero-root or multi-root DAG is already reported by the
      // single-root check above; no root can be established there, so this
      // gate simply does not apply (it composes with that violation rather
      // than crashing or guessing a root).
      if (
        dagRoots.length === 1 &&
        id === dagRoots[0] &&
        (b.status === "REVIEW" || b.status === "ACCEPTANCE_RECOMMENDED" || b.status === "ACCEPTED")
      ) {
        if (!(typeof b.entry_lock_digest === "string" && DIGEST_RE.test(b.entry_lock_digest))) {
          v(
            `state block ${id} is ${b.status} but entry_lock_digest ${JSON.stringify(b.entry_lock_digest ?? "")} is not a non-empty 64-char lowercase SHA-256 — ${id} is the DAG's entry (root) block and its entry-lock record (contracts/baseline-lock.md §2; the entry charter's first required-evidence item) must be digest-bound before review, acceptance recommendation, or acceptance`,
          );
        }
      }
      if (b.status === "PRIVATE_CHECKPOINT") {
        // The status is class-bound: program-dag.toml assigns class
        // "foundational-private-checkpoint" to exactly the block(s) the plan
        // allows to hold a private checkpoint (D1; contracts/stacked-prs.md:53 —
        // "an explicit program checkpoint such as D1"). Any other block in
        // PRIVATE_CHECKPOINT is a fabricated checkpoint. Missing/unknown class
        // (including a block absent from the DAG) fails closed.
        const cls = typeof dagById.get(id)?.class === "string" ? dagById.get(id).class : "";
        if (cls !== "foundational-private-checkpoint") {
          v(
            `state block ${id} is PRIVATE_CHECKPOINT but its DAG class is ${JSON.stringify(cls)} — the PRIVATE_CHECKPOINT status is permitted only for a block whose DAG class is "foundational-private-checkpoint" (program-dag.toml; contracts/stacked-prs.md:53)`,
          );
        }
      }
      if (
        b.status === "ACCEPTANCE_RECOMMENDED" ||
        b.status === "ACCEPTED" ||
        b.status === "PRIVATE_CHECKPOINT"
      ) {
        // The DAG's `class` column decides whether NOT_REQUIRED is even legal:
        // governance.md §2.2 permits skipping ONLY architecture review, ONLY on
        // a subsystem-class block; every foundational* class requires all three
        // mandates (governance.md:106,277). Missing/unknown class fails closed.
        const blockClass = typeof dagById.get(id)?.class === "string" ? dagById.get(id).class : "";
        for (const field of REVIEW_FIELDS) {
          const val = b[field];
          if (val !== "PASS" && val !== "NOT_REQUIRED") {
            v(
              `state block ${id} is ${b.status} but ${field} is ${val === undefined ? "missing" : JSON.stringify(val)} — every mandatory review mandate must be PASS before acceptance recommendation, acceptance, or a private checkpoint (governance.md:181, §9; program.md §7 — checkpoint REVIEW APPROVAL, not pending review)`,
            );
          } else if (
            val === "NOT_REQUIRED" &&
            !(field === "architecture_review" && blockClass === "subsystem")
          ) {
            v(
              `state block ${id} is ${b.status} but ${field} is NOT_REQUIRED and DAG class ${JSON.stringify(blockClass)} does not permit it — NOT_REQUIRED is permitted only for architecture_review on a subsystem-class block (governance.md §2.2); a foundational* block requires all three review mandates PASS on one exact candidate SHA/tree (governance.md:106,277)`,
            );
          }
        }
      }
      if (b.status === "ACCEPTED") {
        requireSha("accepted_sha");
        requireSha("accepted_tree");
        if (b.maintainer_decision !== "ACCEPTED") {
          v(
            `state block ${id} is ACCEPTED but maintainer_decision is ${b.maintainer_decision === undefined ? "missing" : JSON.stringify(b.maintainer_decision)} — acceptance is maintainer-only (governance.md §1.1) and must be recorded as maintainer_decision = "ACCEPTED"`,
          );
        }
        // governance.md:283 / contracts/stacked-prs.md:140 — an accepted
        // identity that DIFFERS from the reviewed candidate identity is legal
        // only with a repository-validated landing-equivalence artifact.
        const diverged = b.accepted_sha !== b.candidate_sha || b.accepted_tree !== b.candidate_tree;
        if (
          diverged &&
          !(
            typeof b.landing_equivalence_digest === "string" &&
            DIGEST_RE.test(b.landing_equivalence_digest)
          )
        ) {
          v(
            `state block ${id} is ACCEPTED with an accepted identity diverging from the reviewed candidate identity but landing_equivalence_digest ${JSON.stringify(b.landing_equivalence_digest ?? "")} is not a 64-char lowercase SHA-256 — a differing accepted identity is legal only with a repository-validated landing-equivalence artifact (governance.md:283, contracts/stacked-prs.md:140)`,
          );
        }
      }
    }
  }

  // -- Single IN_PROGRESS block, bound to current_block
  // templates/program-state.template.toml:18 declares `current_block`; the
  // orchestrator executes "only the next legal bounded block"
  // (ORCHESTRATOR.md:15), so at most one block is IN_PROGRESS and it must be
  // the declared current block.
  // Fail-closed: one IN_PROGRESS block at a time (ORCHESTRATOR.md:15), even
  // though stacked-prs.md:39 and max_active_workers = 3 would allow more.
  // A stacked/parallel regime must relax this check under review, not ad hoc.
  {
    const inProgress = [...stateById.values()].filter((b) => b.status === "IN_PROGRESS");
    if (inProgress.length > 1) {
      v(`more than one block IN_PROGRESS: [${inProgress.map((b) => b.id).join(", ")}]`);
    }
    if (typeof state.current_block === "string" && state.current_block !== "") {
      if (!stateById.has(state.current_block)) {
        v(`current_block ${JSON.stringify(state.current_block)} names no state block`);
      }
      for (const b of inProgress) {
        if (b.id !== state.current_block) {
          v(
            `block ${b.id} is IN_PROGRESS but current_block is ${JSON.stringify(state.current_block)}`,
          );
        }
      }
    } else {
      v("state is missing top-level current_block");
    }
  }

  // -- Live-mode field resolution
  if (opts.mode === "live") {
    // ORCHESTRATOR.md:83 — the live ledger must "resolve every A0-required
    // field": no REQUIRED_* template placeholder may remain. (The template
    // seeds placeholders only in the header/repository/orchestration fields
    // the current block needs; not-yet-started blocks carry empty strings,
    // so a whole-document scan is exact, not over-broad.)
    const scanPlaceholders = (obj, prefix) => {
      for (const [key, value] of Object.entries(obj)) {
        const where = prefix ? `${prefix}.${key}` : key;
        if (typeof value === "string" && value.startsWith("REQUIRED_")) {
          v(`live state still carries template placeholder ${where} = ${JSON.stringify(value)}`);
        } else if (Array.isArray(value)) {
          value.forEach((el, idx) => {
            if (typeof el === "string" && el.startsWith("REQUIRED_")) {
              v(
                `live state still carries template placeholder ${where}[${idx}] = ${JSON.stringify(el)}`,
              );
            } else if (el && typeof el === "object") {
              scanPlaceholders(el, `${where}[${el.id ?? idx}]`);
            }
          });
        } else if (value && typeof value === "object") {
          scanPlaceholders(value, where);
        }
      }
    };
    scanPlaceholders(state, "");

    // Identity shape: candidate_sha/tree etc. are "exact ... SHA/tree"
    // identities (governance.md:251 attaches approval to one exact candidate
    // SHA and tree; templates/program-state.template.toml:47-50). A SHA/tree
    // field is a full 40-char lowercase git object id or empty; a digest
    // field is a 64-char lowercase SHA-256 or empty.
    const scanShapes = (obj, prefix) => {
      for (const [key, value] of Object.entries(obj)) {
        const where = prefix ? `${prefix}.${key}` : key;
        if (typeof value === "string") {
          if (/_(sha|tree)$/.test(key) && value !== "" && !SHA_RE.test(value)) {
            v(
              `live state field ${where} is not a 40-char lowercase hex object id or empty: ${JSON.stringify(value)}`,
            );
          }
          if (/_digest$/.test(key) && value !== "" && !DIGEST_RE.test(value)) {
            v(
              `live state field ${where} is not a 64-char lowercase hex digest or empty: ${JSON.stringify(value)}`,
            );
          }
        } else if (Array.isArray(value)) {
          value.forEach((el, idx) => {
            if (el && typeof el === "object") scanShapes(el, `${where}[${el.id ?? idx}]`);
          });
        } else if (value && typeof value === "object") {
          scanShapes(value, where);
        }
      }
    };
    scanShapes(state, "");

    // Evidence-digest content binding. Shape-checking a digest proves only
    // that a binding was recorded, not that it binds the right bytes — a
    // well-formed but WRONG evidence_digest previously printed OK. When the
    // ledger claims evidence root(s), every declared root must be a real
    // directory and every resolved evidence_digest must match an artifact
    // under one of them (searched in declaration order — evidence for
    // different block series can live under different roots).
    //
    // `evidence_roots` (an array) is the plural form; `evidence_root` (a
    // single string) keeps working unchanged for a lone root. Declaring both
    // with evidence_root non-empty is ambiguous and rejected rather than
    // guessing precedence. An absent/empty declaration (no key, an empty
    // array, or evidence_root = "") is the documented skip: no evidence root
    // is claimed, so nothing here can be verified — not a violation. A root
    // that IS declared must still resolve, and an unresolvable declared root
    // is a violation, not a silent skip of that root.
    const orchestrationTable =
      state.orchestration && typeof state.orchestration === "object" ? state.orchestration : {};
    const hasRoots = "evidence_roots" in orchestrationTable;
    const hasRoot = "evidence_root" in orchestrationTable;
    let declaredRoots = null; // [{raw, field}] — null means nothing declared, skip silently
    if (hasRoots) {
      const val = orchestrationTable.evidence_roots;
      if (!Array.isArray(val)) {
        v(`live state orchestration.evidence_roots is not an array: ${JSON.stringify(val)}`);
      } else if (
        hasRoot &&
        typeof orchestrationTable.evidence_root === "string" &&
        orchestrationTable.evidence_root !== ""
      ) {
        v(
          `live state orchestration declares both evidence_root (${JSON.stringify(orchestrationTable.evidence_root)}) and evidence_roots — ambiguous, declare exactly one`,
        );
      } else {
        declaredRoots = val.map((raw, idx) => ({ raw, field: `evidence_roots[${idx}]` }));
      }
    } else if (hasRoot) {
      const val = orchestrationTable.evidence_root;
      if (typeof val !== "string") {
        v(`live state orchestration.evidence_root is not a string: ${JSON.stringify(val)}`);
      } else if (val !== "") {
        declaredRoots = [{ raw: val, field: "evidence_root" }];
      }
    }
    if (declaredRoots !== null && declaredRoots.length > 0) {
      const resolvedRoots = [];
      for (const { raw, field } of declaredRoots) {
        if (typeof raw !== "string" || raw === "") {
          v(
            `live state orchestration.${field} is not a non-empty string: ${JSON.stringify(raw)}`,
          );
          continue;
        }
        const resolved = resolveExistingDir(raw, opts.state);
        if (resolved === null) {
          v(
            `live state orchestration.${field} ${JSON.stringify(raw)} is not a resolvable directory — evidence_digest bindings cannot be verified`,
          );
          continue;
        }
        resolvedRoots.push(resolved);
      }
      for (const [id, b] of stateById) {
        if (!(typeof b.evidence_digest === "string" && DIGEST_RE.test(b.evidence_digest))) {
          continue;
        }
        let artifact = null;
        let ambiguous = null;
        for (const root of resolvedRoots) {
          const result = resolveEvidenceArtifact(root, id);
          if (result === null) continue;
          if (result.ambiguous) {
            ambiguous = result.ambiguous;
            break; // fail closed on ambiguity — do not fall through to another root
          }
          artifact = result.path;
          break;
        }
        if (ambiguous !== null) {
          v(
            `state block ${id} has evidence_digest ${b.evidence_digest} but multiple nested evidence artifacts resolve for it, ambiguous: [${ambiguous.join(", ")}]`,
          );
          continue;
        }
        if (artifact === null) {
          // Every declared root already failed to resolve — that is reported
          // once per bad root above; do not pile on a second, less precise
          // violation per block for the same underlying cause.
          if (resolvedRoots.length === 0) continue;
          v(
            `state block ${id} has evidence_digest ${b.evidence_digest} but no evidence artifact under [${resolvedRoots.join(", ")}]`,
          );
          continue;
        }
        const actual = createHash("sha256").update(readFileSync(artifact)).digest("hex");
        if (actual !== b.evidence_digest) {
          v(
            `state block ${id} evidence_digest ${b.evidence_digest} does not match the SHA-256 of ${artifact} (${actual})`,
          );
        }
      }
    }

    // -- Git identity verification (see the block comment above
    // verifyLiveGitIdentities for what this proves and why).
    verifyLiveGitIdentities(stateById, v);
  }

  // -- Non-vacuous work
  // contracts/agent-orchestration.md:137 — a required command that "executes
  // zero intended work or cannot be proven non-vacuous" is a mandatory stop.
  // A run that validated zero blocks proved nothing and is a FAILURE.
  const validatedBlocks = stateById.size;
  if (validatedBlocks === 0) {
    v("zero blocks validated — a run that validates zero blocks is a FAILURE, not a pass");
  }
  if (dagIds.length === 0) {
    v("DAG declares zero blocks — nothing to validate against");
  }

  if (violations.length > 0) {
    for (const violation of violations) process.stderr.write(`VIOLATION: ${violation}\n`);
    process.stderr.write(
      `FAIL: ${violations.length} violation(s) in ${opts.state} against ${opts.dag} (mode ${opts.mode})\n`,
    );
    process.exit(1);
  }
  process.stdout.write(
    `OK: ${basename(opts.state)} (${opts.state}) — validated ${validatedBlocks} blocks (non-zero work asserted) against ${opts.dag} in mode ${opts.mode}\n`,
  );
  process.exit(0);
}

main();
