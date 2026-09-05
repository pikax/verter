#!/usr/bin/env node
// Closure-register validator and human-view generator.
//
// The register is INPUT ONLY. Lifecycle status (OPEN / REFUSED /
// PROVEN-BOUNDED / PROVEN) is derived here and nowhere else, and the human
// view is generated here and nowhere else. Nothing in this file searches the
// source tree for a spelling: every check reads a declared artifact at a path
// the register or the program authority names — the register, its schema, the
// program DAG, the charters, the ratified contract, and the CI workflow's own
// declared filter and job blocks.
//
//   node roadmap/0.1.0-tama/tools/closure-register.mjs [--check|--write]
//
// `--check` (the default) validates, fails if the generated view on disk is
// stale, and refuses to print a PASS line unless the DERIVED state is
// reviewable. `--write` rewrites the view.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  PACKAGE_ROOT,
  confinedFile,
  loadAuthority,
  readToml,
  validateSchemaObject,
} from "./lib.mjs";

import {
  ALLOWED_RESIDUES,
  LIVE_TOPOLOGY,
  LIVE_UNIVERSE,
  MUST_CLOSE_FINDINGS,
  RAISING_NODE,
  RAISING_TRAIN,
  REGISTER_RELATIVE,
  REGISTER_SCHEMA,
  REQUIRED_RECEIVING,
  REQUIRED_TRANSFERS,
  RESIDUE_FINDINGS,
  SUBJECT_EXERCISING_PROOFS,
  VIEW_RELATIVE,
} from "./closure-register.pins.mjs";

// Re-exported so that this module stays the instrument's single entry point:
// the control suite and any caller read the pins through the derivation that
// consumes them, rather than having to know they were split out.
export {
  ALLOWED_RESIDUES,
  LIVE_TOPOLOGY,
  LIVE_UNIVERSE,
  MUST_CLOSE_FINDINGS,
  RAISING_NODE,
  RAISING_TRAIN,
  REGISTER_RELATIVE,
  REGISTER_SCHEMA,
  REQUIRED_RECEIVING,
  REQUIRED_TRANSFERS,
  RESIDUE_FINDINGS,
  SUBJECT_EXERCISING_PROOFS,
  VIEW_RELATIVE,
};

/**
 * The four fixed acceptance-criterion roles every charter in this program
 * declares, in ordinal order.
 *
 * A charter's four slots are boilerplate: hundreds of descendants declare an
 * interchangeable `AC1..AC4`, so the bare identifier `<NODE>-AC<n>`
 * discriminates nothing on its own — any ordinal of any descendant resolves.
 * What does discriminate is the ROLE the owning charter attaches to the
 * ordinal. Every citation site therefore declares the role it needs, and the
 * validator resolves that role out of the owner charter's own criterion line
 * and requires an exact match.
 */
export const CRITERION_ROLES = Object.freeze([
  "sole-owner outcome",
  "positive contract",
  "incremental equivalence",
  "bounded work",
]);

/**
 * The closed vocabulary of `shipped_obligation`, plus the open fourth arm.
 *
 * The field used to be a two-way — `met`, or the production path an obligation
 * is unmet at — and that made `met` the only spelling available for three
 * different situations. A remainder this block CARRIES could declare `met`
 * while the same atom sat in `[[transfer]]`, so "the shipped code meets this"
 * and "this is an open question for a successor" were both asserted about one
 * atom and nothing derived the contradiction. And an obligation whose subject
 * is a ratified sentence, a schema, or this instrument itself has no shipped
 * code to meet it at all, so `met` there was a false positive claim standing in
 * for "not applicable" — the exact conflation this register refuses everywhere
 * else.
 *
 *   - `met` — the shipped code meets it. Refused on a transferred atom.
 *   - `authority-only` — the obligation's subject is the ratified artifact the
 *     atom is anchored at rather than shipped code, so "does the code meet it"
 *     has no answer. Refused when the anchor lies in a production surface this
 *     node's charter declares, because then the code IS the subject. Refused on
 *     a transferred atom, which is a different thing entirely.
 *   - `carried` — the obligation is an open question this block does not answer
 *     and hands to a successor. Required to be TRANSFERRED, so it leaves under
 *     the pinned routing and the ordered receiving rows rather than through a
 *     column.
 *   - anything else — the production path the obligation is currently UNMET at.
 *     Also required to be transferred, and additionally bound to a receiving
 *     owner whose own charter may change that path.
 */
export const SHIPPED_OBLIGATION_MET = "met";
export const SHIPPED_OBLIGATION_AUTHORITY_ONLY = "authority-only";
export const SHIPPED_OBLIGATION_CARRIED = "carried";

/**
 * The closed set of failure classes the control suite must discriminate. It is
 * declared here, beside the validator, so the suite cannot quietly drop one:
 * the suite asserts its own registrations equal this set exactly.
 */
export const MANDATORY_CONTROL_CLASSES = Object.freeze([
  "omitted-claim",
  "forbidden-input-status",
  "removed-residue-owner",
  "missing-dependency",
  "stale-evidence",
  "irrelevant-existing-proof",
  "zero-selected-work",
  "skipped-work",
  "inconsistent-counters",
  "unapplied-mutation",
  "disclosed-limit",
  "bounded-to-proven-laundering",
  // A proposition rewritten under a pinned id, so a claim is weakened to fit
  // the evidence rather than the evidence to the claim.
  "hollowed-statement",
  // A lane that re-runs SOMETHING, over a universe with no relation to the
  // record it is declared to refresh.
  "unrelated-lane-selection",
  // A record whose own run executes the artifact its claim is about, reached
  // through an import rather than through a command argument.
  "self-executing-subject",
  // An atom repointed at a different evidence artifact or a different contract
  // sentence, under an unchanged proposition.
  "repointed-anchor",
  // A lane whose declared selection cannot be RELATED to the record's own: a
  // run-time predicate nothing resolves, or a runner whose default universe is
  // a list this check never read.
  "unbound-lane-selection",
  // An obligation the shipped code does not yet meet, disclosed beside a
  // covered atom instead of leaving through an approved remainder, or carried
  // to a receiving sequence no owner of which may change the surface.
  "owed-obligation-unowned",
]);

/**
 * How a runner's terminal summary carries its own counts.
 *
 * A record used to state its counters BESIDE a free-text summary, so any
 * plausible sentence paired with self-consistent numbers was accepted. The
 * counters are now derived FROM the transcript the record claims to have
 * observed: the adapter declares the grammar its runner emits, the validator
 * re-derives the five counts out of the recorded text, and the record's own
 * numbers must equal them exactly. A summary that is not a terminal line of
 * the declared shape parses to nothing and refuses the record.
 */
export const SUMMARY_GRAMMARS = Object.freeze([
  "libtest",
  "nextest",
  "node-test",
  "tool-line",
  "compile-contracts",
]);

/**
 * Who re-derives a record.
 *
 * `instrument` means the control suite re-runs the command itself and compares
 * the live run's own derived counts against the transcription. `external`
 * means this node-only instrument cannot invoke that runner, so the record
 * declares the CI lane that does; the validator resolves that lane against the
 * workflow rather than accepting the claim.
 */
export const REEXECUTION_MODES = Object.freeze(["instrument", "external"]);

/**
 * The gate that would notice.
 *
 * A transcribed count is a claim about a tree, and it stays true only while
 * nothing it observed changes. So every record binds to a lane that re-runs its
 * work, and that binding is RESOLVED here rather than asserted.
 *
 * For a record this instrument re-runs, the lane is this instrument's own: the
 * `tama` filter must cover everything the record cites, and exactly one job
 * must run the instrument's commands under that filter.
 *
 * For an `external` record the adapter names the job that re-runs its runner,
 * the command that job issues, and the filter that gates it. All three resolve
 * against the workflow, and the record's cited artifacts must be covered by
 * THAT filter's patterns — which is also how the selected packages' dependency
 * closure gets covered, since a Rust lane's filter is the crate tree rather
 * than a hand-listed subset.
 */
export const CI_WORKFLOW = ".github/workflows/ci.yml";
export const CI_TRIGGER_FILTER = "tama";
export const INSTRUMENT_COMMANDS = Object.freeze([
  "node roadmap/0.1.0-tama/tools/closure-register.mjs --check",
  "node --test roadmap/0.1.0-tama/tools/closure-register.test.mjs",
]);

/** Derived lifecycle values. `ADMISSIBLE` is deliberately absent. */
export const OPEN = "OPEN";
export const REFUSED = "REFUSED";
export const PROVEN_BOUNDED = "PROVEN-BOUNDED";
export const PROVEN = "PROVEN";
export const READY_FOR_REVIEW = "READY_FOR_REVIEW";

const PLACEHOLDER = "PLACEHOLDER";

function unique(rows, key, label, errors) {
  const seen = new Set();
  for (const row of rows) {
    const id = row[key];
    if (seen.has(id)) errors.push(`${label}: duplicate ${key} ${id}`);
    seen.add(id);
  }
  return seen;
}

/**
 * Terminal styling is presentation, not content: a transcript is compared
 * without it. The escape byte is built from its code point, so the pattern
 * carries no source-level escape of its own.
 */
const ANSI_STYLE = new RegExp(`${String.fromCharCode(27)}\\[[0-9;]*m`, "gu");
const stripStyling = (text) => String(text).replaceAll(ANSI_STYLE, "");

/** Line endings are a checkout property, never evidence. */
const normalizeLines = (text) => String(text).replaceAll("\r\n", "\n");

/**
 * A workflow body with shell line continuations folded back into one line.
 *
 * A step's command is what the shell receives, and a trailing backslash is the
 * author's way of writing that one command over several lines. Every reader
 * below is asking a question about a COMMAND, so a scan that stopped at the
 * newline would answer it about a fragment: an archive build wrapped for width
 * would stop resolving, and the variable a lane's selection is computed from
 * would look unassigned. Folding first makes those readers see the command the
 * runner does.
 */
const joinContinuations = (text) =>
  // The whitespace on both sides of the break is the break's own layout, so the
  // fold collapses it to the one separator the shell sees.
  normalizeLines(text).replaceAll(/[^\S\n]*\\\n[^\S\n]*/gu, " ");

/** How many times `needle` occurs in `haystack`. */
const occurrences = (haystack, needle) => haystack.split(needle).length - 1;

/**
 * A confined repository path that may be a directory.
 *
 * `confinedFile` insists on a regular file, which is right for a fixture or a
 * control's subject. An evidence anchor may legitimately be a directory — a
 * crate, a fixture directory, an authority tree — so this keeps the same
 * confinement (no absolute path, no traversal, resolved target inside the root)
 * while accepting either kind of entry.
 */
function confinedEntry(root, relative, label) {
  const parts = relative.split(/[\\/]/u);
  if (path.isAbsolute(relative) || parts.some((part) => part === "" || part === ".."))
    throw new Error(`${label}: unsafe path: ${relative}`);
  const rootReal = fs.realpathSync(path.resolve(root));
  const real = fs.realpathSync(path.resolve(rootReal, ...parts));
  if (real !== rootReal && !real.startsWith(rootReal + path.sep))
    throw new Error(`${label}: path is not confined: ${relative}`);
  return real;
}

/**
 * Section headings of a Markdown authority document, with their body text.
 *
 * A duplicated heading is reported rather than collapsed: two identically
 * titled sections would otherwise silently resolve to the later one, so an atom
 * bound to the first would be validated against the second's content.
 */
