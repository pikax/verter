// Discrimination control for the DET-1 oracle (`det1-analysis-diff.mjs`).
//
// Proves the token-aware analysis comparator is a DISCRIMINATING oracle,
// not an assumed one. Seven semantic plants must each surface as
// semantic diffs — three content plants (complex prop-type ref->ref
// rename, typeRegistry member rename, removed slot) and four
// token-REFERENCE rewiring plants (reparent a markup node, flip sibling
// order, permute two node identities, reorder markup roots) that leave
// every position token-shaped and are visible only through the token
// bijection. One negative control (a token consistently replaced by
// another 43-char token) must produce ZERO semantic diffs. Every plant
// is applied to a copy of a REAL corpus analysis artifact and proven
// landed before the comparison runs — a plant that fails to apply is
// reported as SKIPPED and fails the control, never passes it.
//
// Usage:
//   node scripts/benchmark/det1-oracle-control.mjs <path-to>.analysis.json
//
// Exits non-zero when any plant verdict is ORACLE-FAILURE or SKIPPED.
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SRC = process.argv[2];
if (!SRC) {
  console.error("Usage: node det1-oracle-control.mjs <path-to>.analysis.json");
  process.exit(2);
}
const COMPARATOR = resolve(dirname(fileURLToPath(import.meta.url)), "det1-analysis-diff.mjs");
const WORK = join(tmpdir(), `det1-oracle-work-${process.pid}`);

const TOKEN_RE = /^[A-Za-z0-9_-]{43}$/;

function findFirstToken(node) {
  if (typeof node === "string") return TOKEN_RE.test(node) ? node : null;
  if (node && typeof node === "object") {
    for (const k of Object.keys(node)) {
      const t = findFirstToken(node[k]);
      if (t) return t;
    }
  }
  return null;
}

const plants = [
  {
    name: "B'. COMPLEX prop type: named ref renamed to a different ref (kind stays 'ref')",
    expectSemantic: true,
    apply(a) {
      const p = (a.props ?? []).find((x) => x.type && x.type.kind === "ref");
      if (!p) throw new Error("no prop with a kind:'ref' type descriptor");
      const before = p.type.name;
      if (typeof before !== "string" || before.length === 0) {
        throw new Error("ref prop has no name");
      }
      p.type = { ...p.type, name: "VERTER_PLANTED_DIFFERENT_REF" };
      return `props[${p.name}].type.name ${before} -> VERTER_PLANTED_DIFFERENT_REF (kind 'ref' unchanged)`;
    },
  },
  {
    name: "R. typeRegistry member rename",
    expectSemantic: true,
    apply(a) {
      const reg = a.typeRegistry ?? a.type_registry;
      if (!reg || typeof reg !== "object") throw new Error("no typeRegistry");
      const keys = Object.keys(reg);
      if (keys.length === 0) throw new Error("empty typeRegistry");
      const payload = JSON.stringify(reg[keys[0]]);
      const renamed = payload.replace(/"name":"([^"]+)"/, '"name":"VERTER_PLANTED_RENAME"');
      if (renamed === payload) throw new Error("rename did not land");
      reg[keys[0]] = JSON.parse(renamed);
      return `typeRegistry[${keys[0]}] member renamed`;
    },
  },
  {
    name: "S. removed slot",
    expectSemantic: true,
    apply(a) {
      if (!Array.isArray(a.slots) || a.slots.length === 0) throw new Error("no slots");
      const n = a.slots[0].name;
      a.slots.splice(0, 1);
      return `slots[${n}] removed`;
    },
  },
  {
    name: "W1. REWIRE: reparent a markup node (parentNodeToken repointed)",
    expectSemantic: true,
    apply(a) {
      const nodes = a.orderedSfcStructure?.markupNodes;
      if (!Array.isArray(nodes)) throw new Error("no markupNodes");
      const child = nodes.find((n) => typeof n.parentNodeToken === "string");
      if (!child) throw new Error("no parented node");
      const newParent = nodes.find(
        (n) => n.nodeToken !== child.parentNodeToken && n.nodeToken !== child.nodeToken,
      );
      if (!newParent) throw new Error("no alternate parent");
      const before = child.parentNodeToken;
      child.parentNodeToken = newParent.nodeToken;
      return `node ${child.nodeToken.slice(0, 8)}… reparented ${before.slice(0, 8)}… -> ${newParent.nodeToken.slice(0, 8)}…`;
    },
  },
  {
    name: "W2. REWIRE: flip sibling order in childNodeTokens",
    expectSemantic: true,
    apply(a) {
      const nodes = a.orderedSfcStructure?.markupNodes;
      if (!Array.isArray(nodes)) throw new Error("no markupNodes");
      const n = nodes.find(
        (x) => Array.isArray(x.childNodeTokens) && x.childNodeTokens.length >= 2,
      );
      if (!n) throw new Error("no node with >=2 children");
      const [t0, t1] = n.childNodeTokens;
      if (t0 === t1) throw new Error("degenerate sibling pair");
      n.childNodeTokens[0] = t1;
      n.childNodeTokens[1] = t0;
      return `siblings flipped under ${n.nodeToken.slice(0, 8)}…`;
    },
  },
  {
    name: "W3. REWIRE: permute two nodes' nodeToken identities",
    expectSemantic: true,
    apply(a) {
      const nodes = a.orderedSfcStructure?.markupNodes;
      if (!Array.isArray(nodes) || nodes.length < 2) throw new Error("need >=2 markupNodes");
      const [x, y] = [nodes[0], nodes[1]];
      if (x.nodeToken === y.nodeToken) throw new Error("degenerate node pair");
      [x.nodeToken, y.nodeToken] = [y.nodeToken, x.nodeToken];
      return "nodeToken identities of nodes[0] and nodes[1] permuted (references untouched)";
    },
  },
  {
    name: "W4. REWIRE: reorder markup_root_tokens",
    expectSemantic: true,
    apply(a) {
      const block = (a.orderedSfcStructure?.blocks ?? []).find(
        (b) => Array.isArray(b.markup_root_tokens) && b.markup_root_tokens.length >= 2,
      );
      if (!block) throw new Error("no block with >=2 markup_root_tokens");
      const r = block.markup_root_tokens;
      if (r[0] === r[1]) throw new Error("degenerate root pair");
      [r[0], r[1]] = [r[1], r[0]];
      return "markup_root_tokens[0] and [1] swapped";
    },
  },
  {
    name: "N. NEGATIVE control: one token replaced by another 43-char token",
    expectSemantic: false,
    apply(a) {
      const tok = findFirstToken(a);
      if (!tok) throw new Error("no token found");
      const replacement = "VERTERPLANT" + "x".repeat(43 - 11);
      if (!TOKEN_RE.test(replacement)) throw new Error("bad replacement token");
      const s = JSON.stringify(a).split(tok).join(replacement);
      const reparsed = JSON.parse(s);
      for (const k of Object.keys(a)) delete a[k];
      Object.assign(a, reparsed);
      return `token ${tok} -> ${replacement} (all occurrences)`;
    },
  },
];

