import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { loadFormatterLock, ROOT, validateFormatterLock } from "./verify.mjs";

export function validateFormatterManifest(manifest, file) {
  const problems = [];
  const check = (ok, why) => {
    if (!ok) problems.push(why);
  };
  const allowed = [
    "name",
    "kind",
    "schema",
    "description",
    "catalog",
    "parameters",
    "noise",
    "requiredCells",
  ];
  check(
    Object.keys(manifest).every((k) => allowed.includes(k)),
    "unknown formatter manifest field",
  );
  check(
    manifest.schema === 1 &&
      manifest.name === "formatter-performance" &&
      (!file || file.replace(/^.*[\\/]/, "") === `${manifest.name}.json`),
    "formatter manifest name/schema",
  );
  check(
    typeof manifest.description === "string" && manifest.description.length > 0,
    "formatter manifest description",
  );
  check(
    manifest.catalog === "catalogs/formatter-performance-v1.toml",
    "formatter performance catalog path",
  );
  check(
    JSON.stringify(manifest.parameters) === JSON.stringify(["candidate", "corpus"]),
    "formatter parameters must be candidate and corpus",
  );
  let lock;
  try {
    lock = loadFormatterLock();
    problems.push(...validateFormatterLock(lock));
  } catch (error) {
    problems.push(error.message);
  }
  if (!lock) return { problems, run: null };
  const expected = lock.performance.mode.map((m) => ({
    mode: m.id,
    latencyP95Ms: m.latency_p95_ms,
    peakRssDeltaBytes: m.peak_rss_delta_bytes,
    peakRssApplicable: m.peak_rss_applicable,
    metrics: ["answerClass", "workCounts", "normalizedP95Ms", "peakRssDeltaBytes"],
    evidenceOnly: ["normalizedP95Ms", "peakRssDeltaBytes"],
  }));
  check(
    isDeepStrictEqual(manifest.requiredCells, expected),
    "formatter required cells differ from catalog modes/latency/RSS",
  );
  return { problems, run: problems.length ? null : { formatter: true, lock } };
}

export function buildFormatterSummary(loaded, { candidate, corpus, dryRun, worker }) {
  if (typeof candidate !== "string" || !candidate.trim())
    throw Error("formatter evidence requires --candidate <review-candidate identity>");
  const lock = loaded.run.lock;
  const fixture = lock.performance.fixture.find((f) => f.id === corpus);
  if (!fixture)
    throw Error(
      `formatter evidence requires a known --corpus; unknown corpus ${corpus ?? "(missing)"}`,
    );
  const catalogBytes = readFileSync(resolve(ROOT, loaded.manifest.catalog));
  return {
    schema: 1,
    run: {
      name: loaded.manifest.name,
      candidate,
      corpus,
      manifestSha256: loaded.sha256,
      performanceCatalogSha256: createHash("sha256").update(catalogBytes).digest("hex"),
      fixtureSha256: fixture.sha256,
    },
    dryRun,
    worker,
    cells: loaded.manifest.requiredCells.map((cell) => ({
      ...cell,
      status: dryRun ? "not run" : "unavailable",
      metrics: Object.fromEntries(
        cell.metrics.map((name) => [
          name,
          {
            status: dryRun ? "not measured" : "unavailable",
            reason: dryRun
              ? "a dry run executes nothing"
              : "this contract-only checkout contains no native formatter benchmark implementation",
          },
        ]),
      ),
    })),
    validation: { verdict: dryRun ? "not run" : "unavailable", problems: [] },
    measurementPolicy: lock.performance.measurement_policy,
  };
}
