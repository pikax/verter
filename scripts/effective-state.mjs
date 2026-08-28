#!/usr/bin/env node
// Effective-state generator. Derives ONE view of "what is actually true about
// block X right now" by reduction over the program's several authorities —
// program-dag.toml, program-state.toml (the ledger), the typed-frontmatter
// rulings corpus, and (when present) the authority registry — instead of
// requiring a human to cross-read all of them.
//
//   node scripts/effective-state.mjs [--dag <path>] [--state <path>]
//     [--rulings-dir <dir>] [--amendments-dir <dir>]
//     [--authority-registry <path>] [--json]
//
// This retired compatibility reducer accepts only a complete explicit fixture
// tree. Live authority inspection belongs to the Rev11 `programctl` CLI.
//
// This tool DETECTS disagreement between authorities. It never repairs the
// ledger, the DAG, or a ruling — those are maintainer-owned artifacts and a
// contradiction found here is a finding to route to the maintainer, not a
// bug for this script to silently paper over. Every field in the derived
// view is computed from the source files on every run: there is no
// hand-maintained block list or ruling list anywhere in this file.
//
// Exit: 0 no `error`-severity findings, 1 one or more `error`-severity
// findings, 2 usage / unreadable / unparseable input (nothing was derived).

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { isAbsolute, join, resolve as resolvePath } from "node:path";
import process from "node:process";
import { TomlError, parseToml } from "./lib/rev11-toml.mjs";
import { FrontmatterError, parseRulingFrontmatter } from "./lib/ruling-frontmatter.mjs";

const REPO_ROOT = resolvePath(new URL("..", import.meta.url).pathname);

const DIGEST_RE = /^[0-9a-f]{64}$/;

// templates/program-state.template.toml — a block in one of these statuses
// has begun real work; its direct DAG predecessors must all be ACCEPTED
// (governance.md's sequencing rule). A block carrying a non-empty stack_id
// is left to validate-program-state.mjs's full stacked-work exception model
// (composite cross-validation against a --stack-window file) — this
// generator does not reimplement that nuance, and skips it rather than
// mis-flag legitimate contingent stacked work as a contradiction.
const BEGUN_STATUSES = new Set([
  "READY",
  "IN_PROGRESS",
  "REVIEW",
  "ACCEPTANCE_RECOMMENDED",
  "ACCEPTED",
  "PRIVATE_CHECKPOINT",
]);

// A token that unambiguously LOOKS like a program block id (A0, BF3, BV0A,
// CM1, L4, ...) — every real DAG id is 1-5 letters, 1-2 digits, then 0-2
// trailing letters. This intentionally does NOT match "AMD-005", "AT-2",
// "JS-1" (hyphenated, not block ids) or free-text binds entries like
// "program-wide (release boundary)" (contains spaces/punctuation).
const BLOCK_ID_SHAPE_RE = /^[A-Z]{1,5}[0-9]{1,2}[A-Z]{0,2}$/;

// -- CLI

function usageFail(msg) {
  process.stderr.write(
    `${msg}\nusage: node scripts/effective-state.mjs [--dag <path>] [--state <path>] [--rulings-dir <dir>] [--amendments-dir <dir>] [--authority-registry <path>] [--json]\n`,
  );
  process.exit(2);
}

function parseArgs(argv) {
  const opts = Object.create(null);
  const known = ["--dag", "--state", "--rulings-dir", "--amendments-dir", "--authority-registry"];
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    if (flag === "--json") {
      opts.json = true;
      continue;
    }
    if (!known.includes(flag)) usageFail(`unknown argument: ${flag}`);
    const value = argv[i + 1];
    if (value === undefined) usageFail(`missing value for ${flag}`);
    opts[flag.slice(2)] = value;
    i++;
  }
  return opts;
}

function resolveOpt(value) {
  return isAbsolute(value) ? value : resolvePath(REPO_ROOT, value);
}

function loadFile(path, what) {
  try {
    return readFileSync(path, "utf8");
  } catch (err) {
    usageFail(`cannot read ${what} file ${path}: ${err.message}`);
  }
}

// -- Discovery (every list below is derived by walking the filesystem, never
// hand-maintained)