function runComparator(dirA, dirB) {
  // The comparator exits 1 on a FAIL verdict by contract; capture stdout
  // either way and parse the JSON report.
  try {
    const stdout = execFileSync("node", [COMPARATOR, dirA, dirB, "--expect-files=1"], {
      encoding: "utf8",
    });
    return { report: JSON.parse(stdout), exit: 0 };
  } catch (e) {
    if (e.stdout === undefined) throw e;
    return { report: JSON.parse(e.stdout), exit: e.status ?? 1 };
  }
}

const pristineRaw = readFileSync(SRC, "utf8");
let failures = 0;
for (const plant of plants) {
  rmSync(WORK, { recursive: true, force: true });
  mkdirSync(join(WORK, "a", "analysis"), { recursive: true });
  mkdirSync(join(WORK, "b", "analysis"), { recursive: true });
  writeFileSync(join(WORK, "a", "analysis", "subject.json"), pristineRaw);
  const mutated = JSON.parse(pristineRaw);
  let desc;
  try {
    desc = plant.apply(mutated);
  } catch (e) {
    console.log(`### ${plant.name}: SKIPPED — ${e.message}`);
    failures++;
    continue;
  }
  const mutatedRaw = JSON.stringify(mutated);
  const landed = mutatedRaw !== JSON.stringify(JSON.parse(pristineRaw));
  if (!landed) {
    console.log(`### ${plant.name}: PLANT FAILED TO LAND`);
    failures++;
    continue;
  }
  writeFileSync(join(WORK, "b", "analysis", "subject.json"), mutatedRaw);
  const { report, exit } = runComparator(join(WORK, "a"), join(WORK, "b"));
  // Expectation covers BOTH the report and the exit-code contract: a
  // semantic plant must FAIL the gate (exit non-zero), the negative
  // control must PASS it (exit 0).
  const ok =
    plant.expectSemantic === report.semantic_diffs > 0 &&
    (plant.expectSemantic ? exit !== 0 : exit === 0);
  if (!ok) failures++;
  console.log(`### ${plant.name}`);
  console.log(`  mutation: ${desc}; landed=YES`);
  console.log(
    `  semantic_diffs=${report.semantic_diffs} token_diffs=${report.token_diffs} exit=${exit} -> ${ok ? "AS-EXPECTED" : "ORACLE-FAILURE"}`,
  );
  if (report.semantic_examples.length > 0) {
    console.log(`  first: ${JSON.stringify(report.semantic_examples[0]).slice(0, 200)}`);
  }
}
rmSync(WORK, { recursive: true, force: true });
console.log(`\n=== ${failures === 0 ? "ORACLE DISCRIMINATES" : `${failures} FAILURES`} ===`);
process.exit(failures === 0 ? 0 : 1);
