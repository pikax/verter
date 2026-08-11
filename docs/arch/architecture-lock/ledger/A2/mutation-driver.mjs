#!/usr/bin/env node
// Bounded comparator mutation campaign binding the FINAL candidate blob.
//
// One representative predicate per comparator. For each: prove the plant is
// present-once-and-new before applying, apply, run the U6 filterset, require the
// named control to fail, byte-restore, and prove restoration by blob identity.
// A run that cannot prove >= 40 executed tests is a hard failure, not a pass.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";

const WT = "<REPO>-wt-a2";
const SRC = resolve(WT, "crates/verter_session/src");
const OUT = resolve("<EVIDENCE>/A2/command-proofs/50-final-blob-mutation-matrix");
mkdirSync(OUT, { recursive: true });

const git = (...a) => execFileSync("git", a, { cwd: WT, encoding: "utf8" }).trim();
const headBlob = (rel) => git("rev-parse", `HEAD:crates/verter_session/src/${rel}`);

// (id, file, exact unique needle, replacement, control that must fail)
const MUTATIONS = [
  [
    "M-LIT",
    "u6_flow_expect_tests.rs",
    "(Lit::Str(e), LiteralValue::String(g)) => *e == g.as_str(),",
    "(Lit::Str(_e), LiteralValue::String(_g)) => true,",
    "literal_expectation_rejects_a_different_value",
  ],
  [
    "M-SIGKIND-NODE",
    "u6_flow_expect_tests.rs",
    "            *kind == SignatureKind::Call\n                && got_params.len() == params.len()",
    "            matches!(kind, SignatureKind::Call | SignatureKind::Construct)\n                && got_params.len() == params.len()",
    "construct_signature_is_distinct_from_call_signature",
  ],
  [
    "M-SIGKIND-CHECKER",
    "u6_flow_expect_tests.rs",
    "                *kind == SignatureKind::Call\n                    && got_params.len() == params.len()",
    "                matches!(kind, SignatureKind::Call | SignatureKind::Construct)\n                    && got_params.len() == params.len()",
    "construct_signature_is_distinct_from_call_signature",
  ],
];

const results = [];
let hardFailure = false;

for (const [id, rel, needle, repl, control] of MUTATIONS) {
  const file = resolve(SRC, rel);
  const original = readFileSync(file);
  const text = original.toString("utf8");
  const before = git("rev-parse", `HEAD:crates/verter_session/src/${rel}`);

  const occurrences = text.split(needle).length - 1;
  const replAlreadyPresent = text.includes(repl);
  if (occurrences !== 1 || replAlreadyPresent) {
    results.push({ id, verdict: "PLANT-NOT-APPLICABLE", occurrences, replAlreadyPresent });
    hardFailure = true;
    continue;
  }

  writeFileSync(file, text.replace(needle, repl));
  const planted = readFileSync(file).toString("utf8");
  const plantProven = planted.split(repl).length - 1 === 1 && !planted.includes(needle);

  let raw = "";
  try {
    raw = execFileSync("cargo", ["nextest", "run", "-p", "verter_session", "-E", "test(u6_flow)", "--no-fail-fast"],
      { cwd: WT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  } catch (e) { raw = `${e.stdout || ""}${e.stderr || ""}`; }

  writeFileSync(resolve(OUT, `${id}.log`),
    `BLOB_BEFORE=${before}\nPLANT_PROVEN=${plantProven}\nNEEDLE=${needle}\nREPL=${repl}\nCONTROL=${control}\n\n${raw}`);

  writeFileSync(file, original);
  const after = git("rev-parse", `HEAD:crates/verter_session/src/${rel}`);
  const restored = readFileSync(file).equals(original);
  const clean = git("status", "--porcelain") === "";

  const summary = (raw.match(/Summary \[[^\]]*\].*/m) || [""])[0].trim();
  const ran = Number((summary.match(/(\d+) tests? run/) || [0, 0])[1]);
  const failed = [...raw.matchAll(/FAIL \[[^\]]*\]\s+(?:\([^)]*\)\s+)?\S+\s+(\S+)/g)].map((m) => m[1]);
  const namedFailed = failed.some((n) => n.includes(control));

  let verdict = "CAUGHT-BY-NAMED-CONTROL";
  if (!plantProven) verdict = "PLANT-DID-NOT-APPLY";
  else if (ran < 40) verdict = "RUN-ERROR-INSUFFICIENT-WORK";
  else if (failed.length === 0) verdict = "SURVIVED";
  else if (!namedFailed) verdict = "MISSED-NAMED-CONTROL";
  if (!restored || !clean || before !== after) verdict = "RESTORE-FAILED";
  if (verdict !== "CAUGHT-BY-NAMED-CONTROL") hardFailure = true;

  results.push({ id, file: rel, blobBefore: before, blobAfter: after, plantProven, control, ran, failed, summary, restored, clean, verdict });
  console.log(`${verdict} ${id} control=${control} ran=${ran} failed=${JSON.stringify(failed)}`);
}

const payload = { candidate: git("rev-parse", "HEAD"), tree: git("rev-parse", "HEAD^{tree}"),
  boundBlobs: { "u6_flow_expect_tests.rs": headBlob("u6_flow_expect_tests.rs") },
  mutations: MUTATIONS.length, hardFailure, results };
writeFileSync(resolve(OUT, "results.json"), JSON.stringify(payload, null, 2));
console.log(`DONE hardFailure=${hardFailure} boundBlob=${payload.boundBlobs["u6_flow_expect_tests.rs"]}`);
process.exit(hardFailure ? 1 : 0);
