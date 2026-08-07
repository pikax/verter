// DET-1 discriminating oracle: token-aware deep compare of the raw
// analysis/**.json artifacts between two same-config corpus runs.
//
// String values matching /^[A-Za-z0-9_-]{43}$/ are per-process opaque
// identities (audit tokens). Tokens are CORRELATED, not ignored: the
// lockstep walk builds a per-file A<->B token bijection — recorded for
// every token pair, INCLUDING identical ones, before any equality
// short-circuit — and any functionality violation (one A-token mapped
// to two different B-tokens) or injectivity violation (two A-tokens
// mapped to one B-token) is a SEMANTIC diff. This is what makes
// token-reference rewiring (reparenting, sibling reordering, identity
// permutation) visible even though every individual position still
// holds a token-shaped value. A consistent whole-file re-mint maps
// old->new uniformly and stays a token diff.
//
// Exit-code contract (the Block 6 DET-1 gate verdict):
//   0 — compared == --expect-files, zero missing, zero semantic diffs
//       (token-bijection violations count as semantic diffs)
//   1 — any semantic diff, any missing file, or compared != expected
//   2 — usage error, or the two run directories are not DISTINCT trees
//
// `compared == --expect-files` is a COUNT check, and a count is not a
// distinctness check. Two defences make the count mean what the gate
// reads it as:
//   * the two run directories (and their analysis/ subtrees) are
//     rejected when they realpath to the same location — a
//     self-comparison is trivially identical and would exit 0 with
//     `token_diffs: 0`, proving nothing;
//   * walk() dedupes by realpath, so a symlinked or hard-linked
//     duplicate subdirectory cannot pad the file list up to the
//     expected count (90 real components plus one symlinked duplicate
//     subdir otherwise reports `compared: 180`).
import { readFileSync, readdirSync, realpathSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const TOKEN_RE = /^[A-Za-z0-9_-]{43}$/;

function usage(message) {
  console.error(`ERROR: ${message}`);
  console.error(
    "Usage: node det1-analysis-diff.mjs <run-dir-A> <run-dir-B> --expect-files=N\n" +
      "  <run-dir-X> must contain an analysis/ subdirectory of *.json artifacts.\n" +
      "  --expect-files=N is REQUIRED: the per-component artifact count each side\n" +
      "  must contribute; a comparison over fewer files exits non-zero.",
  );
  process.exit(2);
}

let dirA;
let dirB;
let expectFiles;
for (const arg of process.argv.slice(2)) {
  if (arg.startsWith("--expect-files=")) {
    expectFiles = Number.parseInt(arg.slice("--expect-files=".length), 10);
  } else if (dirA === undefined) {
    dirA = arg;
  } else if (dirB === undefined) {
    dirB = arg;
  } else {
    usage(`unexpected argument: ${arg}`);
  }
}
if (dirA === undefined || dirB === undefined) usage("two run directories are required");
if (!Number.isInteger(expectFiles) || expectFiles <= 0) {
  usage("--expect-files=N is required and must be a positive integer");
}
for (const d of [dirA, dirB]) {
  try {
    if (!statSync(join(d, "analysis")).isDirectory()) usage(`${d}/analysis is not a directory`);
  } catch {
    usage(`${d}/analysis does not exist`);
  }
}

// DISTINCTNESS. Comparing a tree against itself satisfies every
// assertion this tool makes — zero semantic diffs, zero token diffs,
// zero missing, and `compared == expected` — while proving nothing
// about determinism. Reject it at both levels: the run directories
// themselves, and the analysis/ subtrees actually compared (two
// distinct run directories can still symlink one shared analysis tree).
const realA = realpathSync(dirA);
const realB = realpathSync(dirB);
if (realA === realB) {
  usage(`the two run directories are the same tree (${realA}); a self-comparison proves nothing`);
}
const analysisRealA = realpathSync(join(dirA, "analysis"));
const analysisRealB = realpathSync(join(dirB, "analysis"));
if (analysisRealA === analysisRealB) {
  usage(
    `both run directories resolve to the same analysis tree (${analysisRealA}); ` +
      "a self-comparison proves nothing",
  );
}

// Realpath-deduped walk. Without the dedupe a symlinked (or hard-linked)
// duplicate subdirectory inflates the file list, and a padded list can
// reach `--expect-files` while covering only a fraction of the real
// components — the count gate would pass on a truncated comparison.
function walk(d, seen = new Set()) {
  const out = [];
  for (const e of readdirSync(d)) {
    const p = join(d, e);
    let real;
    try {
      real = realpathSync(p);
    } catch {
      // Broken link or a race with the producer: not a comparable
      // artifact. Skipping keeps it out of `compared`, so the count
      // gate still fails rather than silently comparing fewer files.
      continue;
    }
    if (seen.has(real)) continue;
    seen.add(real);
    if (statSync(p).isDirectory()) out.push(...walk(p, seen));
    else if (p.endsWith(".json")) out.push(p);
  }
  return out;
}

const tokenFieldCounts = new Map();
let tokenDiffs = 0;
const semanticDiffs = [];

function fieldOf(path) {
  const segs = path.split("/").filter((s) => s && !/^\d+$/.test(s));
  return segs[segs.length - 1] ?? "<root>";
}

// Per-file bijection state, reset in compareFile().
let forwardMap; // A-token -> { b, path }
let reverseMap; // B-token -> { a, path }

function recordTokenPair(a, b, file, path) {
  const fwd = forwardMap.get(a);
  if (fwd === undefined) {
    forwardMap.set(a, { b, path });
  } else if (fwd.b !== b) {
    semanticDiffs.push({
      file,
      path,
      kind: "token-bijection-functionality",
      a,
      b,
      previously: `${a} -> ${fwd.b} at ${fwd.path}`,
    });
  }
  const rev = reverseMap.get(b);
  if (rev === undefined) {
    reverseMap.set(b, { a, path });
  } else if (rev.a !== a) {
    semanticDiffs.push({
      file,
      path,
      kind: "token-bijection-injectivity",
      a,
      b,
      previously: `${rev.a} -> ${b} at ${rev.path}`,
    });
  }
}

function diff(a, b, file, path) {
  // Token correlation runs FIRST — before the equality short-circuit —
  // so identical token pairs still pin the bijection and a rewired
  // reference elsewhere becomes a recorded violation.
  if (typeof a === "string" && typeof b === "string" && TOKEN_RE.test(a) && TOKEN_RE.test(b)) {
    recordTokenPair(a, b, file, path);
    if (a !== b) {
      tokenDiffs++;
      const f = fieldOf(path);
      tokenFieldCounts.set(f, (tokenFieldCounts.get(f) ?? 0) + 1);
    }
    return;
  }
  if (a === b) return;
  if (
    a !== null &&
    b !== null &&
    typeof a === "object" &&
    typeof b === "object" &&
    Array.isArray(a) === Array.isArray(b)
  ) {
    if (Array.isArray(a) && a.length !== b.length) {
      semanticDiffs.push({ file, path, kind: "array-length", a: a.length, b: b.length });
      return;
    }
    const keys = [...new Set([...Object.keys(a), ...Object.keys(b)])].sort();
    for (const k of keys) diff(a[k], b[k], file, `${path}/${k}`);
    return;
  }
  semanticDiffs.push({
    file,
    path,
    a: JSON.stringify(a)?.slice(0, 120),
    b: JSON.stringify(b)?.slice(0, 120),
  });
}

function compareFile(f) {
  forwardMap = new Map();
  reverseMap = new Map();
  diff(
    JSON.parse(readFileSync(join(dirA, "analysis", f), "utf8")),
    JSON.parse(readFileSync(join(dirB, "analysis", f), "utf8")),
    f,
    "",
  );
}

const filesA = walk(join(dirA, "analysis"))
  .map((p) => relative(join(dirA, "analysis"), p))
  .sort();
const filesB = walk(join(dirB, "analysis"))
  .map((p) => relative(join(dirB, "analysis"), p))
  .sort();
const missing = [
  ...filesA.filter((f) => !filesB.includes(f)),
  ...filesB.filter((f) => !filesA.includes(f)),
];
let compared = 0;
for (const f of filesA.filter((f) => filesB.includes(f))) {
  compared++;
  compareFile(f);
}

const pass = semanticDiffs.length === 0 && missing.length === 0 && compared === expectFiles;
console.log(
  JSON.stringify(
    {
      pair: [dirA, dirB],
      expected_files: expectFiles,
      compared,
      missing,
      token_diffs: tokenDiffs,
      token_fields: Object.fromEntries([...tokenFieldCounts].sort((x, y) => y[1] - x[1])),
      semantic_diffs: semanticDiffs.length,
      semantic_examples: semanticDiffs.slice(0, 10),
      verdict: pass ? "PASS" : "FAIL",
    },
    null,
    2,
  ),
);
process.exit(pass ? 0 : 1);
