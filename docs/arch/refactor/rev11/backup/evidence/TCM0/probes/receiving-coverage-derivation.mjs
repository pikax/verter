// Derives, from the receiving charters themselves, how much of each relocated obligation is ALREADY
// covered by an existing numbered exit criterion — and how much is not.
//
// It exists because three drafts of the relocation instrument each got this wrong in a new way by
// ASSERTING coverage instead of reading it: one claimed a criterion bound a whole obligation when its
// first sentence scopes it to part; one claimed a receiving block bound none of an obligation when it
// binds the verifying half; and one proposed adding a criterion the receiving charter ALREADY HAS.
// The last is the decisive one: a table written by hand cannot notice a criterion the writer did not
// happen to read, and no amount of care fixes that. This script reads every numbered criterion in
// every receiving charter, so the table becomes a build output.
//
//   node receiving-coverage-derivation.mjs           # print the derivation (never writes)
//   node receiving-coverage-derivation.mjs --write   # print AND update the committed file
//   node receiving-coverage-derivation.mjs --check   # re-derive and compare to the committed file,
//                                                    # exit 1 on any drift
//
// LIMITS, stated rather than implied. Matching is textual: a part is "covered" when a criterion
// contains every one of its discriminating literals. A criterion that covers a part in different
// words is a MISS, and a criterion that merely mentions the literals without binding them is a false
// HIT — so every reported hit carries the criterion's own first line, and a reader is expected to
// read it rather than trust the verdict. This narrows the failure from "did anyone look?" to "is this
// specific quoted sentence the right one?", which is the reduction that matters; it is not a proof.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const CHARTERS = join(HERE, "..", "..", "..", "charters");
const OUT = join(HERE, "..", "receiving-coverage.md");

/** Every numbered exit criterion in a charter, with its number and full body. */
function criteria(charter) {
  const text = readFileSync(join(CHARTERS, `${charter}.md`), "utf8");
  const m = /^## \d+[a-z]?\. Numbered exit criteria\s*$/m.exec(text);
  if (!m)
    throw new Error(`${charter}: no numbered-exit-criteria heading — the charter's shape changed`);
  const from = m.index + m[0].length;
  const rest = text.slice(from);
  const end = /^## /m.exec(rest);
  const body = end ? rest.slice(0, end.index) : rest;
  const out = [];
  const re = /^(\d+)\. (.*)$/gm;
  let hit;
  const marks = [];
  while ((hit = re.exec(body)) !== null)
    marks.push({ n: Number(hit[1]), at: hit.index, head: hit[2] });
  marks.forEach((mk, i) => {
    const stop = i + 1 < marks.length ? marks[i + 1].at : body.length;
    out.push({ charter, n: mk.n, head: mk.head.trim(), body: body.slice(mk.at, stop) });
  });
  if (out.length === 0)
    throw new Error(
      `${charter}: derived ZERO criteria — the derivation is broken, not the charter`,
    );
  return out;
}

// Each part carries the literals that identify a criterion binding it. Chosen to be phrases a
// criterion binding that part must contain, not words that merely co-occur with the topic.
const PARTS = [
  {
    id: "A1",
    obligation: "A — string-encoded surface",
    part: "the DIRECT CodeTransform producer chain",
    search: ["TCM1"],
    must: ["source_projection_map()", "COMPILER-ENFORCED DELETION"],
  },
  {
    id: "A2",
    obligation: "A — string-encoded surface",
    part: "the FFI/NAPI/WASM wire boundary",
    search: ["TCM1"],
    must: ["verter_protocol", "typed shape"],
  },
  // A part can be EXCLUDED rather than covered. Modelling that explicitly is not a nicety: the first
  // run of this script reported A3 as COVERED by two criteria, because both name the field — while
  // both are naming it to put it OUT of scope. A hit on a literal is not a hit on an obligation, and
  // a derivation that cannot say "excluded" will say "covered" instead.
  {
    id: "A3",
    obligation: "A — string-encoded surface",
    part: "externally-supplied INBOUND map fields",
    search: ["TCM1"],
    must: ["FfiBlockOverrideEntry.source_map"],
    excludedBy: ["out-of-scope", "no `CodeTransform` producer relationship"],
  },
  {
    id: "A4",
    obligation: "A — string-encoded surface",
    part: "the `pub use oxc_sourcemap;` re-export",
    search: ["TCM1"],
    // Was `pub use oxc_sourcemap` — the SPELLING of the re-export rather than the obligation over it.
    // The criterion that binds this part names it as a disposition of the re-export and never quotes
    // the `pub use` line, so the old literal missed a criterion that plainly covers it. The reword
    // case this contract discloses is real; this instance of it is closed by matching the obligation.
    must: ["oxc_sourcemap", "disposition"],
  },
  {
    id: "B1",
    obligation: "B — deletion items 17-18",
    part: "RECORDING types introduced or orphaned inside the deleted set",
    search: ["TCM1", "TCM2", "TCM3"],
    must: ["orphan", "deleted set"],
  },
  {
    id: "B2",
    obligation: "B — deletion items 17-18",
    part: "item 18's negative check: exactly one codec ships",
    search: ["TCM2"],
    must: ["one codec", "negative test"],
  },
  {
    id: "B3",
    obligation: "B — deletion items 17-18",
    part: "VERIFYING the accumulated list",
    search: ["TCM4"],
    must: ["items 17-18"],
  },
];