function discoverRulings(rulingsDir) {
  let entries;
  try {
    entries = readdirSync(rulingsDir);
  } catch (err) {
    usageFail(`cannot read rulings directory ${rulingsDir}: ${err.message}`);
  }
  const files = entries.filter((f) => f.endsWith(".md") && f !== "INDEX.md").sort();
  const rulings = [];
  for (const file of files) {
    const path = join(rulingsDir, file);
    let frontmatter;
    try {
      frontmatter = parseRulingFrontmatter(readFileSync(path, "utf8"), file);
    } catch (err) {
      if (err instanceof FrontmatterError) {
        process.stderr.write(`VIOLATION: unparseable ruling frontmatter — ${err.message}\n`);
        process.stderr.write("FAIL: 0 rulings derived — input could not be parsed\n");
        process.exit(2);
      }
      throw err;
    }
    rulings.push({ file, path, frontmatter });
  }
  return rulings;
}

function discoverAmendments(amendmentsDir) {
  let entries;
  try {
    entries = readdirSync(amendmentsDir);
  } catch {
    return new Map(); // amendments dir is optional context, not required input
  }
  const byId = new Map();
  for (const file of entries.filter((f) => f.endsWith(".md")).sort()) {
    const m = /^(AMD-\d+)-/.exec(file);
    if (!m) continue;
    const id = m[1];
    if (!byId.has(id)) byId.set(id, []);
    byId.get(id).push(file);
  }
  return byId;
}

// Mirrors validate-program-state.mjs's amendment ratification read: the
// **Status:** paragraph (declaring line through the next blank line), with
// "not ratified" beating a bare "ratified" mention in the same paragraph.
function readAmendmentRatification(amendmentsDir, files) {
  if (!files || files.length !== 1) {
    return { error: `expected exactly one file, found ${files ? files.length : 0}` };
  }
  const path = join(amendmentsDir, files[0]);
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch (err) {
    return { error: `could not read ${path}: ${err.message}` };
  }
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((l) => l.startsWith("**Status:**"));
  if (start === -1) return { error: `${path} has no **Status:** line` };
  const paragraph = [];
  for (let i = start; i < lines.length; i++) {
    paragraph.push(lines[i]);
    if (lines[i].trim() === "") break;
  }
  const statusText = paragraph.join(" ").trim();
  let ratified;
  if (/\bnot\s+ratified\b/i.test(statusText)) ratified = false;
  else if (/\bratified\b/i.test(statusText)) ratified = true;
  else ratified = false;
  return { path, ratified, statusText: lines[start].trim() };
}

// -- Finding helpers

function finding(type, severity, message, extra) {
  return { type, severity, message, ...extra };
}

// Deterministic ordering: no filesystem-order or Map-iteration-order
// dependence reaches the output.
function sortFindings(findings) {
  return [...findings].sort((a, b) => {
    if (a.type !== b.type) return a.type < b.type ? -1 : 1;
    if (a.message !== b.message) return a.message < b.message ? -1 : 1;
    return 0;
  });
}

// -- Detection classes

// Class: DAG edge referencing an unknown block.
function checkDagEdgesKnown(dagIds, dagById) {
  const out = [];
  for (const id of dagIds) {
    const b = dagById.get(id);
    for (const p of b.predecessors ?? []) {
      if (!dagById.has(p)) {
        out.push(
          finding(
            "DAG_EDGE_UNKNOWN_BLOCK",
            "error",
            `DAG block ${id} names unknown predecessor ${JSON.stringify(p)}`,
            { block: id },
          ),
        );
      }
    }
  }
  return out;
}

// Class: a block whose status is inconsistent with its DAG predecessors —
// begun (READY or later) while a direct predecessor has not been ACCEPTED.
// See BEGUN_STATUSES above for the stack_id carve-out.
function checkStatusPredecessorConsistency(dagById, stateById) {
  const out = [];
  for (const [id, b] of stateById) {
    if (!BEGUN_STATUSES.has(b.status)) continue;
    if (typeof b.stack_id === "string" && b.stack_id.trim() !== "") continue;
    const dagBlock = dagById.get(id);
    if (!dagBlock) continue; // reported separately if the id set disagrees
    const bad = (dagBlock.predecessors ?? []).filter(
      (p) => stateById.get(p)?.status !== "ACCEPTED",
    );
    for (const p of bad) {
      out.push(
        finding(
          "STATUS_PREDECESSOR_INCONSISTENT",
          "error",
          `block ${id} is ${b.status} but predecessor ${p} is ${JSON.stringify(stateById.get(p)?.status ?? "MISSING")} (not ACCEPTED)`,
          { block: id, predecessor: p },
        ),
      );
    }
  }
  return out;
}

