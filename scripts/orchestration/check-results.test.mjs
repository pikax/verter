import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { BEGIN, END, extract, findRegions, run, staleDirectory } from "./check-results.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CLI = path.join(HERE, "check-results.mjs");
const SHA = "49b11dcc7a1b2c3d4e5f60718293a4b5c6d7e8f9";

let dir;
let root;
beforeEach(() => {
  // A results directory is named for the snapshot it holds; the tool refuses one that is not.
  root = mkdtempSync(path.join(tmpdir(), "verter-results-"));
  dir = path.join(root, SHA.slice(0, 12));
  mkdirSync(dir);
});
afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

const put = (name, body) => writeFileSync(path.join(dir, name), body);

function receipt({ lane = "review", result = "PASS", sha = SHA, findings = "none", rows = [], extra = [] } = {}) {
  return [BEGIN, `LANE: ${lane}`, `RESULT: ${result}`, `REVIEWED: ${sha}`, ...extra, `FINDINGS: ${findings}`, ...rows, END].join("\n");
}

const one = (name = "review") => run({ dir, sha: SHA, names: [name] }).results[0];

function exitCode(args) {
  try {
    execFileSync(process.execPath, [CLI, ...args], { stdio: "pipe" });
    return 0;
  } catch (e) {
    return e.status;
  }
}

describe("a sound result", () => {
  it("passes, with its findings extracted", () => {
    put(
      "review.out",
      receipt({
        result: "FAIL",
        findings: "2",
        rows: [
          "FINDING F1 | P1 | src/a.rs:12 | fail-open on the error path",
          "FINDING F2 | P2 | src/b.rs:3 | unclear assertion message",
        ],
      }),
    );
    const r = one();
    expect(r.ok).toBe(true);
    expect(r.blockers).toBe(1);
    expect(r.carried).toBe(1);
    expect(r.receipt.rows[0].id).toBe("F1");
  });

  it("exits 0 even carrying blockers — soundness and findings are different questions", () => {
    put("review.out", receipt({ result: "FAIL", findings: "1", rows: ["FINDING F1 | P0 | a.rs:1 | x"] }));
    expect(exitCode([dir, SHA, "review"])).toBe(0);
  });
});

describe("an agent result that did not arrive", () => {
  it("names every filename it looked for", () => {
    expect(one().problems[0]).toMatch(/no result file/);
  });

  it("refuses an empty file", () => {
    put("review.out", "");
    expect(one().problems.join(" ")).toMatch(/never reached a conclusion/);
  });

  it("refuses a long file with no conclusion, since size is not the check", () => {
    const body = "Analysis paragraph about the candidate.\n".repeat(40);
    expect(body.length).toBeGreaterThan(1000);
    put("review.out", body);
    expect(one().ok).toBe(false);
  });

  it("names a truncated result distinctly from an absent one", () => {
    put("review.out", [BEGIN, "LANE: review", "RESULT: PASS"].join("\n"));
    expect(one().problems.join(" ")).toMatch(/began but never ended/);
  });

  it("names an inverted result", () => {
    put("review.out", [END, "stray", BEGIN].join("\n"));
    expect(one().problems.join(" ")).toMatch(/END marker precedes its BEGIN/);
  });

  it("refuses two results that disagree, and accepts two that are identical", () => {
    put("review.out", `${receipt()}\n${receipt({ result: "FAIL" })}`);
    expect(one().problems.join(" ")).toMatch(/results that disagree/);
    put("review.out", `${receipt()}\n${receipt()}`);
    const r = one();
    expect(r.ok).toBe(true);
    expect(r.notes.join(" ")).toMatch(/identical — an echoed final turn/);
  });

  it("exits 1 rather than passing", () => {
    put("review.out", "no result here");
    expect(exitCode([dir, SHA, "review"])).toBe(1);
  });
});