const cache = new Map();
const get = (c) => {
  if (!cache.has(c)) cache.set(c, criteria(c));
  return cache.get(c);
};

// A field is not an identity unless read WITH the record it belongs to. Conjoining literals over a
// whole criterion body has the same defect one level down: TCM1's criterion 1 runs to dozens of lines,
// so two literals can both appear in it while belonging to different sentences about different things.
// Matching is therefore PARAGRAPH-scoped — all of a part's literals must co-occur inside one paragraph
// of one criterion, which is the smallest record that still carries a complete statement.
// Paragraphs are whitespace-NORMALISED before matching. Without this, any literal that happens to
// straddle a line wrap is invisible: a landed criterion binding an obligation was reported as
// covering NOTHING purely because "deleted set" was written as "deleted\n      set". A matcher that
// silently under-reports on line-wrapping is worse than one that misses on wording, because the
// wording miss is disclosed and this one looked like a substantive verdict.
const paragraphs = (body) => body.split(/\n\s*\n/).map((par) => par.replace(/\s+/g, " "));
// Sentences, for the exclusion rule only. A paragraph is too coarse for it: a paragraph can require
// one field's migration while calling a DIFFERENT field out of scope, and a paragraph-scoped
// exclusion reads that as excluding the first. The exclusion phrase must sit in the same SENTENCE as
// the thing it excludes.
const sentences = (body) => body.replace(/\s+/g, " ").split(/(?<=[.;])\s+/);

// A paragraph that NEGATES an obligation satisfies its literals exactly as well as one that imposes
// it: "`source_projection_map()` is exempt from COMPILER-ENFORCED DELETION" contains both of A1's
// literals and means the opposite. A candidate paragraph carrying one of these is not a covering one.
const NEGATORS = [
  "exempt",
  "out of scope",
  "out-of-scope",
  "not covered",
  "does not apply",
  "no longer required",
];
// The negation guard is SENTENCE-scoped, and a first attempt at it was paragraph-scoped and wrong.
// A criterion legitimately imposes an obligation on one field in one sentence and notes a DIFFERENT
// field is out of scope in the next; a paragraph-scoped guard reads the second sentence as cancelling
// the first and reports a false MISS. Negation, like exclusion, is a property of the sentence that
// carries the literals — not of the neighbourhood they sit in.
const matches = (crit, lits, { allowNegated = false } = {}) =>
  paragraphs(crit.body).some((par) => {
    if (!lits.every((lit) => par.includes(lit))) return false;
    if (allowNegated) return true;
    const bearing = sentences(par).filter((sen) => lits.every((lit) => sen.includes(lit)));
    // Literals spread across sentences within the paragraph: no single sentence negates them, so the
    // paragraph stands as a hit. Split-literal matching is disclosed as a residue rather than guessed.
    if (bearing.length === 0) return true;
    return bearing.some((sen) => !NEGATORS.some((neg) => sen.includes(neg)));
  });
// Exclusion is the one place a negator is REQUIRED rather than disqualifying, and it must co-occur
// with the part's own literals inside one sentence.
const excludes = (crit, lits, marker) =>
  sentences(crit.body).some(
    (sen) => lits.every((lit) => sen.includes(lit)) && sen.includes(marker),
  );

