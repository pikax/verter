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
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve as resolvePath,
  sep,
} from "node:path";
import process from "node:process";
import { TomlError, parseToml } from "./lib/rev11-toml.mjs";
import { evaluateCheckpointException } from "./lib/stack-window-lib.mjs";

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

// templates/program-state.template.toml:50-51 — the declared block-status enum.
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

// templates/program-state.template.toml:52 — the declared review-result enum.
const REVIEW_ENUM = new Set([
  "NOT_REQUIRED",
  "PENDING",
  "PASS",
  "BLOCKING",
  "NOT_PROVEN",
  "INVALIDATED",
]);
const REVIEW_FIELDS = ["conformance_review", "architecture_review", "adversarial_review"];
// Each review mandate's paired reviewed-candidate SHA field — see the ledger
// header comment (templates/program-state.template.toml). Binds a PASS verdict
// to the exact candidate it was issued against.
const REVIEWED_SHA_FIELDS = {
  conformance_review: "conformance_reviewed_sha",
  architecture_review: "architecture_reviewed_sha",
  adversarial_review: "adversarial_reviewed_sha",
};

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
// A conservative, safe-enough branch/ref-name shape: must start and end with
// an alphanumeric character (rules out a leading "-", which `git rev-parse`
// could otherwise misparse as an option) and contain only characters legal
// in a git ref component. Not full `git check-ref-format` compliance — git
// itself is the authority on whether the resolved ref actually exists; this
// only guards the shell-out from a hostile/malformed value.
const REF_NAME_RE = /^[A-Za-z0-9](?:[A-Za-z0-9._/-]*[A-Za-z0-9])?$/;

// Shared "**Status:**" prose-paragraph classification. This is the ONE place
// a document's own text is read for its ratification state — used by the
// enabling_amendment gate below (amendments/AMD-*.md) and by the authority-
// registry's AMENDMENT/RULING document check further down. Do not duplicate
// this parsing; route every ratification-from-text read through here.
//
// The Status field is a markdown paragraph: the declaring line through the
// next blank line (AMD-009's "**RATIFIED ...**" and AMD-001's multi-line
// "NOT part of ..." wrap onto following lines). "NOT RATIFIED" (AMD-005)
// wins over any other "ratified" mention in the same paragraph; bare
// "ratified"/"maintainer-ratified" (AMD-002/003/004, AMD-006/007/008/009/010)
// is ratified; anything else — including AMD-001's "Registered amendment ...
// NOT part of the verbatim-reconstructed authority set", which never uses
// the ratified verb at all — defaults to not-ratified. Returns
// `present: false` when no **Status:** line exists at all; callers decide
// what an absent line means for their document kind (an amendment without
// one is unparseable; a maintainer ruling without one is not, since only
// charters/amendments carry that convention — ARCH-RULING-ORCHESTRATION-
// AUTHORITY-MODEL.md's own inventory of what the gate parses).
function parseStatusParagraph(text) {
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((l) => l.startsWith("**Status:**"));
  if (start === -1) return { present: false };
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
  // Report only the paragraph's first line in violation text — readable,
  // and every existing document's ratification verdict is already decided
  // by its first line; classification above still sees the full paragraph.
  return { present: true, ratified, statusText: lines[start].trim() };
}

// True when `target` (an absolute path) resolves to `dir` itself or
// somewhere strictly beneath it. Used to enforce that an authority-registry
// document's declared `kind` matches where the file actually lives —
// structural placement, not text sniffing.
function isPathUnder(dir, target) {
  const rel = relative(dir, target);
  return rel === "" || (rel !== ".." && !rel.startsWith(`..${sep}`) && !isAbsolute(rel));
}

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
// well-formed sha — including conformance_reviewed_sha/architecture_reviewed_
// sha/adversarial_reviewed_sha, so a fabricated reviewed-candidate identity
// cannot pass) and tree-pairing (every well-formed tree field, resolved
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
    map.set(
      specs[i],
      m && m[2] !== "missing" && m[2] !== "ambiguous" ? { oid: m[1], type: m[2] } : null,
    );
  }
  return map;
}