// Class: block set disagreement between the DAG and the ledger.
function checkBlockSetAgreement(dagIds, dagById, stateById) {
  const out = [];
  for (const id of dagIds) {
    if (!stateById.has(id)) {
      out.push(
        finding("LEDGER_BLOCK_MISSING", "error", `DAG block ${id} has no matching ledger row`, {
          block: id,
        }),
      );
    }
  }
  for (const id of stateById.keys()) {
    if (!dagById.has(id)) {
      out.push(
        finding("DAG_BLOCK_MISSING", "error", `ledger block ${id} has no matching DAG entry`, {
          block: id,
        }),
      );
    }
  }
  return out;
}

// Class: a block referenced by a ruling but absent from the ledger (and vice
// versa). "Absent from the ledger" is checked in two directions: a binds
// token that isn't even a known DAG id at all (RULING_BLOCK_UNKNOWN), and a
// binds token that IS a known DAG id but has no ledger row for it — the
// ledger having silently dropped a block the DAG and a ruling both still
// name (RULING_BLOCK_MISSING_FROM_LEDGER).
function checkRulingBlockReferences(rulings, dagById, stateById) {
  const out = [];
  for (const r of rulings) {
    const binds = Array.isArray(r.frontmatter.binds) ? r.frontmatter.binds : [];
    for (const token of binds) {
      if (!BLOCK_ID_SHAPE_RE.test(token)) continue; // free-text binds entry, not a block id
      if (!dagById.has(token)) {
        out.push(
          finding(
            "RULING_BLOCK_UNKNOWN",
            "error",
            `ruling ${r.frontmatter.ruling_id} (${r.file}) binds unknown block ${JSON.stringify(token)} — not a DAG block id`,
            { ruling: r.frontmatter.ruling_id, file: r.file, block: token },
          ),
        );
        continue;
      }
      if (!stateById.has(token)) {
        out.push(
          finding(
            "RULING_BLOCK_MISSING_FROM_LEDGER",
            "error",
            `ruling ${r.frontmatter.ruling_id} (${r.file}) binds block ${JSON.stringify(token)}, a known DAG block, but the ledger has no row for it`,
            { ruling: r.frontmatter.ruling_id, file: r.file, block: token },
          ),
        );
      }
    }
  }
  return out;
}

// Supersession-claim edges: for each ruling, every {ruling: ID, claim} entry
// in supersedes/superseded_by that names ANOTHER MIGRATED ruling (a
// {document: ...} entry cites something outside this corpus and is not
// resolvable — skipped, per the rulings themselves and INDEX.md).
function collectSupersessionEdges(rulings) {
  const supersedesEdges = []; // {from, to, file} : from supersedes to
  const supersededByEdges = []; // {from, to, file} : from is superseded_by to
  for (const r of rulings) {
    const id = r.frontmatter.ruling_id;
    for (const entry of r.frontmatter.supersedes ?? []) {
      if (typeof entry.ruling === "string" && entry.ruling !== "") {
        supersedesEdges.push({ from: id, to: entry.ruling, file: r.file });
      }
    }
    for (const entry of r.frontmatter.superseded_by ?? []) {
      if (typeof entry.ruling === "string" && entry.ruling !== "") {
        supersededByEdges.push({ from: id, to: entry.ruling, file: r.file });
      }
    }
  }
  return { supersedesEdges, supersededByEdges };
}

// Class: a supersedes edge naming a ruling_id that does not exist in the
// corpus (a typo, or a document reference mis-tagged as `ruling`).
function checkSupersessionTargetsKnown(rulings, rulingIds) {
  const out = [];
  for (const r of rulings) {
    const id = r.frontmatter.ruling_id;
    for (const field of ["supersedes", "superseded_by"]) {
      for (const entry of r.frontmatter[field] ?? []) {
        if (
          typeof entry.ruling === "string" &&
          entry.ruling !== "" &&
          !rulingIds.has(entry.ruling)
        ) {
          out.push(
            finding(
              "RULING_SUPERSESSION_TARGET_UNKNOWN",
              "error",
              `ruling ${id} (${r.file}) ${field} names unknown ruling_id ${JSON.stringify(entry.ruling)}`,
              { ruling: id, file: r.file, target: entry.ruling, field },
            ),
          );
        }
      }
    }
  }
  return out;
}