const rows = PARTS.map((p) => {
  const hits = [];
  for (const c of p.search) {
    for (const crit of get(c)) {
      // Collected allowing negated paragraphs, because the exclusion rule below must be able to SEE a
      // negating criterion. Whether a hit COVERS is decided separately, after.
      if (matches(crit, p.must, { allowNegated: true })) hits.push(crit);
    }
  }
  // An excluded part is one whose hits all say, in the charter's own words, that it is out of scope.
  const excluded =
    p.excludedBy !== undefined &&
    hits.length > 0 &&
    // EVERY hit must exclude, not merely one of them. With `some`, a part covered by one criterion
    // and excluded by another would report EXCLUDED — the covering criterion silently outvoted by
    // the excluding one. The documented rule and the implemented rule must be the same rule.
    hits.every((h) => p.excludedBy.some((lit) => excludes(h, p.must, lit)));
  // A COVERING hit is one whose paragraph does not negate. An excluded part keeps its negated hits,
  // because those are precisely the evidence of its exclusion.
  const covering = hits.filter((h) => matches(h, p.must));
  return { ...p, hits, covering, excluded };
});

const lines = [];
lines.push("# Receiving-owner coverage — derived, not asserted");
lines.push("");
lines.push(
  "Generated by `probes/receiving-coverage-derivation.mjs`. **Do not edit by hand**; re-run it.",
);
lines.push(
  "`node probes/receiving-coverage-derivation.mjs --check` re-derives and exits 1 on drift.",
);
lines.push("");
lines.push(
  "Every HIT quotes the criterion's own opening line. Read it: the script proves somebody looked",
);
lines.push("at every criterion, not that the quoted one binds the part.");
lines.push("");
lines.push("| id | obligation | part | covered by | criterion's own words |");
lines.push("|---|---|---|---|---|");
for (const r of rows) {
  const shown = r.excluded ? r.hits : r.covering;
  const where = r.excluded
    ? `**EXCLUDED** by ${shown.map((h) => `${h.charter} criterion ${h.n}`).join("; ")}`
    : shown.length
      ? shown.map((h) => `${h.charter} criterion ${h.n}`).join("; ")
      : "**NOTHING**";
  const words = shown.length
    ? shown.map((h) => h.head.replace(/\|/g, "\\|").slice(0, 120)).join(" — ")
    : "—";
  lines.push(`| ${r.id} | ${r.obligation} | ${r.part} | ${where} | ${words} |`);
}
lines.push("");
const counts = Object.fromEntries(["TCM1", "TCM2", "TCM3", "TCM4"].map((c) => [c, get(c).length]));
lines.push(
  `Criteria read: ${Object.entries(counts)
    .map(([k, v]) => `${k}=${v}`)
    .join(", ")}.`,
);
lines.push(
  `Parts: ${rows.filter((r) => r.covering.length && !r.excluded).length} covered, ${rows.filter((r) => r.excluded).length} excluded by the receiving charter, ${rows.filter((r) => !r.covering.length && !r.excluded).length} uncovered, of ${rows.length}.`,
);
lines.push("");
lines.push("**Uncovered parts, which are what a binding act must address:**");
const uncovered = rows.filter((r) => !r.covering.length && !r.excluded);
if (uncovered.length === 0) lines.push("");
for (const r of uncovered) lines.push(`- \`${r.id}\` — ${r.part} (${r.obligation})`);
lines.push("");
const rendered = lines.join("\n") + "\n";

if (process.argv.includes("--check")) {
  if (!existsSync(OUT)) {
    console.error("no committed derivation to check against");
    process.exit(1);
  }
  const committed = readFileSync(OUT, "utf8");
  if (committed !== rendered) {
    console.error(
      "DRIFT — the committed derivation no longer matches the charters. Re-run without --check.",
    );
    process.exit(1);
  }
  console.log(
    `ok — ${rows.length} parts, ${Object.values(counts).reduce((a, b) => a + b, 0)} criteria read, derivation matches the committed file`,
  );
  process.exit(0);
}
// Printing must not require write access: the plain run is advertised as a print, and a reader on a
// read-only checkout was getting EPERM from it. Writing is now explicit.
process.stdout.write(rendered);
if (process.argv.includes("--write")) {
  writeFileSync(OUT, rendered);
  process.stderr.write(`\nwrote ${OUT}\n`);
}
