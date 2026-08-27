// TCM0 closure validator — the admission gate for acceptance.
//
// It exists because the previous continuation surface was a curated list of gap rows. Such a list is
// accurate about everything it contains and SILENT about everything it does not, so completing every
// row in it still left charter obligations open — two of them with no evidence in the tree at all.
//
// So this validator does not read a list of obligations. It DERIVES the universe from
// `charters/TCM0.md` — every sentence of every numbered Scope item, the acyclic invariant, and the
// Acceptance clause — and
// then checks the register against it. A universe that is hand-maintained can omit the very thing it
// should have caught; a derived one cannot, because the charter is the thing being satisfied.
//
// Claim form: an invisible `<!-- CLAIM: distinctive verbatim substring -->` in an obligation row's
// obligation cell. `<!-- COMMENTARY: distinctive verbatim substring -->` explicitly accounts for a
// non-obligation span. HTML comments keep the rendered table unchanged.
//
// Obligation-sentence tiling ignores exactly three kinds of non-obligation material:
//   1. the connective words in IGNORABLE_CONNECTIVES below, because they join obligations rather
//      than name one;
//   2. URL destinations, footnote references, and bracketed act/number labels matched by
//      IGNORABLE_CITATIONS below, because they cite an obligation rather than state one; and
//   3. whitespace plus characters that are not Unicode letters or numbers, because those are
//      Markdown emphasis/code delimiters or sentence punctuation, not words.
// Everything else must be tiled by a CLAIM or COMMENTARY span. There is no score or fuzzy matching.
//
// Run: node closure-validator.mjs        (no candidate package needed; this reads the tree only)
// Exit 0 only when acceptance is admissible. Every refusal names the obligation and why.