// Class: a cycle in the resolved supersedes graph (A supersedes B supersedes
// ... supersedes A). Detected, not fixed — reported as a finding, never a
// crash.
function checkSupersessionCycles(supersedesEdges) {
  const adjacency = new Map();
  for (const { from, to } of supersedesEdges) {
    if (!adjacency.has(from)) adjacency.set(from, []);
    adjacency.get(from).push(to);
  }
  const out = [];
  const state = new Map(); // 0 visiting, 1 done
  const reportedCycles = new Set();
  const visit = (node, stack) => {
    if (state.get(node) === 1) return;
    if (state.get(node) === 0) {
      const cycleStart = stack.indexOf(node);
      const cycle = [...stack.slice(cycleStart), node];
      const key = [...cycle].sort().join(",");
      if (!reportedCycles.has(key)) {
        reportedCycles.add(key);
        out.push(
          finding("RULING_SUPERSESSION_CYCLE", "error", `supersedes cycle: ${cycle.join(" -> ")}`, {
            cycle,
          }),
        );
      }
      return;
    }
    state.set(node, 0);
    for (const next of adjacency.get(node) ?? []) {
      visit(next, [...stack, node]);
    }
    state.set(node, 1);
  };
  for (const node of adjacency.keys()) visit(node, []);
  return out;
}

// Class: duplicate ruling_id across the corpus (two files claiming the same
// identity — ambiguous claim resolution).
function checkDuplicateRulingIds(rulings) {
  const byId = new Map();
  for (const r of rulings) {
    const id = r.frontmatter.ruling_id;
    if (!byId.has(id)) byId.set(id, []);
    byId.get(id).push(r.file);
  }
  const out = [];
  for (const [id, files] of byId) {
    if (files.length > 1) {
      out.push(
        finding(
          "DUPLICATE_RULING_ID",
          "error",
          `ruling_id ${JSON.stringify(id)} is declared by ${files.length} files: [${files.join(", ")}]`,
          { ruling: id, files },
        ),
      );
    }
  }
  return out;
}

// Class: an artifact digest cited by a ledger row whose file is missing.
// Mirrors validate-program-state.mjs's evidence-artifact resolution (named
// candidates under <root>/<id>/, one nested-level fallback) but only asks
// "does anything resolve at all" — digest content verification is that
// validator's job, not this generator's.
function resolveEvidenceArtifact(root, id) {
  const named = [
    join(root, id, "landing-record.md"),
    join(root, id, `${id}-exact-candidate-record.md`),
    join(root, id, `${id}-summary.md`),
    join(root, id, "landing-equivalence.md"),
    join(root, `${id}-summary.md`),
  ];
  for (const candidate of named) {
    if (existsSync(candidate)) return candidate;
  }
  const blockDir = join(root, id);
  let entries;
  try {
    entries = readdirSync(blockDir, { withFileTypes: true });
  } catch {
    entries = [];
  }
  for (const entry of entries.sort((a, b) => (a.name < b.name ? -1 : 1))) {
    if (!entry.isDirectory()) continue;
    const candidate = join(blockDir, entry.name, "landing-record.md");
    if (existsSync(candidate)) return candidate;
  }
  return null;
}

function resolveExistingDir(raw, statePath) {
  if (isAbsolute(raw)) return existsSync(raw) ? raw : null;
  for (const candidate of [resolvePath(REPO_ROOT, raw), resolvePath(join(statePath, ".."), raw)]) {
    if (existsSync(candidate)) return candidate;
  }
  return null;
}

function checkEvidenceArtifactsExist(state, stateById, statePath) {
  const out = [];
  const orchestration =
    state.orchestration && typeof state.orchestration === "object" ? state.orchestration : {};
  let roots = [];
  if (Array.isArray(orchestration.evidence_roots)) {
    roots = orchestration.evidence_roots;
  } else if (
    typeof orchestration.evidence_root === "string" &&
    orchestration.evidence_root !== ""
  ) {
    roots = [orchestration.evidence_root];
  }
  const resolvedRoots = roots
    .map((r) => resolveExistingDir(r, statePath))
    .filter((r) => r !== null);
  if (resolvedRoots.length === 0) return out; // nothing declared/resolvable — mirror validator's silent skip
  for (const [id, b] of stateById) {
    if (!(typeof b.evidence_digest === "string" && DIGEST_RE.test(b.evidence_digest))) continue;
    const found = resolvedRoots.some((root) => resolveEvidenceArtifact(root, id) !== null);
    if (!found) {
      out.push(
        finding(
          "ARTIFACT_DIGEST_FILE_MISSING",
          "error",
          `ledger block ${id} evidence_digest ${b.evidence_digest} is set but no artifact resolves under [${resolvedRoots.join(", ")}]`,
          { block: id },
        ),
      );
    }
  }
  return out;
}