function markdownSections(text) {
  const sections = new Map();
  const duplicates = new Set();
  let current = null;
  for (const line of normalizeLines(text).split("\n")) {
    const heading = line.match(/^#{2,6}\s+(.+?)\s*$/u);
    if (heading) {
      current = heading[1];
      if (sections.has(current)) duplicates.add(current);
      else sections.set(current, []);
      continue;
    }
    if (current && line.trim() && sections.has(current)) sections.get(current).push(line.trim());
  }
  return {
    body: new Map([...sections].map(([name, lines]) => [name, lines.join(" ")])),
    duplicates,
  };
}

/** Collapse runs of whitespace so an anchor is matched against prose, not layout. */
const flatten = (text) => text.replaceAll(/\s+/gu, " ").trim();

/**
 * The pinned digest of a proposition the register asserts.
 *
 * Whitespace is normalised first, so reflowing a paragraph is not a change and
 * rewriting what it says is.
 */
export const statementDigest = (text) =>
  createHash("sha256")
    .update(flatten(String(text)))
    .digest("hex")
    .slice(0, 16);

/**
 * The pinned digest of where an atom POINTS.
 *
 * Separate from the proposition digest because the two fail differently: a
 * rewritten statement says something new, a repointed anchor leaves the same
 * words resting on a different artifact. Each field is whitespace-normalised
 * for the same reason the statement is — the contract anchor is matched against
 * flattened prose, so reflowing it changes nothing this validator compares.
 */
export const anchorDigest = (atom) =>
  createHash("sha256")
    .update(
      JSON.stringify(
        ["evidence_anchor", "contract_section", "contract_anchor", "shipped_obligation"].map(
          (field) => (atom[field] ? flatten(atom[field]) : null),
        ),
      ),
    )
    .digest("hex")
    .slice(0, 16);

/**
 * Acceptance criteria a charter body actually declares, as identifier -> role.
 *
 * Every charter states its four slots as `- **<NODE>-AC<n> — <role>:** ...`.
 * The role is the discriminating half: the identifier alone is interchangeable
 * across the whole DAG, the role is what says which obligation the slot bears.
 */
function charterCriteria(text) {
  const found = new Map();
  for (const match of text.matchAll(/\*\*([A-Z][A-Z0-9]*)-AC(\d+)\s+—\s+([^:*]+):\*\*/gu))
    found.set(`${match[1]}-AC${match[2]}`, match[3].trim());
  return found;
}

/**
 * Every owner statement a charter makes about the capability it moves.
 *
 * A charter says this twice, in two registers, and only one of them is
 * generated. `owner=` is a header field a generator writes; the outcome
 * narrative is the sentence a reader actually meets, naming the owner being
 * displaced and the one that ends up sole. Resolving the header alone would
 * leave the narrative half unread, so a charter whose header had been
 * regenerated while its prose still handed the capability to the displaced
 * owner would pass — which is the defect this check exists to refuse, in the
 * form a reader would hit it.
 *
 * `header` is the generated field. `displaced` and `final` are the pair the
 * narrative states, or `null` when it states no pair at all; an absent
 * narrative is a defect rather than a silence, because the sentence is the
 * charter's own statement of the boundary it accepts.
 */
export const OUTCOME_SECTION = "Independently acceptable outcome";

function charterOwners(text) {
  const header = normalizeLines(text).match(/^owner=(.+)$/mu);
  const sections = markdownSections(text);
  // Scoped to the section that OWNS the statement, not to the document. Reading
  // the first matching sentence anywhere in flattened charter text accepts a
  // compliant sentence written above a stale one and reports the charter clean,
  // which is the defect this resolution exists to refuse rather than a corner
  // of it. A duplicated heading is refused for the same reason: two sections of
  // that name would silently resolve to the later one.
  const duplicated = sections.duplicates.has(OUTCOME_SECTION);
  const body = sections.body.get(OUTCOME_SECTION);
  const pairs = body
    ? [
        ...flatten(body).matchAll(
          /The current owner is \*\*(.+?)\*\*\. The final and sole owner is \*\*(.+?)\*\*\./gu,
        ),
      ].map((match) => ({ displaced: match[1].trim(), final: match[2].trim() }))
    : [];
  const distinct = [...new Set(pairs.map((pair) => `${pair.displaced} => ${pair.final}`))];
  return {
    header: header ? header[1].trim() : null,
    // A section stating two different pairs states neither: one of them is
    // stale, and nothing in the text says which.
    conflict: duplicated || distinct.length > 1,
    duplicated,
    displaced: distinct.length === 1 ? pairs[0].displaced : null,
    final: distinct.length === 1 ? pairs[0].final : null,
  };
}

/**
 * The production paths a charter declares it may change.
 *
 * A charter states this once, as the `Production surfaces:` entry of its
 * concrete-surfaces section, and its mutation boundary is defined against it.
 * Reading it is what makes "this block can discharge that obligation" a
 * resolved fact rather than an author's assertion: an obligation carried to a
 * criterion whose owner may not touch the surface that has to change is a
 * carry to nobody.
 */
export function charterSurfaces(text) {
  const body = markdownSections(text).body.get("Concrete surfaces and APIs");
  if (body === undefined) return null;
  const line = flatten(body).match(/Production surfaces:([^.]*)\./u);
  if (!line) return null;
  return [...line[1].matchAll(/`([^`]+)`/gu)].map((match) => match[1]);
}

/** True when a declared surface contains (or is) a repository-relative path. */
export const surfaceContains = (surface, target) =>
  surface === target || target.startsWith(`${surface}/`);

/**
 * The capability half of a `<train>:<capability>` owner identifier.
 *
 * The header names the owner train-qualified and the narrative names the
 * capability alone, so comparing the two requires reading the same half of
 * both. A value carrying no separator is its own capability.
 */
export const ownerCapability = (owner) => owner.slice(owner.indexOf(":") + 1);

/**
 * The artifacts a proof's recorded command is the VERDICT FOR — the things this
 * record's transcript is a statement about. Derived from the adapter plus the
 * record's tail; nothing here is author-declared.
 *
 * What makes an artifact a producer is that the transcript reports on it: the
 * tool a `node <tool>` record invokes prints that transcript itself, and the
 * packages a cargo selector names are what its summary counts. A path-shaped
 * argument is one such artifact, but it is not the only one, and for the runner
 * half of the allowlist it never appears: a `cargo` command selects work with
 * `-p <package>`, never with a path. Deriving producers from path arguments
 * alone therefore left every cargo record with an empty producer set, so the
 * acyclicity rule below could not fire for them at all — the rule would have
 * been vacuous on the adapter the Rust successors use. A selected package's
 * crate root is exactly the artifact its run is the verdict for, so it is a
 * producer too.
 *
 * A module a test file IMPORTS is deliberately not one. `node --test <file>`
 * transcribes the harness's verdict on that file's assertions, so the artifact
 * the record reports on is the test, and a module it exercises is an input to
 * those assertions rather than the thing being reported. That is the difference
 * between running this validator to certify itself — which is barred, because
 * the transcript would then be the subject's own output — and an adversarial
 * suite whose every case passes only when the validator REFUSES a mutation,
 * which a broken permissive validator fails rather than passes.
 */
export function verdictProducers(adapter, proof) {
  const argv = [...adapter.argv_prefix, ...proof.argv_tail];
  const producers = argv.filter((token) => token.includes("/") && /\.[a-z]+$/u.test(token));
  if (adapter.runner === "cargo")
    for (const [index, token] of argv.entries())
      if (token === "-p" && argv[index + 1]) producers.push(`crates/${argv[index + 1]}`);
  return producers;
}

/**
 * The first-party modules a node record's own entry file IMPORTS, as
 * repo-relative paths.
 *
 * A path argument is what a record's transcript reports on, and an imported
 * module is an input to the assertions that transcript reports. That
 * distinction is real, but on its own it leaves a hole with a name: a suite
 * whose cases call a module directly EXECUTES that module, so a claim whose
 * subject is that module can be proven by a record whose run is the subject's
 * own behaviour. Deriving the import edge is what makes the exemption below a
 * pinned, reviewable decision instead of a silence.
 *
 * Only relative specifiers are read. A package specifier names a dependency,
 * not an artifact of this repository, and no claim here takes one as a subject.
 */
export function importedModules(repoRoot, entry) {
  const start = path.resolve(repoRoot, entry);
  const seen = new Set([start]);
  const queue = [start];
  const found = [];
  // The graph is walked TRANSITIVELY. Stopping at the entry file's own
  // specifiers reads one re-export as a boundary: `suite -> shim -> subject`
  // reports the shim and not the subject, so the cycle the rule exists to
  // refuse disappears behind a file that does nothing but forward. That
  // direction is the unsound one — it NARROWS what counts as executed — so the
  // walk continues through every first-party module it reaches, with a visited
  // set for the cycles a module graph is allowed to have.
  //
  // A specifier is read wherever it appears, including across the lines of a
  // multi-line named import and inside a dynamic `import()`, and an import is
  // treated as execution. Both over-read, and both fail closed: they can only
  // widen what counts as executed, never narrow it.
  //
  // Only relative specifiers are followed. A package specifier names a
  // dependency, not an artifact of this repository, and no claim here takes one
  // as a subject.
  while (queue.length) {
    const file = queue.shift();
    let text;
    try {
      text = fs.readFileSync(file, "utf8");
    } catch {
      continue;
    }
    const from = path.dirname(file);
    for (const pattern of [
      /\bfrom\s*["'](\.[^"']+)["']/gu,
      /\bimport\s*\(?\s*["'](\.[^"']+)["']/gu,
    ])
      for (const match of text.matchAll(pattern)) {
        const target = path.resolve(from, match[1]);
        if (seen.has(target)) continue;
        seen.add(target);
        queue.push(target);
        found.push(path.relative(repoRoot, target).replaceAll("\\", "/"));
      }
  }
  return found;
}

/**
 * The INSTALLED packages an entry file's first-party module graph reaches.
 *
 * The counterpart of `importedModules`, over the specifiers that walk stops at.
 * A bare specifier names a dependency rather than an artifact of this
 * repository, which is why the module walk ignores it — but it is exactly what
 * decides whether a script can RUN in the job that hosts this instrument. That
 * job installs no dependencies and no toolchain, deliberately: the instrument
 * is node-only and portable, and every script it executes today is
 * dependency-free. Nothing enforced that, so a package import added to a lane
 * selector would keep resolving on a developer's tree and break a required job
 * that never ran on the pull request that added it. Builtins are excluded —
 * `node:` is always available — and the walk is the same over-reading one, so
 * it can only widen what counts as a dependency.
 */
const PACKAGE_SPECIFIER =
  /^(?:@[a-z0-9-][a-z0-9._-]*\/)?[a-z0-9-][a-z0-9._-]*(?:\/[a-zA-Z0-9._-]+)*$/u;

export function externalSpecifiers(repoRoot, entry) {
  const files = [entry, ...importedModules(repoRoot, entry)];
  const found = new Set();
  for (const file of files) {
    let text;
    try {
      text = fs.readFileSync(path.resolve(repoRoot, file), "utf8");
    } catch {
      continue;
    }
    for (const pattern of [
      /\bfrom\s*["']([^"'.][^"']*)["']/gu,
      /\bimport\s*\(?\s*["']([^"'.][^"']*)["']/gu,
    ])
      for (const match of text.matchAll(pattern))
        if (!match[1].startsWith("node:") && PACKAGE_SPECIFIER.test(match[1])) found.add(match[1]);
  }
  return [...found].sort();
}

/**
 * True when running `producer` is running the artifact `subject` names.
 *
 * Containment holds in both directions on purpose. A producer that is a crate
 * root runs every file beneath it, so it is the verdict for a subject inside
 * it; a producer that is one file inside a subject tree is still the verdict
 * for part of what that subject names. Either way the record cannot also be
 * independent evidence that the subject behaves correctly.
 */
export function producerCoversSubject(producer, subject) {
  return (
    producer === subject || subject.startsWith(`${producer}/`) || producer.startsWith(`${subject}/`)
  );
}

/**
 * The repository artifacts a record's own run exercises: the fixtures it names,
 * the path-shaped arguments its command carries, and the crate root of every
 * package its selector names. This is what makes RELEVANCE derivable — an atom
 * declares the artifact its evidence must exercise, and a record may only cover
 * that atom when this surface reaches it. Nothing here is volunteered by the
 * proof beyond the command and fixtures it already had to state.
 */
export function evidenceSurface(adapter, proof) {
  const surface = new Set(proof.fixtures);
  const argv = [...adapter.argv_prefix, ...proof.argv_tail];
  for (const [index, token] of argv.entries()) {
    if (token.includes("/")) surface.add(token);
    if (adapter.runner === "cargo" && token === "-p" && argv[index + 1])
      surface.add(`crates/${argv[index + 1]}`);
  }
  return [...surface];
}

/**
 * The packages a record's OWN command selects.
 *
 * This is the half a lane's selection has to be related to. Without it the
 * lane resolution could only ask how broad the lane is, and a lane narrowed
 * to an unrelated package answers that question just as well as one that runs
 * the record.
 */
export function selectedPackages(adapter, proof) {
  if (adapter.runner !== "cargo") return [];
  const argv = [...adapter.argv_prefix, ...proof.argv_tail];
  const found = [];
  for (const [index, token] of argv.entries())
    if ((token === "-p" || token === "--package") && argv[index + 1]) found.push(argv[index + 1]);
  return found;
}

/** True when a record's evidence surface reaches the artifact an atom anchors to. */
export function surfaceReaches(surface, anchor) {
  return surface.some(
    (entry) => entry === anchor || entry.startsWith(`${anchor}/`) || anchor.startsWith(`${entry}/`),
  );
}

const counts = (selected, executed, passed, failed, skipped) => ({
  selected,
  executed,
  passed,
  failed,
  skipped,
});

/**
 * The five counts a runner's own terminal summary states, or `null` when the
 * recorded text is not a summary of the declared shape.
 *
 * Nothing here reads the record's authored numbers: this is the independent
 * half of the comparison the validator then makes.
 *
 * A bracketed elapsed time is not part of the transcription. The nextest and
 * libtest grammars match the duration position opaquely (`[^\]]*`), and no
 * comparison — the static count check, the instrument lane's re-execution, or
 * a control lane's clean run — ever reads it: no two runs of the same suite
 * report the same duration, so a transcribed one could only be a stale
 * literal from an earlier run presenting as evidence. A record is free to
 * state that absence in place of a fabricated time.
 */
export function parseTerminalSummary(grammar, summary, countKey) {
  const text = stripStyling(normalizeLines(summary));
  if (grammar === "libtest") {
    // The verdict word is part of the grammar, not decoration. libtest prints
    // `FAILED` for a run that did not clear even when the failure is not
    // expressible in the three counts — a harness abort, a binary that could
    // not start — so accepting the word and then reading `0 failed` off the
    // same line admits a transcribed failing run as evidence.
    const rows = [
      ...text.matchAll(
        /test result:\s*ok\.\s*(\d+)\s+passed;\s*(\d+)\s+failed;\s*(\d+)\s+ignored;/gu,
      ),
    ];
    if (!rows.length) return null;
    // A multi-binary transcription is one record. If any binary reported
    // FAILED, the transcription is a failing run however green its siblings
    // are, so the whole text must be free of the failing verdict.
    if (/test result:\s*FAILED\./u.test(text)) return null;
    let passed = 0;
    let failed = 0;
    let ignored = 0;
    for (const row of rows) {
      passed += Number(row[1]);
      failed += Number(row[2]);
      ignored += Number(row[3]);
    }
    return counts(passed + failed + ignored, passed + failed, passed, failed, ignored);
  }
  if (grammar === "nextest") {
    const row = text.match(
      /Summary\s*\[[^\]]*\]\s*(\d+)\s+tests run:\s*(\d+)\s+passed(?:\s*\([^)]*\))?(?:,\s*(\d+)\s+failed)?,\s*(\d+)\s+skipped/u,
    );
    if (!row) return null;
    const executed = Number(row[1]);
    const passed = Number(row[2]);
    const failed = row[3] === undefined ? executed - passed : Number(row[3]);
    const skipped = Number(row[4]);
    return counts(executed + skipped, executed, passed, failed, skipped);
  }
  if (grammar === "node-test") {
    const observed = {};
    for (const name of ["tests", "pass", "fail", "cancelled", "skipped", "todo"]) {
      // Each count opens a line of the runner's terminal block, optionally
      // behind the reporter's one-glyph marker, or follows a pipe when the
      // block has been transcribed onto one line. Anchoring to that opening is
      // what stops a count being read out of a test NAME further along a line.
      const row = text.match(
        new RegExp(String.raw`(?:^|\|)[^\S\n]*(?:[^\s|]{1,2}[^\S\n]+)?${name}[^\S\n]+(\d+)`, "mu"),
      );
      if (!row) return null;
      observed[name] = Number(row[1]);
    }
    // A cancelled case did not pass and a todo case did not run. Folding them
    // into failed and skipped keeps an interrupted run from reading as a clean
    // one that merely selected less work.
    const passed = observed.pass;
    const failed = observed.fail + observed.cancelled;
    const skipped = observed.skipped + observed.todo;
    return counts(observed.tests, passed + failed, passed, failed, skipped);
  }
  if (grammar === "tool-line") {
    if (!countKey) return null;
    // A tool prints its own verdict word. Transcribing a line without it would
    // let a failing run be recorded as evidence.
    if (!/^[a-z][a-z0-9-]*:\s+PASS\b/u.test(text.trim())) return null;
    const row = text.match(new RegExp(String.raw`\b${countKey}=(\d+)\b`, "u"));
    if (!row) return null;
    const total = Number(row[1]);
    return counts(total, total, total, 0, 0);
  }
  if (grammar === "compile-contracts") {
    // The runner announces its fixture count BEFORE running trybuild, so the
    // banner alone says only how much work was selected — a run that fails
    // afterwards still printed it. The passes are therefore counted from the
    // per-case `... ok` lines the runner emits as each case clears, and any
    // case that never reported one is a failure the record must carry.
    const row = text.match(/compile contracts:\s*owner=\S+,\s*fixtures=(\d+)/u);
    if (!row) return null;
    const total = Number(row[1]);
    const passed = [...text.matchAll(/^test\s+\S+\s+\.\.\.\s+ok\s*$/gmu)].length;
    if (passed > total) return null;
    return counts(total, total, passed, total - passed, 0);
  }
  return null;
}

/**
 * The REFUSAL a negative control observed, read out of the runner's own
 * terminal output, or `null` when the recorded text is not one.
 *
 * A control's `observed` field used to be free prose, constrained only by not
 * parsing as a transcript this validator would admit. Any sentence at all
 * satisfied that, so a mutation that was never planted, or planted and never
 * run, could be recorded with an invented description of a failure nobody saw
 * — which is the whole of what a negative control is supposed to establish, and
 * exactly the unapplied-mutation class this instrument declares.
 *
 * The recorded outcome must therefore be the runner's own refusal, in the
 * runner's own grammar, and each grammar has one:
 *
 *   - `libtest` prints `test result: FAILED.` with a nonzero failed count;
 *   - `nextest` prints its `Summary` line, either with a failed count or with
 *     zero tests run, which is the refusal a selector matching nothing yields;
 *   - `node:test` prints its counted block with a nonzero fail or cancelled;
 *   - a `tool-line` runner prints its errors on `ERROR:` lines and exits
 *     nonzero, which is the only refusal channel those tools have;
 *   - the compile-contract runner prints its fixture banner and then either
 *     trybuild's `N of M tests failed` line or fewer `... ok` lines than the
 *     banner announced.
 *
 * None of those can be produced by describing a failure: they are transcribed
 * from a run that happened. This does not make the transcription tamper-proof
 * — nothing available to a node-only instrument does — it moves the bar from
 * "any sentence" to "the runner's own refusal, in its own shape, consistent
 * with the mutation's own subject and the uniqueness checks beside it".
 */
export function parseRefusal(grammar, observed, countKey) {
  const text = stripStyling(normalizeLines(observed ?? ""));
  if (grammar === "libtest") {
    // Every `test result:` line the run emitted, not only the failing ones.
    // A failed count alone is the weakest thing a libtest refusal states, and
    // it is the one number a stale transcript is most likely to still get
    // right: a suite that grew by a case still fails exactly once under the
    // same mutation, so comparing `failed` greens a transcript whose passing
    // count, ignored count and binary list have all moved. The whole shape is
    // aggregated instead, and order-independently — a multi-binary run does
    // not fix the order its binaries report in.
    const rows = [
      ...text.matchAll(
        /test result:\s*(ok|FAILED)\.\s*(\d+)\s+passed;\s*(\d+)\s+failed;\s*(\d+)\s+ignored;/gu,
      ),
    ];
    if (!rows.length) return null;
    let passed = 0;
    let failed = 0;
    let ignored = 0;
    let failing = 0;
    for (const row of rows) {
      if (row[1] === "FAILED") failing += 1;
      passed += Number(row[2]);
      failed += Number(row[3]);
      ignored += Number(row[4]);
    }
    // A binary can report `FAILED` with a zero failed count — a harness abort,
    // or a binary that never started — so the verdict word is part of the
    // refusal rather than a decoration over the counts.
    return failing > 0 ? { binaries: rows.length, failing, passed, failed, ignored } : null;
  }
  if (grammar === "nextest") {
    const row = text.match(
      /Summary\s*\[[^\]]*\]\s*(\d+)\s+tests run:\s*(\d+)\s+passed(?:\s*\([^)]*\))?(?:,\s*(\d+)\s+failed)?,\s*(\d+)\s+skipped/u,
    );
    if (!row) return null;
    const executed = Number(row[1]);
    const passed = Number(row[2]);
    const failed = row[3] === undefined ? executed - passed : Number(row[3]);
    const skipped = Number(row[4]);
    if (failed > 0) return { executed, passed, failed, skipped };
    return executed === 0 ? { executed, passed, failed, skipped, selectedNothing: true } : null;
  }
  if (grammar === "node-test") {
    const counted = parseTerminalSummary("node-test", text, countKey);
    return counted && counted.failed > 0 ? { ...counted } : null;
  }
  if (grammar === "tool-line") {
    // A tool with no counted block states its refusal as the errors it printed,
    // so those lines ARE the outcome. "It printed at least one error" would
    // accept a refusal for some entirely different reason as the transcribed
    // one. Sorted, because a tool is free to reorder its own diagnostics.
    const errors = [...text.matchAll(/^ERROR:\s*(\S.*?)\s*$/gmu)].map((row) => row[1]).sort();
    return errors.length ? { errors } : null;
  }
  if (grammar === "compile-contracts") {
    const banner = text.match(/compile contracts:\s*owner=\S+,\s*fixtures=(\d+)/u);
    if (!banner) return null;
    const fixtures = Number(banner[1]);
    const cleared = [...text.matchAll(/^test\s+\S+\s+\.\.\.\s+ok\s*$/gmu)].length;
    const reported = text.match(/(\d+)\s+of\s+(\d+)\s+tests failed/u);
    if (reported && Number(reported[1]) > 0)
      return { fixtures, cleared, failed: Number(reported[1]) };
    return cleared < fixtures ? { fixtures, cleared, failed: fixtures - cleared } : null;
  }
  return null;
}

/**
 * The path patterns a `dorny/paths-filter` filter declares, or `null` when the
 * workflow declares no such filter.
 *
 * The workflow is declared configuration, not source: this reads the filter's
 * own block by indentation rather than searching a tree for a spelling.
 */
export function triggerPaths(workflow, filterName) {
  const lines = normalizeLines(workflow).split("\n");
  const head = lines.findIndex((line) =>
    new RegExp(String.raw`^\s+${filterName}:\s*$`, "u").test(line),
  );
  if (head === -1) return null;
  const indent = lines[head].length - lines[head].trimStart().length;
  const patterns = [];
  for (const line of lines.slice(head + 1)) {
    if (!line.trim()) continue;
    if (line.length - line.trimStart().length <= indent) break;
    if (line.trim().startsWith("#")) continue;
    // Every quoting a YAML sequence entry admits, because none of them changes
    // the pattern and stopping at one truncates the list: a filter read as
    // narrower than it is turns a covered artifact into a hard error.
    const entry = line.trim().match(/^-\s*(?:'([^']+)'|"([^"]+)"|([^'"#\s][^#]*?))\s*(?:#.*)?$/u);
    if (!entry) return patterns.length ? patterns : null;
    patterns.push((entry[1] ?? entry[2] ?? entry[3]).trim());
  }
  return patterns;
}

/** The workflow's jobs, as job id to body text. */
export function workflowJobs(workflow) {
  const jobs = new Map();
  let inJobs = false;
  let current = null;
  let body = [];
  const flush = () => {
    if (current) jobs.set(current, body.join("\n"));
    current = null;
    body = [];
  };
  for (const line of normalizeLines(workflow).split("\n")) {
    if (/^jobs:\s*$/u.test(line)) {
      inJobs = true;
      continue;
    }
    if (!inJobs) continue;
    if (line.trim() && /^\S/u.test(line)) {
      flush();
      inJobs = false;
      continue;
    }
    const head = line.match(/^ {2}([A-Za-z0-9_-]+):\s*$/u);
    if (head) {
      flush();
      current = head[1];
      continue;
    }
    if (current) body.push(line);
  }
  flush();
  return jobs;
}

/**
 * A pattern whose only wildcard is one `*` inside its final path segment, split
 * into the literal text on either side of that star, or `null` when the pattern
 * is not of that shape.
 *
 * `dorny/paths-filter` accepts this form and the live lanes use it, so treating
 * it as unreadable would report a resolvable pattern as unresolved and, worse,
 * leave an artifact it really does cover looking uncovered.
 */
function segmentGlob(pattern) {
  const slash = pattern.lastIndexOf("/");
  const head = slash === -1 ? "" : pattern.slice(0, slash + 1);
  const tail = pattern.slice(slash + 1);
  if (head.includes("*")) return null;
  const star = tail.indexOf("*");
  if (star === -1 || tail.indexOf("*", star + 1) !== -1) return null;
  return { prefix: head + tail.slice(0, star), suffix: tail.slice(star + 1) };
}

/**
 * True when a pattern this check resolves matches a repository-relative path.
 *
 * Three shapes are resolved: an exact path, a `prefix/**` subtree whose prefix
 * is literal, and a single `*` inside a final path segment. Any other glob is
 * reported as unresolved by `unresolvedTriggerPatterns` and is treated here as
 * a non-match — the conservative direction, since a pattern this check cannot
 * read must never be credited with coverage it may not have. A `prefix/**`
 * whose prefix itself carries a star is one of those: the trailing subtree is
 * readable but the prefix is not, so it is a non-match here rather than a
 * literal prefix that would silently match nothing.
 */
function positiveTriggerMatch(pattern, target) {
  if (pattern === target) return true;
  if (pattern.endsWith("/**")) {
    if (pattern.slice(0, -3).includes("*")) return false;
    // A cited artifact may itself be the subtree root — an evidence anchor is
    // often a crate or a fixture directory. `prefix/**` is what makes any change
    // beneath that root re-run the lane, which is exactly the coverage the root
    // needs, so the root matches its own subtree pattern.
    return target.startsWith(pattern.slice(0, -2)) || target === pattern.slice(0, -3);
  }
  const glob = segmentGlob(pattern);
  if (!glob) return false;
  if (target.length < glob.prefix.length + glob.suffix.length) return false;
  if (!target.startsWith(glob.prefix) || !target.endsWith(glob.suffix)) return false;
  // A star inside one segment does not cross a directory boundary.
  return !target.slice(glob.prefix.length, target.length - glob.suffix.length).includes("/");
}

/**
 * True when a filter covers a repository-relative path.
 *
 * `dorny/paths-filter` patterns may be NEGATIONS: a leading `!` excludes paths
 * that would otherwise match. Reading a negation as an ordinary pattern is the
 * one unsound direction available here — it would credit a record with coverage
 * the lane explicitly excludes — so an exclusion is modelled rather than
 * ignored: a path is covered when some positive pattern matches it and no
 * negation does.
 */
export function triggerCovers(patterns, target) {
  const positive = patterns.filter((pattern) => !pattern.startsWith("!"));
  const negative = patterns.filter((pattern) => pattern.startsWith("!"));
  if (negative.some((pattern) => positiveTriggerMatch(pattern.slice(1), target))) return false;
  return positive.some((pattern) => positiveTriggerMatch(pattern, target));
}

/**
 * The patterns of a filter whose shape this check cannot resolve.
 *
 * A filter is declared configuration a record leans on, so a pattern that is
 * none of the three readable shapes is reported wherever a filter is consulted
 * — the instrument's own and every refreshing lane's — rather than only in the
 * instrument's own filter: silently treating it as a literal that matches
 * nothing is safe for coverage but hides the fact that the lane's real reach is
 * unknown to this check. A trailing `/**` is not on its own enough to be
 * readable: a pattern whose prefix carries its own star names one subtree per
 * matching directory, which `positiveTriggerMatch` cannot expand, so it is
 * reported here rather than quietly matching nothing.
 */
export function unresolvedTriggerPatterns(patterns) {
  return patterns.filter((pattern) => {
    const body = pattern.startsWith("!") ? pattern.slice(1) : pattern;
    if (!body.includes("*")) return false;
    if (body.endsWith("/**")) return body.slice(0, -3).includes("*");
    return segmentGlob(body) === null;
  });
}

/**
 * The command line a job issues, located by the command a record declares.
 *
 * A step is either `- run: <command>` or a line of a block scalar, so the
 * leading list marker and `run:` key are stripped and what remains is the
 * command as the runner receives it. Returning the WHOLE line is the point: a
 * declaration that stops before the lane's own selection arguments would
 * otherwise satisfy a containment check while hiding every flag that decides
 * what the lane actually runs.
 */
export function laneCommandLine(body, command) {
  for (const raw of joinContinuations(body).split("\n")) {
    const line = raw
      .trim()
      .replace(/^-\s+/u, "")
      .replace(/^run:\s*/u, "")
      .trim();
    if (line.includes(command)) return line;
  }
  return null;
}

/**
 * The jobs a job declares it needs, in every form the workflow schema admits:
 * the inline list, the bare scalar, and the block sequence.
 *
 * The block form is not a formatting variant this check may skip. An author who
 * reflows `needs:` onto its own lines changes nothing about what the lane
 * consumes, so a reader that saw only the first entry — with its list marker
 * still attached — would derive a limit, turn a claim bounded, and report a
 * cause with no relation to the evidence.
 *
 * A comment interleaved with the entries is the same case. It is not an entry
 * and it is not the end of the block, so it is skipped rather than treated as
 * the terminator: stopping there truncates the list at whatever the author
 * happened to annotate, which produces exactly the false limit above.
 */
export function jobNeeds(body) {
  const lines = normalizeLines(body).split("\n");
  const index = lines.findIndex((line) => /^\s*needs:/u.test(line));
  if (index === -1) return [];
  const clean = (token) =>
    token
      .trim()
      // A trailing `# ...` annotates the entry; it is not part of the job id.
      .replace(/\s+#.*$/u, "")
      .trim()
      .replaceAll(/^['"]|['"]$/gu, "");
  const inline = lines[index].replace(/^\s*needs:\s*/u, "").trim();
  if (inline) {
    const inner = inline.startsWith("[") ? inline.slice(1, inline.lastIndexOf("]")) : inline;
    return inner.split(",").map(clean).filter(Boolean);
  }
  const indent = lines[index].match(/^\s*/u)[0].length;
  const found = [];
  for (const line of lines.slice(index + 1)) {
    if (!line.trim()) continue;
    if (line.match(/^\s*/u)[0].length <= indent) break;
    if (line.trim().startsWith("#")) continue;
    const entry = line.trim().match(/^-\s*(.+)$/u);
    if (!entry) break;
    found.push(clean(entry[1]));
  }
  return found.filter(Boolean);
}

/**
 * The command a job issues that BUILDS `archive` over the whole workspace.
 *
 * Two substring hits on one line are not a build. A comment explaining the step,
 * or an `echo` reporting it, carries both the breadth flag and the archive path,
 * so a reader keyed on those alone certifies an archive no job produces, and the
 * consuming lane then resolves against nothing. What makes a line the producer
 * is that it RUNS the runner's archive subcommand over the whole workspace and
 * names that exact archive as its output.
 */
export function archiveBuildCommand(body, archive) {
  for (const raw of joinContinuations(body).split("\n")) {
    const line = raw
      .trim()
      .replace(/^-\s+/u, "")
      .replace(/^run:\s*/u, "")
      .trim();
    if (!line || line.startsWith("#")) continue;
    const tokens = line.split(/\s+/u).map((token) => token.replaceAll(/^['"]|['"]$/gu, ""));
    // A quoted comment is still a comment: the marker survives quote
    // stripping even though the raw line begins with the quote.
    if (tokens[0].startsWith("#") || tokens[0] === "echo" || tokens[0] === ":") continue;
    const flag = tokens.findIndex(
      (token, at) =>
        token === `--archive-file=${archive}` ||
        (token === "--archive-file" && tokens[at + 1] === archive),
    );
    if (flag === -1 || !tokens.includes("--workspace")) continue;
    // `archive` is the subcommand that WRITES the file and `run` is the one that
    // reads it. Both name the same flag, so the subcommand is what separates the
    // producer from another consumer.
    if (!tokens.slice(0, flag).includes("archive")) continue;
    return line;
  }
  return null;
}

/**
 * The command a job's own step computes `$NAME` from, or `null`.
 *
 * A lane whose selection is spelled `$SOME_VAR` publishes a variable name, which
 * tells a reader nothing about what is excluded. Resolving the assignment to the
 * command that produces it is what makes the published narrowing legible. It is
 * deliberately NOT an evaluation of the result: what that command yields is a
 * property of the script it runs, which this check does not execute.
 */
export function laneVariableSource(body, name) {
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/u.test(name)) return null;
  // `export` is one way to write the assignment and not the property that makes
  // it one. A step that drops it — or writes it as a `NAME=$(...)` prefix — has
  // changed nothing about where the lane's selection comes from, so keying on
  // the keyword turned a semantically irrelevant shell edit into a silently
  // unresolved producer.
  const assign = joinContinuations(body).match(
    new RegExp(
      String.raw`^\s*(?:-\s+)?(?:run:\s*)?(?:export\s+)?` +
        name +
        String.raw`=["']?\$\((.+?)\)["']?\s*$`,
      "mu",
    ),
  );
  return assign ? assign[1].trim() : null;
}

/**
 * The arguments of a command that NARROW what its runner selects, grouped by
 * WHAT they narrow.
 *
 * The grouping is the whole content of the distinction below. A package or
 * target argument narrows the universe to something this check can compare
 * against the record's own selection, and a comparison that can be made must be
 * made rather than annotated. A filter expression or a shard partition narrows
 * WITHIN a universe, by a predicate over test names this check does not
 * evaluate; the honest treatment of one of those is to publish it and say that
 * it is unevaluated, not to fold it away and not to claim it was resolved.
 */
const PACKAGE_FLAGS = Object.freeze(["-p", "--package"]);
const EXCLUDE_FLAGS = Object.freeze(["--exclude"]);
const TARGET_FLAGS = Object.freeze(["--test", "--bin", "--example", "--bench"]);
const WITHIN_UNIVERSE_FLAGS = Object.freeze(["-E", "--filter-expr", "--partition"]);
const SELECTION_FLAGS = Object.freeze([
  ...PACKAGE_FLAGS,
  ...EXCLUDE_FLAGS,
  ...TARGET_FLAGS,
  ...WITHIN_UNIVERSE_FLAGS,
]);

/**
 * A command's selection arguments as `{ flag, values, text }`.
 *
 * A flag's value is the tokens up to the next flag, which keeps a quoted value
 * carrying spaces intact. `--` and everything after it is one part: it is the
 * harness-argument separator, and what follows it selects work in every runner
 * here.
 */
export function selectionParts(command) {
  const tokens = command.trim().split(/\s+/u);
  const found = [];
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token === "--") {
      if (index + 1 < tokens.length)
        found.push({
          flag: "--",
          values: tokens.slice(index + 1),
          text: tokens.slice(index).join(" "),
        });
      break;
    }
    const flag = token.includes("=") ? token.slice(0, token.indexOf("=")) : token;
    if (!SELECTION_FLAGS.includes(flag)) continue;
    if (token.includes("=")) {
      found.push({ flag, values: [token.slice(token.indexOf("=") + 1)], text: token });
      continue;
    }
    let cursor = index + 1;
    while (cursor < tokens.length && !tokens[cursor].startsWith("-")) cursor += 1;
    found.push({
      flag,
      values: tokens.slice(index + 1, cursor),
      text: tokens.slice(index, cursor).join(" "),
    });
    index = cursor - 1;
  }
  return found;
}

/** The same parts as the literal argument text, in command order. */
export function selectionArguments(command) {
  return selectionParts(command).map((part) => part.text);
}

/**
 * The operands a non-cargo command selects work with WITHOUT a flag.
 *
 * A runner whose selection is positional carries no flag this check knows, so
 * treating "no selection flag" as "the whole universe" would publish a false
 * exhaustiveness claim for a command that names one test file or one spec. The
 * program's own script or subcommand operand is not a selection — it IS the
 * program — so it is dropped, and anything after it is a selection.
 */
export function positionalOperands(command) {
  const tokens = command.trim().split(/\s+/u);
  const operands = [];
  for (let index = 1; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token === "--") break;
    if (token.startsWith("-")) {
      // A flag whose value is a separate token consumes it.
      if (!token.includes("=") && index + 1 < tokens.length && !tokens[index + 1].startsWith("-"))
        index += 1;
      continue;
    }
    operands.push(token);
  }
  // The first operand of an interpreter or task runner is the program it runs.
  return ["node", "npx", "pnpm", "npm", "yarn", "bun", "deno"].includes(tokens[0])
    ? operands.slice(1)
    : operands;
}

/**
 * Run a lane's own declared selection command and return what it printed.
 *
 * A lane whose selection is computed by a script is only resolvable by asking
 * that script, so this executes it — the same command the workflow issues, in
 * the repository it issues it from. Two things keep that narrow. The command
 * comes from the REGISTER, which declares it, and the workflow must agree; and
 * only the allowlisted node runner is executable, so the register cannot name a
 * shell, an interpreter, or anything else this instrument's adapter vocabulary
 * already refuses. A failure to run is a resolution failure, never a pass.
 */
/**
 * One run per command per root, for one analysis.
 *
 * Several records lean on the same lane, and a selector that printed different
 * things to different records would make the derivation depend on how many
 * times it was asked.
 *
 * The scope is ONE analysis, not the process. A second `analyze()` over the
 * same root — the shape the control suite runs, and the shape any in-process
 * re-entry has — must read the producer script as it is NOW; serving it the
 * first run's stdout would make a later derivation depend on an earlier
 * unrelated one. So the memo is cleared where an analysis begins.
 */
const laneSelectorRuns = new Map();

export function resetLaneSelectorRuns() {
  laneSelectorRuns.clear();
}

export function runLaneSelector(repoRoot, command) {
  const memo = JSON.stringify([repoRoot, command]);
  if (laneSelectorRuns.has(memo)) return laneSelectorRuns.get(memo);
  const answer = executeLaneSelector(repoRoot, command);
  laneSelectorRuns.set(memo, answer);
  return answer;
}

function executeLaneSelector(repoRoot, command) {
  const tokens = command.trim().split(/\s+/u);
  if (tokens[0] !== "node")
    return { ok: false, reason: `${JSON.stringify(command)} is not a node command` };
  const result = spawnSync(process.execPath, tokens.slice(1), {
    cwd: repoRoot,
    encoding: "utf8",
    timeout: 120_000,
    maxBuffer: 8 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.error) return { ok: false, reason: `could not run it: ${result.error.message}` };
  if (result.signal) return { ok: false, reason: `it was killed by ${result.signal}` };
  if (result.status !== 0) return { ok: false, reason: `it exited ${result.status}` };
  return { ok: true, stdout: String(result.stdout).trim() };
}

/** The index of the `)` closing the `(` at `open`, or -1. */
function matchingClose(text, open) {
  let depth = 0;
  for (let index = open; index < text.length; index += 1) {
    if (text[index] === "(") depth += 1;
    else if (text[index] === ")") {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return -1;
}

/** Strip the parentheses that wrap a whole expression, however many there are. */
function unwrapGroup(text) {
  let inner = text.trim();
  while (inner.startsWith("(") && matchingClose(inner, 0) === inner.length - 1)
    inner = inner.slice(1, -1).trim();
  return inner;
}

/** Split on an infix operator that sits outside every parenthesis group. */
function splitOutsideGroups(text, operator) {
  const marker = ` ${operator} `;
  const parts = [];
  let depth = 0;
  let current = "";
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] === "(") depth += 1;
    if (text[index] === ")") depth -= 1;
    if (depth === 0 && text.startsWith(marker, index)) {
      parts.push(current);
      current = "";
      index += marker.length - 1;
      continue;
    }
    current += text[index];
  }
  parts.push(current);
  return parts.map((part) => part.trim()).filter(Boolean);
}

/**
 * Every predicate head this reader knows, split by what it can say about a
 * WHOLE package.
 *
 * `package()` is the only head that names one outright. The name/target-scoped
 * heads restrict which cases inside a package run, which this reader lists and
 * deliberately does not evaluate — whether those names reach a record's own
 * cases is a property of the test binaries. Everything else moves the package
 * set itself (`deps`/`rdeps` expand it, `none` empties it, `all` is the
 * universe) and is not decomposable into which packages it drops.
 */
const NAME_SCOPED_HEADS = ["test", "binary", "binary_id", "kind", "platform"];
const SET_MOVING_HEADS = ["deps", "rdeps", "none", "all"];
const PREDICATE_HEAD = /\b(package|test|binary|binary_id|kind|platform|deps|rdeps|none|all)\s*\(/gu;

/** The predicate heads a sub-expression is built from, deduplicated. */
const predicateHeads = (text) => [
  ...new Set([...text.matchAll(PREDICATE_HEAD)].map((match) => match[1])),
];

/**
 * What a nextest filter expression does to whole PACKAGES, and what it leaves
 * narrowed within one.
 *
 * The distinction is the entire content of this reader. A lane that excludes a
 * package the record selects does not run that record's work, and that is a
 * limit. A lane that excludes some cases BY NAME inside a package it still runs
 * is a narrowing this check lists and does not evaluate, because whether those
 * names reach the record's own cases is a property of the test binaries rather
 * than of the expression.
 *
 * Two properties make that distinction hold rather than merely be intended.
 *
 * The operators are split in the expression language's own precedence — `or`,
 * then `and`, then `not` — so `not package(a) or package(b)` decomposes as
 * "not a, or b" rather than as "neither a nor b". Splitting conjunctions first
 * read a lower-precedence operator as the top of the tree and attributed an
 * exclusion to a disjunct that never carried one.
 *
 * And every leaf is CLASSIFIED. Recognising a predicate head and returning was
 * the hole: a positive `test(...)` restricts the whole run by name, `none()`
 * selects nothing at all, and `deps()`/`rdeps()` move the package set, yet each
 * decomposed to "excluding no package, with no test-name narrowing" — the same
 * silent breadth this reader exists to refuse, reached through a leaf instead
 * of through an unread variable. A leaf that is not `package()` and not the
 * `all()` universe is therefore unresolved, which is a resolution failure.
 *
 * `included` is non-null only when the expression positively restricts the run
 * to a package set; `unresolved` carries any conjunct whose shape this reader
 * cannot classify, and a non-empty `unresolved` is a resolution failure rather
 * than a silent pass.
 */
export function packageSelection(expression) {
  const excluded = new Set();
  const narrowed = [];
  const unresolved = [];
  let included = null;
  // What `not <operand>` takes away. A whole package is an exclusion this
  // reader relates to a record's own selection; a name- or target-scoped
  // predicate over one is a narrowing it publishes unevaluated; anything that
  // moves the package set is neither, and is refused rather than dropped.
  const negate = (disjunct) => {
    const inner = unwrapGroup(disjunct);
    const whole = inner.match(/^package\(([^()]+)\)$/u);
    if (whole) {
      excluded.add(whole[1]);
      return;
    }
    const heads = predicateHeads(inner);
    if (
      !heads.length ||
      /(^|\s)not\s/u.test(inner) ||
      heads.some((head) => SET_MOVING_HEADS.includes(head)) ||
      !heads.every((head) => head === "package" || NAME_SCOPED_HEADS.includes(head))
    ) {
      unresolved.push(inner);
      return;
    }
    narrowed.push(inner);
  };
  const visit = (node) => {
    const text = unwrapGroup(node);
    if (!text) return;
    // Lowest precedence first, so a tighter-binding operator is never read as
    // the root of the tree.
    const disjuncts = splitOutsideGroups(text, "or");
    if (disjuncts.length > 1) {
      const packages = disjuncts.map((disjunct) =>
        unwrapGroup(disjunct).match(/^package\(([^()]+)\)$/u),
      );
      if (packages.every(Boolean)) {
        const named = packages.map((match) => match[1]);
        included = included ? included.filter((name) => named.includes(name)) : named;
        return;
      }
      unresolved.push(text);
      return;
    }
    const conjuncts = splitOutsideGroups(text, "and");
    if (conjuncts.length > 1) {
      for (const conjunct of conjuncts) visit(conjunct);
      return;
    }
    if (/^not\s/u.test(text)) {
      for (const disjunct of splitOutsideGroups(unwrapGroup(text.replace(/^not\s+/u, "")), "or"))
        negate(disjunct);
      return;
    }
    const only = text.match(/^package\(([^()]+)\)$/u);
    if (only) {
      included = included ? included.filter((name) => name === only[1]) : [only[1]];
      return;
    }
    // `all()` is the universe and takes nothing away. Every other positive leaf
    // restricts the run in a way this reader cannot relate to a record's own
    // packages, so it is a resolution failure and not silent breadth.
    if (/^all\s*\(\s*\)$/u.test(text)) return;
    unresolved.push(text);
  };
  // An expression whose parentheses do not balance is not one this reader may
  // decompose at all: every split below would land in the wrong place.
  let depth = 0;
  for (const character of expression) {
    if (character === "(") depth += 1;
    if (character === ")") depth -= 1;
    if (depth < 0) break;
  }
  if (depth !== 0) return { excluded, narrowed, unresolved: [expression], included };
  visit(expression);
  return { excluded, narrowed, unresolved, included };
}

/**
 * The value a record's own command passes to a flag.
 *
 * This is the half a lane's enumerated universe has to contain: a runner that
 * iterates a declared list runs the record's work only when the record's own
 * selection is one of the entries.
 */
export function flagValue(adapter, proof, flag) {
  const argv = [...adapter.argv_prefix, ...proof.argv_tail];
  const index = argv.indexOf(flag);
  return index === -1 ? null : (argv[index + 1] ?? null);
}

/**
 * How a refreshing lane's SELECTION is resolved against the record's own.
 *
 * Naming a job that issues a command is not yet a statement that the job runs
 * the work the record transcribes: the lane may select a narrower universe than
 * the record's packages, in which case a change to one of them re-runs
 * something else. The resolution is therefore derived from the lane's COMPLETE
 * command line — the validator refuses a declaration that is only a prefix of
 * it — and it is derived in two stages.
 *
 * First, narrowing this check CAN relate to the record is related to it and
 * decides the result. A lane that names packages must name every package the
 * record selects; a lane that excludes one of them, or that narrows to a single
 * build target, does not run this record's work and is refused. Breadth alone
 * used to decide the whole answer, which accepted a lane narrowed to an
 * unrelated package as long as it consumed a whole-workspace archive.
 *
 * Second, the universe the lane draws from must reach the record at all, and
 * only three shapes resolve:
 *
 *   - the command selects the whole workspace outright;
 *   - the command consumes a prebuilt archive, some job in this workflow BUILDS
 *     that exact archive over the whole workspace with its archive subcommand,
 *     and the refreshing job declares that builder among its `needs`;
 *   - the command is not a cargo selector, carries no selection argument and no
 *     operand beyond the program it runs, and the register declares the
 *     enumeration that runner iterates — which this check RUNS, so the record's
 *     own selection is resolved against the list the lane's own script prints
 *     rather than against the phrase "its own default universe".
 *
 * A record's own `count_source` is deliberately NOT one of them. Re-deriving a
 * selected COUNT from a fixture directory is a property of the record and of
 * this validator; it says nothing about what the lane selects.
 *
 * What remains after both stages is within-universe narrowing — a filter
 * expression or a shard partition. A variable spelling is resolved to the
 * command that computes it, that command must be the one the register declares,
 * and it is EXECUTED: the expression it prints is decomposed into the packages
 * it excludes outright, which are related to this record's own selection and
 * turn the lane into a limit when one of them is selected here, and the
 * name-scoped exclusions it leaves inside a package the lane still runs, which
 * are published by package and explicitly NOT evaluated — whether those names
 * reach this record's cases is a property of the test binaries rather than of
 * the expression. A variable no step assigns, a producer that is not the
 * declared one, an expression this reader cannot decompose, or a command that
 * will not run are resolution FAILURES rather than silent breadth.
 *
 * `covers` is what the lane demonstrably runs, `how` is the derived sentence the
 * generated view publishes, and `citations` are the artifacts this resolution
 * had to read the lane's own value out of, cited so a change to them cannot go
 * unnoticed by the lanes that depend on this answer.
 */
export function refreshSelectionCoverage(adapter, jobs, recordPackages = [], options = {}) {
  const { repoRoot = null, laneSelection = null } = options;
  const command = adapter.refresh_command;
  const body = jobs.get(adapter.refresh_job) ?? "";
  const parts = selectionParts(command);
  const valuesOf = (flags) =>
    parts.filter((part) => flags.includes(part.flag)).flatMap((part) => part.values);
  const named = (items) => items.map((item) => "`" + item + "`").join(", ");

  const lanePackages = valuesOf(PACKAGE_FLAGS);
  if (lanePackages.length) {
    if (!recordPackages.length)
      return {
        covers: false,
        how: `the lane narrows to ${named(lanePackages)} and this record names no package that selection can be related to`,
        citations: [],
      };
    const missing = recordPackages.filter((name) => !lanePackages.includes(name));
    if (missing.length)
      return {
        covers: false,
        how: `the lane narrows to ${named(lanePackages)}, which does not include ${named(missing)}, so it does not run this record's work`,
        citations: [],
      };
  }
  const excluded = recordPackages.filter((name) => valuesOf(EXCLUDE_FLAGS).includes(name));
  if (excluded.length)
    return {
      covers: false,
      how: `the lane excludes ${named(excluded)}, which this record selects`,
      citations: [],
    };
  const targets = parts.filter((part) => TARGET_FLAGS.includes(part.flag));
  if (targets.length)
    return {
      covers: false,
      how: `the lane narrows to the ${named(targets.map((part) => part.text))} target, which this check cannot relate to this record's own selection`,
      citations: [],
    };

  // Within-universe narrowing, RESOLVED where its producer is declared and
  // published where it is not evaluable.
  //
  // Publishing a variable name and stopping was the hole: a predicate nobody
  // read was credited as breadth, so a lane could be narrowed to anything at
  // all and the derivation still said it re-ran this record. So the producer is
  // resolved out of the job, checked against the producer the register
  // declares, EXECUTED, and the expression it yields is decomposed: a package
  // this record selects that the predicate excludes is a limit, and a
  // name-scoped narrowing inside a package the lane still runs is listed by
  // package rather than evaluated, because whether those names reach this
  // record's cases is a property of the test binaries.
  const citations = [];
  // A cited script is an ENTRY, not the artifact the value came out of.
  //
  // Harvesting the path-shaped tokens of the executed command line cites the
  // file the lane names and stops there, and a lane selector is routinely a
  // thin entry: the package exclusions and the name-scoped selectors this
  // check decomposes and PUBLISHES are declared one or two relative imports
  // away. Citing the entry alone leaves those modules outside the instrument's
  // own trigger paths, so editing them changes the expression this view
  // publishes on a pull request the instrument's lane is not even eligible on
  // — the break merges green there and surfaces on an unrelated roadmap change
  // by an author who touched none of it, which is exactly what citing evidence
  // to a lane exists to prevent.
  //
  // So the entry's transitive first-party module graph is cited with it,
  // through the same walk the acyclicity edge uses and for the same reason: a
  // re-export must not be read as a boundary. It over-reads — an import is
  // treated as a contribution — and that direction is the safe one, because it
  // can only widen what a change has to re-run.
  const citeSource = (source) => {
    const entries = [];
    for (const token of source.split(/\s+/u)) {
      if (!token.includes("/") || !/\.[a-z]+$/u.test(token)) continue;
      entries.push(token);
      citations.push(token);
      if (repoRoot) citations.push(...importedModules(repoRoot, token));
    }
    return entries;
  };
  // A selector this check RUNS has to be runnable where this check runs. The
  // job that hosts the instrument installs no dependencies and no toolchain,
  // because the instrument is node-only and portable by construction; a
  // selector that reached an installed package would resolve here, where a
  // developer's tree has one, and fail there. The graph is already walked for
  // citations, so the answer is derived rather than assumed.
  const installedDependencies = (entries) =>
    repoRoot ? [...new Set(entries.flatMap((entry) => externalSpecifiers(repoRoot, entry)))] : [];
  const within = parts.filter(
    (part) => WITHIN_UNIVERSE_FLAGS.includes(part.flag) || part.flag === "--",
  );
  const resolved = [];
  const unresolvedVariables = [];
  const producers = new Set();
  const producerEntries = [];
  for (const part of within)
    for (const reference of part.text.matchAll(/\$([A-Za-z_][A-Za-z0-9_]*)/gu)) {
      const source = laneVariableSource(body, reference[1]);
      if (!source) {
        unresolvedVariables.push(reference[1]);
        continue;
      }
      producers.add(source);
      resolved.push("`$" + reference[1] + "` is computed by `" + source + "`");
      producerEntries.push(...citeSource(source));
    }
  const refuse = (how) => ({ covers: false, how, citations });
  if (unresolvedVariables.length)
    return refuse(
      `the lane narrows by ${named(unresolvedVariables.map((name) => "$" + name))}, which no step of job ${adapter.refresh_job} assigns, so the published narrowing would be a bare variable name and what it selects is not resolvable here`,
    );
  if (producers.size > 1)
    return refuse(
      `the lane's run-time narrowing is computed by more than one command (${named([...producers])}), which this check cannot relate to a single declared producer`,
    );
  let predicate = "";
  if (producers.size === 1) {
    const source = [...producers][0];
    if (adapter.refresh_selection_producer !== source)
      return refuse(
        `job ${adapter.refresh_job} computes its run-time narrowing with \`${source}\`, not the declared \`${adapter.refresh_selection_producer ?? "(none)"}\`, so the command this check would evaluate is not the one the register declares`,
      );
    if (!repoRoot) return refuse("the lane's run-time narrowing was not evaluated");
    const installed = installedDependencies(producerEntries);
    if (installed.length)
      return refuse(
        `the lane's selection command \`${source}\` reaches the installed package ${named(installed)}, which the job that runs this instrument does not install, so this resolution would hold only where a tree happens to have it`,
      );
    const run = runLaneSelector(repoRoot, source);
    if (!run.ok)
      return refuse(`the lane's selection command \`${source}\` did not resolve: ${run.reason}`);
    const selection = packageSelection(run.stdout);
    if (selection.unresolved.length)
      return refuse(
        `the expression \`${source}\` prints carries ${named(selection.unresolved)}, a shape this check cannot decompose into what it excludes`,
      );
    const dropped = recordPackages.filter((name) => selection.excluded.has(name));
    if (dropped.length)
      return refuse(
        `the predicate \`${source}\` computes excludes ${named(dropped)}, a package this record selects`,
      );
    if (selection.included) {
      const missing = recordPackages.filter((name) => !selection.included.includes(name));
      if (missing.length)
        return refuse(
          `the predicate \`${source}\` computes restricts the run to ${named(selection.included)}, which does not include ${named(missing)}`,
        );
    }
    const packagesOf = (entry) =>
      [...entry.matchAll(/package\(([^()]+)\)/gu)].map((match) => match[1]);
    const inside = [...new Set(selection.narrowed.flatMap(packagesOf))].filter((name) =>
      recordPackages.includes(name),
    );
    // The disclosed count is the narrowing that lands INSIDE this record's own
    // packages, not the predicate's whole name-scoped population. Publishing
    // the total beside the named packages reads as scoped to them and
    // overstates the unevaluated limit by every disjunct aimed somewhere else,
    // and the point of this sentence is to say exactly what was and was not
    // checked.
    const insideNarrowed = selection.narrowed.filter((entry) =>
      packagesOf(entry).some((name) => recordPackages.includes(name)),
    );
    predicate =
      ` and its predicate resolves to an expression excluding ${selection.excluded.size ? named([...selection.excluded].sort()) : "no package"}` +
      (inside.length
        ? `, narrowing by test name inside ${named(inside.sort())} — ${insideNarrowed.length} name-scoped exclusions this check lists but does not evaluate against this record's own cases`
        : ", with no test-name narrowing inside any package this record selects");
  }
  const narrowed = within.length
    ? `, narrowed at run time by \`${within.map((part) => part.text).join(" ")}\`${
        resolved.length ? ` (${resolved.join("; ")})` : ""
      }${predicate}`
    : "";

  if (command.includes("--workspace"))
    return { covers: true, how: `the lane selects the whole workspace${narrowed}`, citations };
  const archive = command.match(/--archive-file[= ]\s*(\S+)/u);
  if (archive) {
    const builder = [...jobs].find(([, other]) => archiveBuildCommand(other, archive[1]));
    if (!builder)
      return {
        covers: false,
        how: `no job in this workflow builds ${archive[1]} over the whole workspace with its archive subcommand, so the lane's selection is not resolvable as covering this record's packages`,
        citations,
      };
    // Building an archive somewhere in the workflow is not the same as this
    // lane consuming it: without the dependency edge the two jobs are unrelated
    // and the lane's input is whatever it happens to find.
    if (!jobNeeds(body).includes(builder[0]))
      return {
        covers: false,
        how: `job ${adapter.refresh_job} does not declare \`${builder[0]}\` among its needs, so it is not resolvable as consuming the whole-workspace ${archive[1]} that job builds`,
        citations,
      };
    return {
      covers: true,
      how: `the lane consumes the whole-workspace archive \`${builder[0]}\` builds${narrowed}`,
      citations,
    };
  }
  const program = command.trim().split(/\s+/u)[0];
  if (program !== "cargo") {
    const operands = positionalOperands(command);
    if (operands.length)
      return refuse(
        `\`${command}\` selects work with the positional operand ${named(operands)}, which this check cannot relate to this record's own selection`,
      );
    if (!parts.length) {
      // "Its own default universe" is a sentence about a list this check had
      // never read. A runner that iterates a declared set re-runs this record
      // only while the record's own selection is IN that set, and dropping an
      // entry from it is invisible to every other check here — the job still
      // exists, still issues the same line, and is still gated on the same
      // filter. So the universe is enumerated by the lane's own script and the
      // record's selection is resolved against what it printed.
      const enumerator = adapter.refresh_selection_enumerator;
      const flag = adapter.refresh_selection_flag;
      if (!enumerator || !flag)
        return refuse(
          `\`${command}\` carries no selection this check can relate to this record's own, and the register declares no enumeration of the universe that runner iterates`,
        );
      // The enumeration has to be the lane's OWN invocation, asked to print
      // what it iterates — not merely another run of the same program. Matching
      // the first two tokens accepted any argument vector at all after them, so
      // a list printed under a different selection would still have been read
      // as the list the lane iterates. The declared enumeration is therefore
      // this lane's exact command line plus exactly one flag: same program,
      // same arguments, one switch that turns the run into its own inventory.
      const laneTokens = command.trim().split(/\s+/u);
      const enumeratorTokens = enumerator.trim().split(/\s+/u);
      const extra = enumeratorTokens.slice(laneTokens.length);
      const sharesPrefix = laneTokens.every((token, at) => enumeratorTokens[at] === token);
      if (!sharesPrefix || extra.length !== 1 || !extra[0].startsWith("-"))
        return refuse(
          `the declared enumeration \`${enumerator}\` is not \`${command}\` plus a single listing flag, so the list it prints is not the list this lane iterates`,
        );
      if (laneSelection === null)
        return refuse(
          `this record's command names no \`${flag}\` value, so there is nothing to resolve against the universe \`${command}\` iterates`,
        );
      if (!repoRoot) return refuse("the lane's enumerated universe was not evaluated");
      const installed = installedDependencies(
        enumerator.split(/\s+/u).filter((token) => token.includes("/") && /\.[a-z]+$/u.test(token)),
      );
      if (installed.length)
        return refuse(
          `the lane's enumeration \`${enumerator}\` reaches the installed package ${named(installed)}, which the job that runs this instrument does not install, so this resolution would hold only where a tree happens to have it`,
        );
      const listed = runLaneSelector(repoRoot, enumerator);
      if (!listed.ok)
        return refuse(`the lane's enumeration \`${enumerator}\` did not resolve: ${listed.reason}`);
      const entries = listed.stdout
        .split("\n")
        .map((entry) => entry.trim())
        .filter(Boolean);
      citeSource(enumerator);
      if (!entries.includes(laneSelection))
        return refuse(
          `\`${command}\` iterates ${named(entries)}, which does not include this record's \`${flag} ${laneSelection}\``,
        );
      return {
        covers: true,
        how: `the lane runs \`${command}\`, whose own enumeration \`${enumerator}\` lists ${entries.length} entries including this record's \`${flag} ${laneSelection}\``,
        citations,
      };
    }
  }
  return {
    covers: false,
    how: `${JSON.stringify(command)} names no selection this check can resolve as covering this record's packages`,
    citations,
  };
}

function descendantsOf(authority, rootId) {
  const children = new Map();
  for (const node of authority.nodes) {
    for (const predecessor of node.predecessors || []) {
      if (!children.has(predecessor)) children.set(predecessor, []);
      children.get(predecessor).push(node.id);
    }
  }
  const reached = new Set();
  const queue = [rootId];
  while (queue.length) {
    for (const child of children.get(queue.shift()) || []) {
      if (reached.has(child)) continue;
      reached.add(child);
      queue.push(child);
    }
  }
  return reached;
}

/**
 * Validate the register and derive every status. Returns `{ errors, model }`;
 * `model` is only meaningful when `errors` is empty.
 *
 * `universe` is the pinned claim/atom/row universe the register must equal
 * exactly, and `repoRoot` is the root that fixture and subject paths resolve
 * against. Both are parameters so the control suite can hold a pinned universe
 * over a mutated register — the only way an omitted claim can fail rather than
 * quietly shrink the set the register defines for itself.
 */
export function analyze(packageRoot = PACKAGE_ROOT, options = {}) {
  const universe = options.universe ?? LIVE_UNIVERSE;
  const topology = options.topology ?? LIVE_TOPOLOGY;
  // Pinned beside the universe and injectable for the same reason: a control
  // has to be able to hold the pin fixed while it moves the register under it.
  const exercising = options.exercising ?? SUBJECT_EXERCISING_PROOFS;
  const repoRoot = options.repoRoot ?? path.resolve(packageRoot, "..", "..");
  // One analysis, one reading of every lane selector. See `runLaneSelector`.
  resetLaneSelectorRuns();
  const errors = [];
  const registerFile = confinedFile(packageRoot, REGISTER_RELATIVE, "closure register");
  const register = readToml(registerFile);

  const schema = JSON.parse(
    fs.readFileSync(
      confinedFile(path.join(packageRoot, "schemas"), REGISTER_SCHEMA, "schema"),
      "utf8",
    ),
  );
  errors.push(...validateSchemaObject(register, schema, "closure register"));
  if (errors.length) return { errors, model: null };

  const adapters = new Map(register.adapter.map((row) => [row.id, row]));
  unique(register.adapter, "id", "adapter", errors);
  const REFRESH_FIELDS = ["refresh_job", "refresh_filter", "refresh_command"];
  for (const adapter of register.adapter) {
    // A node runner is one this instrument can invoke, so its records are
    // re-executed rather than trusted; declaring otherwise would make the
    // capability author-set.
    if (adapter.runner === "node" && adapter.reexecution !== "instrument")
      errors.push(
        `adapter ${adapter.id}: this instrument runs node itself, so its records are re-executed rather than trusted`,
      );
    // A record this instrument cannot re-run is not therefore unrefreshed: it
    // names the lane that does re-run it, and that lane is resolved below. The
    // declaration is mandatory in one direction and forbidden in the other, so
    // neither an omission nor a decorative lane on a re-executed record can
    // change which binding applies.
    for (const field of REFRESH_FIELDS) {
      if (adapter.reexecution === "external" && !adapter[field])
        errors.push(
          `adapter ${adapter.id}: a record this instrument does not re-run must declare ${field}`,
        );
      if (adapter.reexecution === "instrument" && adapter[field])
        errors.push(
          `adapter ${adapter.id}: ${field} is meaningless on a record the control suite re-runs itself`,
        );
    }
    // The two selection-resolution declarations answer different questions and
    // neither may stand in for the other, so each is required exactly where its
    // lane's shape leaves the selection otherwise unread: a lane that narrows at
    // run time declares the producer of that narrowing, and a lane whose runner
    // iterates its own list declares the enumeration of it plus the flag the
    // record's own selection is spelled with. A declaration on any other lane is
    // decoration, and decoration is what makes an unresolved binding look
    // resolved.
    if (adapter.refresh_selection_enumerator && !adapter.refresh_selection_flag)
      errors.push(
        `adapter ${adapter.id}: an enumerated lane universe must name the flag that carries a record's own selection`,
      );
    if (adapter.refresh_selection_flag && !adapter.refresh_selection_enumerator)
      errors.push(
        `adapter ${adapter.id}: refresh_selection_flag resolves nothing without the enumeration it is matched against`,
      );
    const narrows = adapter.refresh_command
      ? selectionParts(adapter.refresh_command).some(
          (part) =>
            (WITHIN_UNIVERSE_FLAGS.includes(part.flag) || part.flag === "--") &&
            /\$[A-Za-z_]/u.test(part.text),
        )
      : false;
    if (narrows && !adapter.refresh_selection_producer)
      errors.push(
        `adapter ${adapter.id}: its lane narrows at run time by a variable, so the command that computes it must be declared and resolved rather than published as a name`,
      );
    if (!narrows && adapter.refresh_selection_producer)
      errors.push(
        `adapter ${adapter.id}: refresh_selection_producer names a producer for a narrowing this lane does not carry`,
      );
  }

  const claims = new Map(register.claim.map((row) => [row.id, { ...row, atoms: [] }]));
  unique(register.claim, "id", "claim", errors);
  unique(register.atom, "id", "atom", errors);
  unique(register.proof, "id", "proof", errors);
  unique(register.control, "id", "control", errors);
  unique(register.finding, "id", "finding", errors);
  unique(register.residue, "id", "residue", errors);

  unique(register.transfer, "atom", "transfer", errors);

  const atoms = new Map();
  for (const atom of register.atom) {
    if (!claims.has(atom.claim)) errors.push(`atom ${atom.id}: unknown claim ${atom.claim}`);
    else claims.get(atom.claim).atoms.push(atom.id);
    atoms.set(atom.id, atom);
  }

  // --- the pinned universe ------------------------------------------------
  // Exact set equality in both directions, per claim and per row kind. An
  // omission and an addition are the same defect seen from opposite sides.
  const sorted = (values) => [...values].sort();
  const setDiff = (actual, expected, label) => {
    const actualSet = new Set(actual);
    const expectedSet = new Set(expected);
    for (const id of sorted(expectedSet))
      if (!actualSet.has(id))
        errors.push(`${label}: ${JSON.stringify(id)} omitted from the register`);
    for (const id of sorted(actualSet))
      if (!expectedSet.has(id))
        errors.push(`${label}: ${JSON.stringify(id)} is not in the pinned universe`);
  };
  setDiff(claims.keys(), Object.keys(universe.claims), "claim universe");
  for (const [claimId, expectedAtoms] of Object.entries(universe.claims)) {
    const claim = claims.get(claimId);
    if (!claim) continue;
    setDiff(claim.atoms, expectedAtoms, `atom universe of ${claimId}`);
  }
  unique(register.row, "subject", "row", errors);
  for (const [kind, subjects] of Object.entries(universe.rows))
    setDiff(
      register.row.filter((row) => row.kind === kind).map((row) => row.subject),
      subjects,
      `${kind} rows`,
    );

  // --- the pinned propositions --------------------------------------------
  // Pinning identifiers alone leaves the other half of the same hole open. A
  // claim, an atom, or a finding could keep its id, its evidence and its
  // derived status while what it ASSERTS was rewritten to something the
  // evidence already satisfies — a proposition weakened to fit its proof, with
  // an unchanged count and a status that never left PROVEN. The set checks
  // above cannot see that, because nothing about the set changed. So each
  // proposition's digest is pinned beside its id and compared here, which does
  // not stop an author from correcting a statement; it stops one from doing it
  // invisibly, because the repin is a line of the same review.
  const propositions = new Map();
  for (const claim of register.claim) propositions.set(`claim:${claim.id}`, claim.statement);
  for (const atom of register.atom) propositions.set(`atom:${atom.id}`, atom.statement);
  for (const finding of register.finding)
    propositions.set(`finding:${finding.id}`, finding.statement);
  // A row's disposition and a remainder's statement are propositions too: one
  // says how a displaced route was rejected, the other says which question is
  // being carried and why. Both were reachable by an edit that moved no id, no
  // count and no derived status, which is the hole the pin above closes for the
  // three kinds that happen to carry an id of their own.
  for (const row of register.row) propositions.set(`row:${row.subject}`, row.disposition);
  for (const residue of register.residue)
    propositions.set(`residue:${residue.id}`, residue.statement);
  // A negative control's two prose fields are the whole record of what it
  // demonstrated. `mutation` says which property was broken and `observed` is
  // the transcript of the refusal that broke it — the only place a control's
  // discriminating power is written down. Leaving them unpinned left a control
  // hollowable while every id, count, coverage list and derived status stayed
  // exactly where it was, which is the same laundering the claim pins refuse,
  // in the one place the hollowed-statement control could not see it.
  for (const control of register.control) {
    propositions.set(`control:${control.id}.mutation`, control.mutation);
    propositions.set(`control:${control.id}.observed`, control.observed);
  }
  // A receiving row's gate is the sentence that says what the owner has to
  // clear before the remainder is discharged; the derivation constrains only
  // its opening. A record's skip basis is the sentence that says why declared
  // skips are expected rather than unexpected — the difference between a
  // record the counter check admits and one it refuses.
  for (const row of register.receiving)
    propositions.set(`receiving:${row.residue}#${row.order}.gate`, row.gate);
  for (const proof of register.proof)
    if (proof.skip_basis) propositions.set(`proof:${proof.id}.skip_basis`, proof.skip_basis);
  setDiff(propositions.keys(), Object.keys(universe.statements ?? {}), "statement universe");
  for (const [key, expected] of Object.entries(universe.statements ?? {})) {
    const actual = propositions.get(key);
    if (actual === undefined) continue;
    const derived = statementDigest(actual);
    if (derived !== expected)
      errors.push(
        `statement pin: ${key} asserts ${derived}, not the pinned ${expected}; a proposition may be corrected, but not rewritten while its evidence and its derived status stay where they are`,
      );
  }
  // Where an atom POINTS, pinned beside what it says. The statement pin cannot
  // see a repoint: the words do not move, so an atom can be aimed at whatever
  // artifact a green record happens to touch, or rested on a contract sentence
  // the contract no longer states, with every count and every derived status
  // unchanged.
  setDiff(
    register.atom.map((atom) => `atom:${atom.id}`),
    Object.keys(universe.anchors ?? {}),
    "anchor universe",
  );
  for (const atom of register.atom) {
    const expected = (universe.anchors ?? {})[`atom:${atom.id}`];
    if (expected === undefined) continue;
    const derived = anchorDigest(atom);
    if (derived !== expected)
      errors.push(
        `anchor pin: atom:${atom.id} points at ${derived}, not the pinned ${expected}; an atom's evidence artifact, contract section, contract quotation, and owed surface may be corrected, but not repointed while its statement and its derived status stay where they are`,
      );
  }

  // --- external-artifact resolutions -------------------------------------
  // Each of these reads an artifact this register does not own, which is what
  // keeps the obligations acyclic: the register cannot satisfy them itself.
  let obligations = 0;

  // Every artifact the register leans on, attributed to the filter of the lane
  // that would re-run the record citing it. A path nothing can invalidate is
  // evidence nothing can refresh, and which lane refreshes it depends on which
  // runner produced it.
  const citedByFilter = new Map();
  const cite = (filter, target) => {
    if (!citedByFilter.has(filter)) citedByFilter.set(filter, new Set());
    citedByFilter.get(filter).add(target);
  };

  /**
   * An artifact THIS VALIDATOR opens, cited to both lanes that must notice it.
   *
   * Attributing such an artifact to the refreshing lane alone was an
   * attribution error with a real failure: the lane that re-runs the record's
   * TESTS is not the lane that re-runs the VALIDATOR, and the validator's own
   * answer depends on these bytes — a control's replaced text must occur
   * exactly once in its subject, a directory-counted record's selection is
   * re-derived by reading that directory, a fixture and a package manifest must
   * resolve. A change to any of them can turn `--check` red while the
   * instrument's own lane is not even eligible on that pull request, so the
   * breakage lands green and surfaces later on an unrelated change by an author
   * who touched none of it. Citing to the instrument's own filter as well makes
   * the existing coverage check force that lane to be triggered by exactly the
   * inputs the validator reads.
   */
  const citeRead = (filter, target) => {
    if (filter) cite(filter, target);
    cite(CI_TRIGGER_FILTER, target);
  };

  const contractFile = confinedFile(
    packageRoot,
    register.ratification.contract,
    "ratified contract",
  );
  const contractSections = markdownSections(fs.readFileSync(contractFile, "utf8"));
  for (const heading of contractSections.duplicates)
    errors.push(`ratified contract: duplicate section heading ${JSON.stringify(heading)}`);
  for (const atom of register.atom) {
    if (!atom.contract_section) continue;
    obligations += 1;
    const body = contractSections.body.get(atom.contract_section);
    if (body === undefined) {
      errors.push(`atom ${atom.id}: contract section not found: ${atom.contract_section}`);
      continue;
    }
    // A heading is not a contract. Binding an atom to a section that merely
    // exists lets the section be gutted to a placeholder while the atom still
    // derives proven, so the atom quotes the sentence that states it and the
    // quotation must still be there.
    if (!atom.contract_anchor) {
      errors.push(`atom ${atom.id}: a contract section binding must quote the text it relies on`);
      continue;
    }
    obligations += 1;
    if (!flatten(body).includes(flatten(atom.contract_anchor)))
      errors.push(
        `atom ${atom.id}: contract section ${JSON.stringify(atom.contract_section)} no longer states ${JSON.stringify(atom.contract_anchor)}`,
      );
  }
  for (const atom of register.atom) {
    if (atom.contract_anchor && !atom.contract_section)
      errors.push(`atom ${atom.id}: a contract anchor without a contract section quotes nothing`);
    // The artifact an atom's evidence must exercise. A bare top-level tree is
    // not an anchor: it would be satisfied by anything beneath it, which is the
    // relevance hole this closes rather than a shorthand for it.
    obligations += 1;
    if (!atom.evidence_anchor.includes("/"))
      errors.push(
        `atom ${atom.id}: evidence anchor ${atom.evidence_anchor} is a whole top-level tree, which discriminates nothing`,
      );
    else
      try {
        confinedEntry(repoRoot, atom.evidence_anchor, `atom ${atom.id} evidence anchor`);
        cite(CI_TRIGGER_FILTER, atom.evidence_anchor);
      } catch (error) {
        errors.push(`atom ${atom.id}: evidence anchor does not resolve: ${error.message}`);
      }
  }
  for (const key of ["instrument_authority", "decision"]) {
    obligations += 1;
    try {
      confinedFile(packageRoot, register.ratification[key], `ratification ${key}`);
    } catch (error) {
      errors.push(`ratification ${key}: ${error.message}`);
    }
  }

  const authority = loadAuthority(packageRoot);
  const nodesById = new Map(authority.nodes.map((node) => [node.id, node]));
  const descendants = descendantsOf(authority, RAISING_NODE);
  const charterTextCache = new Map();
  const charterText = (nodeId) => {
    if (charterTextCache.has(nodeId)) return charterTextCache.get(nodeId);
    const node = nodesById.get(nodeId);
    let text = null;
    if (node) {
      try {
        text = fs.readFileSync(confinedFile(packageRoot, node.charter, "charter"), "utf8");
      } catch (error) {
        errors.push(`charter ${nodeId}: ${error.message}`);
      }
    }
    charterTextCache.set(nodeId, text);
    return text;
  };
  const charterCache = new Map();
  const criteriaFor = (nodeId) => {
    if (charterCache.has(nodeId)) return charterCache.get(nodeId);
    const text = charterText(nodeId);
    const criteria = text === null ? null : charterCriteria(text);
    charterCache.set(nodeId, criteria);
    return criteria;
  };

  // The downstream charters are the narratives a reader meets before this
  // register, and each states its owner twice: once in the generated `owner=`
  // header, and once in the outcome sentence naming the owner being displaced
  // and the one that ends up sole. Both are resolved, because they can
  // disagree — a regenerated header over untouched prose still hands the
  // capability to the displaced owner everywhere a reader looks. A charter
  // missing the header, missing the sentence, or naming anyone but the
  // ratified owner as final fails here rather than being closed by assertion.
  for (const node of authority.nodes) {
    if (node.train !== RAISING_TRAIN) continue;
    obligations += 1;
    const text = charterText(node.id);
    if (text === null) continue;
    const owners = charterOwners(text);
    if (owners.header === null) errors.push(`charter ${node.id}: declares no owner header`);
    else if (owners.header !== register.ratification.owner)
      errors.push(
        `charter ${node.id}: declares owner ${JSON.stringify(owners.header)}, not the ratified ${JSON.stringify(register.ratification.owner)}`,
      );
    // A historical identity wrapper narrates no outcome — it records that a
    // rejected node existed and carries the header alone. Requiring the
    // sentence there would fail a charter that has no boundary to state; the
    // narrative obligation belongs to the charters that DELIVER one.
    if (node.semantic_role !== "delivery") continue;
    // A pair is only the charter's statement while there is exactly one of it.
    // Reading the first match in flattened document text accepted a compliant
    // sentence written above a stale one — the reader meets both and the check
    // saw one — so a duplicated section or a second, different pair inside the
    // owning section is refused rather than resolved by position.
    if (owners.conflict) {
      errors.push(
        owners.duplicated
          ? `charter ${node.id}: declares two "${OUTCOME_SECTION}" sections, so its outcome narrative resolves to whichever one comes last`
          : `charter ${node.id}: its "${OUTCOME_SECTION}" section states more than one current/final owner pair, so one of them is stale and nothing there says which`,
      );
      continue;
    }
    if (owners.final === null) {
      errors.push(`charter ${node.id}: its outcome narrative states no current/final owner pair`);
      continue;
    }
    const ratified = ownerCapability(register.ratification.owner);
    if (owners.final !== ratified)
      errors.push(
        `charter ${node.id}: narrates the final and sole owner as ${JSON.stringify(owners.final)}, not the ratified ${JSON.stringify(ratified)}`,
      );
    if (owners.displaced !== register.ratification.displaced_owner)
      errors.push(
        `charter ${node.id}: narrates the displaced owner as ${JSON.stringify(owners.displaced)}, not the ${JSON.stringify(register.ratification.displaced_owner)} this register displaces`,
      );
  }

  /**
   * An acceptance criterion may only be cited when a real charter declares it,
   * under the ROLE the citation says it needs, on this block or one of its
   * strict descendants, and inside this block's own train.
   *
   * Identifier existence alone proves nothing: every charter in the program
   * declares the same four boilerplate ordinals, so any descendant's `AC1..AC4`
   * would resolve. The two additional predicates are what make the binding
   * discriminate — the role ties the citation to the slot's stated obligation,
   * and the train bound stops an unrelated vertical's block from appearing as
   * the enforcer of this vertical's remainder.
   */
  const resolveCriterion = (criterion, role, where) => {
    if (!CRITERION_ROLES.includes(role))
      errors.push(
        `${where}: a criterion citation must declare one of ${CRITERION_ROLES.join(" | ")}`,
      );
    const owner = criterion.slice(0, criterion.indexOf("-AC"));
    if (!nodesById.has(owner)) {
      errors.push(`${where}: criterion owner ${owner} is not a DAG node`);
      return;
    }
    if (owner !== RAISING_NODE && !descendants.has(owner))
      errors.push(
        `${where}: criterion owner ${owner} is neither ${RAISING_NODE} nor a strict descendant`,
      );
    if (nodesById.get(owner).train !== RAISING_TRAIN)
      errors.push(`${where}: criterion owner ${owner} is outside the ${RAISING_TRAIN} train`);
    const criteria = criteriaFor(owner);
    if (!criteria) return;
    if (!criteria.has(criterion)) {
      errors.push(`${where}: criterion ${criterion} is not declared by ${owner}`);
      return;
    }
    const declared = criteria.get(criterion);
    if (declared !== role)
      errors.push(`${where}: ${owner} declares ${criterion} as "${declared}", not "${role}"`);
  };

  // --- negative controls --------------------------------------------------
  // A control used to be an author's sentence about a mutation nobody could
  // locate. It now names the artifact it edited and both halves of the edit, so
  // "unique" and "new" are VERIFIED against the live tree: the replaced text
  // must be present exactly once, and the introduced text must be absent. A
  // mutation that could not have applied, or one still sitting in the tree,
  // fails here.
  const controls = new Map(register.control.map((row) => [row.id, row]));
  const controlUse = new Map(register.control.map((row) => [row.id, 0]));
  const controlSubjects = new Map();
  for (const control of register.control) {
    const where = `control ${control.id}`;
    if (control.observed.trim() === PLACEHOLDER)
      errors.push(`${where}: observed outcome is still a placeholder`);
    if (control.kind === "source") {
      if (control.argv_delta) errors.push(`${where}: argv_delta belongs to a command mutation`);
      const missing = ["subject", "reverted", "applied"].filter((field) => !control[field]);
      if (missing.length) {
        errors.push(`${where}: a source mutation must name ${missing.join(", ")}`);
        continue;
      }
      // Both halves must span a line boundary. A single-line fragment of a file
      // this register itself owns would match its own record — the control
      // would then be proving something about its own text.
      for (const field of ["reverted", "applied"])
        if (!control[field].includes("\n"))
          errors.push(
            `${where}: ${field} must span a line boundary so it cannot match its own record`,
          );
      if (control.reverted === control.applied)
        errors.push(`${where}: the mutation replaces its text with itself`);
      obligations += 1;
      let text = null;
      try {
        text = normalizeLines(
          fs.readFileSync(confinedFile(repoRoot, control.subject, `${where} subject`), "utf8"),
        );
        controlSubjects.set(control.id, control.subject);
      } catch (error) {
        errors.push(`${where}: subject ${control.subject} does not resolve: ${error.message}`);
      }
      if (text !== null) {
        const found = occurrences(text, control.reverted);
        if (found !== 1)
          errors.push(
            `${where}: the text this mutation replaced occurs ${found} times in ${control.subject}, so the mutation was not a unique application`,
          );
        if (occurrences(text, control.applied) !== 0)
          errors.push(
            `${where}: the mutation is still present in ${control.subject}, so it was never reverted and the tree under review is the mutated one`,
          );
      }
    } else {
      for (const field of ["subject", "reverted", "applied"])
        if (control[field]) errors.push(`${where}: ${field} belongs to a source mutation`);
      if (!control.argv_delta?.length)
        errors.push(`${where}: a command mutation must name the arguments it added`);
    }
  }

  // A claim's subject is the artifact it is about. Resolving it here means a
  // renamed or deleted subject fails rather than silently disarming the
  // acyclicity rule that reads it.
  const claimSubjects = new Map();
  for (const claim of register.claim) {
    const resolved = [];
    for (const subject of claim.subject) {
      obligations += 1;
      try {
        confinedFile(repoRoot, subject, `claim ${claim.id} subject`);
        cite(CI_TRIGGER_FILTER, subject);
        resolved.push(subject);
      } catch (error) {
        errors.push(`claim ${claim.id}: subject ${subject} does not resolve: ${error.message}`);
      }
    }
    claimSubjects.set(claim.id, new Set(resolved));
  }

  const coverage = new Map();
  const refusedClaims = new Set();
  const exercisedSubjects = new Map();
  const proofExercise = new Map();
  const proofLimits = new Map();
  const proofFilter = new Map();
  for (const proof of register.proof) {
    const where = `proof ${proof.id}`;
    const adapter = adapters.get(proof.adapter);
    if (!adapter) errors.push(`${where}: adapter ${proof.adapter} is not allowlisted`);
    const filter = adapter?.reexecution === "external" ? adapter.refresh_filter : CI_TRIGGER_FILTER;
    proofFilter.set(proof.id, filter);
    proofLimits.set(proof.id, []);
    const control = controls.get(proof.control);
    if (!control) errors.push(`${where}: unknown control ${proof.control}`);
    else controlUse.set(proof.control, controlUse.get(proof.control) + 1);
    if (proof.terminal_summary.trim() === PLACEHOLDER)
      errors.push(`${where}: terminal summary is still a placeholder`);
    // A named fixture that does not exist is the same defect as a missing
    // dependency: the record cites an input its run could not have read.
    for (const fixture of proof.fixtures) {
      obligations += 1;
      try {
        confinedFile(repoRoot, fixture, `${where} fixture`);
        citeRead(filter, fixture);
      } catch (error) {
        errors.push(`${where}: fixture ${fixture} does not resolve: ${error.message}`);
      }
    }
    if (control && controlSubjects.has(control.id))
      citeRead(filter, controlSubjects.get(control.id));
    const producers = adapter ? verdictProducers(adapter, proof) : [];
    // A module the record's own entry file imports is not a producer — the
    // transcript reports the harness's verdict on that file's assertions — but
    // calling it is still executing it, so the edge is derived and decided
    // rather than left unseen.
    const executed =
      adapter?.runner === "node"
        ? producers.flatMap((entry) => importedModules(repoRoot, entry))
        : [];
    const surface = adapter ? evidenceSurface(adapter, proof) : [];

    let refused = false;
    // The counters are the transcript's, not the record's. A record used to
    // pair any plausible sentence with self-consistent numbers; now the numbers
    // must be readable out of the summary the record says it observed, in the
    // shape its adapter's runner actually emits.
    if (adapter) {
      const grammar = adapter.summary_grammar;
      if (grammar === "tool-line" && !proof.count_key) {
        errors.push(`${where}: a tool-line summary must name the key that carries its count`);
        refused = true;
      } else if (grammar !== "tool-line" && proof.count_key) {
        errors.push(`${where}: a count key is only meaningful for a tool-line summary`);
        refused = true;
      }
      const observed = parseTerminalSummary(grammar, proof.terminal_summary, proof.count_key);
      if (!observed) {
        errors.push(`${where}: terminal summary is not a ${grammar} summary`);
        refused = true;
      } else
        for (const field of ["selected", "executed", "passed", "failed", "skipped"])
          if (proof[field] !== observed[field]) {
            errors.push(
              `${where}: ${field} is ${proof[field]}, but its transcript states ${observed[field]}`,
            );
            refused = true;
          }
      // A negative control is a REFUSAL, and the refusal has to be the
      // RUNNER's, transcribed in the runner's own grammar. Checking only that
      // the recorded text is not a transcript this validator would admit left
      // any sentence at all admissible, so a mutation nobody planted could
      // carry an invented description of a failure nobody saw.
      if (control) {
        obligations += 1;
        const outcome = parseTerminalSummary(grammar, control.observed, proof.count_key);
        if (
          outcome &&
          outcome.failed === 0 &&
          outcome.executed > 0 &&
          outcome.skipped === (proof.expected_skips ?? 0)
        )
          errors.push(
            `control ${control.id}: its observed outcome is a transcript this validator would admit, so it records no refusal`,
          );
        else if (!parseRefusal(grammar, control.observed, proof.count_key))
          errors.push(
            `control ${control.id}: its observed outcome carries no ${grammar} refusal this check can read, so it records no refusal the mutation is known to have produced — a description of a failure is not a transcript of one`,
          );
        if (control.kind === "command")
          for (const token of control.argv_delta ?? [])
            if ([...adapter.argv_prefix, ...proof.argv_tail].includes(token))
              errors.push(
                `control ${control.id}: ${JSON.stringify(token)} is already part of the command it claims to have mutated, so the mutation was not new`,
              );
      }
      // Some runners derive their own selection from a directory the record
      // already cites. Where the adapter says so, the selected count is
      // re-derived from that directory instead of being trusted, so adding or
      // removing a case there invalidates the transcription rather than passing.
      if (adapter.count_source === "fixture-directory") {
        obligations += 1;
        const directories = new Set(proof.fixtures.map((fixture) => path.posix.dirname(fixture)));
        if (directories.size !== 1)
          errors.push(
            `${where}: a directory-counted record must cite fixtures from exactly one directory`,
          );
        else {
          const directory = [...directories][0];
          try {
            const entries = fs
              .readdirSync(confinedEntry(repoRoot, directory, `${where} fixture directory`))
              .filter((entry) => entry.endsWith(".rs"));
            // The whole DIRECTORY is a validator input here, not just the
            // fixtures the record happens to name: adding one case beside them
            // changes the count this check re-derives.
            citeRead(filter, directory);
            if (proof.selected !== entries.length) {
              errors.push(
                `${where}: selected is ${proof.selected}, but ${directory} holds ${entries.length} cases the runner would select`,
              );
              refused = true;
            }
          } catch (error) {
            errors.push(`${where}: fixture directory ${directory} does not resolve: ${error}`);
          }
        }
      }
    }
    // A selector naming a package this workspace does not have could not have
    // run the work the record claims, which is the same defect as a fixture
    // that does not resolve.
    if (adapter?.runner === "cargo") {
      const argv = [...adapter.argv_prefix, ...proof.argv_tail];
      for (const [index, token] of argv.entries()) {
        if (token !== "-p") continue;
        const selected = argv[index + 1];
        obligations += 1;
        if (!selected) {
          errors.push(`${where}: -p names no package`);
          refused = true;
          continue;
        }
        const manifest = `crates/${selected}/Cargo.toml`;
        try {
          confinedFile(repoRoot, manifest, `${where} package`);
          citeRead(filter, manifest);
        } catch {
          errors.push(`${where}: package ${selected} is not a crate of this workspace`);
          refused = true;
        }
      }
    }

    if (proof.selected < 1) {
      errors.push(`${where}: zero selected work`);
      refused = true;
    }
    if (proof.executed !== proof.passed + proof.failed) {
      errors.push(`${where}: counters do not reconcile (executed != passed + failed)`);
      refused = true;
    }
    if (proof.selected !== proof.executed + proof.skipped) {
      errors.push(`${where}: counters do not reconcile (selected != executed + skipped)`);
      refused = true;
    }
    if (proof.failed !== 0) {
      errors.push(`${where}: ${proof.failed} failed`);
      refused = true;
    }
    // "Zero unexpected skips" is an exact-match rule, not a zero rule: a real
    // workspace selection always carries ignored cases, so the record declares
    // that count in advance and any drift refuses the record. A declared count
    // must also state what it covers, so the skips stay visible in the view
    // rather than being absorbed into a number nobody reads.
    const expectedSkips = proof.expected_skips ?? 0;
    if (proof.skipped !== expectedSkips) {
      errors.push(`${where}: ${proof.skipped} skips against ${expectedSkips} declared`);
      refused = true;
    }
    if (expectedSkips > 0 && !proof.skip_basis) {
      errors.push(`${where}: a declared skip count must state its basis`);
      refused = true;
    }
    if (expectedSkips === 0 && proof.skip_basis)
      errors.push(`${where}: skip basis stated without a declared skip count`);

    for (const atomId of proof.covers) {
      const atom = atoms.get(atomId);
      if (!atom) {
        errors.push(`${where}: covers unknown atom ${atomId}`);
        continue;
      }
      // Relevance, derived. `covers` is a list the author writes, so on its own
      // it would let any green record be credited with any obligation. The atom
      // declares the artifact its evidence has to exercise, and the record's
      // surface — its fixtures, its command's path arguments, and the crate
      // roots of the packages it selects — has to reach it.
      obligations += 1;
      if (!surfaceReaches(surface, atom.evidence_anchor)) {
        errors.push(
          `${where}: nothing this record runs or reads reaches ${atom.evidence_anchor}, which is what ${atomId} requires its evidence to exercise`,
        );
        continue;
      }
      citeRead(filter, atom.evidence_anchor);
      // Acyclicity, derived and unconditional. The subject binding is not a
      // field a record volunteers — it falls out of the claim's declared
      // subject artifacts and the artifacts this record's own command runs. A
      // record whose run produces the verdict for an artifact cannot also be
      // the evidence that the artifact behaves correctly, and a cycle refuses
      // the claim rather than merely logging beside it.
      const cyclic = producers.filter((producer) =>
        [...(claimSubjects.get(atom.claim) || new Set())].some((subject) =>
          producerCoversSubject(producer, subject),
        ),
      );
      if (cyclic.length)
        errors.push(
          `${where}: cyclic coverage — its own run of ${cyclic.join(", ")} is the subject of ${atom.claim}`,
        );
      // The same question through the import graph. A record whose entry file
      // reaches its claim's subject — directly or through any chain of
      // first-party modules — is exercising that subject, which is admissible
      // only for the pinned case beside this validator and only for the stated
      // reason; anything else is a cycle by another route.
      const exercised = executed.filter((module) =>
        [...(claimSubjects.get(atom.claim) || new Set())].some((subject) =>
          producerCoversSubject(module, subject),
        ),
      );
      let exerciseRefused = false;
      if (exercised.length) {
        exercisedSubjects.set(proof.id, exercised);
        if (exercising[proof.id])
          proofExercise.set(proof.id, `${exercising[proof.id]} (${exercised.join(", ")})`);
        else {
          exerciseRefused = true;
          errors.push(
            `${where}: cyclic coverage — its own run reaches ${exercised.join(", ")} through its first-party import graph, the subject of ${atom.claim}, and no exemption is pinned for it`,
          );
        }
      }
      if (refused || cyclic.length || exerciseRefused) refusedClaims.add(atom.claim);
      else {
        if (!coverage.has(atomId)) coverage.set(atomId, []);
        coverage.get(atomId).push(proof.id);
      }
    }
  }
  for (const [id, uses] of controlUse)
    if (uses !== 1) errors.push(`control ${id}: referenced by ${uses} proofs, expected exactly 1`);
  // A carve-out nobody needs is a carve-out nobody is reviewing, so the pin is
  // stale-failing: a record listed as exercising its subject that no longer
  // does fails here rather than sitting as a standing licence.
  for (const id of Object.keys(exercising)) {
    obligations += 1;
    if (!exercisedSubjects.has(id))
      errors.push(
        `proof ${id}: pinned as exercising its claim's subject, but its run reaches no subject of any claim it covers`,
      );
  }

  // --- the gate that would notice ----------------------------------------
  // Re-execution is what keeps a transcription current, and this instrument can
  // only re-run its own runner. Every other record names the lane that re-runs
  // it, and both bindings are RESOLVED against the workflow rather than
  // assumed. Where a binding does not resolve, the record carries a derived
  // limit — nothing here is a field an author could decline to fill in.
  obligations += 1;
  let workflow = null;
  try {
    workflow = fs.readFileSync(confinedFile(repoRoot, CI_WORKFLOW, "ci workflow"), "utf8");
  } catch (error) {
    errors.push(`ci workflow: ${error.message}`);
  }
  // The workflow's jobs, needed both here and by the per-record selection
  // resolution below: a lane's reach is a question about a RECORD's own
  // packages, so it cannot be answered once per adapter.
  let jobs = new Map();
  const patternCache = new Map();
  const patternsFor = (filter) => {
    if (!patternCache.has(filter))
      patternCache.set(filter, workflow ? triggerPaths(workflow, filter) : null);
    return patternCache.get(filter);
  };
  if (workflow) {
    jobs = workflowJobs(workflow);
    const ownPatterns = patternsFor(CI_TRIGGER_FILTER);
    if (!ownPatterns || !ownPatterns.length)
      errors.push(`ci workflow: no ${CI_TRIGGER_FILTER} trigger filter`);
    else
      for (const pattern of unresolvedTriggerPatterns(ownPatterns))
        errors.push(
          `ci workflow: ${CI_TRIGGER_FILTER} trigger pattern ${pattern} is not a form this check resolves`,
        );
    const running = [...jobs].filter(([, body]) =>
      INSTRUMENT_COMMANDS.every((command) => body.includes(command)),
    );
    obligations += 1;
    if (running.length !== 1)
      errors.push(
        `ci workflow: expected exactly one job running the instrument, found ${running.length}`,
      );
    else if (!running[0][1].includes(`needs.detect-changes.outputs.${CI_TRIGGER_FILTER} == 'true'`))
      errors.push(
        `ci workflow: job ${running[0][0]} runs the instrument but is not gated on the ${CI_TRIGGER_FILTER} filter whose paths it depends on`,
      );

    // The lane an external record leans on: it must exist, it must issue the
    // command the adapter says it does, and it must be gated on the filter the
    // record's inputs are then measured against.
    for (const adapter of register.adapter) {
      if (adapter.reexecution !== "external") continue;
      if (!adapter.refresh_job || !adapter.refresh_filter || !adapter.refresh_command) continue;
      const where = `adapter ${adapter.id}`;
      obligations += 1;
      const body = jobs.get(adapter.refresh_job);
      if (body === undefined)
        errors.push(`${where}: ${adapter.refresh_job} is not a job of the workflow`);
      else {
        // Containment alone accepts a PREFIX. A declaration that stops before
        // the lane's own selection arguments then satisfies every check here
        // while the flags that decide what the lane runs stay invisible, so the
        // declaration must be the command line the job issues, whole.
        const line = laneCommandLine(body, adapter.refresh_command);
        if (line === null)
          errors.push(
            `${where}: job ${adapter.refresh_job} does not run ${JSON.stringify(adapter.refresh_command)}, so it is not the lane that re-runs this record`,
          );
        else if (line !== adapter.refresh_command)
          errors.push(
            `${where}: job ${adapter.refresh_job} issues ${JSON.stringify(line)}, not the declared ${JSON.stringify(adapter.refresh_command)}; a declaration that is only a prefix of the lane's command hides the selection arguments that decide what it runs`,
          );
        if (!body.includes(`needs.detect-changes.outputs.${adapter.refresh_filter} == 'true'`))
          errors.push(
            `${where}: job ${adapter.refresh_job} is not gated on the ${adapter.refresh_filter} filter its records are measured against`,
          );
      }
      const patterns = patternsFor(adapter.refresh_filter);
      if (!patterns || !patterns.length)
        errors.push(`${where}: the workflow declares no ${adapter.refresh_filter} trigger filter`);
      else
        // A refreshing lane's reach is measured against these patterns exactly
        // as the instrument's own is, so an unreadable pattern is reported here
        // too rather than silently treated as a non-match.
        for (const pattern of unresolvedTriggerPatterns(patterns))
          errors.push(
            `${where}: ${adapter.refresh_filter} trigger pattern ${pattern} is not a form this check resolves`,
          );
    }
  }

  // A cited artifact outside its lane's trigger paths is evidence no change can
  // ever invalidate. That is a limit on the record, derived here, and a limited
  // claim is bounded — which is inadmissible without an approved transfer.
  const limitedClaims = new Set();
  const proofRefresh = new Map();
  for (const proof of register.proof) {
    const filter = proofFilter.get(proof.id);
    const limits = proofLimits.get(proof.id);
    const adapterRow = adapters.get(proof.adapter);
    // The refresh REACH, derived and published rather than left to prose. A
    // reader of the generated view otherwise sees one undifferentiated
    // "refreshed by" column over two materially different guarantees: a record
    // the control suite re-runs and re-derives here, and a record whose lane
    // re-runs the work while the transcription itself is compared against
    // nothing. Saying which one a record has is not a limit — the evidence
    // model this instrument implements binds a record to no tree, SHA, or
    // digest — but leaving it unsaid publishes the weaker guarantee as the
    // stronger one.
    // A record that runs the artifact its claim is about says so in the same
    // published sentence. Leaving that to the pin alone would publish the
    // ordinary guarantee for a record that does not have it.
    const exercise = proofExercise.get(proof.id);
    const exercised = exercise
      ? `; this record's own run exercises its claim's subject — ${exercise}`
      : "";
    if (adapterRow?.reexecution === "instrument")
      proofRefresh.set(
        proof.id,
        `the control suite re-runs this command and compares the counts its own run derives${exercised}`,
      );
    else if (adapterRow) {
      // The lane must also run work this record's OWN selection reaches, so
      // the resolution is per RECORD rather than per adapter: two records on
      // one adapter can select different packages, and a lane narrowed away
      // from one of them refreshes the other and not it.
      obligations += 1;
      const coverage = adapterRow.refresh_command
        ? refreshSelectionCoverage(adapterRow, jobs, selectedPackages(adapterRow, proof), {
            repoRoot,
            laneSelection: adapterRow.refresh_selection_flag
              ? flagValue(adapterRow, proof, adapterRow.refresh_selection_flag)
              : null,
          })
        : null;
      proofRefresh.set(
        proof.id,
        `\`${adapterRow.refresh_job}\` (\`${adapterRow.refresh_filter}\`) re-runs this work — ${coverage?.how ?? "its selection is unresolved"}; this instrument cannot invoke that runner, so it does not re-derive these counts${exercised}`,
      );
      if (coverage && !coverage.covers) limits.push(coverage.how);
      // An artifact the lane's own selection is read out of is evidence this
      // derivation depends on, so it is cited exactly as a fixture is: a
      // change to it must reach both the lane that re-runs the record and the
      // lane that re-derives this view.
      for (const target of coverage?.citations ?? []) citeRead(adapterRow.refresh_filter, target);
    }
    if (!filter) {
      limits.push(
        "no lane is declared for this record, so nothing re-runs the work it transcribes",
      );
      continue;
    }
    const patterns = patternsFor(filter);
    if (!patterns || !patterns.length) {
      limits.push(`the ${filter} lane does not resolve, so nothing re-runs this record`);
      continue;
    }
    const adapter = adapters.get(proof.adapter);
    const control = controls.get(proof.control);
    const cited = new Set(proof.fixtures);
    if (control && controlSubjects.has(control.id)) cited.add(controlSubjects.get(control.id));
    for (const atomId of proof.covers) {
      const atom = atoms.get(atomId);
      if (atom) cited.add(atom.evidence_anchor);
    }
    if (adapter?.runner === "cargo") {
      const argv = [...adapter.argv_prefix, ...proof.argv_tail];
      for (const [index, token] of argv.entries())
        if (token === "-p" && argv[index + 1]) cited.add(`crates/${argv[index + 1]}/Cargo.toml`);
    }
    for (const target of [...cited].sort()) {
      obligations += 1;
      if (!triggerCovers(patterns, target))
        limits.push(
          `${target} is outside the ${filter} trigger paths, so no change to it re-runs the work this record transcribes`,
        );
    }
  }
  // The instrument's own subjects are measured against its own lane.
  {
    const patterns = patternsFor(CI_TRIGGER_FILTER);
    for (const target of [...(citedByFilter.get(CI_TRIGGER_FILTER) ?? [])].sort()) {
      obligations += 1;
      if (patterns && patterns.length && !triggerCovers(patterns, target))
        errors.push(
          `ci workflow: ${target} is cited as evidence but no ${CI_TRIGGER_FILTER} trigger path covers it, so a change to it never re-runs the instrument`,
        );
    }
  }
  for (const proof of register.proof)
    if (proofLimits.get(proof.id).length)
      for (const atomId of proof.covers)
        if (coverage.get(atomId)?.includes(proof.id)) limitedClaims.add(atoms.get(atomId).claim);

  // --- residues, receiving coverage, transfers ---------------------------
  const residueIds = register.residue.map((row) => row.id);
  for (const id of residueIds)
    if (!ALLOWED_RESIDUES.includes(id)) errors.push(`residue ${id}: not an admissible residue`);
  for (const id of ALLOWED_RESIDUES)
    if (!residueIds.includes(id)) errors.push(`residue ${id}: missing from the register`);

  const receivingByResidue = new Map(residueIds.map((id) => [id, []]));
  for (const row of register.receiving) {
    const where = `receiving ${row.residue}#${row.order}`;
    if (!receivingByResidue.has(row.residue)) {
      errors.push(`${where}: unknown residue`);
      continue;
    }
    receivingByResidue.get(row.residue).push(row);
    obligations += 1;
    if (!nodesById.has(row.owner_node)) {
      errors.push(`${where}: owner ${row.owner_node} is not a DAG node`);
      continue;
    }
    if (!descendants.has(row.owner_node))
      errors.push(
        `${where}: owner ${row.owner_node} is not a strict descendant of ${RAISING_NODE}`,
      );
    if (!row.criterion.startsWith(`${row.owner_node}-`))
      errors.push(`${where}: criterion ${row.criterion} is not owned by ${row.owner_node}`);
    else resolveCriterion(row.criterion, row.criterion_role, where);
    // The gate is author prose beside a resolved criterion, which is exactly
    // where a misrouted citation used to become invisible: the row could name
    // one block and the published gate sentence describe another. The gate must
    // therefore open by naming the owner it was resolved against.
    if (!row.gate.startsWith(`${row.owner_node} acceptance:`))
      errors.push(
        `${where}: gate must open with "${row.owner_node} acceptance:" to match its resolved owner`,
      );
  }
  for (const [id, rows] of receivingByResidue) {
    if (!rows.length) {
      errors.push(`residue ${id}: no receiving criterion`);
      continue;
    }
    const orders = rows.map((row) => row.order).sort((a, b) => a - b);
    for (const [index, order] of orders.entries())
      if (order !== index + 1) errors.push(`residue ${id}: receiving order is not 1..n`);
    // The pinned owner SEQUENCE, not merely a set of authorized owners. A
    // remainder whose baseline must exist before TCM1 can compare against it
    // is not received by dropping TCM1 and keeping the later three, and the
    // projection remainder is not received by swapping which block selects
    // which topology. Both are substitutions every other check here accepts.
    obligations += 1;
    const required = topology.receiving[id];
    if (required) {
      const declared = [...rows].sort((a, b) => a.order - b.order).map((row) => row.owner_node);
      if (declared.join(" > ") !== [...required].join(" > "))
        errors.push(
          `residue ${id}: receiving owners are ${declared.join(" > ") || "(none)"}, not the required ${[...required].join(" > ")}`,
        );
    }
  }

  const transferred = new Map();
  for (const row of register.transfer) {
    const where = `transfer ${row.atom}`;
    if (!atoms.has(row.atom)) errors.push(`${where}: unknown atom`);
    if (!residueIds.includes(row.residue)) errors.push(`${where}: unknown residue ${row.residue}`);
    obligations += 1;
    try {
      confinedFile(packageRoot, row.approved_by, `${where} approval`);
    } catch (error) {
      errors.push(`${where}: ${error.message}`);
    }
    transferred.set(row.atom, row.residue);
  }
  // Which atom leaves through which remainder is pinned, not merely bounded to
  // the admissible set: an atom re-routed to a different allowed residue lands
  // under receiving rows that were resolved for a different question, and every
  // other check here would still pass.
  {
    obligations += 1;
    const declared = [...transferred].map(([atom, residue]) => `${atom}=>${residue}`).sort();
    const required = Object.entries(topology.transfers)
      .map(([atom, residue]) => `${atom}=>${residue}`)
      .sort();
    for (const entry of required)
      if (!declared.includes(entry))
        errors.push(`transfer topology: the required transfer ${entry} is missing`);
    for (const entry of declared)
      if (!required.includes(entry))
        errors.push(`transfer topology: ${entry} is not the pinned routing for that atom`);
  }

  // --- atom enforcement bindings -----------------------------------------
  // A contract obligation this block only *states* is inadmissible as prose: it
  // must name the acceptance criterion that enforces it, and that criterion
  // must be declared by this node or a strict descendant. A transferred atom is
  // exempt because its remainder already carries ordered receiving rows, and an
  // obligation the shipped code does not meet must be one of those.
  const owedAtoms = new Map();
  // The production surfaces the raising node's own charter declares. An atom
  // anchored inside one of them is an obligation ABOUT shipped code, whatever
  // its author would rather say; an atom anchored at a contract sentence, a
  // schema, the DAG or this instrument is not. Read from the charter rather
  // than from the shape of the path, so it moves when the charter does.
  const raisingCharter = charterText(RAISING_NODE);
  const raisingSurfaces = raisingCharter === null ? null : charterSurfaces(raisingCharter);
  for (const atom of register.atom) {
    const where = `atom ${atom.id}`;
    if (atom.received_by && transferred.has(atom.id))
      errors.push(
        `${where}: transferred atoms are received through their residue, not received_by`,
      );
    else if (atom.received_by) {
      obligations += 1;
      resolveCriterion(atom.received_by, atom.received_by_role, where);
    } else if (atom.contract_section && !transferred.has(atom.id))
      errors.push(`${where}: a contract obligation must name the criterion that enforces it`);

    // An OWED obligation: one the contract states and the shipped code does not
    // meet. It is a REMAINDER, and it leaves the way every other remainder
    // does.
    //
    // Every atom declares which case it is, because the field used to be
    // optional and that made owedness an author's silence rather than a derived
    // fact. An atom could disclose an unmet composition in its own statement,
    // omit the field, keep its quotation coverage, and derive PROVEN — routing
    // around the whole mechanism below by writing nothing. The declaration is
    // now required, so reaching that status takes a positive claim about bytes
    // a reviewer can check rather than an absence nobody can see.
    //
    // And the claim has to be the RIGHT one of three, not the only one on
    // offer. A single `met` spelling made "the code meets this", "there is no
    // code this is about" and "this is a question for a successor" one word,
    // which is how three of this register's own transferred remainders came to
    // declare that the shipped code met them. Each named value is now refused
    // against a contradiction the register already carries elsewhere.
    //
    // Marking it beside a covered atom was the hole. The atom stayed covered,
    // the claim still derived PROVEN, and the disclosure lived in a column — so
    // an obligation known not to hold reached the summary row a reader scans
    // under the same status as one that does, through a route no approved
    // transfer, no allowed residue, and no ordered receiving sequence had to
    // authorise. That is a fourth remainder in substance, volunteered by an
    // author rather than gated, and the closed residue set exists precisely to
    // make a fourth one unavailable.
    //
    // So an owed obligation must be TRANSFERRED, which puts it under the pinned
    // routing, the approval artifact, and the ordered receiving rows every
    // other remainder passes through; the surface it is unmet at then binds
    // that carry to a block that can actually discharge it — at least one owner
    // in the receiving sequence must declare a production surface containing
    // the path. A carry to a sequence of blocks none of which may touch the
    // code is a carry to nobody. Every way out of this block is an error rather
    // than a silent skip: a check whose failure mode is to stop checking
    // publishes the same unearned status it was written to prevent.
    obligations += 1;
    if (typeof atom.shipped_obligation !== "string" || !atom.shipped_obligation.trim()) {
      errors.push(
        `${where}: every atom must declare what the shipped code owes it — "${SHIPPED_OBLIGATION_MET}", "${SHIPPED_OBLIGATION_AUTHORITY_ONLY}", "${SHIPPED_OBLIGATION_CARRIED}", or the production path it is unmet at`,
      );
      continue;
    }

    // The contradictions the vocabulary makes derivable. Each is a pair of
    // claims the register already carries elsewhere, so none of them is an
    // author's word against itself: an atom cannot be both met and carried, an
    // atom anchored at production code cannot be exempt from the question, and
    // a carry is only a carry if it goes through an approved transfer.
    obligations += 1;
    if (atom.shipped_obligation === SHIPPED_OBLIGATION_MET && transferred.has(atom.id)) {
      errors.push(
        `${where}: declares the shipped code meets it while leaving through ${transferred.get(atom.id)} as an open remainder; a transferred atom is "${SHIPPED_OBLIGATION_CARRIED}" or the production path it is unmet at`,
      );
      continue;
    }
    // Deliberately NOT gated on coverage here. An atom declaring `met` that no
    // record covers is already the OPEN derivation this instrument produces —
    // raising it as an error too would let a tool reading only its error list
    // look like it had noticed, which is the gap the certification check closes
    // from the other side.
    if (atom.shipped_obligation === SHIPPED_OBLIGATION_MET) continue;

    if (atom.shipped_obligation === SHIPPED_OBLIGATION_AUTHORITY_ONLY) {
      obligations += 1;
      if (transferred.has(atom.id))
        errors.push(
          `${where}: an obligation carried to ${transferred.get(atom.id)} is "${SHIPPED_OBLIGATION_CARRIED}", not exempt from the question`,
        );
      else if (raisingSurfaces === null)
        errors.push(
          `${where}: ${RAISING_NODE}'s charter declares no production surfaces, so whether this atom is anchored at shipped code cannot be derived`,
        );
      else if (
        raisingSurfaces.some(
          (surface) =>
            surfaceContains(surface, atom.evidence_anchor) ||
            surfaceContains(atom.evidence_anchor, surface),
        )
      )
        errors.push(
          `${where}: is anchored at ${atom.evidence_anchor}, inside a production surface ${RAISING_NODE} declares, so the shipped code is its subject and "${SHIPPED_OBLIGATION_AUTHORITY_ONLY}" is not available`,
        );
      continue;
    }

    if (atom.shipped_obligation === SHIPPED_OBLIGATION_CARRIED) {
      obligations += 1;
      if (!transferred.has(atom.id))
        errors.push(
          `${where}: declares its obligation carried, but no approved transfer carries it — a remainder leaves through the pinned routing, never through a status column`,
        );
      continue;
    }

    const owedSurface = atom.shipped_obligation;
    const residue = transferred.get(atom.id);
    obligations += 1;
    if (!residue) {
      errors.push(
        `${where}: an obligation the shipped code does not meet is a remainder and must leave through an approved transfer to an admissible residue, not through a status column`,
      );
      continue;
    }
    obligations += 1;
    try {
      confinedEntry(repoRoot, owedSurface, `${where} owed surface`);
    } catch (error) {
      errors.push(`${where}: owed surface does not resolve: ${error.message}`);
      continue;
    }
    const receivers = (receivingByResidue.get(residue) ?? [])
      .slice()
      .sort((a, b) => a.order - b.order)
      .map((row) => row.owner_node);
    obligations += 1;
    if (!receivers.length) {
      errors.push(
        `${where}: residue ${residue} declares no receiving owner, so there is no block this obligation is carried to`,
      );
      continue;
    }
    let reached = null;
    let unreadable = null;
    for (const owner of receivers) {
      const text = charterText(owner);
      if (text === null) {
        unreadable = owner;
        break;
      }
      const surfaces = charterSurfaces(text);
      if (surfaces && surfaces.some((surface) => surfaceContains(surface, owedSurface))) {
        reached = owner;
        break;
      }
    }
    if (unreadable !== null)
      errors.push(
        `${where}: receiving owner ${unreadable} has no resolvable charter, so whether it may change ${owedSurface} cannot be derived`,
      );
    else if (reached === null)
      errors.push(
        `${where}: none of the owners receiving ${residue} (${receivers.join(" > ")}) declares a production surface containing ${owedSurface}, so no block this obligation is carried to may change the surface that has to change`,
      );
    else owedAtoms.set(atom.id, { surface: owedSurface, residue, owner: reached });
  }

  // --- findings -----------------------------------------------------------
  const expected = new Set([...MUST_CLOSE_FINDINGS, ...Object.keys(RESIDUE_FINDINGS)]);
  const seenFindings = new Set();
  const findingsByClaim = new Map();
  const findingsByResidue = new Map(residueIds.map((id) => [id, []]));
  for (const finding of register.finding) {
    seenFindings.add(finding.id);
    if (!expected.has(finding.id))
      errors.push(`finding ${finding.id}: not in the closed finding universe`);
    const routes = [finding.claim, finding.residue].filter(Boolean);
    if (routes.length !== 1) {
      errors.push(`finding ${finding.id}: must route to exactly one claim or residue`);
      continue;
    }
    if (finding.claim) {
      if (Object.hasOwn(RESIDUE_FINDINGS, finding.id))
        errors.push(
          `finding ${finding.id}: is a remainder entry and may not close against a claim`,
        );
      else if (!claims.has(finding.claim))
        errors.push(`finding ${finding.id}: unknown claim ${finding.claim}`);
      else {
        // Routing a finding to a claim is an assignment; naming the atom that
        // discriminates it is a closure. Without this a finding could be filed
        // against a claim none of whose obligations speak to it, and the
        // register would still read as complete.
        obligations += 1;
        if (!finding.atom)
          errors.push(
            `finding ${finding.id}: must name the atom that discriminates it, not just the claim it is filed under`,
          );
        else if (!atoms.has(finding.atom))
          errors.push(`finding ${finding.id}: unknown atom ${finding.atom}`);
        else if (atoms.get(finding.atom).claim !== finding.claim)
          errors.push(
            `finding ${finding.id}: atom ${finding.atom} belongs to ${atoms.get(finding.atom).claim}, not ${finding.claim}`,
          );
        if (!findingsByClaim.has(finding.claim)) findingsByClaim.set(finding.claim, []);
        findingsByClaim.get(finding.claim).push(finding.id);
      }
    } else {
      if (finding.atom)
        errors.push(
          `finding ${finding.id}: a remainder entry is carried by its residue, not an atom`,
        );
      const required = RESIDUE_FINDINGS[finding.id];
      if (!required)
        errors.push(`finding ${finding.id}: must close against a claim, not a residue`);
      else if (required !== finding.residue)
        errors.push(`finding ${finding.id}: routed to ${finding.residue}, expected ${required}`);
      else if (!findingsByResidue.has(finding.residue))
        // The residue row itself is gone. That already has its own diagnostic;
        // the point here is to still emit it rather than throw on the lookup
        // and abort before any error is returned at all.
        errors.push(`finding ${finding.id}: residue ${finding.residue} has no register row`);
      else findingsByResidue.get(finding.residue).push(finding.id);
    }
  }
  for (const id of expected)
    if (!seenFindings.has(id)) errors.push(`finding ${id}: omitted from the register`);

  // --- deletion and survivor rows ----------------------------------------
  const rowKinds = new Set(register.row.map((row) => row.kind));
  for (const kind of ["deletion", "survivor"])
    if (!rowKinds.has(kind)) errors.push(`rows: no ${kind} row`);
  for (const row of register.row) {
    obligations += 1;
    if (!/^[A-Z][A-Z0-9]*-AC\d+$/u.test(row.receiving_criterion)) {
      errors.push(
        `row "${row.subject}": receiving criterion ${row.receiving_criterion} is not an acceptance id`,
      );
      continue;
    }
    resolveCriterion(row.receiving_criterion, row.receiving_criterion_role, `row "${row.subject}"`);
  }

  // --- derivation ---------------------------------------------------------
  const claimStatus = new Map();
  for (const [id, claim] of claims) {
    const uncovered = claim.atoms.filter(
      (atomId) => !coverage.has(atomId) && !transferred.has(atomId),
    );
    const bounded = claim.atoms.filter((atomId) => transferred.has(atomId));
    // An obligation the shipped code does not meet is a reason this claim is
    // bounded, stated HERE where the lifecycle is derived rather than left to
    // the atom-side rule that routes it. Reading owedness only as a count
    // beside the status was the defect: the derivation never saw it, so a claim
    // carrying a requirement known not to hold derived PROVEN like any other.
    const owed = claim.atoms.filter((atomId) => owedAtoms.has(atomId));
    let status;
    if (refusedClaims.has(id)) status = REFUSED;
    else if (uncovered.length) status = OPEN;
    else if (bounded.length || owed.length || limitedClaims.has(id)) status = PROVEN_BOUNDED;
    else status = PROVEN;
    // Boundedness is only admissible when EVERY reason for it is an approved
    // transfer to an allowed remainder. A derived limit is not a transfer, so
    // a limited claim is bounded and inadmissible — that is the whole content
    // of "a disclosed limit must force bounded status" — and an owed obligation
    // is admissible on exactly the same terms as any other remainder, never on
    // terms of its own.
    const admissible =
      status === PROVEN ||
      (status === PROVEN_BOUNDED &&
        !limitedClaims.has(id) &&
        bounded.length > 0 &&
        owed.every((atomId) => bounded.includes(atomId)) &&
        bounded.every((atomId) => ALLOWED_RESIDUES.includes(transferred.get(atomId))));
    if (status === PROVEN_BOUNDED && !admissible) {
      errors.push(`claim ${id}: bounded without an approved transfer to an admissible residue`);
      // The claim id alone says a status is wrong without saying what made it
      // wrong. A derived limit is the usual cause and it is already computed
      // per record, so it is reported here beside the claim it bounded — an
      // author who changed a lane's selection script otherwise reads a claim
      // name and has to rediscover which artifact moved.
      for (const proof of register.proof)
        for (const atomId of proof.covers)
          if (claim.atoms.includes(atomId) && coverage.get(atomId)?.includes(proof.id))
            for (const limit of proofLimits.get(proof.id) ?? [])
              errors.push(`claim ${id}: bounded by ${proof.id} — ${limit}`);
    }
    claimStatus.set(id, { status, uncovered, bounded, owed, admissible });
  }

  const worst = [...claimStatus.values()].some((row) => row.status === REFUSED)
    ? REFUSED
    : [...claimStatus.values()].some((row) => row.status === OPEN)
      ? OPEN
      : READY_FOR_REVIEW;
  const state = errors.length ? REFUSED : worst;

  return {
    errors,
    model: {
      register,
      claims,
      atoms,
      claimStatus,
      coverage,
      transferred,
      proofLimits,
      proofRefresh,
      findingsByClaim,
      findingsByResidue,
      receivingByResidue,
      owedAtoms,
      obligations,
      state,
    },
  };
}

/**
 * Whether the tool may certify this analysis.
 *
 * Deriving `OPEN` and then printing a pass line is the same defect as accepting
 * an author-set status: the command that owns the derivation would be
 * publishing a verdict its own derivation does not support. Dropping one atom
 * id from a proof's `covers` list is enough to reach that state, so the check
 * is on the derived value rather than on the error list alone.
 */
/**
 * Whether a derivation may be PUBLISHED — written to the generated view.
 *
 * Publication is a separate decision from certification and it is the stricter
 * one. Certification asks whether the derived state is reviewable; publication
 * asks whether this run may leave a warm artifact behind at all. A run that
 * produced errors did not derive the register — it derived a partial reading of
 * a register it refused — so writing its view would leave a generated file on
 * disk that a later reader, and the freshness comparison itself, would take for
 * the current derivation. A degraded run therefore publishes nothing: the view
 * on disk stays the last one a clean derivation produced, and `--check` keeps
 * comparing against a fresh render rather than against a partial one.
 *
 * The two halves together are what make the generated view incremental-equals-
 * fresh: the committed artifact must equal what a fresh derivation renders
 * byte for byte, and no refused run may ever become the artifact it is compared
 * against.
 */
export function publication({ errors, model }) {
  if (errors.length) return { publish: false, reason: `${errors.length} errors` };
  if (!model) return { publish: false, reason: "no derivation" };
  return { publish: true, reason: null };
}

export function certification({ errors, model }) {
  if (errors.length) return { ok: false, reason: `${errors.length} errors` };
  if (!model) return { ok: false, reason: "no derivation" };
  if (model.state !== READY_FOR_REVIEW)
    return { ok: false, reason: `derived state is ${model.state}, not ${READY_FOR_REVIEW}` };
  return { ok: true, reason: null };
}

const VIEW_HEADER = `<!-- GENERATED by roadmap/0.1.0-tama/tools/closure-register.mjs from
     ${REGISTER_RELATIVE}. Do not edit: \`--check\` compares this file byte for
     byte against the regenerated view and fails when they differ. -->`;

/** A table cell: a literal pipe would otherwise split the column. */
const cell = (text) => String(text).replaceAll("|", "\\|");

export function renderView(model) {
  const { register, claims, atoms, claimStatus, coverage, transferred, proofLimits } = model;
  const lines = [VIEW_HEADER, "", "# TypeScript mapper closure", ""];
  lines.push(`Instrument state: **${model.state}**.`, "");
  lines.push(
    `Owner: ${register.ratification.owner}.`,
    "",
    `Displaced: ${register.ratification.displaced_owner}.`,
    "",
    `Ratified contract: [\`${register.ratification.contract}\`](../../${register.ratification.contract}).`,
    "",
  );

  lines.push("## Claims", "");
  // `Owed` names which of a claim's carried atoms are carried because the
  // shipped code does not meet them, as opposed to because the question is
  // still open. It is a breakdown of a status the derivation already reached —
  // an owed obligation leaves through an approved transfer, so the row it
  // appears on can never read PROVEN. Publishing owedness as a column BESIDE an
  // unaffected status was the defect this replaces.
  lines.push(
    "| Claim | Derived | Atoms | Covered | Owed | Transferred | Findings closed |",
    "| --- | --- | --- | --- | --- | --- | --- |",
  );
  for (const [id, claim] of claims) {
    const derived = claimStatus.get(id);
    const covered = claim.atoms.filter((atomId) => coverage.has(atomId)).length;
    const owed = claim.atoms.filter((atomId) => model.owedAtoms.has(atomId)).length;
    const closed = (model.findingsByClaim.get(id) || []).join(", ") || "—";
    lines.push(
      `| \`${id}\` | ${derived.status} | ${claim.atoms.length} | ${covered} | ${owed} | ${derived.bounded.length} | ${closed} |`,
    );
  }
  lines.push("");
  for (const [id, claim] of claims) {
    lines.push(`### ${id}`, "", claim.statement, "");
    lines.push(
      `Subject: ${claim.subject.map((subject) => `\`${subject}\``).join(", ")}. A proof whose own recorded command runs one of these may not cover this claim.`,
      "",
    );
    for (const atomId of claim.atoms) {
      const atom = atoms.get(atomId);
      const proofs = coverage.get(atomId);
      const where = proofs
        ? proofs.map((proof) => `\`${proof}\``).join(", ")
        : `transferred to \`${transferred.get(atomId)}\``;
      const enforced = atom.received_by
        ? `; enforced at \`${atom.received_by}\` (${atom.received_by_role})`
        : "";
      const owed = model.owedAtoms.get(atomId);
      lines.push(
        `- \`${atomId}\`${owed ? " **[owed]**" : ""} — ${atom.statement} (${where}${enforced})`,
      );
      lines.push(`  - evidence must exercise \`${atom.evidence_anchor}\``);
      if (owed)
        lines.push(
          `  - OWED: the shipped code does not meet this yet, so it leaves through \`${owed.residue}\`. \`${owed.owner}\` receives that remainder and its charter puts \`${owed.surface}\` inside its declared production surfaces.`,
        );
      if (atom.contract_anchor)
        lines.push(`  - \`${atom.contract_section}\` states: "${cell(atom.contract_anchor)}"`);
    }
    lines.push("");
  }

  lines.push("## Proof records", "");
  lines.push(
    "| Proof | Command | Selected | Passed | Skipped | Refresh reach | Derived limits |",
    "| --- | --- | --- | --- | --- | --- | --- |",
  );
  const adapters = new Map(register.adapter.map((row) => [row.id, row]));
  for (const proof of register.proof) {
    const adapter = adapters.get(proof.adapter);
    const command = [adapter.runner, ...adapter.argv_prefix, ...proof.argv_tail].join(" ");
    const refreshed = model.proofRefresh.get(proof.id) ?? "unresolved";
    const limits = proofLimits.get(proof.id) ?? [];
    lines.push(
      `| \`${proof.id}\` | \`${command}\` | ${proof.selected} | ${proof.passed} | ${proof.skipped} | ${cell(refreshed)} | ${limits.length ? cell(limits.join("; ")) : "none"} |`,
    );
  }
  lines.push("");
  for (const proof of register.proof) {
    // A runner whose verdict is per case rather than one summary line is
    // transcribed in full, so the view reproduces it as a block rather than
    // folding several terminal lines into one.
    const summary = proof.terminal_summary.split("\n");
    if (summary.length === 1) lines.push(`- \`${proof.id}\` terminal summary: ${summary[0]}`);
    else {
      lines.push(`- \`${proof.id}\` terminal summary:`, "");
      for (const row of summary) lines.push(`      ${row}`);
      lines.push("");
    }
    if (proof.skip_basis) lines.push(`  - declared skips: ${proof.skip_basis}`);
  }
  lines.push("");

  lines.push("## Negative controls", "");
  for (const control of register.control) {
    const applied =
      control.kind === "source"
        ? `\`${control.subject}\``
        : `command arguments ${control.argv_delta.map((token) => `\`${token}\``).join(" ")}`;
    lines.push(
      `- \`${control.id}\` (${control.kind}, ${control.uniqueness}) — ${control.mutation}`,
      `  - applied to: ${applied}`,
      // A transcribed refusal is the runner's own multi-line output, so a
      // continuation line has to stay inside the bullet it belongs to rather
      // than becoming a sibling paragraph the reader has to reassociate.
      `  - observed: ${control.observed.replaceAll("\n", "\n    ")}`,
    );
  }
  lines.push("");

  lines.push("## Remainders", "");
  for (const residue of register.residue) {
    lines.push(`### ${residue.id}`, "", residue.statement, "");
    lines.push(
      `Findings carried: ${(model.findingsByResidue.get(residue.id) || []).join(", ")}.`,
      "",
    );
    for (const row of [...model.receivingByResidue.get(residue.id)].sort(
      (a, b) => a.order - b.order,
    ))
      lines.push(
        `${row.order}. \`${row.owner_node}\` / \`${row.criterion}\` (${row.criterion_role}) — ${row.gate}`,
      );
    lines.push("");
  }

  lines.push("## Findings", "");
  lines.push("| Finding | Routed to | Closed by | Statement |", "| --- | --- | --- | --- |");
  for (const finding of register.finding)
    lines.push(
      `| \`${finding.id}\` | \`${finding.claim || finding.residue}\` | ${finding.atom ? `\`${finding.atom}\`` : "its receiving rows"} | ${cell(finding.statement)} |`,
    );
  lines.push("");

  lines.push("## Deletion and survivor rows", "");
  lines.push(
    "| Kind | Subject | Disposition | Received by | Role |",
    "| --- | --- | --- | --- | --- |",
  );
  for (const row of register.row)
    lines.push(
      `| ${row.kind} | ${cell(row.subject)} | ${cell(row.disposition)} | \`${row.receiving_criterion}\` | ${row.receiving_criterion_role} |`,
    );
  // The trailing empty entry supplies the file's final newline. Appending one
  // as well would leave a blank line at end of file.
  lines.push("");
  return lines.join("\n");
}