function verifyLiveGitIdentities(stateById, v, pinnedTrunk) {
  const cwd = process.cwd();
  const probe = runGit(["rev-parse", "--is-inside-work-tree"], cwd);
  if (probe.status !== 0 || (probe.stdout ?? "").trim() !== "true") {
    v(
      `live mode requires a git repository to verify base_sha/candidate_sha/accepted_sha/candidate_tree/accepted_tree against real git objects, but git is unavailable at ${cwd}: ${gitFailureReason(probe)} — this is a loud setup failure, never a silent skip of identity verification`,
    );
    return;
  }

  // Reviewed-candidate SHAs join the SAME existence batch (check 1 below) —
  // no second git shell-out path. They are NOT tree-paired and NOT subject to
  // the ACCEPTED-only reachability/ancestry checks (3 & 4): a reviewed SHA is
  // evidence a review was issued against a real commit, not a landing claim.
  const SHA_FIELDS = [
    "base_sha",
    "candidate_sha",
    "implementation_candidate_sha",
    "accepted_sha",
    ...Object.values(REVIEWED_SHA_FIELDS),
  ];
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
  // (reachable from the CONFIGURED TRUNK REF's live tip — never checkout
  // HEAD, which routinely differs from the ledger-declared trunk in an
  // ordinary worktree, e.g. a feature-branch checkout or a review worktree,
  // for reasons that have nothing to do with whether accepted_sha actually
  // landed. This is the SAME defect class Finding G fixed for the trunk-pin
  // check itself (resolvePinnedTrunk) — a second call site that sampled
  // checkout HEAD instead of the ledger's own named trunk, found and closed
  // in round 4 (FIX 3). pinnedTrunk is resolved once, by resolvePinnedTrunk,
  // before this function runs; a null pin means that check already recorded
  // its own violation, so accepted_sha reachability cannot be established
  // against an untrustworthy trunk pin either.
  if (pinnedTrunk === null) {
    v(
      `accepted_sha landing/reachability cannot be verified — the ledger's pinned integration trunk (repository.integration_head_sha) failed to resolve or revalidate against the live repository (see the trunk-pin violation above); reachability cannot be checked against an untrustworthy trunk pin`,
    );
    return;
  }
  const revListRes = runGit(["rev-list", pinnedTrunk], cwd);
  if (revListRes.status !== 0) {
    v(
      `live mode could not enumerate commits reachable from the configured trunk ref's tip ${pinnedTrunk} (git rev-list) to verify accepted_sha landing: ${gitFailureReason(revListRes)}`,
    );
    return;
  }
  const reachableFromTip = new Set(revListRes.stdout.split("\n").filter((l) => l !== ""));
  reachableFromTip.add(pinnedTrunk);

  for (const [id, b] of stateById) {
    if (b.status !== "ACCEPTED") continue;
    if (wellFormed(b, "accepted_sha")) {
      const info = resolved.get(b.accepted_sha);
      if (info && info.type === "commit" && !reachableFromTip.has(b.accepted_sha)) {
        v(
          `state block ${id} is ACCEPTED with accepted_sha ${b.accepted_sha} but that commit is not reachable from the configured trunk ref's tip ${pinnedTrunk} — it is not genuinely landed (a dangling or since-rewritten commit is not sufficient evidence of acceptance)`,
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

// Is `ancestorId` a (direct or transitive) predecessor of `id` in the DAG?
// Small active sets only (bounded by MAX_CONCURRENT_IMPLEMENTATION), so a
// plain DFS over program-dag.toml's `predecessors` edges is cheap and needs
// no memoisation.
function isTransitivePredecessor(dagById, ancestorId, id) {
  const seen = new Set();
  const stack = [...(dagById.get(id)?.predecessors ?? [])];
  while (stack.length > 0) {
    const p = stack.pop();
    if (p === ancestorId) return true;
    if (seen.has(p)) continue;
    seen.add(p);
    stack.push(...(dagById.get(p)?.predecessors ?? []));
  }
  return false;
}

// Resolves and revalidates the ledger's PINNED INTEGRATION-TRUNK identity
// (state.repository.integration_head_sha) against the LIVE TIP OF THE
// CONFIGURED INTEGRATION REF (repository.integration_branch, resolved as
// refs/heads/<integration_branch>) — never checkout HEAD, and never
// repository.branch/head_sha.
//
// AMD-013 round 5 (the entry-lock-vs-integration-trunk correction):
// repository.branch/head_sha are the IMMUTABLE A0 entry-lock checkout
// (baseline-lock.md §2; cross-checked against entry_checkout_sha/tree by
// verifyEntryLockIdentity below) — they name where the program's authority
// package was checked out ONCE, at entry, and never move again. Round 4
// pointed this same resolution machinery at THOSE fields, which is why FIX 3
// (below, and in verifyLiveGitIdentities) discovered every post-entry
// ACCEPTED block's accepted_sha unreachable from repository.branch's tip:
// the entry-lock branch is not, and was never meant to be, the operational
// trunk every landing/rehearsal replays against — this program lands onto
// its own long-lived integration branch, which advances with every accepted
// block, while the entry-lock pin correctly never does. `integration_branch`
// / `integration_head_sha` are new, MUTABLE per-validation-run fields naming
// that real, moving trunk explicitly, so this resolution (and everything
// downstream of it — accepted_sha reachability, the fixed-landing-order
// rehearsal) is never again silently aimed at the wrong ref. repository.
// branch/head_sha keep their exact prior meaning and are UNTOUCHED by this
// split (see verifyEntryLockIdentity) — this is an added pair of fields, not
// a redefinition of the existing ones.
//
// AMD-013 v3 review (Finding C) named the ORIGINAL defect this whole
// resolution shape answers: an ambient `git rev-parse HEAD` sampled at
// validation time — trunk could advance between two validator runs with
// nothing in the ledger recording (or re-checking) which trunk was actually
// rehearsed against. The FIRST fix for that (comparing against checkout HEAD
// instead) was itself wrong: HEAD names only where THIS WORKTREE happens to
// be checked out, which routinely differs from the ledger-declared trunk
// branch (a review worktree on a feature branch, a detached-HEAD CI
// checkout) — that mismatch is not trunk drift, and gating the whole check
// on "more than one block active" was papering over comparing against the
// wrong ref, not a genuine staleness exemption. repository.integration_branch
// — the ledger's own explicit, named integration-trunk ref — is the correct
// oracle, and once the oracle is right there is no reason to run this only
// sometimes: it runs on EVERY live-mode validation.
// Returns the pinned SHA on success; on any failure (malformed/absent
// integration_branch or integration_head_sha, a git failure, or a live
// branch tip that has moved past the ledger's pin) records a violation and
// returns null — callers must not fall back to an un-pinned ambient HEAD,
// and must not fall back to the entry-lock repository.branch/head_sha pair.
function resolvePinnedTrunk(state, cwd, v) {
  const repo = state.repository && typeof state.repository === "object" ? state.repository : {};
  if (!(typeof repo.integration_branch === "string" && REF_NAME_RE.test(repo.integration_branch))) {
    v(
      `live state repository.integration_branch ${JSON.stringify(repo.integration_branch ?? "")} is not a well-formed branch name — the ledger must name the EXPLICIT configured integration-trunk ref the pinned repository.integration_head_sha is validated against (distinct from the immutable entry-lock repository.branch), not let the validator fall back to sampling checkout HEAD`,
    );
    return null;
  }
  if (!(typeof repo.integration_head_sha === "string" && SHA_RE.test(repo.integration_head_sha))) {
    v(
      `live state repository.integration_head_sha ${JSON.stringify(repo.integration_head_sha ?? "")} is not a resolved 40-char lowercase git object id — the ledger must PIN the integration-trunk identity every rehearsal replays against, not let the validator sample an ambient git rev-parse HEAD at run time`,
    );
    return null;
  }
  const trunkRef = `refs/heads/${repo.integration_branch}`;
  const tipRes = runGit(["rev-parse", "--verify", trunkRef], cwd);
  if (tipRes.status !== 0) {
    v(
      `live mode could not resolve the configured integration-trunk ref ${trunkRef} (git rev-parse --verify, repository.integration_branch = ${JSON.stringify(repo.integration_branch)}) to revalidate the ledger's pinned trunk repository.integration_head_sha ${repo.integration_head_sha}: ${gitFailureReason(tipRes)}`,
    );
    return null;
  }
  const liveTrunkTip = tipRes.stdout.trim();
  if (liveTrunkTip !== repo.integration_head_sha) {
    v(
      `live state repository.integration_head_sha ${repo.integration_head_sha} does not match the live tip of the configured integration-trunk ref ${trunkRef} (${liveTrunkTip}) — the integration trunk has advanced since the ledger pinned it, and a pinned trunk that has silently gone stale must be resynced before it is trustworthy rehearsal input, not rehearsed against a moving target`,
    );
    return null;
  }
  return repo.integration_head_sha;
}

// Validates the IMMUTABLE A0 entry-lock identity (repository.branch/
// head_sha/head_tree, contracts/baseline-lock.md §2) — distinct from, and
// never a substitute for, resolvePinnedTrunk's mutable integration-trunk
// pin above. repository.branch/head_sha/head_tree are bound ONCE, at A0, to
// the program's entry checkout — they must never move again, so this check
// re-verifies exactly that: well-formed, AND byte-equal to the top-level
// entry_checkout_sha/entry_checkout_tree the A0 entry-lock record itself
// binds. A divergence here means repository.branch/head_sha/head_tree were
// edited after entry — the exact drift baseline-lock.md's immutability
// promise forbids — never a live git resolution (the entry-lock branch may
// no longer even exist as a live ref by the time this runs; that is
// expected and is not itself a violation). Live mode only, matching every
// other identity check in this file — a template ledger's fields are
// unresolved placeholders with no promise to cross-check.
function verifyEntryLockIdentity(state, v) {
  const repo = state.repository && typeof state.repository === "object" ? state.repository : {};
  if (!(typeof repo.branch === "string" && REF_NAME_RE.test(repo.branch))) {
    v(
      `live state repository.branch ${JSON.stringify(repo.branch ?? "")} is not a well-formed branch name — the immutable A0 entry-lock branch (contracts/baseline-lock.md §2) must stay a well-formed ref name`,
    );
  }
  const shaOk = typeof repo.head_sha === "string" && SHA_RE.test(repo.head_sha);
  if (!shaOk) {
    v(
      `live state repository.head_sha ${JSON.stringify(repo.head_sha ?? "")} is not a resolved 40-char lowercase git object id — the immutable A0 entry-lock checkout SHA (contracts/baseline-lock.md §2) must stay pinned`,
    );
  } else if (repo.head_sha !== state.entry_checkout_sha) {
    v(
      `live state repository.head_sha ${repo.head_sha} does not equal the top-level entry_checkout_sha ${JSON.stringify(state.entry_checkout_sha ?? "")} — the immutable A0 entry-lock SHA has drifted from its own entry-checkout record (contracts/baseline-lock.md §2); repository.branch/head_sha never move after entry, unlike repository.integration_branch/integration_head_sha`,
    );
  }
  const treeOk = typeof repo.head_tree === "string" && SHA_RE.test(repo.head_tree);
  if (!treeOk) {
    v(
      `live state repository.head_tree ${JSON.stringify(repo.head_tree ?? "")} is not a resolved 40-char lowercase tree object id — the immutable A0 entry-lock checkout TREE (contracts/baseline-lock.md §2) must stay pinned`,
    );
  } else if (repo.head_tree !== state.entry_checkout_tree) {
    v(
      `live state repository.head_tree ${repo.head_tree} does not equal the top-level entry_checkout_tree ${JSON.stringify(state.entry_checkout_tree ?? "")} — the immutable A0 entry-lock TREE has drifted from its own entry-checkout record (contracts/baseline-lock.md §2)`,
    );
  }
}

// Content-verifies the digest-bound A0 entry-lock record (contracts/
// baseline-lock.md §2) against repository.branch/head_sha/head_tree and the
// top-level entry_checkout_sha/entry_checkout_tree — closing the "immutable
// by convention, not by check" gap in verifyEntryLockIdentity above.
// verifyEntryLockIdentity only cross-checks those five fields AGAINST EACH
// OTHER — every one of them an equally mutable field on the SAME in-memory
// ledger, so a single coordinated edit that rewrites all five consistently
// (e.g. to a different branch/checkout entirely) passes that check cleanly.
// This binds them instead to something the ledger cannot also rewrite in the
// same edit: the DAG root block's OWN entry_lock_digest, already required
// to be a real SHA-256 (see the entry_lock_digest gate above), is here
// content-verified — exactly like the evidence_digest binding above — against
// a real `<root>/<id>/entry-lock.toml` file resolved under a declared
// evidence root, and that file's own `[repository]` table (branch,
// entry_checkout_sha, entry_checkout_tree) is required to equal the ledger's
// repository.branch/head_sha/head_tree and entry_checkout_sha/tree exactly.
// A rewrite of the in-memory fields alone cannot also rewrite the separately-
// hashed, digest-pinned file this now cross-checks against.
//
// Deliberately narrower than the evidence_digest binding above: it looks
// only for the exact filename `entry-lock.toml` (never the evidence_digest
// candidate-name list), and an entry-lock.toml that fails to RESOLVE under
// any declared root is a silent skip, not a violation — entry_lock_digest's
// existing shape-only posture for a fixture/ledger that never wrote this
// specific artifact. A RESOLVED record that fails to match — wrong bytes, or
// bytes that hash correctly but disagree with the ledger's own repository/
// entry_checkout fields — is always a violation.
function verifyEntryLockRecordBinding(state, dagRoots, stateById, resolvedRoots, v) {
  if (dagRoots.length !== 1 || resolvedRoots.length === 0) return;
  const rootId = dagRoots[0];
  const b = stateById.get(rootId);
  if (!b || !(typeof b.entry_lock_digest === "string" && DIGEST_RE.test(b.entry_lock_digest))) {
    return;
  }
  let artifactPath = null;
  for (const root of resolvedRoots) {
    const candidate = join(root, rootId, "entry-lock.toml");
    let isFile = false;
    try {
      isFile = statSync(candidate).isFile();
    } catch {
      // missing — try the next root
    }
    if (isFile) {
      artifactPath = candidate;
      break;
    }
  }
  if (artifactPath === null) return; // nothing resolved under any declared root — silent skip

  const bytes = readFileSync(artifactPath);
  const actual = createHash("sha256").update(bytes).digest("hex");
  if (actual !== b.entry_lock_digest) {
    v(
      `state block ${rootId} entry_lock_digest ${b.entry_lock_digest} does not match the SHA-256 of ${artifactPath} (${actual})`,
    );
    return; // do not cross-check field contents against bytes that already fail their own digest
  }

  let parsed;
  try {
    parsed = parseToml(bytes.toString("utf8"));
  } catch (err) {
    v(`state block ${rootId} entry-lock record ${artifactPath} is not valid TOML: ${err.message}`);
    return;
  }
  const recordRepo =
    parsed.repository && typeof parsed.repository === "object" ? parsed.repository : {};
  const repo = state.repository && typeof state.repository === "object" ? state.repository : {};
  const checks = [
    ["repository.branch", repo.branch, "repository.branch", recordRepo.branch],
    [
      "repository.head_sha",
      repo.head_sha,
      "repository.entry_checkout_sha",
      recordRepo.entry_checkout_sha,
    ],
    [
      "repository.head_tree",
      repo.head_tree,
      "repository.entry_checkout_tree",
      recordRepo.entry_checkout_tree,
    ],
    [
      "entry_checkout_sha",
      state.entry_checkout_sha,
      "repository.entry_checkout_sha",
      recordRepo.entry_checkout_sha,
    ],
    [
      "entry_checkout_tree",
      state.entry_checkout_tree,
      "repository.entry_checkout_tree",
      recordRepo.entry_checkout_tree,
    ],
  ];
  for (const [ledgerField, ledgerVal, recordField, recordVal] of checks) {
    if (ledgerVal !== recordVal) {
      v(
        `live state ${ledgerField} ${JSON.stringify(ledgerVal ?? "")} does not equal ${recordField} ${JSON.stringify(recordVal ?? "")} in the digest-bound entry-lock record ${artifactPath} — the immutable A0 entry lock must match its own digest-bound record, not merely its own other, equally mutable, ledger fields`,
      );
    }
  }
}

// Fixed-landing-order cumulative rehearsal for every concurrently ACTIVE
// block (IN_PROGRESS ∪ REVIEW ∪ ACCEPTANCE_RECOMMENDED — see the
// concurrent-implementation-ceiling comment block above for why REVIEW is
// active). Runs only when more than one block is concurrently active — a
// single active block has nothing to land against but real trunk, which the
// ordinary git-identity checks already cover. Live mode only (needs real git
// objects); template-mode candidate fields carry no promise of naming real
// commits.
//
// AMD-013 v3 review (Finding A): this program does not land by MERGING —
// every landing here is a rebase-or-squash onto trunk followed by a
// fast-forward (contracts/stacked-prs.md §9 lists only "Bottom-up" and
// "Atomic final only" as legal landing modes, and its own accepted_sha/tree
// commentary names "a reviewed rebase, squash, merge commit, or merge-queue
// base advance" as the shapes accepted_sha may take — never a landing-time
// two-parent merge of the candidate against trunk). A prior draft of this
// rehearsal used `git merge-tree --write-tree` followed by a TWO-parent
// `git commit-tree` — modeling a MERGE COMMIT. That (a) synthesises
// candidate ancestry no squash/rebase/cherry-pick landing ever creates, and
// (b) let each step's merge-base be whatever git's own commit-graph search
// found between that synthetic two-parent commit and the next candidate —
// NOT the block's own declared base_sha, so a wrong or stale declared base
// was checked for ancestry and then silently ignored by the rehearsal
// itself (Finding D).
//
// This rehearsal instead REPLAYS each block's own delta — the diff from its
// declared base_sha to its rehearsal candidate — onto the cumulative result
// of every prior block, the exact operation `git rebase --onto`/`git
// cherry-pick` perform (a three-way merge whose base is the commit's OWN
// original parent, not whatever an ancestry search finds between two
// unrelated trees): `git merge-tree --write-tree --merge-base=<base_sha>
// <cumulative> <candidate>`. `--merge-base` (git >= 2.38) pins the
// three-way merge's base tree explicitly — the declared base_sha IS the
// delta basis, not a value that is merely ancestry-checked and then
// ignored. Each clean step synthesises a real, unreferenced,
// worktree-untouched SINGLE-PARENT commit via `git commit-tree <tree> -p
// <cumulative>` — modeling the single-parent commit a rebase/squash
// landing actually produces, never a merge commit — so the next step's
// replay sees genuine linear ancestry.
//
//   1. requires every concurrently active block to declare a positive,
//      pairwise-distinct `landing_order` (MAINTAINER-RULING-CONCURRENCY-
//      CEILING-AND-ROSTER.md: "a fixed landing order"; ARCH-RULING-
//      CONCURRENCY-OPERATING-MODEL.md: "a declared order") — with no
//      trustworthy order, nothing below can be rehearsed, so this step
//      alone determines whether rehearsal proceeds at all;
//   2. requires the sole ACCEPTANCE_RECOMMENDED block, when one exists, to
//      be FIRST in that order — contracts/stacked-prs.md's "LAND_READY ...
//      the one currently eligible landing block is ACCEPTANCE_RECOMMENDED";
//   3. requires landing_order to respect every DAG predecessor edge between
//      two concurrently active blocks, AND every same-stack layer ordering
//      (Finding E): where two concurrently active blocks share the same
//      non-empty stack_id and both carry an integer stack_layer, the lower
//      stack_layer must carry the lower landing_order — the same
//      bottom-up-lands-first rule contracts/stacked-prs.md §9 states for a
//      LANDABLE stack, cross-checked here rather than left unbound;
//   4. requires every concurrently active block to declare a well-formed
//      base_sha that is a real ancestor of its rehearsal candidate (`git
//      merge-base --is-ancestor`) — base_sha is no longer optional/
//      decorative for IN_PROGRESS: it is the explicit basis the delta is
//      computed from (Finding D), so a block with no trustworthy base has
//      nothing to replay;
//   5. resolves the ledger's PINNED trunk (resolvePinnedTrunk, Finding C —
//      never an ambient `git rev-parse HEAD` sampled here) and walks the
//      fixed order, at each step replaying that block's OWN
//      base_sha..candidate delta onto the cumulative result of every prior
//      block via the explicit-merge-base call above, then folding it into a
//      single-parent commit. Exit 0 at every step is a clean cumulative
//      landing; a conflict or rehearsal failure at any step stops the walk
//      there — nothing past an unrehearsable step can be vouched for.
//
// The rehearsal identity per block is candidate_sha for REVIEW/
// ACCEPTANCE_RECOMMENDED (contracts/stacked-prs.md:140 — "the exact
// cumulative candidate reviewers inspected"; once a PASS mandate binds to
// it, REVIEWED_SHA_FIELDS' own check already fences it against silent
// drift) and the SEPARATE implementation_candidate_sha for IN_PROGRESS (a
// block with no review yet has no "exact reviewed candidate" to preserve;
// giving its evolving WIP tip its own field, rather than overloading
// candidate_sha, keeps candidate_sha's documented meaning intact for every
// status that actually carries one).
//
// A concurrently active block missing a well-formed rehearsal identity or
// base_sha, or a non-positive/duplicate/order-violating landing_order, or
// an unresolved/stale pinned trunk, cannot be rehearsed at all: the
// no-conflict condition must be established before concurrency is granted,
// not assumed absent evidence, so each is its own violation and the git
// walk is skipped entirely for the whole active set (a partially-
// trustworthy order proves nothing about the untrustworthy part).
//
// One named, unresolved limit remains (recorded, not hidden — see AMD-013
// §8): a single stack window legally holding up to six open REVIEW layers
// at once (A6, contracts/stacked-prs.md §4) could, on its own, approach or
// exceed the program-wide active-block ceiling this file also enforces (see
// that check's own comment) — this rehearsal counts ACTIVE BLOCKS, not
// orchestrator/train identity, a coarser, deliberately conservative proxy
// for "concurrent claude-max trains" absent a ledger field naming the
// latter directly. The previously-named second limit (Finding E, second
// bullet — implementation_candidate_sha was a trusted, unverifiable
// declaration) is CLOSED: implementation_ref binds the pin to a real, live
// git ref, and the rehearsal REQUIRES that ref's resolved tip to equal the
// pin exactly, so a stale pin is now a violation, not a silent trust.
//
// Round 4, FIX 1 + FIX 2: validates the implementation_ref/
// implementation_candidate_sha trust boundary for ONE IN_PROGRESS block.
// Runs UNCONDITIONALLY over every IN_PROGRESS block (see
// verifyImplementationRefFields below) rather than only when
// verifyConcurrentLandingSafety's rehearsal is in play — the prior scoping
// to `active.length > 1` meant an ordinary single-IN_PROGRESS ledger (the
// live ledger's own current shape) validated NEITHER field at all, the same
// conditional-scoping mistake Finding G made for the trunk pin. Returns the
// validated implementation_candidate_sha on success, or null on any failure
// (a violation has already been recorded, exactly once, here).
function checkImplementationRefBinding(id, b, cwd, v) {
  const cand = b.implementation_candidate_sha;
  if (typeof cand !== "string" || !SHA_RE.test(cand)) {
    v(
      `block ${id} is IN_PROGRESS but implementation_candidate_sha is not a resolved 40-char lowercase git object id: ${JSON.stringify(cand ?? "")} — every IN_PROGRESS block must bind a real rehearsal identity, whether or not another block is concurrently active alongside it`,
    );
    return null;
  }
  const ref = b.implementation_ref;
  if (typeof ref !== "string" || !REF_NAME_RE.test(ref)) {
    v(
      `block ${id} is IN_PROGRESS but implementation_ref ${JSON.stringify(ref ?? "")} is not a well-formed ref name — implementation_candidate_sha must be bound to a resolvable live ref, not trusted as a bare declaration`,
    );
    return null;
  }
  // FIX 1: a raw 40-char object id and the literal HEAD pseudoref both
  // satisfy REF_NAME_RE's shape check AND resolve cleanly via `git rev-parse
  // --verify` — a raw OID resolves to itself, and HEAD resolves to wherever
  // THIS worktree happens to be checked out — so neither can ever expose a
  // stale pin no matter how far implementation_candidate_sha has drifted
  // from the block's real WIP branch. Reject both explicitly, before any
  // git call, rather than relying on a downstream resolution check to catch
  // them incidentally (it would not: both resolve just fine).
  if (ref === "HEAD" || SHA_RE.test(ref)) {
    v(
      `block ${id} implementation_ref ${JSON.stringify(ref)} is a raw object id or the literal HEAD pseudoref, not an actual branch ref — either always "resolves" (to itself, or to wherever this worktree happens to be checked out) regardless of how stale implementation_candidate_sha is, so implementation_ref must name a real, independently-resolvable branch (e.g. refs/heads/<branch>), never any rev-parse-able object or pseudoref`,
    );
    return null;
  }
  const refRes = runGit(["rev-parse", "--verify", ref], cwd);
  if (refRes.status !== 0) {
    v(
      `block ${id} implementation_ref ${JSON.stringify(ref)} could not be resolved (git rev-parse --verify): ${gitFailureReason(refRes)} — implementation_candidate_sha cannot be bound to a ref that does not resolve to a real commit`,
    );
    return null;
  }
  // FIX 1: confirms the resolved object is a REAL branch (refs/heads/...),
  // not some other rev-parse-able spec the shape check and the explicit
  // HEAD/raw-OID rejection above don't already exclude (e.g. a tag).
  const symRes = runGit(["rev-parse", "--symbolic-full-name", ref], cwd);
  const fullRef = symRes.status === 0 ? symRes.stdout.trim() : "";
  // Computed OUTSIDE the v(...) template literal, never nested inside it: a
  // backtick-delimited template literal nested inside another one defeats
  // the mutation suite's textual check-inventory extraction (CALLEE_RE stops
  // at the first unescaped backtick), which would silently truncate this
  // check out of existence for coverage purposes.
  const symDetail = symRes.status !== 0 ? `: ${gitFailureReason(symRes)}` : "";
  if (symRes.status !== 0 || !fullRef.startsWith("refs/heads/")) {
    v(
      `block ${id} implementation_ref ${JSON.stringify(ref)} does not resolve to a real branch (git rev-parse --symbolic-full-name reports ${JSON.stringify(fullRef)}${symDetail}) — implementation_ref must name an actual refs/heads/... branch, never any other rev-parse-able object or pseudoref`,
    );
    return null;
  }
  const liveRefTip = refRes.stdout.trim();
  if (liveRefTip !== cand) {
    v(
      `block ${id} implementation_ref ${JSON.stringify(ref)} resolves to ${liveRefTip}, but the declared implementation_candidate_sha is ${cand} — the pin does not match the live ref's current tip; a stale pin is not verifiable rehearsal input`,
    );
    return null;
  }
  return cand;
}

// FIX 2: runs checkImplementationRefBinding over EVERY IN_PROGRESS block,
// unconditionally — never gated on how many blocks are concurrently active.
// Returns a Map id -> validated implementation_candidate_sha (or null for a
// block that failed validation; the violation was already recorded above),
// consumed by verifyConcurrentLandingSafety below so the rehearsal reuses
// this result rather than re-running (and re-reporting) the same checks a
// second time when >1 block is concurrently active.
function verifyImplementationRefFields(stateById, v) {
  const cwd = process.cwd();
  const results = new Map();
  for (const [id, b] of stateById) {
    if (b.status !== "IN_PROGRESS") continue;
    results.set(id, checkImplementationRefBinding(id, b, cwd, v));
  }
  return results;
}

function verifyConcurrentLandingSafety(
  active,
  dagById,
  state,
  v,
  pinnedTrunk,
  implementationRefResults,
) {
  if (active.length < 2) return;
  const cwd = process.cwd();
  // Trunk-pin resolution (resolvePinnedTrunk, Finding C) now runs
  // UNCONDITIONALLY on every live-mode validation (see the call site in
  // main()) — it is no longer resolved in here. A null pin means the caller
  // already recorded the specific trunk-pin violation; this function records
  // its OWN, distinct violation naming why the rehearsal itself cannot run,
  // rather than silently skipping.
  if (pinnedTrunk === null) {
    v(
      `the fixed-landing-order rehearsal cannot run for ${active.length} concurrently active block(s) — the ledger's pinned integration trunk (repository.integration_head_sha) failed to resolve or revalidate against the live repository (see the trunk-pin violation above); nothing here can be rehearsed against an untrustworthy trunk pin`,
    );
    return;
  }
  let structurallyValid = true;
  const entries = [];
  for (const b of active) {
    // Round 4, FIX 1 + FIX 2: an IN_PROGRESS entry's rehearsal identity
    // (implementation_candidate_sha) and its implementation_ref binding were
    // already fully validated, UNCONDITIONALLY, by verifyImplementationRefFields
    // (called once from main() before this function runs) — reuse that result
    // rather than re-running (and, worse, re-reporting under different wording)
    // the same checks a second time here. A `null` entry means a violation was
    // already recorded there; this function must not emit a second one for the
    // same underlying cause.
    let cand;
    if (b.status === "IN_PROGRESS") {
      cand = implementationRefResults.get(b.id);
      if (cand === null || cand === undefined) {
        structurallyValid = false;
        continue;
      }
    } else {
      cand = b.candidate_sha;
      if (typeof cand !== "string" || !SHA_RE.test(cand)) {
        v(
          `block ${b.id} is concurrently active (${b.status}) alongside ${active.length - 1} other block(s) but candidate_sha is not a resolved 40-char lowercase git object id: ${JSON.stringify(cand ?? "")} — the fixed-landing-order rehearsal (contracts/stacked-prs.md, MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md) cannot be established without a real candidate identity for every concurrently active block`,
        );
        structurallyValid = false;
        continue;
      }
    }
    if (typeof b.base_sha !== "string" || !SHA_RE.test(b.base_sha)) {
      v(
        `block ${b.id} is concurrently active (${b.status}) alongside ${active.length - 1} other block(s) but base_sha is not a resolved 40-char lowercase git object id: ${JSON.stringify(b.base_sha ?? "")} — the fixed-landing-order rehearsal replays each block's own base_sha..candidate delta (AMD-013 v3), so a well-formed base is required for every concurrently active block, not merely checked when one happens to be present`,
      );
      structurallyValid = false;
      continue;
    }
    if (!Number.isInteger(b.landing_order) || b.landing_order < 1) {
      v(
        `block ${b.id} is concurrently active alongside ${active.length - 1} other block(s) but landing_order ${JSON.stringify(b.landing_order ?? "")} is not a positive integer — every concurrently active block must declare a fixed landing_order (MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md: "a fixed landing order")`,
      );
      structurallyValid = false;
      continue;
    }
    entries.push({
      id: b.id,
      status: b.status,
      candidate: cand,
      base: b.base_sha,
      order: b.landing_order,
      stackId: typeof b.stack_id === "string" ? b.stack_id.trim() : "",
      stackLayer: b.stack_layer,
    });
  }

  const byOrder = new Map();
  for (const e of entries) {
    if (!byOrder.has(e.order)) byOrder.set(e.order, []);
    byOrder.get(e.order).push(e.id);
  }
  for (const [order, ids] of byOrder) {
    if (ids.length > 1) {
      v(
        `landing_order ${order} is shared by concurrently active blocks [${ids.join(", ")}] — the fixed landing order must be an unambiguous total order over every concurrently active block`,
      );
      structurallyValid = false;
    }
  }

  if (entries.length > 0) {
    const minOrder = Math.min(...entries.map((e) => e.order));
    for (const e of entries) {
      if (e.status === "ACCEPTANCE_RECOMMENDED" && e.order !== minOrder) {
        v(
          `block ${e.id} is ACCEPTANCE_RECOMMENDED — the currently eligible landing block (contracts/stacked-prs.md) — but landing_order ${e.order} is not the minimum among concurrently active blocks: the block under final certification must be first in the fixed landing order`,
        );
        structurallyValid = false;
      }
    }
  }

  for (const a of entries) {
    for (const b of entries) {
      if (a.id === b.id) continue;
      if (isTransitivePredecessor(dagById, a.id, b.id) && !(a.order < b.order)) {
        v(
          `block ${a.id} is a predecessor of concurrently active block ${b.id} (program-dag.toml) but landing_order ${a.order} is not before ${b.order} — a predecessor must land before its dependent in the fixed landing order`,
        );
        structurallyValid = false;
      }
      // Finding E: landing_order must also respect intra-stack layer order
      // — contracts/stacked-prs.md §9's bottom-up-lands-first rule — for
      // two concurrently active blocks sharing the same stack, independent
      // of whether a DAG predecessor edge exists between them (a stack's
      // private sublayers need not be DAG predecessors of one another).
      if (
        a.stackId !== "" &&
        a.stackId === b.stackId &&
        Number.isInteger(a.stackLayer) &&
        Number.isInteger(b.stackLayer) &&
        a.stackLayer < b.stackLayer &&
        !(a.order < b.order)
      ) {
        v(
          `block ${a.id} (stack ${a.stackId}, stack_layer ${a.stackLayer}) is a lower stack layer than concurrently active same-stack block ${b.id} (stack_layer ${b.stackLayer}) but landing_order ${a.order} is not before ${b.order} — a lower stack layer must land before a higher layer in the same stack (contracts/stacked-prs.md §9, bottom-up landing)`,
        );
        structurallyValid = false;
      }
    }
  }

  // A structurally invalid order/identity set cannot be rehearsed — nothing
  // below is meaningful without a trustworthy order to walk.
  if (!structurallyValid) return;

  entries.sort((x, y) => x.order - y.order);

  let cumulative = pinnedTrunk;

  for (const e of entries) {
    const anc = runGit(["merge-base", "--is-ancestor", e.base, e.candidate], cwd);
    if (anc.status !== 0 && anc.status !== 1) {
      v(
        `block ${e.id} base_sha ${e.base} ancestry against its rehearsal candidate ${e.candidate} could not be checked: ${gitFailureReason(anc)}`,
      );
      return;
    }
    if (anc.status === 1) {
      v(
        `block ${e.id} declared base_sha ${e.base} is not an ancestor of its rehearsal candidate ${e.candidate} — the declared delta cannot be trusted for the fixed-landing-order rehearsal`,
      );
      return;
    }
    // No --quiet: --quiet "allows merge-tree ... to avoid writing most
    // objects created by merges" and suppresses the toplevel-tree-OID
    // output entirely on a clean merge — this rehearsal needs that OID to
    // synthesise the next step's cumulative commit, so it always requests
    // full output. --merge-base=<base> pins the three-way merge's base tree
    // to the block's OWN declared base_sha — replaying its delta onto
    // cumulative, rather than letting git's ancestry search pick a
    // merge-base between the synthetic cumulative commit and the candidate
    // (Finding A/D).
    const merge = runGit(
      ["merge-tree", "--write-tree", `--merge-base=${e.base}`, cumulative, e.candidate],
      cwd,
    );
    if (merge.status === 1) {
      v(
        `block ${e.id} (landing_order ${e.order}) does not land cleanly onto the cumulative result of every prior block in the fixed landing order — replaying its base_sha ${e.base}..${e.candidate} delta via git merge-tree --write-tree --merge-base=${e.base} against ${cumulative} reports real content conflicts (contracts/stacked-prs.md, MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md)`,
      );
      return;
    }
    if (merge.status !== 0) {
      v(
        `block ${e.id} (landing_order ${e.order}) cumulative landing rehearsal could not be checked — git merge-tree --write-tree --merge-base=${e.base} between ${cumulative} and ${e.candidate} failed: ${gitFailureReason(merge)}`,
      );
      return;
    }
    const tree = merge.stdout.trim().split("\n")[0];
    // ONE parent only (cumulative) — this synthesises the SINGLE-PARENT
    // commit a rebase/squash landing actually produces, never a two-parent
    // merge commit (Finding A): the block's own delta replayed on top of
    // trunk-so-far, not a merge of two histories.
    const commit = runGit(
      ["commit-tree", tree, "-p", cumulative, "-m", `landing rehearsal: ${e.id}`],
      cwd,
    );
    if (commit.status !== 0) {
      v(
        `block ${e.id} (landing_order ${e.order}) cumulative landing rehearsal could not synthesise a rehearsal commit — git commit-tree failed: ${gitFailureReason(commit)}`,
      );
      return;
    }
    cumulative = commit.stdout.trim();
  }
}

// Validation

function usageFail(msg) {
  process.stderr.write(
    `${msg}\nusage: node scripts/validate-program-state.mjs --dag <program-dag.toml> --state <program-state.toml> --mode template|live [--stack-window <stack-window.toml>] [--authority <authority-registry.toml> | --no-authority]\n` +
      `In live mode the block-authorization registry is MANDATORY by default, resolved next to --state as authority-registry.toml unless --authority names a different path. --no-authority is the only opt-out, and it must be named explicitly.\n`,
  );
  process.exit(2);
}

const VALUE_FLAGS = ["--dag", "--state", "--mode", "--stack-window", "--authority"];
const BOOLEAN_FLAGS = ["--no-authority"];

function parseArgs(argv) {
  const opts = Object.create(null);
  let i = 0;
  while (i < argv.length) {
    const flag = argv[i];
    if (BOOLEAN_FLAGS.includes(flag)) {
      opts[flag.slice(2)] = true; // e.g. opts["no-authority"]
      i += 1;
      continue;
    }
    if (!VALUE_FLAGS.includes(flag)) usageFail(`unknown argument: ${flag}`);
    const value = argv[i + 1];
    if (value === undefined) usageFail(`missing value for ${flag}`);
    opts[flag.slice(2)] = value;
    i += 2;
  }
  if (!opts.dag || !opts.state || !opts.mode)
    usageFail("--dag, --state, and --mode are all required");
  if (opts.mode !== "template" && opts.mode !== "live") {
    usageFail(`--mode must be "template" or "live", got ${JSON.stringify(opts.mode)}`);
  }
  if (typeof opts.authority === "string" && opts["no-authority"] === true) {
    usageFail("--authority and --no-authority are mutually exclusive");
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
    // templates/program-state.template.toml:50-51 — closed status enum.
    if (!BLOCK_STATUS_ENUM.has(b.status)) {
      v(`state block ${id} has status ${JSON.stringify(b.status)} outside the declared enum`);
    }
    // templates/program-state.template.toml:52 — closed review enum.
    for (const field of REVIEW_FIELDS) {
      if (field in b && !REVIEW_ENUM.has(b[field])) {
        v(
          `state block ${id} has ${field} ${JSON.stringify(b[field])} outside the declared review enum`,
        );
      }
    }
  }

  // -- Review verdict identity binding (nothing previously bound a verdict to
  // the exact candidate it was issued against — see templates/program-state.
  // template.toml's conformance_reviewed_sha/architecture_reviewed_sha/
  // adversarial_reviewed_sha comment). Structural, so it runs in BOTH modes:
  // a PASS mandate must carry a well-formed reviewed SHA equal to the row's
  // CURRENT candidate_sha (a fix round, a rebase, or a restack that advances
  // candidate_sha without a fresh review leaves the old PASS stale); a
  // non-PASS mandate must carry no reviewed SHA at all. Existence of the
  // reviewed SHA as a real git commit object is checked separately, in live
  // mode only, by verifyLiveGitIdentities below.
  for (const [id, b] of stateById) {
    for (const [mandateField, shaField] of Object.entries(REVIEWED_SHA_FIELDS)) {
      if (!(mandateField in b)) continue;
      const mandate = b[mandateField];
      const reviewedSha = b[shaField];
      if (mandate === "PASS") {
        if (!(typeof reviewedSha === "string" && SHA_RE.test(reviewedSha))) {
          v(
            `state block ${id} has ${mandateField} = PASS but ${shaField} is not a non-empty 40-char lowercase git object id: ${JSON.stringify(reviewedSha ?? "")} — a PASS verdict must bind the exact candidate it was issued against`,
          );
          continue;
        }
        if (!(typeof b.candidate_sha === "string" && SHA_RE.test(b.candidate_sha))) {
          v(
            `state block ${id} has ${mandateField} = PASS with ${shaField} = ${reviewedSha} but candidate_sha is not a non-empty 40-char lowercase git object id — cannot verify the verdict is bound to the current candidate`,
          );
          continue;
        }
        if (reviewedSha !== b.candidate_sha) {
          v(
            `state block ${id} has ${mandateField} = PASS but ${shaField} = ${reviewedSha} does not equal candidate_sha = ${b.candidate_sha} — the verdict was issued against a different candidate and is stale`,
          );
        }
      } else if (typeof reviewedSha === "string" && reviewedSha !== "") {
        v(
          `state block ${id} has ${mandateField} = ${JSON.stringify(mandate)} (not PASS) but ${shaField} = ${JSON.stringify(reviewedSha)} is non-empty — a non-PASS mandate must not carry a reviewed candidate SHA`,
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

    // (a) a PRIVATE_CHECKPOINT predecessor. contracts/stacked-prs.md:39,53 let a
    //     PRIVATE_CHECKPOINT predecessor satisfy sequencing only inside a
    //     validated stack window and only for the final acceptance block.
    //     AMD-001 §3: this refusal is SUPERSEDED (never simply deleted) by
    //     the composite stack-window cross-validation when the caller passes
    //     --stack-window — evaluateCheckpointException (scripts/lib/
    //     stack-window-lib.mjs) is the SOLE model of that exception, shared
    //     with scripts/validate-stack-window.mjs, so this validator never
    //     grows a second, parallel notion of the same question. With no
    //     --stack-window given, the original fail-closed refusal stands
    //     unchanged.
    for (const p of dagBlock.predecessors ?? []) {
      if (stateById.get(p)?.status !== "PRIVATE_CHECKPOINT") continue;
      const stackWindowPath = opts["stack-window"];
      if (!stackWindowPath) {
        v(
          `block ${id} is ${b.status} with predecessor ${p} in PRIVATE_CHECKPOINT — a PRIVATE_CHECKPOINT predecessor satisfies sequencing only inside a validated stack window for the final acceptance block (contracts/stacked-prs.md), which this validator does not model — fail closed`,
        );
        continue;
      }
      const result = evaluateCheckpointException({
        windowPath: stackWindowPath,
        predecessorId: p,
        successorId: id,
        stateById,
        dagById,
      });
      if (!result.ok) {
        v(
          `block ${id} is ${b.status} with predecessor ${p} in PRIVATE_CHECKPOINT — composite stack-window validation via --stack-window ${stackWindowPath} did not establish the checkpoint exception (AMD-001 §2): ${result.problems.join("; ")}`,
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

  // -- Amendment authority gate
  // templates/program-state.template.toml — `enabling_amendment` names the AMD-ID
  // whose ratification is this block's execution authority ("" when none is
  // needed). Until that amendment's own docs/arch/refactor/rev11/amendments/
  // <AMD-ID>-*.md Status line is ratified, the amendment "has no execution
  // authority" (its own wording) and the block it introduced must go no further
  // than LOCKED. This was previously recorded only as free-text prose in a
  // block's `notes` field — BV1 was unlocked and dispatched on DAG predecessor
  // satisfaction alone while its enabling AMD-005 sat PROPOSED, because nothing
  // read the prose. This check makes the dependency machine-enforced.
  {
    const AMENDMENT_GATED_STATUSES = new Set([
      "READY",
      "IN_PROGRESS",
      "REVIEW",
      "ACCEPTANCE_RECOMMENDED",
      "ACCEPTED",
    ]);
    const amendmentsDir = join(dirname(opts.dag), "amendments");
    const statusCache = new Map(); // amdId -> {path, ratified, statusText} | {error}

    const resolveAmendmentStatus = (amdId) => {
      if (statusCache.has(amdId)) return statusCache.get(amdId);
      const record = (result) => {
        statusCache.set(amdId, result);
        return result;
      };
      let entries;
      try {
        entries = readdirSync(amendmentsDir);
      } catch (err) {
        return record({
          error: `amendments directory ${amendmentsDir} could not be read: ${err.message}`,
        });
      }
      const matches = entries
        .filter((name) => name.startsWith(`${amdId}-`) && name.endsWith(".md"))
        .sort();
      if (matches.length !== 1) {
        return record({
          error: `expected exactly one file matching ${amdId}-*.md under ${amendmentsDir}, found ${matches.length}${matches.length ? ` [${matches.join(", ")}]` : ""}`,
        });
      }
      const filePath = join(amendmentsDir, matches[0]);
      let text;
      try {
        text = readFileSync(filePath, "utf8");
      } catch (err) {
        return record({ error: `amendment file ${filePath} could not be read: ${err.message}` });
      }
      const parsed = parseStatusParagraph(text);
      if (!parsed.present) {
        return record({
          error: `amendment file ${filePath} has no **Status:** line — its ratification state cannot be parsed`,
        });
      }
      return record({ path: filePath, ratified: parsed.ratified, statusText: parsed.statusText });
    };

    for (const [id, b] of stateById) {
      const amdId = typeof b.enabling_amendment === "string" ? b.enabling_amendment.trim() : "";
      if (amdId === "") continue;
      const resolved = resolveAmendmentStatus(amdId);
      if (resolved.error) {
        v(
          `state block ${id} declares enabling_amendment ${JSON.stringify(amdId)} but ${resolved.error}`,
        );
        continue;
      }
      if (resolved.ratified) continue;
      const reasons = [];
      if (typeof b.status === "string" && AMENDMENT_GATED_STATUSES.has(b.status)) {
        reasons.push(`status is ${b.status}`);
      }
      if (b.maintainer_decision === "ACCEPTED") {
        reasons.push(`maintainer_decision is ACCEPTED`);
      }
      if (reasons.length > 0) {
        v(
          `state block ${id} has enabling_amendment ${amdId} but ${resolved.path} is not ratified (Status: ${resolved.statusText}) — an unratified enabling amendment has no execution authority, so the block must not advance beyond LOCKED: ${reasons.join(", ")}`,
        );
      }
    }
  }

  // -- Concurrent implementation ceiling + serialised FINAL certification
  //
  // MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md: "up to 5 concurrent
  // blocks/trains on claude-max" — a maintainer-ratified relaxation of the
  // single-IN_PROGRESS-at-a-time gate this check previously enforced
  // unconditionally (that check's own former comment said a parallel regime
  // "must relax this check under review, not ad hoc" — this IS that
  // reviewed relaxation). The ceiling is a permission, not an instruction:
  // it does not by itself establish that any given set of concurrent
  // blocks is conflict-free (see verifyConcurrentLandingSafety below).
  //
  // ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md: "Allow up to five disjoint
  // blocks in IMPLEMENTATION and targeted testing; SERIALISE final
  // certification" — because the gate cascade under concurrent
  // certification is quadratic while "implementation and review
  // iteration ... dominates wall-clock" (that ruling groups REVIEW's
  // iterative revise/re-review cycle with implementation, in the PARALLEL
  // bucket, not the serialised one). So the ledger's THREE statuses split
  // into three concurrency CLASSES for the purpose of the per-status rules
  // below (current_block binding, ACCEPTANCE_RECOMMENDED-first ordering),
  // but the numeric ceiling itself (Finding B, AMD-013 v3 review) is a
  // SINGLE program-wide cap over the whole active set, not per-class:
  //   - IN_PROGRESS         — implementation;
  //   - REVIEW               — review iteration — contracts/stacked-prs.md
  //                           §4's per-stack open-layer limit and §3.3's
  //                           cross-stack ownership-disjointness rule bound
  //                           how many blocks may sit in REVIEW WITHIN one
  //                           stack or across independent windows, but
  //                           neither is a program-wide numeric cap
  //                           equivalent to the maintainer ruling's flat
  //                           ceiling — so REVIEW blocks are NOT exempt from
  //                           the active-set ceiling below; contracts/
  //                           stacked-prs.md:100 ("Green upper LANDABLE
  //                           layers remain REVIEW, not accepted in
  //                           advance") establishes that REVIEW is not
  //                           capped at ONE the way ACCEPTANCE_RECOMMENDED
  //                           is, not that it is uncapped altogether;
  //   - ACCEPTANCE_RECOMMENDED — FINAL certification: "freeze it once, run
  //                           ONE full gate, obtain ONE impact-bounded
  //                           mandate re-attestation" — capped at exactly
  //                           one block, program-wide, matching
  //                           contracts/stacked-prs.md's own "LAND_READY
  //                           means ... the one currently eligible landing
  //                           block is ACCEPTANCE_RECOMMENDED", AND counted
  //                           within the same active-set ceiling as every
  //                           other concurrently active block (Finding B —
  //                           it is one of the "concurrent blocks/trains",
  //                           not a permitted 6th slot beyond them).
  //
  // current_block names the sole ACCEPTANCE_RECOMMENDED block when one
  // exists ("current_block ... names the certifying block" — certifying
  // now means ACCEPTANCE_RECOMMENDED specifically, the "currently eligible
  // landing block", not REVIEW's parallel iteration). With nothing
  // ACCEPTANCE_RECOMMENDED, current_block instead names any concurrently
  // ACTIVE block (IN_PROGRESS or REVIEW) — the single-active-block serial
  // case (still legal; every cap here is a ceiling, not a floor) satisfies
  // this trivially.
  const MAX_CONCURRENT_IMPLEMENTATION = 5;
  const FINAL_CERTIFICATION_STATUSES = new Set(["ACCEPTANCE_RECOMMENDED"]);
  const ACTIVE_STATUSES = new Set(["IN_PROGRESS", "REVIEW", "ACCEPTANCE_RECOMMENDED"]);
  const certifying = [...stateById.values()].filter((b) =>
    FINAL_CERTIFICATION_STATUSES.has(b.status),
  );
  const active = [...stateById.values()].filter((b) => ACTIVE_STATUSES.has(b.status));
  {
    // Finding B (AMD-013 v3 review): the prior draft capped ONLY
    // `implementing.length` (IN_PROGRESS), so 5 IN_PROGRESS blocks plus 1
    // ACCEPTANCE_RECOMMENDED block was silently legal — six concurrently
    // active trains against a ruling whose own words are "up to 5
    // concurrent blocks/trains", not "up to 5 IN_PROGRESS plus 1 more".
    // This check now caps the WHOLE active set (IN_PROGRESS ∪ REVIEW ∪
    // ACCEPTANCE_RECOMMENDED) regardless of which status a given block
    // holds — see the comment block above for why neither
    // contracts/stacked-prs.md §4 nor §3.3 substitutes for a program-wide
    // cap. This is a conservative, block-counting proxy for "concurrent
    // claude-max trains" (a single stack window legally holding up to six
    // open REVIEW layers, A6, could on its own approach or exceed this
    // ceiling — a named, unresolved tension recorded in AMD-013 §8, not
    // silently swept under).
    if (active.length > MAX_CONCURRENT_IMPLEMENTATION) {
      v(
        `more than ${MAX_CONCURRENT_IMPLEMENTATION} blocks concurrently active (IN_PROGRESS/REVIEW/ACCEPTANCE_RECOMMENDED) — the ratified concurrent-implementation/train ceiling (MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md: "up to 5 concurrent blocks/trains") counts every concurrently active block regardless of status, not merely IN_PROGRESS: [${active.map((b) => b.id).join(", ")}]`,
      );
    }
    if (certifying.length > 1) {
      v(
        `more than one block ACCEPTANCE_RECOMMENDED (final certification must serialise to exactly one block at a time, ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md): [${certifying.map((b) => b.id).join(", ")}]`,
      );
    }
    if (typeof state.current_block === "string" && state.current_block !== "") {
      if (!stateById.has(state.current_block)) {
        v(`current_block ${JSON.stringify(state.current_block)} names no state block`);
      } else if (certifying.length > 0) {
        for (const b of certifying) {
          if (b.id !== state.current_block) {
            v(
              `block ${b.id} is ACCEPTANCE_RECOMMENDED (certifying) but current_block is ${JSON.stringify(state.current_block)} — current_block must name the sole block under final certification (ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md)`,
            );
          }
        }
      } else if (active.length > 0 && !active.some((b) => b.id === state.current_block)) {
        v(
          `current_block ${JSON.stringify(state.current_block)} is not one of the concurrently active (IN_PROGRESS/REVIEW) blocks [${active.map((b) => b.id).join(", ")}] and no block is ACCEPTANCE_RECOMMENDED`,
        );
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
    // SHA and tree; templates/program-state.template.toml:63-66). A SHA/tree
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
    // Hoisted out of the block below (rather than declared `const` inside
    // it) so the entry-lock content-binding check further down — a SEPARATE
    // artifact from any evidence_digest artifact — can reuse the same
    // resolved roots without re-parsing orchestration.evidence_root(s) or
    // re-emitting its unresolvable-root violations a second time.
    let resolvedRoots = [];
    if (declaredRoots !== null && declaredRoots.length > 0) {
      for (const { raw, field } of declaredRoots) {
        if (typeof raw !== "string" || raw === "") {
          v(`live state orchestration.${field} is not a non-empty string: ${JSON.stringify(raw)}`);
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

    // -- Block authorization registry (docs/arch/refactor/rev11/rulings/
    // ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md RULING 1 — replace prose
    // `**Status:**` gates with digest-bound, ratified block-authorization
    // records in a repository authority registry). MANDATORY in live mode —
    // an enforcement a caller must remember to opt into is the same prose
    // gate this replaces (nothing passed --authority; the check was dead).
    // Resolution: --authority <path> names an explicit registry; otherwise
    // the default is authority-registry.toml next to --state (the real
    // ledger's own layout); --no-authority is the sole, explicit, named
    // opt-out (parseArgs rejects combining it with --authority). Once a
    // registry is in force, enforcement is exhaustive and fails closed: an
    // unreadable/unparseable registry, a document with a malformed shape,
    // unknown/mismatched kind, or wrong-directory placement, a digest that
    // does not match current bytes or a path that does not resolve, an
    // AMENDMENT/RULING document whose own **Status:** paragraph is not
    // ratified, an authorization missing required fields or citing an
    // unknown document, and — the core rule this replaces the old prose gate
    // with — any block that has left LOCKED with no authorization record at
    // all are all violations, never a silent skip.
    const authorityPath =
      opts["no-authority"] === true
        ? null
        : typeof opts.authority === "string" && opts.authority !== ""
          ? opts.authority
          : join(dirname(opts.state), "authority-registry.toml");
    if (authorityPath !== null) {
      let authorityDoc = null;
      try {
        authorityDoc = parseToml(readFileSync(authorityPath, "utf8"), authorityPath);
      } catch (err) {
        v(`authority registry ${authorityPath} could not be read or parsed: ${err.message}`);
      }
      if (authorityDoc !== null) {
        // Kind-to-directory: a document's declared `kind` must match where
        // it actually lives, resolved the SAME way (relative to cwd) doc.path
        // is resolved below — otherwise a `kind` field is just an unchecked
        // label an authorization row can set to whatever passes review
        // (proven bypass: cite an arbitrary correctly-digested file and tag
        // it CHARTER, which carries no ratification-text check).
        const VALID_DOCUMENT_KINDS = new Set(["CHARTER", "AMENDMENT", "RULING"]);
        const rev11Dir = resolvePath(process.cwd(), dirname(opts.dag));
        const KIND_DIR = {
          CHARTER: join(rev11Dir, "charters"),
          AMENDMENT: join(rev11Dir, "amendments"),
          RULING: join(rev11Dir, "rulings"),
        };

        // [[document]] — one row per digest-bound authority artifact (a
        // charter, an enabling amendment, a binding ruling, ...), referenced
        // by id from [[authorization]] records below so a shared document
        // (e.g. one ruling backing two blocks) is declared once.
        const docRecords = Array.isArray(authorityDoc.document) ? authorityDoc.document : [];
        const docsById = new Map();
        for (const doc of docRecords) {
          if (
            typeof doc.id !== "string" ||
            doc.id === "" ||
            typeof doc.path !== "string" ||
            doc.path === "" ||
            typeof doc.sha256 !== "string" ||
            !DIGEST_RE.test(doc.sha256)
          ) {
            v(
              `authority registry ${authorityPath} has a [[document]] with a missing id/path or a malformed sha256: ${JSON.stringify(doc)}`,
            );
            continue;
          }
          if (typeof doc.kind !== "string" || !VALID_DOCUMENT_KINDS.has(doc.kind)) {
            v(
              `authority registry ${authorityPath} document ${doc.id} has kind ${JSON.stringify(doc.kind ?? "")}, not one of CHARTER/AMENDMENT/RULING`,
            );
            continue;
          }
          const resolvedDocPath = resolvePath(process.cwd(), doc.path);
          if (!isPathUnder(KIND_DIR[doc.kind], resolvedDocPath)) {
            v(
              `authority registry ${authorityPath} document ${doc.id} declares kind ${doc.kind} but path ${doc.path} does not resolve under ${KIND_DIR[doc.kind]} — a document's kind must match where it actually lives`,
            );
            continue;
          }
          if (docsById.has(doc.id)) {
            v(
              `authority registry ${authorityPath} declares more than one [[document]] with id ${JSON.stringify(doc.id)}`,
            );
            continue;
          }
          docsById.set(doc.id, doc);
        }
        // Digest binding: authority is bound to exact bytes, not a path —
        // a moved/rewritten document with a stale sha256 is a violation, not
        // a silently-trusted reference. A digest match only proves the
        // recorded bytes are the ones on disk; it does NOT prove those bytes
        // ratify anything, so AMENDMENT/RULING documents are additionally
        // classified by their own **Status:** paragraph (parseStatusParagraph
        // — the SAME parser the enabling_amendment gate above uses, never a
        // second one). AMENDMENT documents follow the documented convention
        // (ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md: "amendments/AMD-*.md
        // — a **Status:** PROSE line; the new gate parses it") strictly: no
        // Status line is unparseable, same as the enabling_amendment gate.
        // RULING documents are not held to that convention (the same
        // inventory names it for charters/amendments only) — a maintainer
        // ruling's own text and placement under rulings/ is the ratification
        // act, so an absent Status line is not a violation; but when a
        // Status line IS present and says DRAFT/NOT RATIFIED (the convention
        // already used by draft architecture-consult rulings, e.g.
        // ARCH-RULING-C1-FOUR-FORKS.md / ARCH-RULING-D1-SIX-FORKS.md), that
        // still fails closed. CHARTER documents are never classified this
        // way: a charter's own Status vocabulary (e.g. "PREPARED") never
        // uses ratified/not-ratified wording — charters rest on the base
        // program authority (see this registry's own header comment).
        for (const doc of docsById.values()) {
          const docPath = resolvePath(process.cwd(), doc.path);
          let bytes;
          try {
            bytes = readFileSync(docPath);
          } catch {
            v(
              `authority registry ${authorityPath} document ${doc.id} cites path ${doc.path} which does not exist on disk — authority is not bound to exact bytes`,
            );
            continue;
          }
          const actual = createHash("sha256").update(bytes).digest("hex");
          if (actual !== doc.sha256) {
            v(
              `authority registry ${authorityPath} document ${doc.id} sha256 ${doc.sha256} does not match the current SHA-256 of ${doc.path} (${actual}) — stale digest`,
            );
          }
          if (doc.kind === "AMENDMENT") {
            const parsed = parseStatusParagraph(bytes.toString("utf8"));
            if (!parsed.present) {
              v(
                `authority registry ${authorityPath} document ${doc.id} (AMENDMENT) ${doc.path} has no **Status:** line — its ratification state cannot be parsed`,
              );
            } else if (!parsed.ratified) {
              v(
                `authority registry ${authorityPath} document ${doc.id} (AMENDMENT) ${doc.path} is not ratified (Status: ${parsed.statusText}) — an unratified amendment grants no authority`,
              );
            }
          } else if (doc.kind === "RULING") {
            const parsed = parseStatusParagraph(bytes.toString("utf8"));
            if (parsed.present && !parsed.ratified) {
              v(
                `authority registry ${authorityPath} document ${doc.id} (RULING) ${doc.path} declares Status: ${parsed.statusText} — not ratified, grants no authority`,
              );
            }
          }
        }

        // [[authorization]] — one row per block that has left LOCKED, citing
        // the document ids that make up its authority closure plus who
        // ratified it, when, and the scope it covers.
        const authById = new Map();
        const authRecords = Array.isArray(authorityDoc.authorization)
          ? authorityDoc.authorization
          : [];
        for (const rec of authRecords) {
          if (typeof rec.block !== "string" || rec.block === "") {
            v(
              `authority registry ${authorityPath} has an [[authorization]] record with no string block id: ${JSON.stringify(rec)}`,
            );
            continue;
          }
          if (authById.has(rec.block)) {
            v(
              `authority registry ${authorityPath} declares more than one [[authorization]] record for block ${JSON.stringify(rec.block)}`,
            );
          }
          authById.set(rec.block, rec);
          const missingMeta = [];
          if (!(typeof rec.ratified_by === "string" && rec.ratified_by.trim() !== ""))
            missingMeta.push("ratified_by");
          if (!(typeof rec.ratified_at === "string" && rec.ratified_at.trim() !== ""))
            missingMeta.push("ratified_at");
          if (!(typeof rec.scope === "string" && rec.scope.trim() !== ""))
            missingMeta.push("scope");
          if (missingMeta.length > 0) {
            v(
              `authority registry ${authorityPath} authorization for block ${rec.block} is missing required field(s): ${missingMeta.join(", ")}`,
            );
          }
          const documents = Array.isArray(rec.documents) ? rec.documents : [];
          if (documents.length === 0) {
            v(
              `authority registry ${authorityPath} authorization for block ${rec.block} names zero authority documents — an authorization must cite at least one digest-bound document`,
            );
          }
          for (const docId of documents) {
            if (typeof docId !== "string" || !docsById.has(docId)) {
              v(
                `authority registry ${authorityPath} authorization for block ${rec.block} references unknown document id ${JSON.stringify(docId)}`,
              );
            }
          }
        }

        // The core rule: a block must not leave LOCKED without a
        // machine-checkable authorization record (BEGUN_STATUSES mirrors the
        // sequencing gate's own definition of "has begun" above).
        for (const [id, b] of stateById) {
          if (typeof b.status !== "string" || !BEGUN_STATUSES.has(b.status)) continue;
          if (!authById.has(id)) {
            v(
              `state block ${id} is ${b.status} — past LOCKED — but authority registry ${authorityPath} has no [[authorization]] record for it: a block must not leave LOCKED without a digest-bound, ratified authorization record`,
            );
          }
        }
      }
    }

    // -- Immutable entry-lock identity (verifyEntryLockIdentity, AMD-013
    // round 5): repository.branch/head_sha/head_tree, cross-checked against
    // entry_checkout_sha/entry_checkout_tree. Runs UNCONDITIONALLY, before
    // trunk resolution, and independently of it — this is the A0 entry
    // binding, never the operational trunk oracle below.
    verifyEntryLockIdentity(state, v);

    // -- Entry-lock RECORD content binding (verifyEntryLockRecordBinding,
    // AMD-013 ratification correction 1): the check above cross-checks
    // repository.branch/head_sha/head_tree/entry_checkout_sha/
    // entry_checkout_tree only against EACH OTHER — a coordinated rewrite of
    // all five stays internally consistent and passes. This additionally
    // binds them to the DAG root's digest-bound entry-lock.toml record,
    // which the same in-memory edit cannot also rewrite. Reuses the
    // resolvedRoots already resolved for the evidence_digest binding above.
    verifyEntryLockRecordBinding(state, dagRoots, stateById, resolvedRoots, v);

    // -- Pinned INTEGRATION-trunk resolution (resolvePinnedTrunk, Finding C,
    // corrected per the second AMD-013 review round, RE-TARGETED at round 5
    // from repository.branch/head_sha — the immutable entry lock — onto the
    // new repository.integration_branch/integration_head_sha pair): runs
    // UNCONDITIONALLY on every live-mode validation, not only when more than
    // one block is concurrently active. The original conditional scoping was
    // justified by comparing against checkout HEAD, which legitimately
    // drifts from trunk in an ordinary worktree; now that the oracle is the
    // CONFIGURED integration ref (repository.integration_branch) rather than
    // checkout HEAD — or the immutable entry-lock branch — there is no
    // remaining reason to skip this on an ordinary single-active-block run.
    const pinnedTrunk = resolvePinnedTrunk(state, process.cwd(), v);

    // -- implementation_ref/implementation_candidate_sha binding (round 4,
    // FIX 2 — see verifyImplementationRefFields above): runs UNCONDITIONALLY
    // over every IN_PROGRESS block, regardless of how many blocks are
    // concurrently active. Its result is reused by
    // verifyConcurrentLandingSafety below rather than re-checked there.
    const implementationRefResults = verifyImplementationRefFields(stateById, v);

    // -- Fixed-landing-order cumulative rehearsal (see the block comment
    // above verifyConcurrentLandingSafety for what this proves and why); it
    // consumes the trunk pin and the implementation_ref results resolved
    // above rather than re-resolving either.
    verifyConcurrentLandingSafety(active, dagById, state, v, pinnedTrunk, implementationRefResults);

    // -- Git identity verification (see the block comment above
    // verifyLiveGitIdentities for what this proves and why). Consumes the
    // same pinned trunk (round 4, FIX 3) rather than sampling checkout HEAD.
    verifyLiveGitIdentities(stateById, v, pinnedTrunk);
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