// Class: a missing DAG edge implied by a ruling but absent from
// program-dag.toml — a ruling text explicitly says to ADD an edge (a
// ratification-pending amendment, or a drift the DAG never picked up); the
// edge is not present as a predecessor relationship in the live DAG. Scoped
// tightly to the "add ... edge X -> Y" / "ADD `X -> Y`" phrasing actually
// used in this corpus (see ARCH-RULING-C2-FIVE-FORKS.md) so this does not
// degrade into flagging every incidental "X -> Y" status-transition mention
// in a ruling's prose.
const EDGE_ADD_RES = [
  /\badd(?:s|ed)?\b(?:\s+the)?\s+(?:dag\s+)?edge\s*`?\s*([A-Z][A-Z0-9]{0,5})\s*->\s*([A-Z][A-Z0-9]{0,5})\s*`?/gi,
  /\badd(?:s|ed)?\s*`\s*([A-Z][A-Z0-9]{0,5})\s*->\s*([A-Z][A-Z0-9]{0,5})\s*`/gi,
];

function checkMissingImpliedDagEdges(rulings, dagById) {
  const out = [];
  const seen = new Set();
  for (const r of rulings) {
    let text;
    try {
      text = readFileSync(r.path, "utf8");
    } catch {
      continue;
    }
    for (const re of EDGE_ADD_RES) {
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(text))) {
        const from = m[1].toUpperCase();
        const to = m[2].toUpperCase();
        if (!dagById.has(from) || !dagById.has(to)) continue;
        const key = `${from} ${to}`;
        if (seen.has(key)) continue;
        const predecessors = dagById.get(to).predecessors ?? [];
        if (!predecessors.includes(from)) {
          seen.add(key);
          out.push(
            finding(
              "MISSING_DAG_EDGE_IMPLIED_BY_RULING",
              "error",
              `ruling ${r.frontmatter.ruling_id} (${r.file}) says to add edge ${from} -> ${to}, but program-dag.toml block ${to} does not list ${from} as a predecessor`,
              { ruling: r.frontmatter.ruling_id, file: r.file, from, to },
            ),
          );
        }
      }
    }
  }
  return out;
}

// -- Effective-state view (derived per-block reduction, not just findings)

function buildEffectiveBlocks(dagIds, dagById, stateById, rulings, amendmentRatification) {
  const rulingsByBlock = new Map();
  for (const r of rulings) {
    for (const token of r.frontmatter.binds ?? []) {
      if (!BLOCK_ID_SHAPE_RE.test(token)) continue;
      if (!rulingsByBlock.has(token)) rulingsByBlock.set(token, []);
      rulingsByBlock.get(token).push(r.frontmatter.ruling_id);
    }
  }
  const blocks = [];
  for (const id of dagIds) {
    const dagBlock = dagById.get(id);
    const stateBlock = stateById.get(id);
    const enablingAmendment =
      typeof stateBlock?.enabling_amendment === "string"
        ? stateBlock.enabling_amendment.trim()
        : "";
    blocks.push({
      id,
      class: dagBlock.class ?? "",
      predecessors: dagBlock.predecessors ?? [],
      ledger_status: stateBlock?.status ?? null,
      enabling_amendment:
        enablingAmendment === ""
          ? null
          : {
              id: enablingAmendment,
              ...(amendmentRatification.get(enablingAmendment) ?? { error: "unresolved" }),
            },
      related_rulings: (rulingsByBlock.get(id) ?? []).slice().sort(),
    });
  }
  return blocks;
}

// -- Output