import { readFileSync, existsSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const TCM0 = join(HERE, "..");
const REV11 = join(TCM0, "..", "..");
const CHARTER = join(REV11, "charters", "TCM0.md");
const REGISTER = join(TCM0, "closure-register.md");

const problems = [];
const notes = [];
const fail = (msg) => problems.push(msg);

const IGNORABLE_CONNECTIVES = new Set([
  "a",
  "an",
  "and",
  "as",
  "at",
  "but",
  "by",
  "for",
  "from",
  "in",
  "into",
  "nor",
  "of",
  "on",
  "or",
  "so",
  "than",
  "that",
  "the",
  "then",
  "to",
  "when",
  "where",
  "which",
  "while",
  "with",
  "yet",
]);
const IGNORABLE_CITATIONS = [
  /\]\(https?:\/\/[^\s)]+\)/gu,
  /\[\^[^\]\n]+\]/gu,
  /\[(?:[A-Z][A-Z0-9-]*|\d+)\]/gu,
];
const WORD = /[\p{L}\p{N}]+(?:[-'’][\p{L}\p{N}]+)*/gu;

// ---------------------------------------------------------------- derive the universe
const charter = readFileSync(CHARTER, "utf8");
const headingStarts = [...charter.matchAll(/^## (.+)$/gm)];
const charterSections = headingStarts.map((heading, index) => ({
  title: heading[1].trim(),
  body: charter
    .slice(heading.index + heading[0].length, headingStarts[index + 1]?.index ?? charter.length)
    .trim(),
}));
const hasAcceptance = charterSections.some((section) => section.title === "Acceptance");
if (!hasAcceptance) fail("could not locate the charter's Acceptance clause");
function requireSection(title) {
  const matches = charterSections.filter((section) => section.title === title);
  if (matches.length !== 1) {
    fail(
      `could not derive exactly one charter section headed "${title}" — found ${matches.length}`,
    );
  }
  return matches[0]?.body ?? "";
}

const scopeBlock = requireSection("Scope");
const invariantBlock = requireSection("The acyclic invariant this locks");
const acceptanceBlock = requireSection("Acceptance");
if (scopeBlock.length < 100)
  fail("could not locate the charter's Scope section — the charter's shape changed");

// A Scope item is a top-level ordered-list entry: `N. ` at column 0. Its sentence universe is
// segmented from the charter body at runtime; register prose never supplies or supplements it.
const scopeStarts = [...scopeBlock.matchAll(/^(\d+)\. /gm)];
const sentenceSegmenter = new Intl.Segmenter("en", { granularity: "sentence" });
const deriveSentences = (body) =>
  [...sentenceSegmenter.segment(body.replace(/\s+/g, " ").trim())]
    .map(({ segment }) => segment.trim())
    .filter(Boolean);
const scopeItems = scopeStarts.map((match, index) => {
  const bodyStart = match.index + match[0].length;
  const bodyEnd = scopeStarts[index + 1]?.index ?? scopeBlock.length;
  const body = scopeBlock.slice(bodyStart, bodyEnd);
  return {
    number: Number(match[1]),
    sentences: deriveSentences(body),
  };
});
const scopeNumbers = scopeItems.map(({ number }) => number);
if (scopeNumbers.length === 0)
  fail("derived ZERO Scope items from the charter — the derivation is broken, not the register");
// A derivation that silently drops an item is the failure mode this whole file exists to prevent, so
// check the sequence is a gapless 1..N rather than trusting the count.
scopeNumbers.forEach((n, i) => {
  if (n !== i + 1)
    fail(
      `Scope items derived from the charter are not a gapless 1..N sequence: got ${scopeNumbers.join(",")}`,
    );
});
for (const item of scopeItems) {
  if (item.sentences.length === 0)
    fail(`derived ZERO sentences from Scope item ${item.number} — sentence derivation is broken`);
}
const invariantSentences = deriveSentences(invariantBlock);
const acceptanceSentences = deriveSentences(acceptanceBlock);
if (invariantSentences.length === 0)
  fail("derived ZERO sentences from the acyclic-invariant section — sentence derivation is broken");
if (acceptanceSentences.length === 0)
  fail("derived ZERO sentences from the Acceptance clause — sentence derivation is broken");
const obligationUnits = [
  ...scopeItems.map((item) => ({
    key: `S${item.number}`,
    label: `Scope item ${item.number}`,
    rowPrefix: `S${item.number}.`,
    sentences: item.sentences,
  })),
  {
    key: "INV",
    label: "Acyclic invariant",
    rowPrefix: "INV.",
    sentences: invariantSentences,
  },
  { key: "A", label: "Acceptance", rowPrefix: "A.", sentences: acceptanceSentences },
];

// ---------------------------------------------------------------- read the register
const register = readFileSync(REGISTER, "utf8");
const ROW = /^\|\s*(S\d+\.[a-z]|INV\.[a-z]|A\.[a-z]|X\.[a-z])\s*\|([^|]*)\|\s*([A-Z-]+)/gm;
const rows = [...register.matchAll(ROW)].map((m) => ({
  id: m[1].trim(),
  obligation: m[2].replace(/<!--\s*(?:CLAIM|COMMENTARY|REMAINDER|OWNER):[\s\S]*?-->/g, "").trim(),
  status: m[3].trim(),
  claims: [...m[2].matchAll(/<!--\s*(CLAIM|COMMENTARY|REMAINDER|OWNER):\s*([\s\S]*?)\s*-->/g)].map(
    (claim) => ({
      kind: claim[1],
      text: claim[2].trim(),
    }),
  ),
}));
if (rows.length === 0) fail("the register contains no parsable rows — the row format changed");

const ids = new Set(rows.map((r) => r.id));
if (ids.size !== rows.length) fail("the register contains duplicate ids");

// ---------------------------------------------------------------- 1. totality over the charter
for (const n of scopeNumbers) {
  const covering = rows.filter((r) => r.id.startsWith(`S${n}.`));
  if (covering.length === 0) {
    fail(`Scope item ${n} has NO register row — the register is not total over the charter`);
  }
}
if (rows.filter((r) => r.id.startsWith("INV.")).length === 0) {
  fail("the acyclic-invariant section has no register rows");
}
const claimedSentences = new Set();
const sentenceCoverage = new Map(
  obligationUnits.flatMap((unit) =>
    unit.sentences.map((sentence, sentenceIndex) => [
      `${unit.key}:${sentenceIndex}`,
      new Uint8Array(sentence.length),
    ]),
  ),
);
for (const row of rows.filter((r) => !r.id.startsWith("X."))) {
  const rowUnit = obligationUnits.find((unit) => row.id.startsWith(unit.rowPrefix));
  if (!rowUnit) {
    fail(`${row.id} does not belong to a derived charter obligation section`);
    continue;
  }
  // Only CLAIM and COMMENTARY tile the charter. REMAINDER and OWNER are row-local facts about a
  // bounded proof, not quotations of charter text, so tiling must not read them as claimed spans.
  const tilingClaims = row.claims.filter((c) => c.kind === "CLAIM" || c.kind === "COMMENTARY");
  if (tilingClaims.length === 0) {
    fail(`${row.id} has NO sentence claim marker`);
    continue;
  }
  for (const claim of tilingClaims) {
    const matches = obligationUnits.flatMap((unit) =>
      unit.sentences
        .map((sentence, sentenceIndex) => ({ unit, sentence, sentenceIndex }))
        .filter(({ sentence }) => sentence.includes(claim.text)),
    );
    if (matches.length === 0) {
      fail(`${row.id} claims an invented obligation absent from the charter: "${claim.text}"`);
      continue;
    }
    if (matches.length > 1) {
      fail(
        `${row.id} claim is not distinctive; it matches ${matches.length} charter obligation sentences: "${claim.text}"`,
      );
      continue;
    }
    const [{ unit, sentenceIndex }] = matches;
    if (unit !== rowUnit) {
      fail(`${row.id} claims a sentence from ${unit.label}: "${claim.text}"`);
      continue;
    }
    const sentenceKey = `${unit.key}:${sentenceIndex}`;
    const sentence = unit.sentences[sentenceIndex];
    const coverage = sentenceCoverage.get(sentenceKey);
    for (
      let offset = sentence.indexOf(claim.text);
      offset !== -1;
      offset = sentence.indexOf(claim.text, offset + 1)
    ) {
      coverage.fill(1, offset, offset + claim.text.length);
    }
    claimedSentences.add(sentenceKey);
  }
}
for (const unit of obligationUnits) {
  unit.sentences.forEach((sentence, sentenceIndex) => {
    const sentenceKey = `${unit.key}:${sentenceIndex}`;
    if (!claimedSentences.has(sentenceKey)) {
      fail(`${unit.label} has an UNACCOUNTED sentence: "${sentence}"`);
      return;
    }

    const ignorableCitationRanges = IGNORABLE_CITATIONS.flatMap((pattern) =>
      [...sentence.matchAll(pattern)].map((match) => [match.index, match.index + match[0].length]),
    );
    const coverage = sentenceCoverage.get(sentenceKey);
    const uncoveredWords = [...sentence.matchAll(WORD)]
      .filter((word) => !IGNORABLE_CONNECTIVES.has(word[0].toLocaleLowerCase("en")))
      .filter(
        (word) =>
          !ignorableCitationRanges.some(
            ([start, end]) => word.index >= start && word.index + word[0].length <= end,
          ),
      )
      .filter((word) =>
        [...word[0].matchAll(/[\p{L}\p{N}]/gu)].some(
          (character) => coverage[word.index + character.index] === 0,
        ),
      )
      .map((word) => word[0]);
    if (uncoveredWords.length > 0) {
      fail(
        `${unit.label} sentence ${sentenceIndex + 1} is UNTILED; uncovered text: "${uncoveredWords.join(" ")}"; sentence: "${sentence}"`,
      );
    }
  });
}
if (hasAcceptance && rows.filter((r) => r.id.startsWith("A.")).length === 0) {
  fail("the Acceptance clause has no register rows");
}

// ---------------------------------------------------------------- 2. status vocabulary is closed
const ADMITS = new Set(["PROVEN", "PROVEN-BOUNDED", "RULED", "NOT-OWNED"]);
const REFUSES = new Set(["OPEN", "PROPOSAL", "WITHDRAWN"]);
for (const r of rows) {
  if (!ADMITS.has(r.status) && !REFUSES.has(r.status)) {
    fail(
      `${r.id} carries status "${r.status}", which is outside the closed vocabulary — an informal status is how a row avoids being counted`,
    );
  }
}

// ------------------------------------------------- 2b. PROVEN-BOUNDED is a CHECKED status
// PROVEN-BOUNDED exists because the vocabulary could not express an honest bounded result: a row
// proven for what was exercised, with a remainder nobody ran, had only PROVEN available and so
// asserted more than it had. It is admissible where PROVEN is, but it is NOT a proof of the
// remainder — so a row may not take it and then leave the remainder unnamed, which would make it
// a quieter PROVEN. Both halves are REQUIRED and are read structurally, from markers, rather than
// from prose: a remainder the gate cannot read is a remainder the gate cannot hold anyone to.
for (const r of rows.filter((x) => x.status === "PROVEN-BOUNDED")) {
  const remainder = r.claims.find((c) => c.kind === "REMAINDER" && c.text.length > 0);
  const owner = r.claims.find((c) => c.kind === "OWNER" && c.text.length > 0);
  if (!remainder || !owner) {
    const missing = [!remainder && "REMAINDER", !owner && "OWNER"].filter(Boolean).join(" and ");
    fail(
      `${r.id} is PROVEN-BOUNDED but is MALFORMED — it names no ${missing}. The status is proven-for-what-was-exercised with the remainder and its owner both named; without them it is an unmarked PROVEN over work nobody did`,
    );
  }
}
notes.push(
  `${rows.filter((x) => x.status === "PROVEN-BOUNDED").length} bounded rows checked for a named remainder and owner`,
);

// ---------------------------------------------------------------- 3. no mandatory row is open
for (const r of rows) {
  if (r.id.startsWith("X.")) continue; // X rows are the declared non-obligations
  if (REFUSES.has(r.status)) {
    fail(
      `${r.id} is ${r.status} — acceptance is refused while a charter obligation is not closed: ${r.obligation}`,
    );
  }
}

// ---------------------------------------------------------------- 4. a NOT-OWNED row must name an owner
const dag = readFileSync(join(REV11, "program-dag.toml"), "utf8");
const dependents = [
  ...dag.matchAll(/id = "([A-Z0-9]+)"\s*\n(?:.*\n)*?predecessors = \[([^\]]*)\]/g),
]
  .filter((m) => m[2].includes('"TCM0"'))
  .map((m) => m[1]);
if (dependents.length === 0)
  fail("derived ZERO blocks depending on TCM0 from the DAG — the derivation is broken");
notes.push(`blocks depending on TCM0, derived from the DAG: ${dependents.join(", ")}`);

// The check is for a NAMED owner, not for a long sentence. A length heuristic would pass a verbose
// row that names nobody and fail a terse one that names somebody — measuring the wrong thing.
const OWNER_NAMES = new RegExp(
  `\\b(?:${dependents.join("|")}|Non-scope|maintainer|program orchestrator)\\b`,
);
for (const r of rows.filter((x) => x.status === "NOT-OWNED")) {
  const line = register.split("\n").find((l) => l.startsWith(`| ${r.id}`)) ?? "";
  if (!OWNER_NAMES.test(line)) {
    fail(
      `${r.id} is NOT-OWNED but names no real owner — "not ours" without one is how an obligation disappears`,
    );
  }
}

// ---------------------------------------------------------------- 5. named proof artifacts exist
// This check ranged over `probes/*.mjs` only, while the register's PROOF column names markdown
// documents far more often than it names probes. So a row could cite a document that does not exist
// and the gate still reported ADMISSIBLE — the same shape this whole file exists to prevent, one level
// out: a check whose asserted scope (the named proofs) is wider than the scope it examined (the
// executable ones). It is now scoped to the PROOF CELL of every row, which is exactly the surface the
// claim "the register names X as proof" ranges over, and it accepts any artifact kind.
const proofCells = register
  .split("\n")
  .filter((line) => /^\|\s*(S\d+\.[a-z]|INV\.[a-z]|A\.[a-z]|X\.[a-z])\s*\|/.test(line))
  .map((line) => line.split("|")[4] ?? "");
const artifacts = proofCells.flatMap((cell) =>
  [...cell.matchAll(/`([A-Za-z0-9._/-]+\.(?:mjs|sh|md))`/g)].map((m) => m[1]),
);
const uniqueArtifacts = [...new Set(artifacts)];
if (uniqueArtifacts.length === 0) fail("the register names no proof artifact at all");
// A proof is resolved against this block's evidence folder first, then the program tree, because rows
// legitimately cite rulings and charters that live above it. Absent from both is absent.
const REPO_ROOT = join(REV11, "..", "..", "..", "..");
for (const a of uniqueArtifacts) {
  const found =
    existsSync(join(TCM0, a)) ||
    existsSync(join(REV11, a)) ||
    existsSync(join(REV11, "rulings", a)) ||
    // A citation carrying an explicit repo-relative path resolves from the repo root. A BARE basename
    // does not, deliberately: a reader cannot open one, so a proof must be cited by a path that works.
    (a.includes("/") && existsSync(join(REPO_ROOT, a)));
  if (!found) fail(`the register names ${a} as proof, and it does not exist`);
}
notes.push(`${uniqueArtifacts.length} distinct proof artifacts named in proof cells, all present`);

// ---------------------------------------------------------------- 5b. RULED is a CHECKED status
// `RULED` was previously admitted on the strength of the token alone, which makes it the cheapest way
// for an open obligation to look closed: write the word, cite nothing, pass the gate. A status that
// asserts "a ratified act closed this" has to be checked against the act.
const RULINGS = join(REV11, "rulings");
const registryText = readFileSync(
  join(REV11, "..", "..", "architecture-lock", "ledger", "authority-registry.toml"),
  "utf8",
);
for (const r of rows.filter((x) => x.status === "RULED")) {
  const line = register.split("\n").find((l) => l.startsWith(`| ${r.id}`)) ?? "";
  const cited = [...line.matchAll(/`([A-Z][A-Za-z0-9-]*RULING[A-Za-z0-9.-]*\.md)`/g)].map(
    (m) => m[1],
  );
  if (cited.length === 0) {
    fail(
      `${r.id} is RULED but cites no ruling document — the status is then an assertion, not a closure`,
    );
    continue;
  }
  for (const doc of cited) {
    const abs = join(RULINGS, doc);
    if (!existsSync(abs)) {
      fail(`${r.id} cites ${doc}, which does not exist under rulings/`);
      continue;
    }
    // If the registry pins that document, the pin must still match: a ruling whose bytes moved since
    // ratification is not the ruling that was ratified.
    const pinIdx = registryText.indexOf(`rulings/${doc}`);
    if (pinIdx !== -1) {
      const after = registryText.slice(pinIdx, pinIdx + 400);
      const pin = /sha256 = "([0-9a-f]{64})"/.exec(after);
      if (!pin) {
        fail(`${r.id} cites ${doc}, which the registry lists without a digest`);
      } else {
        const actual = createHash("sha256").update(readFileSync(abs)).digest("hex");
        if (actual !== pin[1]) {
          fail(
            `${r.id} cites ${doc}, whose bytes no longer match the digest ratified for it (registry ${pin[1].slice(0, 12)}…, file ${actual.slice(0, 12)}…)`,
          );
        } else {
          notes.push(`${r.id}: ${doc} present and digest-matched against the registry`);
        }
      }
    } else {
      notes.push(
        `${r.id}: ${doc} present; the registry does not pin it, so only existence is checked`,
      );
    }
  }
}

// ---------------------------------------------------------------- 6. no mandatory row owned downstream
// A block that depends on TCM0 cannot close TCM0: it may not start until TCM0 is accepted. Assigning
// a mandatory obligation to one is an unsatisfiable cycle, and it reads as a disposition.
for (const r of rows) {
  if (r.id.startsWith("X.")) continue;
  if (r.status !== "RULED") continue;
  // A RULED transfer to a dependent is legitimate ONLY because a ratified ruling made it; require the
  // citation to be present on the row rather than inferred.
  const line = register.split("\n").find((l) => l.startsWith(`| ${r.id}`));
  if (line && dependents.some((d) => line.includes(d)) && !/RULING|Q\d/.test(line)) {
    fail(
      `${r.id} transfers an obligation to ${dependents.find((d) => line.includes(d))} without citing the ruling that authorised it`,
    );
  }
}

// ---------------------------------------------------------------- 7. one owning store for status
// A mutable fact restated in a non-owning layer is a second normative store, and repairing one copy
// leaves the others asserting the old value. That has happened three times in this evidence set. So
// the register owns obligation status, and no other evidence file may restate it.
//
// LIMIT, stated rather than implied: this is textual. It catches a status token sitting beside a row
// id — the pattern that actually bit — not every possible paraphrase of a status. A document that
// describes a row's state in prose without naming the row evades it.
const EVIDENCE = join(TCM0);
const STATUS_TOKENS = /\b(PROVEN-BOUNDED|PROVEN|RULED|NOT-OWNED|OPEN|PROPOSAL|WITHDRAWN)\b/;
const ROW_ID = /\b(S\d+\.[a-z]|INV\.[a-z]|A\.[a-z]|X\.[a-z])\b/;
// The scan used to be ONE nonrecursive readdir over immediate `.md` files. That reached 25 of the 49
// artifacts while the register advertised refusal "in any evidence file other than this one", so
// `probes/*.mjs` and everything under `reviews/` were never scanned at all. An instrument whose reach
// is narrower than its stated scope makes every verdict it has issued mean less than it says — the
// block's own recurring species, this time inside the thing that certifies the rest. The universe is
// now the one `claim-sweep-universe.md` defines: every artifact under this folder, plus the summary
// one directory up.
function walkEvidence(dir) {
  const out = [];
  for (const d of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, d.name);
    if (d.isDirectory()) out.push(...walkEvidence(p));
    else if (/\.(md|mjs|sh)$/.test(d.name)) out.push(p);
  }
  return out;
}
const evidenceFiles = [
  ...walkEvidence(EVIDENCE).filter((f) => f !== join(EVIDENCE, "closure-register.md")),
  join(EVIDENCE, "..", "TCM0-summary.md"),
].filter((f) => existsSync(f));
for (const f of evidenceFiles) {
  const lines = readFileSync(f, "utf8").split("\n");
  lines.forEach((line, i) => {
    if (!ROW_ID.test(line) || !STATUS_TOKENS.test(line)) return;
    // A reference that names the owner is exactly what the rule asks for, so it is not a restatement.
    if (/closure-register\.md/.test(line)) return;
    fail(
      `${relative(TCM0, f)}:${i + 1} restates an obligation status beside a row id — the register owns status; cite the row instead of repeating its value`,
    );
  });
}
notes.push(`${evidenceFiles.length} evidence artifacts checked for restated status`);

// ------------------------------------------- 7b. the register is checked AGAINST ITSELF
// Excluding the register from the scan above is correct — it OWNS status — but it left the gate
// structurally blind to the one document it governs, so a row could carry one status while the prose
// in its own section asserted another, and nothing looked. That is not a restatement in a non-owning
// layer; it is the owning layer disagreeing with itself, which is worse.
//
// Scoped to the SECTION, deliberately, because the failure is block-level: the row and the sentence
// contradicting it sat six paragraphs apart, and a line-oriented filter cannot see a block-level
// strike. A status token in a section's prose must be one the rows of that same section actually
// carry; narration that quotes a status the section does not hold is exactly the ambiguity this
// catches.
const registerLines = register.split("\n");
const SECTION_TOKEN = /`(PROVEN-BOUNDED|PROVEN|RULED|NOT-OWNED|OPEN|PROPOSAL|WITHDRAWN)`/g;
let secTitle = "(preamble)";
let secRows = [];
let secProse = [];
const auditSection = () => {
  if (secRows.length === 0) return;
  const held = new Set(secRows.map((r) => r.status));
  for (const p of secProse) {
    for (const m of p.text.matchAll(SECTION_TOKEN)) {
      if (!held.has(m[1])) {
        fail(
          `closure-register.md:${p.line} asserts \`${m[1]}\` in the prose of section "${secTitle}", whose rows carry ${[...held].join(", ")} — the register contradicts itself, and the row's status is what the gate acts on`,
        );
      }
    }
  }
};
registerLines.forEach((line, i) => {
  if (/^##\s/.test(line)) {
    auditSection();
    secTitle = line.replace(/^#+\s*/, "").trim();
    secRows = [];
    secProse = [];
    return;
  }
  const m = line.match(
    /^\|\s*(S\d+\.[a-z]|INV\.[a-z]|A\.[a-z]|X\.[a-z])\s*\|([^|]*)\|\s*([A-Z-]+)/,
  );
  if (m) {
    secRows.push({ id: m[1], status: m[3].trim() });
    return;
  }
  if (/^\|/.test(line)) return;
  if (line.trim()) secProse.push({ line: i + 1, text: line });
});
auditSection();
notes.push(`register audited against itself, section by section`);

// ---------------------------------------------------------------- report
console.log(
  `derived ${scopeNumbers.length} Scope items from the charter: ${scopeNumbers.join(", ")}`,
);
console.log(
  `derived Scope sentences: ${scopeItems.map((item) => `S${item.number}=${item.sentences.length}`).join(", ")}`,
);
console.log(
  `derived obligation sections: Scope=${scopeItems.reduce((count, item) => count + item.sentences.length, 0)}, acyclic invariant=${invariantSentences.length}, Acceptance=${acceptanceSentences.length}`,
);
console.log(
  `claiming Scope rows: ${scopeItems
    .map(
      (item) =>
        `S${item.number}=${rows.filter((row) => row.id.startsWith(`S${item.number}.`) && row.claims.length > 0).length}`,
    )
    .join(", ")}`,
);
console.log(`register rows parsed: ${rows.length}`);
for (const n of notes) console.log(`  ${n}`);
if (problems.length === 0) {
  console.log(
    "\nADMISSIBLE — every charter obligation is closed, or explicitly not owned with a reason.",
  );
  process.exit(0);
}
console.log(`\nREFUSED — ${problems.length} obligation(s) block acceptance:`);
for (const p of problems) console.log(`  - ${p}`);
process.exit(1);
