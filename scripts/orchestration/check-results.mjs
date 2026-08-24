#!/usr/bin/env node
/**
 * Structural check on agent results: is there a conclusion, does it belong to the tree that was
 * reviewed, and does it say what it found.
 *
 * Every check here prevents a failure this program actually had. Nothing here judges content.
 *
 * usage: node scripts/orchestration/check-results.mjs <dir> <sha> <name>...
 * exit:  0 every result is structurally sound (with findings or without)
 *        1 one or more are not
 *        2 usage error
 */
import { existsSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const BEGIN = "===VERTER-RECEIPT-BEGIN===";
export const END = "===VERTER-RECEIPT-END===";
export const RESULTS = ["PASS", "FAIL"];

const LANE_RE = /^[ \t]*LANE:[ \t]*(.*?)[ \t]*$/;
const RESULT_RE = /^[ \t]*RESULT:[ \t]*([A-Za-z_-]+)[ \t]*$/;
const REVIEWED_RE = /^[ \t]*REVIEWED:[ \t]*(.*?)[ \t]*$/;
const FINDINGS_RE = /^[ \t]*FINDINGS:[ \t]*(.*?)[ \t]*$/;
const ROW_RE =
  /^[ \t]*FINDING[ \t]+([A-Za-z0-9_.-]+)[ \t]*\|[ \t]*([^|]*?)[ \t]*\|[ \t]*([^|]*?)[ \t]*\|[ \t]*(.*)$/;
// Diagnosis only. The strict row pattern above never relaxes: a permissive one would swallow a
// malformed row silently, which is the opposite of what this is for.
const NEAR_MISS_RE = /^[ \t]*FINDING\b/;
const SEVERITY_RE = /^P([0-3])$/;
const SHA_RE = /^[0-9a-f]{40}$/i;
const RECEIPT_SHA_RE = /^[0-9a-f]{12,40}$/i;
const SAFE_NAME_RE = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

/** An absent, truncated and inverted receipt are different dispatch failures, so each is named. */
export function findRegions(text) {
  const lines = text.split(/\r?\n/);
  const regions = [];
  const problems = [];
  let open = null;
  for (let i = 0; i < lines.length; i++) {
    const t = lines[i].trim();
    if (t === BEGIN) {
      if (open !== null) problems.push("a result began again before the previous one ended");
      open = i;
    } else if (t === END) {
      if (open === null) problems.push("malformed — an END marker precedes its BEGIN");
      else {
        regions.push(lines.slice(open + 1, i));
        open = null;
      }
    }
  }
  if (open !== null) problems.push("began but never ended — truncated, or the agent died mid-write");
  if (regions.length === 0 && problems.length === 0) {
    problems.push("no result — the agent never reached a conclusion");
  }
  return { regions, problems };
}

export function parse(lines) {
  const r = { lane: null, laneLines: [], result: null, results: [], sha: null, shaLines: [], declared: null, declaredLines: [], rows: [], conflictingIds: [], nearMiss: [], duplicates: 0, text: lines.join("\n") };
  const raw = [];
  for (const line of lines) {
    let m;
    if ((m = LANE_RE.exec(line))) {
      r.laneLines.push(m[1]);
      if (r.lane === null) r.lane = m[1];
    } else if ((m = RESULT_RE.exec(line))) {
      r.results.push(m[1].toUpperCase());
      if (r.result === null) r.result = m[1].toUpperCase();
    } else if ((m = REVIEWED_RE.exec(line))) {
      r.shaLines.push(m[1]);
      if (r.sha === null) r.sha = m[1];
    } else if ((m = FINDINGS_RE.exec(line))) {
      r.declaredLines.push(m[1]);
      if (r.declared === null) r.declared = m[1];
    } else if ((m = ROW_RE.exec(line))) {
      raw.push({ id: m[1], severity: m[2].trim(), location: m[3].trim(), summary: m[4].trim() });
    } else if (NEAR_MISS_RE.test(line)) {
      r.nearMiss.push(line.trim());
    }
  }
  // Identical repeats are an artifact, conflicts are the defect. A real CLI extract captures the
  // agent's final turn twice, so rows and whole receipts both arrive doubled.
  const seen = new Set();
  for (const row of raw) {
    const key = `${row.id}|${row.severity}|${row.location}|${row.summary}`;
    if (seen.has(key)) {
      r.duplicates += 1;
      continue;
    }
    seen.add(key);
    if (r.rows.some((x) => x.id === row.id)) r.conflictingIds.push(row.id);
    r.rows.push(row);
  }
  return r;
}

/**
 * An unfilled template block echoed into a log is not a conclusion.
 *
 * Tests specimen SYNTAX only, never closed-set membership: `RESULT: LAND` is a real value that is
 * wrong, and the agent must be told which value is wrong rather than that its result "looks like a
 * template". A check that shares its oracle with a validity check hides the likelier mistake.
 */
export function isTemplate(r) {
  const specimen = (v) => v !== null && (v.includes("{{") || /^</.test(v));
  return (
    specimen(r.lane) ||
    specimen(r.sha) ||
    specimen(r.declared) ||
    (r.lane === null && r.result === null && r.sha === null && r.declared === null)
  );
}

function canonical(r) {
  return JSON.stringify([r.lane, r.result, r.sha, r.declared, r.rows]);
}

export function extract(text) {
  const { regions, problems } = findRegions(text);
  const real = regions.map(parse).filter((r) => !isTemplate(r));
  if (real.length === 0) {
    const why = regions.length > 0 ? ["the only delimited block is an unfilled template, not a conclusion"] : [];
    return { receipt: null, problems: [...why, ...problems], notes: [] };
  }
  if (new Set(real.map(canonical)).size > 1) {
    return { receipt: null, problems: [`${real.length} results that disagree`, ...problems], notes: [] };
  }
  const notes = real.length > 1 ? [`the result appears ${real.length} times, identical — an echoed final turn`] : [];
  return { receipt: real.at(-1), problems, notes };
}

export function validate(r, sha, lane) {
  const problems = [];

  // Without this a result saved under another lane's filename satisfies that lane.
  if ([...new Set(r.laneLines)].length > 1) problems.push(`conflicting LANE lines: ${[...new Set(r.laneLines)].join(" ")}`);
  if (r.lane === null) problems.push("no LANE line");
  else if (lane !== undefined && r.lane !== lane) problems.push(`LANE: ${r.lane} but this file is ${lane}`);

  const distinct = [...new Set(r.results)];
  if (distinct.length === 0) problems.push("no RESULT line");
  else if (distinct.length > 1) problems.push(`conflicting results: ${distinct.join(" ")}`);
  else if (!RESULTS.includes(r.result)) problems.push(`RESULT: ${r.result} is not ${RESULTS.join(" or ")}`);

  if ([...new Set(r.shaLines)].length > 1) problems.push(`conflicting REVIEWED lines: ${[...new Set(r.shaLines)].join(" ")}`);
  if (r.sha === null) problems.push("no REVIEWED line");
  else if (!RECEIPT_SHA_RE.test(r.sha)) problems.push(`REVIEWED='${r.sha}' is not a sha of at least 12 characters`);
  else if (!sha.startsWith(r.sha)) problems.push(`REVIEWED=${r.sha} is not the reviewed tree ${sha.slice(0, 12)}`);

  if ([...new Set(r.declaredLines)].length > 1) problems.push(`conflicting FINDINGS lines: ${[...new Set(r.declaredLines)].join(" ")}`);
  if (r.declared === null) problems.push("no FINDINGS line (need 'FINDINGS: none' or a count)");
  else if (!/^(none|\d+)$/i.test(r.declared)) problems.push(`FINDINGS: '${r.declared}' is not a count or 'none'`);
  else {
    const n = /^none$/i.test(r.declared) ? 0 : Number(r.declared);
    if (n !== r.rows.length) problems.push(`declared FINDINGS: ${r.declared} but listed ${r.rows.length}`);
  }

  for (const id of [...new Set(r.conflictingIds)]) {
    problems.push(`FINDING ${id} appears more than once with different content`);
  }

  if (r.nearMiss.length > 0) {
    const f = r.nearMiss[0];
    problems.push(
      `${r.nearMiss.length} line(s) start with FINDING but do not match ` +
        "`FINDING <id> | <severity> | <file>:<line> | <summary>` — first: " +
        `'${f.length > 90 ? `${f.slice(0, 90)}…` : f}'`,
    );
  }

  for (const row of r.rows) {
    if (!SEVERITY_RE.test(row.severity)) problems.push(`FINDING ${row.id}: severity '${row.severity}' is not P0-P3`);
    if (!/^\S+:\d+$/.test(row.location)) problems.push(`FINDING ${row.id}: location '${row.location}' is not <file>:<line>`);
    if (row.summary.length === 0) problems.push(`FINDING ${row.id}: empty summary`);
  }

  const blockers = r.rows.filter((x) => /^P[01]$/.test(x.severity)).length;
  const carried = r.rows.length - blockers;
  // A FAIL with nothing blocking is summarised downstream as a clean result.
  if (r.result === "FAIL" && blockers === 0) problems.push("RESULT: FAIL with no P0/P1 finding — a FAIL names what blocks");
  if (r.result === "PASS" && blockers > 0) problems.push(`RESULT: PASS with ${blockers} P0/P1 finding(s)`);

  return { problems, blockers, carried };
}

export function checkOne({ dir, sha, name }) {
  const files = [`${name}-verdict.md`, `${name}.md`, `${name}.out`]
    .map((f) => path.join(dir, f))
    .filter((f) => existsSync(f));
  if (files.length === 0) return { name, ok: false, problems: [`no result file (looked for ${name}-verdict.md, ${name}.md, ${name}.out)`], notes: [] };

  // Several files for one lane is a competing-result hazard: picking the first silently prefers one
  // of an agent's outputs over another.
  const extracted = files.map((f) => ({ f, ...extract(readFileSync(f, "utf8")) }));
  const usable = extracted.filter((e) => e.receipt);
  // A malformed competing file must fail even when a valid one exists: silently ignored, it is
  // indistinguishable from a lane that never ran — the failure this tool exists to catch.
  const broken = extracted.filter((e) => !e.receipt);
  if (files.length > 1 && broken.length > 0) {
    return {
      name,
      ok: false,
      file: broken[0].f,
      bytes: statSync(broken[0].f).size,
      problems: broken.map((e) => `competing result file '${path.basename(e.f)}' is not a sound result: ${e.problems.join("; ")}`),
      notes: [],
    };
  }
  if (usable.length > 1 && new Set(usable.map((e) => canonical(e.receipt))).size > 1) {
    return {
      name,
      ok: false,
      file: usable[0].f,
      bytes: statSync(usable[0].f).size,
      problems: [`competing result files disagree: ${usable.map((e) => path.basename(e.f)).join(", ")}`],
      notes: [],
    };
  }

  const chosen = usable[0] ?? extracted[0];
  const file = chosen.f;
  const { receipt, problems: structural, notes } = chosen;
  if (!receipt) return { name, ok: false, file, bytes: statSync(file).size, problems: structural, notes };

  const { problems, blockers, carried } = validate(receipt, sha, name);
  if (receipt.duplicates > 0) notes.push(`${receipt.duplicates} duplicate FINDING line(s) collapsed`);
  const all = [...structural, ...problems];
  return { name, ok: all.length === 0, file, bytes: statSync(file).size, receipt, problems: all, notes, blockers, carried };
}

/**
 * A results directory is named for the snapshot it holds. Reusing one across snapshots lets a
 * leftover file from an earlier freeze answer for a lane that produced nothing this time.
 */
export function staleDirectory(dir, sha) {
  const base = path.basename(path.resolve(dir));
  if (!/^[0-9a-f]{7,40}$/i.test(base)) {
    return `results directory '${base}' is not named for a snapshot — name it for the reviewed sha`;
  }
  return sha.startsWith(base) ? null : `results directory '${base}' is not the reviewed tree ${sha.slice(0, 12)}`;
}

export function run({ dir, sha, names }) {
  const stale = staleDirectory(dir, sha);
  const results = names.map((name) => checkOne({ dir, sha, name }));
  return {
    results,
    stale,
    ok: stale === null && results.every((r) => r.ok),
    blockers: results.reduce((n, r) => n + (r.blockers ?? 0), 0),
    carried: results.reduce((n, r) => n + (r.carried ?? 0), 0),
  };
}

export function format(round) {
  const out = [];
  for (const r of round.results) {
    if (r.ok) {
      out.push(`OK      ${r.name.padEnd(16)} ${r.receipt.result.padEnd(5)} blockers=${r.blockers} carried=${r.carried}  ${r.bytes}B`);
      for (const row of r.receipt.rows) out.push(`          FINDING ${row.id} | ${row.severity} | ${row.location} | ${row.summary}`);
    } else {
      out.push(`MISSING ${r.name.padEnd(16)} ${r.bytes === undefined ? "" : `${r.bytes}B`}`);
      for (const p of r.problems) out.push(`          - ${p}`);
    }
    for (const n of r.notes) out.push(`          ! ${n}`);
  }
  if (round.stale) out.push(`STALE   ${round.stale}`);
  if (!round.ok) {
    out.push("");
    out.push("An agent result that is absent, truncated or inconclusive is BLOCKED — never a pass.");
    out.push("Check the log head, that the prompt went in on stdin, that the output path was writable,");
    out.push("and that the process finished rather than died.");
  } else {
    out.push(`ALL SOUND — ${round.results.length} result(s) on ${round.sha ?? ""} blockers=${round.blockers} carried=${round.carried}`.trimEnd());
  }
  return out.join("\n");
}

export function parseArgv(argv) {
  const [dir, sha, ...names] = argv;
  if (!dir || !sha || names.length === 0) return { error: "missing directory, sha or name" };
  if (!SHA_RE.test(sha)) return { error: `'${sha}' is not a full 40-character sha` };
  for (const n of names) if (!SAFE_NAME_RE.test(n)) return { error: `'${n}' is not a safe filename component` };
  return { dir, sha, names };
}

function main(argv) {
  const parsed = parseArgv(argv);
  if (parsed.error) {
    process.stderr.write(`usage: node scripts/orchestration/check-results.mjs <dir> <sha> <name>...\n       ${parsed.error}\n`);
    return 2;
  }
  const round = run(parsed);
  process.stdout.write(`${format({ ...round, sha: parsed.sha.slice(0, 12) })}\n`);
  return round.ok ? 0 : 1;
}

// Windows argv[1] is a native path and never equals a file URL, so a string compare disables the CLI.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exit(main(process.argv.slice(2)));
}

export { main };
