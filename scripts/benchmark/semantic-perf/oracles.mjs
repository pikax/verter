// Where the next TypeScript answers differently from the oracle. Both
// references are measured by the same method (measure-expected.mjs):
// expected.json by the released compiler every run is classified against,
// expected-next.json (--next) by the pinned next one, which classifies
// nothing. Given a run's results.json, it also shows which of the two
// Verter's answers agree with where they differ.
//
//   node scripts/benchmark/semantic-perf/oracles.mjs [results.json] [--out <file.md>]

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { sha256Text } from "./provenance.mjs";
import { SETTINGS, selectScenarios, tsconfigText } from "./scenarios.mjs";
import { referenceFor } from "./summary.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));

// A reference cell is compared only when it was measured on the scenario as
// the catalog defines it now.
function fresh(expected, scenario, setting) {
  const entry = expected.scenarios?.[scenario.id];
  const cell = entry?.settings?.[setting.id];
  return Boolean(
    cell &&
    entry.sourceSha256 === sha256Text(scenario.source) &&
    cell.receipt?.tsconfigSha256 === sha256Text(tsconfigText(setting, scenario)),
  );
}

// Two compilers that both run out of a resource answer alike (neither
// answers), whichever limit each reached first.
const answerKey = (a) =>
  `${a.killed ? "exhausted" : a.gap ? `gap:${a.gap}` : a.errorAny ? "error-any" : `type:${a.digest.sha256}`}|${(a.codes ?? []).join(",")}`;
export const shown = (a) =>
  `${a.gap ?? (a.errorAny ? "error any" : a.digest.preview)}${a.codes?.length ? ` (${a.codes.map((c) => `TS${c}`).join(", ")})` : ""}`;

/**
 * Compare two references cell by cell over `scenarios`:
 * { agree, differ: [{ scenario, setting, oracle, next }], uncompared: [{ scenario, setting, why }] }.
 * With a run, each differing cell the run answered also carries `verter`:
 * "oracle", "next" or "neither" — whose answer Verter's equals.
 */
export function compareOracles(expected, next, scenarios, run = null) {
  const cells = new Map((run?.summary?.cells ?? []).map((c) => [c.key, c]));
  const out = { agree: 0, differ: [], uncompared: [] };
  for (const scenario of scenarios)
    for (const setting of SETTINGS) {
      if (!fresh(expected, scenario, setting) || !fresh(next, scenario, setting)) {
        out.uncompared.push({ scenario: scenario.id, setting: setting.id, why: "stale" });
        continue;
      }
      const oracle = referenceFor(expected, scenario.id, setting.id);
      const nextAnswer = referenceFor(next, scenario.id, setting.id);
      // A print too large to record is kept as a digest of its text, which
      // member order alone changes: equal or not, it proves nothing.
      if (oracle.truncated || nextAnswer.truncated) {
        out.uncompared.push({ scenario: scenario.id, setting: setting.id, why: "too large" });
        continue;
      }
      if (answerKey(oracle) === answerKey(nextAnswer)) {
        out.agree++;
        continue;
      }
      const row = { scenario: scenario.id, setting: setting.id, oracle, next: nextAnswer };
      const verter = cells.get(`${scenario.id}/${setting.id}`)?.arms?.verter?.answerDigest?.sha256;
      if (verter)
        row.verter =
          verter === oracle.digest?.sha256
            ? "oracle"
            : verter === nextAnswer.digest?.sha256
              ? "next"
              : "neither";
      out.differ.push(row);
    }
  return out;
}

export function renderOracles(result, expected, next, run = null) {
  const esc = (s) => String(s).replace(/\|/g, "\\|").replace(/\n/g, " ").slice(0, 160);
  const lines = [
    `# tsc ${expected.typescript} (oracle) vs tsc ${next.typescript} (next)`,
    "",
    `Both measured by the same method; tsc ${expected.typescript} classifies every run, tsc ${next.typescript} classifies nothing.`,
    "",
    `- cells compared: ${result.agree + result.differ.length}`,
    `- same answer: ${result.agree}`,
    `- different answer: ${result.differ.length}`,
    `- not compared: ${result.uncompared.length}`,
    "",
  ];
  if (result.differ.length) {
    const withVerter = Boolean(run);
    lines.push(
      "## Where they answer differently",
      "",
      `| scenario | setting | tsc ${expected.typescript} | tsc ${next.typescript} |${withVerter ? " Verter equals |" : ""}`,
      `| --- | --- | --- | --- |${withVerter ? " --- |" : ""}`,
    );
    for (const d of result.differ)
      lines.push(
        `| ${d.scenario} | ${d.setting} | ${esc(shown(d.oracle))} | ${esc(shown(d.next))} |${withVerter ? ` ${d.verter === "oracle" ? `tsc ${expected.typescript}` : d.verter === "next" ? `tsc ${next.typescript}` : d.verter ? "neither" : "not run"} |` : ""}`,
      );
    lines.push("");
  }
  const idsOf = (why) => [
    ...new Set(result.uncompared.filter((u) => u.why === why).map((u) => u.scenario)),
  ];
  if (result.uncompared.length) lines.push("## Not compared", "");
  if (idsOf("too large").length)
    lines.push(`- a print too large to record, on either side: ${idsOf("too large").join(", ")}`);
  if (idsOf("stale").length)
    lines.push(
      `- a reference missing or measured on an older scenario: ${idsOf("stale").join(", ")}; re-measure with \`measure-expected.mjs --only <ids>\` (and \`--next\`)`,
    );
  if (result.uncompared.length) lines.push("");
  return lines.join("\n");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const oi = args.indexOf("--out");
  const outFile = oi >= 0 ? args[oi + 1] : null;
  const runPath = args.find((a, i) => !a.startsWith("--") && args[i - 1] !== "--out");
  const read = (p) => JSON.parse(readFileSync(p, "utf8"));
  const expected = read(join(HERE, "expected.json"));
  let next;
  try {
    next = read(join(HERE, "expected-next.json"));
  } catch {
    console.error(
      "no expected-next.json: measure it with node scripts/benchmark/semantic-perf/measure-expected.mjs --next",
    );
    process.exit(2);
  }
  const run = runPath ? read(runPath) : null;
  const result = compareOracles(expected, next, selectScenarios([]), run);
  const markdown = renderOracles(result, expected, next, run);
  if (outFile) writeFileSync(outFile, markdown + "\n");
  else console.log(markdown);
}