function printHuman(view) {
  const lines = [];
  lines.push(
    `effective-state: ${view.blocks.length} blocks, ${view.rulings.length} rulings, ${view.findings.length} finding(s) (${view.errorCount} error)`,
  );
  if (view.authorityRegistry.present) {
    lines.push(`authority-registry.toml: present at ${view.authorityRegistry.path}`);
  } else {
    lines.push("authority-registry.toml: absent (tolerated — lands separately)");
  }
  if (view.findings.length === 0) {
    lines.push("no contradictions detected");
  } else {
    for (const f of view.findings) {
      lines.push(`[${f.severity.toUpperCase()}] ${f.type}: ${f.message}`);
    }
  }
  process.stdout.write(lines.join("\n") + "\n");
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const explicitFixtureInputs = ["dag", "state", "rulings-dir", "amendments-dir", "authority-registry"].every((key) => typeof opts[key] === "string");
  if (!explicitFixtureInputs) {
    process.stderr.write("effective-state legacy defaults are retired; use docs/arch/refactor/rev11/tools/programctl.mjs, or provide every explicit fixture input\n");
    process.exitCode = 2;
    return;
  }
  const dagPath = resolveOpt(opts.dag);
  const statePath = resolveOpt(opts.state);
  const rulingsDir = resolveOpt(opts["rulings-dir"]);
  const amendmentsDir = resolveOpt(opts["amendments-dir"]);
  const authorityRegistryPath = resolveOpt(opts["authority-registry"]);

  let dag;
  let state;
  try {
    dag = parseToml(loadFile(dagPath, "DAG"), dagPath);
    state = parseToml(loadFile(statePath, "state"), statePath);
  } catch (err) {
    if (err instanceof TomlError) {
      process.stderr.write(`VIOLATION: ${err.message}\n`);
      process.stderr.write("FAIL: 0 blocks derived — input could not be parsed\n");
      process.exit(2);
    }
    throw err;
  }

  const dagBlocks = Array.isArray(dag.block) ? dag.block : [];
  const dagIds = [];
  const dagById = new Map();
  for (const b of dagBlocks) {
    if (typeof b.id === "string" && b.id !== "" && !dagById.has(b.id)) {
      dagIds.push(b.id);
    }
    if (typeof b.id === "string" && b.id !== "") dagById.set(b.id, b);
  }
  dagIds.sort();

  const stateBlocks = Array.isArray(state.block) ? state.block : [];
  const stateById = new Map();
  for (const b of stateBlocks) {
    if (typeof b.id === "string" && b.id !== "") stateById.set(b.id, b);
  }

  const rulings = discoverRulings(rulingsDir);
  const rulingIds = new Set(rulings.map((r) => r.frontmatter.ruling_id));

  const amendmentFiles = discoverAmendments(amendmentsDir);
  const amendmentRatification = new Map();
  for (const [id, files] of amendmentFiles) {
    amendmentRatification.set(id, readAmendmentRatification(amendmentsDir, files));
  }

  const { supersedesEdges } = collectSupersessionEdges(rulings);

  const findings = [
    ...checkDagEdgesKnown(dagIds, dagById),
    ...checkBlockSetAgreement(dagIds, dagById, stateById),
    ...checkStatusPredecessorConsistency(dagById, stateById),
    ...checkRulingBlockReferences(rulings, dagById, stateById),
    ...checkSupersessionTargetsKnown(rulings, rulingIds),
    ...checkSupersessionCycles(supersedesEdges),
    ...checkDuplicateRulingIds(rulings),
    ...checkEvidenceArtifactsExist(state, stateById, statePath),
    ...checkMissingImpliedDagEdges(rulings, dagById),
  ];
  const sorted = sortFindings(findings);
  const errorCount = sorted.filter((f) => f.severity === "error").length;

  const view = {
    blocks: buildEffectiveBlocks(dagIds, dagById, stateById, rulings, amendmentRatification),
    rulings: rulings.map((r) => ({
      ruling_id: r.frontmatter.ruling_id,
      file: r.file,
      type: r.frontmatter.type,
    })),
    authorityRegistry: { present: existsSync(authorityRegistryPath), path: authorityRegistryPath },
    findings: sorted,
    errorCount,
  };

  if (opts.json) {
    process.stdout.write(JSON.stringify(view, null, 2) + "\n");
  } else {
    printHuman(view);
  }
  // Not process.exit(): the report can exceed the stdout pipe's buffer, and
  // exit() tears the process down before an in-flight async pipe write
  // flushes, silently truncating large output. exitCode + natural process
  // end lets Node drain stdout first.
  process.exitCode = errorCount > 0 ? 1 : 0;
}

main();