describe("the reviewed tree", () => {
  it("refuses a different sha that merely shares a prefix", () => {
    put("review.out", receipt({ sha: `${SHA.slice(0, 12)}${"e".repeat(28)}` }));
    expect(one().problems.join(" ")).toMatch(/is not the reviewed tree/);
  });

  it("refuses an absent REVIEWED line, and a sha too short to bind", () => {
    put("review.out", [BEGIN, "LANE: review", "RESULT: PASS", "FINDINGS: none", END].join("\n"));
    expect(one().problems).toContain("no REVIEWED line");
    put("review.out", receipt({ sha: SHA.slice(0, 9) }));
    expect(one().problems.join(" ")).toMatch(/at least 12 characters/);
  });

  it("accepts a genuine abbreviation", () => {
    put("review.out", receipt({ sha: SHA.slice(0, 12) }));
    expect(one().ok).toBe(true);
  });
});

describe("what the result claims to have found", () => {
  it("refuses a declared count that disagrees with the rows listed", () => {
    put("review.out", receipt({ findings: "none", rows: ["FINDING F1 | P0 | a.rs:1 | a blocker"] }));
    expect(one().problems.join(" ")).toMatch(/declared FINDINGS: none but listed 1/);
  });

  it("collapses identical repeated rows before counting", () => {
    put("review.out", receipt({ result: "FAIL", findings: "1", rows: ["FINDING F1 | P1 | a.rs:1 | x", "FINDING F1 | P1 | a.rs:1 | x"] }));
    const r = one();
    expect(r.ok).toBe(true);
    expect(r.notes.join(" ")).toMatch(/1 duplicate FINDING line/);
  });

  it("names a near-miss row rather than reporting it only as a count mismatch", () => {
    put("review.out", receipt({ result: "FAIL", findings: "1", rows: ["FINDING | TST-001 | P1 | CLAUDE.md:485 | extra leading bar"] }));
    const msg = one().problems.join(" ");
    expect(msg).toMatch(/start with FINDING but do not match/);
    expect(msg).toMatch(/declared FINDINGS: 1 but listed 0/);
  });

  it("refuses conflicting RESULT lines but accepts identical repeats", () => {
    put("review.out", receipt({ extra: ["RESULT: FAIL"] }));
    expect(one().problems.join(" ")).toMatch(/conflicting results/);
    put("review.out", receipt({ extra: ["RESULT: PASS"] }));
    expect(one().ok).toBe(true);
  });

  it("refuses a RESULT outside the closed set", () => {
    put("review.out", receipt({ result: "LAND" }));
    expect(one().problems.join(" ")).toMatch(/RESULT: LAND is not PASS or FAIL/);
  });

  it("refuses a verdict inconsistent with its own findings, in both directions", () => {
    put("review.out", receipt({ result: "FAIL" }));
    expect(one().problems.join(" ")).toMatch(/FAIL with no P0\/P1 finding/);
    put("review.out", receipt({ result: "PASS", findings: "1", rows: ["FINDING F1 | P0 | a.rs:1 | x"] }));
    expect(one().problems.join(" ")).toMatch(/PASS with 1 P0\/P1/);
  });

  it("refuses a row whose severity or location cannot be routed", () => {
    put("review.out", receipt({ result: "FAIL", findings: "1", rows: ["FINDING F1 | P9 | somewhere | x"] }));
    const msg = one().problems.join(" ");
    expect(msg).toMatch(/severity 'P9' is not P0-P3/);
    expect(msg).toMatch(/location 'somewhere' is not <file>:<line>/);
  });
});

describe("real captured CLI output", () => {
  const read = (f) => readFileSync(path.join(HERE, "fixtures", f), "utf8");

  it("never turns an echoed prompt into a verdict", () => {
    // Real `codex exec` trace: the prompt, including its own specimen block, is echoed under `user`.
    const trace = read("codex-trace-echoed-prompt.txt");
    expect(trace).toContain(BEGIN);
    expect(findRegions(trace).regions).toHaveLength(1);
    expect(extract(trace).receipt).toBeNull();
  });

  it("takes the agent's own result when one follows the echo", () => {
    const real = receipt({ result: "FAIL", sha: SHA, findings: "1", rows: ["FINDING C1 | P1 | docs/c.md:9 | no evidence cited"] });
    put("review.out", `${read("codex-trace-echoed-prompt.txt")}\n${real}\n__DONE__\n`);
    expect(one().receipt.result).toBe("FAIL");
  });

  it("counts a doubled final turn once", () => {
    // Real extract: the shell takes everything after the last speaker marker and catches the final
    // turn twice, so the block and its rows both arrive doubled.
    const doubled = read("codex-receipt-doubled-final-turn.txt");
    expect(doubled.match(/^FINDING /gm)).toHaveLength(26);
    expect(findRegions(doubled).regions).toHaveLength(2);
  });
});