/** True when the generated view on disk matches the view the register renders. */
export function viewIsFresh(packageRoot, model) {
  const viewFile = path.join(packageRoot, VIEW_RELATIVE);
  if (!fs.existsSync(viewFile)) return false;
  return fs.readFileSync(viewFile, "utf8") === renderView(model);
}

function main() {
  const write = process.argv.includes("--write");
  const analysis = analyze();
  const { errors, model } = analysis;
  const publishable = publication(analysis);
  if (!publishable.publish) {
    if (errors.length) console.error(errors.map((error) => `ERROR: ${error}`).join("\n"));
    else console.error(`ERROR: refusing to publish the register: ${publishable.reason}`);
    process.exit(1);
  }
  const view = renderView(model);
  const viewFile = path.join(PACKAGE_ROOT, VIEW_RELATIVE);
  if (write) {
    fs.mkdirSync(path.dirname(viewFile), { recursive: true });
    fs.writeFileSync(viewFile, view);
  } else {
    const onDisk = fs.existsSync(viewFile) ? fs.readFileSync(viewFile, "utf8") : null;
    if (onDisk !== view) {
      console.error(`ERROR: ${VIEW_RELATIVE} is stale; regenerate with --write`);
      process.exit(1);
    }
  }
  const certified = certification(analysis);
  if (!certified.ok) {
    console.error(`ERROR: refusing to certify the register: ${certified.reason}`);
    process.exit(1);
  }
  const register = model.register;
  console.log(
    `closure-register: PASS instrument=${register.instrument} claims=${register.claim.length} atoms=${register.atom.length} proofs=${register.proof.length} controls=${register.control.length} findings=${register.finding.length} residues=${register.residue.length} obligations=${model.obligations} state=${model.state}`,
  );
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