describe("usage", () => {
  it("rejects a missing argument, an abbreviated sha and an unsafe name", () => {
    expect(exitCode([])).toBe(2);
    expect(exitCode([dir, SHA.slice(0, 9), "review"])).toBe(2);
    expect(exitCode([dir, SHA, "../../etc/passwd"])).toBe(2);
  });
});

describe("a result is bound to its lane", () => {
  it("refuses a result saved under another lane's filename", () => {
    put("architecture.out", receipt({ lane: "correctness" }));
    expect(run({ dir, sha: SHA, names: ["architecture"] }).results[0].problems.join(" ")).toMatch(
      /LANE: correctness but this file is architecture/,
    );
  });

  it("refuses an absent or conflicting LANE line", () => {
    put("review.out", [BEGIN, "RESULT: PASS", `REVIEWED: ${SHA}`, "FINDINGS: none", END].join("\n"));
    expect(one().problems).toContain("no LANE line");
    put("review.out", receipt({ extra: ["LANE: other"] }));
    expect(one().problems.join(" ")).toMatch(/conflicting LANE lines/);
  });
});

describe("competing and stale results", () => {
  it("refuses two files for one lane that disagree, and accepts two that agree", () => {
    put("review.out", receipt({ result: "PASS" }));
    put("review-verdict.md", receipt({ result: "FAIL", findings: "1", rows: ["FINDING F1 | P0 | a.rs:1 | x"] }));
    expect(one().problems.join(" ")).toMatch(/competing result files disagree/);
    put("review-verdict.md", receipt({ result: "PASS" }));
    expect(one().ok).toBe(true);
  });

  it("fails when a competing file is malformed, rather than silently preferring the valid one", () => {
    put("review.out", receipt());
    put("review-verdict.md", [BEGIN, "LANE: review", "RESULT: PASS"].join("\n"));
    const r = one();
    expect(r.ok).toBe(false);
    expect(r.problems.join(" ")).toMatch(/competing result file 'review-verdict.md' is not a sound result/);
  });

  it("refuses conflicting REVIEWED lines", () => {
    put("review.out", receipt({ extra: [`REVIEWED: ${"a".repeat(40)}`] }));
    expect(one().problems.join(" ")).toMatch(/conflicting REVIEWED lines/);
  });

  it("refuses a duplicate finding id carrying different content", () => {
    put(
      "review.out",
      receipt({
        result: "FAIL",
        findings: "2",
        rows: ["FINDING F1 | P0 | a.rs:1 | one", "FINDING F1 | P2 | a.rs:1 | actually minor"],
      }),
    );
    expect(one().problems.join(" ")).toMatch(/FINDING F1 appears more than once with different content/);
  });

  it("refuses a results directory that is not the reviewed snapshot", () => {
    expect(staleDirectory(path.join(root, "deadbeefdead"), SHA)).toMatch(/is not the reviewed tree/);
    expect(staleDirectory(path.join(root, "round-2"), SHA)).toMatch(/is not named for a snapshot/);
    expect(staleDirectory(dir, SHA)).toBeNull();
  });

  it("fails the whole run on a stale directory even when every result is sound", () => {
    put("review.out", receipt());
    const other = path.join(root, "round-2");
    mkdirSync(other);
    writeFileSync(path.join(other, "review.out"), receipt());
    const r = run({ dir: other, sha: SHA, names: ["review"] });
    expect(r.results[0].ok).toBe(true);
    expect(r.ok).toBe(false);
    expect(r.stale).toMatch(/not named for a snapshot/);
  });
});
